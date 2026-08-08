// SPDX-FileCopyrightText: © 2026 Julian Andrews
// SPDX-License-Identifier: 0BSD

use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wayland_backend::client::ObjectId;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, protocol::wl_registry};

use crate::river::{
    river_node_v1::RiverNodeV1,
    river_output_v1::RiverOutputV1,
    river_pointer_binding_v1::RiverPointerBindingV1,
    river_seat_v1::{Modifiers, RiverSeatV1},
    river_window_manager_v1::RiverWindowManagerV1,
    river_window_v1::{Edges, RiverWindowV1},
    river_xkb_binding_v1::RiverXkbBindingV1,
    river_xkb_bindings_v1::RiverXkbBindingsV1,
};

mod river {
    pub extern crate wayland_client;
    pub use wayland_client::protocol::*;

    mod interfaces {
        pub(super) mod rwm {
            pub use wayland_client::protocol::__interfaces::*;
            wayland_scanner::generate_interfaces!("./protocol/river-window-management-v1.xml");
        }

        pub(super) mod rxkb {
            use super::rwm::*;
            wayland_scanner::generate_interfaces!("./protocol/river-xkb-bindings-v1.xml");
        }
    }

    use self::interfaces::rwm::*;
    use self::interfaces::rxkb::*;
    wayland_scanner::generate_client_code!("./protocol/river-window-management-v1.xml");
    wayland_scanner::generate_client_code!("./protocol/river-xkb-bindings-v1.xml");
}

mod ipc;
mod wm_core;

use wm_core::ids::{OutputId, TagId, ViewId};
use wm_core::state::{PINNED_TERM_APP_ID, WmCore, WmCoreError};
use wm_core::view::DEFAULT_FLOATING_GEOMETRY;

/// Logs `result`'s error (if any) as `"{context}: {e:?}"`, otherwise no-ops.
/// `wm_core` mutators only fail on invalid/unknown ids that call sites here
/// already guard against structurally (NFR2) — this exists purely so a
/// future regression is visible instead of silently discarded.
fn log_wm_core_err(result: Result<(), WmCoreError>, context: &str) {
    if let Err(e) = result {
        eprintln!("{context}: {e:?}");
    }
}

#[derive(Debug, Clone, Copy)]
enum Action {
    None,
    SpawnFoot,
    Close,
    FocusNext,
    Move,
    Resize,
    Exit,
    TagCycle,
    TagCreate,
    OpenTagPicker,
}

#[derive(Debug, Clone)]
enum SeatOp {
    None,
    Move {
        window_proxy: RiverWindowV1,
        start_x: i32,
        start_y: i32,
    },
    Resize {
        window_proxy: RiverWindowV1,
        start_x: i32,
        start_y: i32,
        start_width: i32,
        start_height: i32,
        edges: Edges,
    },
}

#[derive(Debug, Default)]
struct AppData {
    river_wm: Option<RiverWindowManagerV1>,
    river_xkb: Option<RiverXkbBindingsV1>,
    wm: WindowManager,
}

#[derive(Debug, Default)]
struct WindowManager {
    windows: VecDeque<Window>,
    outputs: HashMap<ObjectId, Output>,
    seats: HashMap<ObjectId, Seat>,
    wm_core: Arc<Mutex<WmCore>>,
}

#[derive(Debug)]
struct Window {
    proxy: RiverWindowV1,
    node: RiverNodeV1,
    new: bool,
    closed: bool,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    pointer_move_requested: Option<RiverSeatV1>,
    pointer_resize_requested: Option<RiverSeatV1>,
    pointer_resize_requested_edges: Edges,
    app_id: String,
    view_id: Option<ViewId>,
}

#[derive(Debug)]
struct Output {
    proxy: RiverOutputV1,
    removed: bool,
    output_id: OutputId,
}

#[derive(Debug)]
struct Seat {
    proxy: RiverSeatV1,
    new: bool,
    removed: bool,
    focused: Option<RiverWindowV1>,
    hovered: Option<RiverWindowV1>,
    interacted: Option<RiverWindowV1>,
    xkb_bindings: HashMap<ObjectId, XkbBinding>,
    pointer_bindings: HashMap<ObjectId, PointerBinding>,
    pending_action: Action,
    op: SeatOp,
    op_dx: i32,
    op_dy: i32,
    op_release: bool,
}

#[derive(Debug)]
struct XkbBinding {
    proxy: RiverXkbBindingV1,
    action: Action,
}

#[derive(Debug)]
struct PointerBinding {
    proxy: RiverPointerBindingV1,
    action: Action,
}

impl WindowManager {
    fn handle_manage_start(
        &mut self,
        proxy: &RiverWindowManagerV1,
        river_xkb: &RiverXkbBindingsV1,
        qh: &QueueHandle<AppData>,
    ) {
        self.remove_outputs();
        self.remove_windows();
        self.remove_seats();
        self.init_new_windows();
        self.init_new_seats(river_xkb, qh);
        self.manage_windows();
        self.manage_seats(proxy);
        proxy.manage_finish();
    }

    fn handle_render_start(&mut self, proxy: &RiverWindowManagerV1) {
        for seat in &mut self.seats.values_mut() {
            match &seat.op {
                SeatOp::None => {}
                SeatOp::Move {
                    window_proxy,
                    start_x,
                    start_y,
                } => {
                    if let Some(window) = self
                        .windows
                        .iter_mut()
                        .find(|window| &window.proxy == window_proxy)
                    {
                        window.set_position(start_x + seat.op_dx, start_y + seat.op_dy);
                    }
                }
                SeatOp::Resize {
                    window_proxy,
                    start_x,
                    start_y,
                    start_width,
                    start_height,
                    edges,
                } => {
                    if let Some(window) = self
                        .windows
                        .iter_mut()
                        .find(|window| &window.proxy == window_proxy)
                    {
                        let (mut x, mut y) = (*start_x, *start_y);
                        if edges.contains(Edges::Left) {
                            x += start_width - window.width;
                        }
                        if edges.contains(Edges::Top) {
                            y += start_height - window.height;
                        }
                        window.set_position(x, y);
                    }
                }
            }
        }

        proxy.render_finish();
    }

    fn remove_outputs(&mut self) {
        self.outputs.retain(|_, output| {
            if output.removed {
                output.proxy.destroy();
                return false;
            }
            true
        });
    }

    fn remove_windows(&mut self) {
        let old_windows = std::mem::take(&mut self.windows);
        let mut wm_core_guard = ipc::lock_recovering(&self.wm_core);
        let wm_core = &mut *wm_core_guard;
        self.windows = old_windows
            .into_iter()
            .filter(|window| {
                if window.closed {
                    for seat in self.seats.values_mut() {
                        // Upstream tinyrwm (pinned commit 04a3f9f) code, kept
                        // as-vendored; clippy::collapsible_if only started
                        // firing on this nested-if pattern under the
                        // devcontainer's toolchain (rustc/clippy 1.97).
                        #[allow(clippy::collapsible_if)]
                        if let SeatOp::Move { window_proxy, .. }
                        | SeatOp::Resize { window_proxy, .. } = &seat.op
                        {
                            if window_proxy == &window.proxy {
                                seat.op_end();
                            }
                        }
                    }
                    if let Some(id) = window.view_id {
                        // This id is always valid through this call path —
                        // only this module ever registers/removes a
                        // `wm-core` view id — so a failure here is believed
                        // structurally unreachable; log rather than
                        // silently swallow, so a future regression that
                        // does hit `Err` stays visible (NFR2, error
                        // propagation).
                        if let Err(e) = wm_core.unregister_view(id) {
                            eprintln!("Failed to unregister view {id:?} from wm_core: {e:?}");
                        }
                    }
                    return false;
                }
                true
            })
            .collect();
    }

    fn remove_seats(&mut self) {
        self.seats.retain(|_, seat| {
            if seat.removed {
                seat.xkb_bindings
                    .values_mut()
                    .for_each(|binding| binding.proxy.destroy());
                seat.pointer_bindings
                    .values_mut()
                    .for_each(|binding| binding.proxy.destroy());
                seat.proxy.destroy();
                return false;
            }
            true
        });
    }

    fn init_new_windows(&mut self) {
        let mut wm_core = ipc::lock_recovering(&self.wm_core);
        for window in self.windows.iter_mut().filter(|w| w.new) {
            let view_id = wm_core.register_view(&window.app_id);
            window.view_id = Some(view_id);
            if window.app_id == PINNED_TERM_APP_ID {
                window.set_position(window.x, window.y);
                window.proxy.propose_dimensions(window.width, window.height);
                log_wm_core_err(
                    wm_core.set_view_floating(view_id, false),
                    "Failed to set pinned terminal non-floating",
                );
                log_wm_core_err(
                    wm_core.lower_view(view_id),
                    "Failed to lower pinned terminal in stacking order",
                );
            } else {
                log_wm_core_err(
                    wm_core.set_view_geometry(view_id, DEFAULT_FLOATING_GEOMETRY),
                    "Failed to set default floating geometry",
                );
                window.set_position(DEFAULT_FLOATING_GEOMETRY.x, DEFAULT_FLOATING_GEOMETRY.y);
                window.proxy.propose_dimensions(
                    DEFAULT_FLOATING_GEOMETRY.width,
                    DEFAULT_FLOATING_GEOMETRY.height,
                );
                window.node.place_top();
            }
            window.new = false;
        }
    }

    /// Composes Story 1.5's Tasks 2-3: claims the lazy-spawn-once
    /// pinned-terminal slot for `tag_id` and, if this is the first claim,
    /// spawns it. Called from `manage_seats`, once per tag that a seat's
    /// `Action::TagCycle` just switched an output onto (Story 1.7) —
    /// deliberately called after (not during) the seat loop that holds
    /// `wm_core`'s mutable borrow.
    fn ensure_pinned_terminal_spawned(&mut self, tag_id: TagId) {
        match ipc::lock_recovering(&self.wm_core).claim_pinned_terminal_spawn(tag_id) {
            Ok(Some(session_name)) => spawn_pinned_terminal(&session_name),
            Ok(None) => {}
            Err(e) => {
                eprintln!("Failed to check pinned-terminal spawn state for tag {tag_id:?}: {e:?}")
            }
        }
    }

    fn init_new_seats(&mut self, river_xkb: &RiverXkbBindingsV1, qh: &QueueHandle<AppData>) {
        // See xkbcommon/xkbcommon-keysyms.h
        const SPACE: u32 = 0x20;
        const N: u32 = 0x6e;
        const Q: u32 = 0x71;
        const ESC: u32 = 0xff1b;
        const TAB: u32 = 0xff09;
        const T: u32 = 0x74;
        // Story 2.2 gap #4: `Mod4+A` ("Assign") opens the tag-manager
        // picker, following this file's existing single-letter-mnemonic
        // convention.
        const A: u32 = 0x61;
        // See linux/input-event-codes.h
        const BTN_LEFT: u32 = 0x110;
        const BTN_RIGHT: u32 = 0x111;
        let mods = Modifiers::Mod4;

        for seat in self.seats.values_mut() {
            if seat.new {
                seat.create_xkb_binding(river_xkb, qh, mods, SPACE, Action::SpawnFoot);
                seat.create_xkb_binding(river_xkb, qh, mods, Q, Action::Close);
                seat.create_xkb_binding(river_xkb, qh, mods, N, Action::FocusNext);
                seat.create_xkb_binding(river_xkb, qh, mods, ESC, Action::Exit);
                seat.create_xkb_binding(river_xkb, qh, mods, TAB, Action::TagCycle);
                seat.create_xkb_binding(river_xkb, qh, mods, T, Action::TagCreate);
                seat.create_xkb_binding(river_xkb, qh, mods, A, Action::OpenTagPicker);
                seat.create_pointer_binding(qh, mods, BTN_LEFT, Action::Move);
                seat.create_pointer_binding(qh, mods, BTN_RIGHT, Action::Resize);
                seat.new = false;
            }
        }
    }

    fn manage_windows(&mut self) {
        for window in self.windows.iter_mut() {
            if let Some(seat_proxy) = window.pointer_move_requested.take() {
                let seat = self
                    .seats
                    .get_mut(&seat_proxy.id())
                    .expect("Seat not found");
                seat.pointer_move(window);
            }
            if let Some(seat_proxy) = window.pointer_resize_requested.take() {
                let seat = self
                    .seats
                    .get_mut(&seat_proxy.id())
                    .expect("Seat not found");
                seat.pointer_resize(window, window.pointer_resize_requested_edges);
            }
        }
    }

    /// The deterministic "active output" for keybind-driven tag actions:
    /// the lowest-`OutputId` (first-registered) output. Real focused-output
    /// tracking has no live protocol signal to compute from yet (see Story
    /// 1.7's Description/Technical notes) — this is a conscious,
    /// documented scope boundary, correct-by-construction for the dominant
    /// single-output case (ADR-005) and at least deterministic under
    /// multi-output, not a hidden guess.
    fn active_output_id(&self) -> Option<OutputId> {
        self.outputs.values().map(|o| o.output_id).min()
    }

    fn manage_seats(&mut self, wm_proxy: &RiverWindowManagerV1) {
        // Computed before the `&mut self.wm_core` borrow below begins:
        // `active_output_id` is a whole-`&self` method call (it reads
        // `self.outputs`), which cannot run *during* the loop's mutable
        // `wm_core` borrow even though the two fields are disjoint (Story
        // 1.7).
        let active_output_id = self.active_output_id();
        let mut pending_terminal_spawns: Vec<TagId> = Vec::new();
        let mut wm_core_guard = ipc::lock_recovering(&self.wm_core);
        let wm_core = &mut *wm_core_guard;
        for seat in self.seats.values_mut() {
            // Code review follow-up (Story 1.5, finding #2): when the
            // interacted window is the pinned terminal, this pass already
            // gives it real Wayland keyboard focus directly below, so the
            // unconditional `seat.focus_top` call further down (which
            // always targets `self.windows.back()`, and thus would
            // immediately re-focus + re-`place_top()` whatever real window
            // is actually on top) must be skipped for this one pass —
            // otherwise it would instantly undo the direct focus call in
            // the same iteration.
            let mut pinned_terminal_focused_directly = false;
            if let Some(window_proxy) = seat.interacted.take() {
                let i = self
                    .windows
                    .iter()
                    .position(|window| window.proxy == window_proxy)
                    .expect("Interacted window not found");
                let window = self.windows.remove(i).unwrap();
                // Keep wm_core's stacking_order synchronized with the real
                // z-order on every click-to-focus reorder, not just on
                // FocusNext — otherwise stacking_order silently and
                // permanently diverges from self.windows after the first
                // mouse click, which Story 1.6's tiling geometry will read
                // from (Story 1.4 code-review follow-up). `raise_view`
                // itself already no-ops for the pinned terminal
                // (wm_core::state), so this call is safe regardless.
                if let Some(view_id) = window.view_id
                    && let Err(e) = wm_core.raise_view(view_id)
                {
                    eprintln!("Failed to raise view {view_id:?} in wm_core stacking order: {e:?}");
                }
                if window.app_id == PINNED_TERM_APP_ID {
                    // FR4: the pinned terminal must always render at the
                    // bottom of the real z-order, so — unlike every other
                    // window — it must not be pushed to the back of
                    // `self.windows` (which is what drives `focus_top`'s
                    // `place_top()` call). It can still receive real
                    // keyboard focus though: issue the same underlying
                    // Wayland-focus + wm_core::set_focus calls `focus_top`
                    // would perform, just without `place_top()` or the
                    // reorder, and re-insert it at its original position so
                    // it isn't dropped from `self.windows`.
                    seat.proxy.focus_window(&window.proxy);
                    seat.focused = Some(window.proxy.clone());
                    if let Some(view_id) = window.view_id
                        && let Err(e) = wm_core.set_focus(view_id)
                    {
                        eprintln!("Failed to set focus for view {view_id:?} in wm_core: {e:?}");
                    }
                    self.windows.insert(i, window);
                    pinned_terminal_focused_directly = true;
                } else {
                    self.windows.push_back(window);
                }
            }
            if !pinned_terminal_focused_directly {
                seat.focus_top(&self.windows, wm_core);
            }
            if let Some(tag_id) =
                seat.do_action(&mut self.windows, wm_proxy, wm_core, active_output_id)
            {
                pending_terminal_spawns.push(tag_id);
            }
            if seat.op_release {
                seat.op_end();
                seat.op_release = false;
            } else {
                seat.op_manage();
            }
        }
        // Story 2.1: explicitly drop the lock before
        // `ensure_pinned_terminal_spawned` below re-locks the same mutex —
        // `std::sync::Mutex` is not reentrant, so holding this guard across
        // that call would deadlock the very thread that's supposed to
        // recover from a *different* thread's poisoning, the first time
        // this loop is non-empty.
        drop(wm_core_guard);
        // First production call site for `ensure_pinned_terminal_spawned`
        // (dormant since Story 1.5): run after the seat loop, not inside
        // it, so this borrow starts only once `wm_core`'s mutable borrow
        // above has ended (NLL). Multiple seats cycling onto the same tag
        // in one pass is harmless — `claim_pinned_terminal_spawn` is
        // already idempotent (Story 1.5), so a duplicate entry here just
        // resolves to a no-op `Ok(None)` on the second call (YAGNI: no
        // dedup needed).
        for tag_id in pending_terminal_spawns {
            self.ensure_pinned_terminal_spawned(tag_id);
        }
    }
}

impl Window {
    fn new(proxy: RiverWindowV1, qh: &QueueHandle<AppData>) -> Self {
        let node = proxy.get_node(qh, ());
        Window {
            proxy,
            node,
            new: true,
            closed: false,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            pointer_move_requested: None,
            pointer_resize_requested: None,
            pointer_resize_requested_edges: Edges::None,
            app_id: String::new(),
            view_id: None,
        }
    }

    fn set_position(&mut self, x: i32, y: i32) {
        self.node.set_position(x, y);
        self.x = x;
        self.y = y;
    }
}

impl Output {
    fn new(proxy: RiverOutputV1, output_id: OutputId) -> Self {
        Self {
            proxy,
            removed: false,
            output_id,
        }
    }
}

/// Code review follow-up (Story 2.2, finding #1): resolves the `tag-picker`
/// binary's path as a sibling of the WM's own running executable, rather
/// than trusting `$PATH` — nothing in this repo installs the built
/// `tag-picker` binary onto `PATH`, and both binaries land in the same
/// Cargo workspace `target/{profile}/` directory, so `wm_exe`'s parent
/// directory is exactly where `tag-picker` lives too. Falls back to the
/// bare name if `wm_exe` unexpectedly has no parent (e.g. a bare filename
/// with no directory component) — `Command::spawn()` will then fail the
/// same `$PATH`-dependent way the old code always did, handled by the
/// existing error-logging call site rather than invented here.
fn tag_picker_path(wm_exe: &Path) -> PathBuf {
    match wm_exe.parent() {
        Some(dir) => dir.join("tag-picker"),
        None => PathBuf::from("tag-picker"),
    }
}

/// Spawns the pinned terminal: `foot -a pinned-term zellij attach --create
/// <session_name>`. Same `WAYLAND_DEBUG` removal and `Ok`/`Err` handling as
/// `Seat::do_action`'s `Action::SpawnFoot` arm (consistency, not
/// reinvention). Arguments are passed individually to `Command`, not
/// through a shell, so arbitrary tag names in `session_name` carry no
/// shell-injection risk regardless of their contents.
// Called from `ensure_pinned_terminal_spawned`, which gained its own
// production call site in `manage_seats` in Story 1.7.
fn spawn_pinned_terminal(session_name: &str) {
    match std::process::Command::new("foot")
        .arg("-a")
        .arg(PINNED_TERM_APP_ID)
        .arg("zellij")
        .arg("attach")
        .arg("--create")
        .arg(session_name)
        .env_remove("WAYLAND_DEBUG")
        .spawn()
    {
        Ok(_) => {}
        Err(e) => eprintln!("Failed to spawn pinned terminal: {e}"),
    }
}

impl Seat {
    fn new(proxy: RiverSeatV1) -> Self {
        Self {
            proxy,
            new: true,
            removed: false,
            focused: None,
            hovered: None,
            interacted: None,
            xkb_bindings: HashMap::new(),
            pointer_bindings: HashMap::new(),
            pending_action: Action::None,
            op: SeatOp::None,
            op_dx: 0,
            op_dy: 0,
            op_release: false,
        }
    }

    fn create_xkb_binding(
        &mut self,
        river_xkb: &RiverXkbBindingsV1,
        qh: &QueueHandle<AppData>,
        mods: Modifiers,
        keysym: u32,
        action: Action,
    ) {
        let proxy = river_xkb.get_xkb_binding(&self.proxy, keysym, mods, qh, self.proxy.id());
        proxy.enable();
        let binding = XkbBinding { proxy, action };
        self.xkb_bindings.insert(binding.proxy.id(), binding);
    }

    fn create_pointer_binding(
        &mut self,
        qh: &QueueHandle<AppData>,
        mods: Modifiers,
        button: u32,
        action: Action,
    ) {
        let proxy = self
            .proxy
            .get_pointer_binding(button, mods, qh, self.proxy.id());
        proxy.enable();
        let binding = PointerBinding { proxy, action };
        self.pointer_bindings.insert(binding.proxy.id(), binding);
    }

    /// Executes `self.pending_action`, returning `Some(tag_id)` when the
    /// action just switched the active output onto `tag_id` — the signal
    /// `manage_seats` uses, after this seat loop ends, to ensure that tag's
    /// pinned terminal is spawned (`WindowManager::ensure_pinned_terminal_spawned`).
    /// Every other arm returns `None`. `active_output_id` is the
    /// deterministic "active output" `Action::TagCycle` acts on (see
    /// `WindowManager::active_output_id`); `Action::TagCreate` never
    /// switches any output (AC: "creating a tag does not switch any output
    /// to it"), so it never returns `Some`.
    fn do_action(
        &mut self,
        windows: &mut VecDeque<Window>,
        wm_proxy: &RiverWindowManagerV1,
        wm_core: &mut WmCore,
        active_output_id: Option<OutputId>,
    ) -> Option<TagId> {
        let pending_action = self.pending_action;
        self.pending_action = Action::None;
        match pending_action {
            Action::None => None,
            // Don't pass WAYLAND_DEBUG on to children, the added noise makes
            // debugging the window manager itself impractical.
            Action::SpawnFoot => {
                match std::process::Command::new("foot")
                    .env_remove("WAYLAND_DEBUG")
                    .spawn()
                {
                    Ok(_) => {}
                    Err(e) => eprintln!("Failed to spawn foot: {e}"),
                }
                None
            }
            Action::Close => {
                // Check the pinned-terminal exclusion against this seat's
                // own real focus target (self.focused's Window.app_id),
                // not wm_core.closable_focused_view()'s single, WM-wide
                // focused_view — with multiple seats, the global field can
                // reflect a *different* seat's focus by the time this runs
                // (last-seat-processed-in-manage_seats wins), which could
                // let the pinned terminal be closed via this seat's own
                // request even though it isn't this seat's real focus, or
                // could spuriously block a legitimate close. Looking the
                // window up in `windows` and reading its own `app_id`
                // keeps the decision local and per-seat-correct regardless
                // of seat count or wm_core's global focus state (Story 1.4
                // code-review follow-up).
                if let Some(window_proxy) = self.focused.as_ref() {
                    let is_pinned_terminal = windows
                        .iter()
                        .find(|window| &window.proxy == window_proxy)
                        .is_some_and(|window| window.app_id == PINNED_TERM_APP_ID);
                    if !is_pinned_terminal {
                        window_proxy.close();
                    }
                }
                None
            }
            Action::FocusNext => {
                // wm_core.cycle_focus()'s returned ViewId is the source of
                // truth for which window to focus next — look it up in
                // `windows` and move it to the back (real z-order) rather
                // than independently rotating `windows` and letting the
                // two mechanisms diverge (Story 1.4 code-review
                // follow-up). `focus_top` then issues the real
                // focus_window/place_top proxy calls against
                // `windows.back()`, which is now guaranteed to be the same
                // window cycle_focus just chose.
                if let Some(next_view_id) = wm_core.cycle_focus()
                    && let Some(i) = windows
                        .iter()
                        .position(|window| window.view_id == Some(next_view_id))
                {
                    // Defensive guard (Story 1.5 code review follow-up,
                    // finding #2): `wm_core::state::cycle_focus` already
                    // excludes the pinned terminal from its candidates, so
                    // `next_view_id` should never actually resolve to it —
                    // but check the real `Window`'s own `app_id` here too,
                    // so this arm is correct on its own terms rather than
                    // correct only by accident of `cycle_focus`'s behavior
                    // elsewhere. If it somehow did resolve to the pinned
                    // terminal, skip the `windows.remove`/`push_back`/
                    // `place_top()` reorder (FR4: always bottom) but still
                    // give it real keyboard focus, same direct-focus
                    // pattern click-to-focus uses in `manage_seats`.
                    if windows[i].app_id == PINNED_TERM_APP_ID {
                        let window = &windows[i];
                        self.proxy.focus_window(&window.proxy);
                        self.focused = Some(window.proxy.clone());
                        if let Err(e) = wm_core.set_focus(next_view_id) {
                            eprintln!(
                                "Failed to set focus for view {next_view_id:?} in wm_core: {e:?}"
                            );
                        }
                    } else {
                        let window = windows.remove(i).unwrap();
                        windows.push_back(window);
                        self.focus_top(windows, wm_core);
                    }
                }
                None
            }
            Action::Move => {
                if let (Some(window_proxy), SeatOp::None) = (self.hovered.as_ref(), &self.op) {
                    let window = windows
                        .iter()
                        .find(|window| &window.proxy == window_proxy)
                        .expect("Hovered window not found");
                    self.pointer_move(window);
                }
                None
            }
            Action::Resize => {
                if let (Some(window_proxy), SeatOp::None) = (self.hovered.as_ref(), &self.op) {
                    let window = windows
                        .iter()
                        .find(|window| &window.proxy == window_proxy)
                        .expect("Hovered window not found");
                    self.pointer_resize(window, Edges::Bottom.union(Edges::Right));
                }
                None
            }
            Action::Exit => {
                wm_proxy.exit_session();
                None
            }
            Action::TagCycle => match active_output_id {
                Some(output_id) => match wm_core.cycle_tag(output_id) {
                    Ok(Some(tag_id)) => Some(tag_id),
                    Ok(None) => None,
                    Err(e) => {
                        eprintln!("Failed to cycle tag on output {output_id:?}: {e:?}");
                        None
                    }
                },
                None => {
                    eprintln!("Tag-cycle keybind pressed but no output is registered yet");
                    None
                }
            },
            Action::TagCreate => {
                if let Err(e) = wm_core.create_tag_with_generated_name() {
                    eprintln!("Failed to create tag: {e:?}");
                }
                None
            }
            // Story 2.2: fire-and-forget process spawn, no `wm_core` access.
            // `tag-picker` resolves the focused view itself via its own
            // `get-state` IPC call, so this arm never switches an output's
            // active tag (same reasoning `Action::TagCreate`'s own `None`
            // return already documents) and thus never triggers
            // `manage_seats`' pinned-terminal-spawn signal.
            //
            // Code review follow-up (finding #1): spawning `"tag-picker"` by
            // bare name relied on `$PATH`, but nothing in this repo installs
            // the built binary there — in a real session this silently
            // ENOENTs and `Mod4+A` does nothing. Resolve the sibling
            // binary's path relative to the WM's own running executable
            // instead (`tag_picker_path`); if `current_exe()` itself fails,
            // log and skip spawning rather than guessing a path or
            // panicking (NFR2). Once resolved, the spawn/error-handling
            // shape is otherwise byte-for-byte the same as
            // `Action::SpawnFoot`'s above.
            Action::OpenTagPicker => {
                match std::env::current_exe() {
                    Ok(wm_exe) => {
                        match std::process::Command::new(tag_picker_path(&wm_exe))
                            .env_remove("WAYLAND_DEBUG")
                            .spawn()
                        {
                            Ok(_) => {}
                            Err(e) => eprintln!("Failed to spawn tag-picker: {e}"),
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to resolve wm's own executable path: {e}")
                    }
                }
                None
            }
        }
    }

    fn op_end(&mut self) {
        if let SeatOp::Resize { window_proxy, .. } = &self.op {
            window_proxy.inform_resize_end();
        }
        self.proxy.op_end();
        self.op = SeatOp::None;
    }

    fn op_manage(&mut self) {
        match &self.op {
            SeatOp::None | SeatOp::Move { .. } => {}
            SeatOp::Resize {
                window_proxy,
                start_width,
                start_height,
                edges,
                ..
            } => {
                let (mut width, mut height) = (*start_width, *start_height);
                if edges.contains(Edges::Left) {
                    width -= self.op_dx;
                }
                if edges.contains(Edges::Right) {
                    width += self.op_dx;
                }
                if edges.contains(Edges::Top) {
                    height -= self.op_dy;
                }
                if edges.contains(Edges::Bottom) {
                    height += self.op_dy;
                }
                window_proxy.propose_dimensions(width.max(1), height.max(1));
            }
        }
    }

    fn focus_top(&mut self, windows: &VecDeque<Window>, wm_core: &mut WmCore) {
        match windows.back() {
            Some(window) => {
                self.proxy.focus_window(&window.proxy);
                window.node.place_top();
                self.focused = Some(window.proxy.clone());
                let view_id = window.view_id.expect(
                    "every window reaches focus_top only after init_new_windows registered it earlier in the same handle_manage_start call",
                );
                // set_focus is expected to succeed here: view_id was
                // registered in wm_core by init_new_windows earlier in the
                // same handle_manage_start call, so it should not be
                // unknown to wm_core; log rather than silently swallow an
                // Err, so a future regression that does hit it stays
                // visible (NFR2, error propagation), same pattern as
                // remove_windows' unregister_view logging.
                if let Err(e) = wm_core.set_focus(view_id) {
                    eprintln!("Failed to set focus for view {view_id:?} in wm_core: {e:?}");
                }
            }
            None => {
                self.proxy.clear_focus();
                self.focused = None;
                wm_core.clear_focus();
            }
        }
    }

    fn pointer_move(&mut self, window: &Window) {
        self.interacted = Some(window.proxy.clone());
        self.proxy.op_start_pointer();
        self.op = SeatOp::Move {
            window_proxy: window.proxy.clone(),
            start_x: window.x,
            start_y: window.y,
        };
        self.op_dx = 0;
        self.op_dy = 0;
    }

    fn pointer_resize(&mut self, window: &Window, edges: Edges) {
        self.interacted = Some(window.proxy.clone());
        self.proxy.op_start_pointer();
        window.proxy.inform_resize_start();
        self.op = SeatOp::Resize {
            window_proxy: window.proxy.clone(),
            start_x: window.x,
            start_y: window.y,
            start_width: window.width,
            start_height: window.height,
            edges,
        };
        self.op_dx = 0;
        self.op_dy = 0;
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for AppData {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            const RIVER_WINDOW_MANAGER_V1_VERSION: u32 = 4;
            const RIVER_XKB_BINDINGS_V1_VERSION: u32 = 1;
            match interface.as_str() {
                "river_window_manager_v1" => {
                    if version < RIVER_WINDOW_MANAGER_V1_VERSION {
                        eprintln!(
                            "Server river_window_manager_v1 v{version}, but we need at least v{RIVER_WINDOW_MANAGER_V1_VERSION}",
                        );
                        std::process::exit(1);
                    }
                    let wm = registry.bind::<RiverWindowManagerV1, _, _>(
                        name,
                        RIVER_WINDOW_MANAGER_V1_VERSION,
                        qh,
                        (),
                    );
                    state.river_wm = Some(wm);
                }
                "river_xkb_bindings_v1" => {
                    if version < RIVER_XKB_BINDINGS_V1_VERSION {
                        eprintln!(
                            "Server supports river_xkb_bindings_v1 v{version}, but we need at least v{RIVER_XKB_BINDINGS_V1_VERSION}"
                        );
                        std::process::exit(1);
                    }
                    let xkb = registry.bind::<RiverXkbBindingsV1, _, _>(
                        name,
                        RIVER_XKB_BINDINGS_V1_VERSION,
                        qh,
                        (),
                    );
                    state.river_xkb = Some(xkb);
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<RiverWindowManagerV1, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverWindowManagerV1,
        event: <RiverWindowManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use river::river_window_manager_v1::Event;
        match event {
            Event::Unavailable => {
                eprintln!("Error: Another WM is already running");
                std::process::exit(1);
            }
            Event::Finished => std::process::exit(0),
            Event::ManageStart => {
                let river_xkb = state
                    .river_xkb
                    .as_ref()
                    .expect("river_xkb_bindings_v1 missing");
                state.wm.handle_manage_start(proxy, river_xkb, qh)
            }
            Event::RenderStart => state.wm.handle_render_start(proxy),
            Event::SessionLocked => {}
            Event::SessionUnlocked => {}
            Event::Window { id } => state.wm.windows.push_back(Window::new(id, qh)),
            Event::Output { id } => {
                let output_id = ipc::lock_recovering(&state.wm.wm_core).register_output();
                state.wm.outputs.insert(id.id(), Output::new(id, output_id));
            }
            Event::Seat { id } => {
                state.wm.seats.insert(id.id(), Seat::new(id));
            }
        }
    }

    wayland_client::event_created_child!(AppData, RiverWindowManagerV1, [
        river::river_window_manager_v1::EVT_WINDOW_OPCODE => (RiverWindowV1, ()),
        river::river_window_manager_v1::EVT_OUTPUT_OPCODE => (RiverOutputV1, ()),
        river::river_window_manager_v1::EVT_SEAT_OPCODE => (RiverSeatV1, ())
    ]);
}

impl Dispatch<RiverWindowV1, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverWindowV1,
        event: <RiverWindowV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use river::river_window_v1::Event;
        let window = match state.wm.windows.iter_mut().find(|o| &o.proxy == proxy) {
            Some(window) => window,
            None => return,
        };
        match event {
            Event::Closed => window.closed = true,
            Event::DimensionsHint {
                min_width: _,
                min_height: _,
                max_width: _,
                max_height: _,
            } => {}
            Event::Dimensions { width, height } => (window.width, window.height) = (width, height),
            // Protocol allows app_id == null (window never set one, or
            // cleared it); treat that the same as Window's own default of
            // an empty string rather than panicking (NFR2).
            Event::AppId { app_id } => window.app_id = app_id.unwrap_or_default(),
            Event::Title { title: _ } => {}
            Event::Parent { parent: _ } => {}
            Event::DecorationHint { hint: _ } => {}
            Event::PointerMoveRequested { seat } => window.pointer_move_requested = Some(seat),
            Event::PointerResizeRequested { seat, edges } => {
                window.pointer_resize_requested = Some(seat);
                window.pointer_resize_requested_edges =
                    edges.into_result().expect("Invalid edges for resize");
            }
            Event::ShowWindowMenuRequested { x: _, y: _ } => {}
            Event::MaximizeRequested => {}
            Event::UnmaximizeRequested => {}
            Event::FullscreenRequested { output: _ } => {}
            Event::ExitFullscreenRequested => {}
            Event::MinimizeRequested => {}
            Event::UnreliablePid { unreliable_pid: _ } => {}
            Event::PresentationHint { .. } => {}
            Event::Identifier { .. } => {}
        }
    }
}

impl Dispatch<RiverOutputV1, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverOutputV1,
        event: <RiverOutputV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use river::river_output_v1::Event;
        let output = state
            .wm
            .outputs
            .get_mut(&proxy.id())
            .expect("Output not found");
        match event {
            Event::Removed => output.removed = true,
            Event::WlOutput { name: _ } => {}
            Event::Position { x: _, y: _ } => {}
            Event::Dimensions {
                width: _,
                height: _,
            } => {}
        }
    }
}

impl Dispatch<RiverSeatV1, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverSeatV1,
        event: <RiverSeatV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use river::river_seat_v1::Event;
        let seat = state.wm.seats.get_mut(&proxy.id()).expect("Seat not found");
        match event {
            Event::Removed => seat.removed = true,
            Event::WlSeat { name: _ } => {}
            Event::PointerEnter { window } => seat.hovered = Some(window),
            Event::PointerLeave => seat.hovered = None,
            Event::WindowInteraction { window } => seat.interacted = Some(window),
            Event::ShellSurfaceInteraction {
                shell_surface: _shell_surface,
            } => {}
            Event::OpDelta { dx, dy } => (seat.op_dx, seat.op_dy) = (dx, dy),
            Event::OpRelease => seat.op_release = true,
            Event::PointerPosition { x: _, y: _ } => {}
        }
    }
}

impl Dispatch<RiverXkbBindingV1, ObjectId> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverXkbBindingV1,
        event: <RiverXkbBindingV1 as Proxy>::Event,
        data: &ObjectId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use river::river_xkb_binding_v1::Event;
        let seat = state.wm.seats.get_mut(data).expect("Seat not found");
        let binding = seat
            .xkb_bindings
            .get(&proxy.id())
            .expect("xkb_binding not found");
        match event {
            Event::Pressed => seat.pending_action = binding.action,
            Event::Released => {}
            Event::StopRepeat => {}
        }
    }
}

impl Dispatch<RiverPointerBindingV1, ObjectId> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverPointerBindingV1,
        event: <RiverPointerBindingV1 as Proxy>::Event,
        data: &ObjectId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use river::river_pointer_binding_v1::Event;
        let seat = state.wm.seats.get_mut(data).expect("Seat not found");
        let binding = seat
            .pointer_bindings
            .get(&proxy.id())
            .expect("xkb_binding not found");
        match event {
            Event::Pressed => seat.pending_action = binding.action,
            Event::Released => {}
        }
    }
}

wayland_client::delegate_noop!(AppData: ignore RiverXkbBindingsV1);
wayland_client::delegate_noop!(AppData: ignore RiverNodeV1);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Queue up a get_registry event.
    let conn = Connection::connect_to_env()?;
    let display = conn.display();
    let mut event_queue = conn.new_event_queue();
    let _registry = display.get_registry(&event_queue.handle(), ());

    // Initial state
    let mut app_data = AppData::default();

    // Roundtrip to process the get_registry event and bind interfaces.
    event_queue.roundtrip(&mut app_data)?;
    if app_data.river_wm.is_none() {
        eprintln!("river_window_manager_v1 global not found! Is river running?");
        std::process::exit(1);
    }
    if app_data.river_xkb.is_none() {
        eprintln!("river_xkb_bindings_v1 global not found! Is river running with xkb support?");
        std::process::exit(1);
    }

    // A failed IPC-server bind (e.g. a permissions issue) is logged and
    // does not abort WM startup: the WM's core job — managing windows —
    // must not depend on the IPC server. Deliberate degrade-not-crash
    // choice, consistent with NFR2's priority ordering, and mirrors this
    // file's existing `log_wm_core_err`-style "log, don't abort"
    // convention rather than `std::process::exit`, which this file
    // otherwise reserves for unrecoverable Wayland-protocol-level
    // failures only.
    let socket_path = ipc::server::default_socket_path();
    if let Err(e) = ipc::server::spawn(Arc::clone(&app_data.wm.wm_core), &socket_path) {
        eprintln!("Failed to start IPC server on {socket_path:?}: {e}");
    }

    loop {
        event_queue.blocking_dispatch(&mut app_data)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Code review follow-up (Story 2.2, finding #1): `tag_picker_path` is
    // the pure path-resolution logic pulled out of `Action::OpenTagPicker`'s
    // handler so it's testable without actually calling `current_exe()`.
    // The `current_exe()`/`Command::spawn()` call site itself stays
    // untested I/O glue, same carve-out as the rest of this file.

    #[test]
    fn tag_picker_path_resolves_to_sibling_of_debug_wm_exe() {
        assert_eq!(
            tag_picker_path(Path::new("/workspaces/buoy-wm/target/debug/wm")),
            PathBuf::from("/workspaces/buoy-wm/target/debug/tag-picker")
        );
    }

    #[test]
    fn tag_picker_path_resolves_to_sibling_of_release_wm_exe() {
        assert_eq!(
            tag_picker_path(Path::new("/workspaces/buoy-wm/target/release/wm")),
            PathBuf::from("/workspaces/buoy-wm/target/release/tag-picker")
        );
    }

    #[test]
    fn tag_picker_path_falls_back_to_bare_name_when_wm_exe_has_no_parent() {
        // `Path::parent()` returns `None` only when the path terminates in a
        // root or prefix (or is empty) — `/` is the concrete case that hits
        // the fallback branch, not just a single-component relative path
        // (whose `.parent()` is `Some("")`, which `join` already reduces to
        // the bare name anyway).
        assert_eq!(tag_picker_path(Path::new("/")), PathBuf::from("tag-picker"));
    }
}
