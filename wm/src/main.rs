// SPDX-FileCopyrightText: © 2026 Julian Andrews
// SPDX-License-Identifier: 0BSD

use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wayland_backend::client::ObjectId;
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    protocol::{wl_output, wl_registry},
};

use crate::river::{
    river_layer_shell_v1::RiverLayerShellV1,
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

        pub(super) mod rlayer {
            use super::rwm::*;
            wayland_scanner::generate_interfaces!("./protocol/river-layer-shell-v1.xml");
        }
    }

    use self::interfaces::rlayer::*;
    use self::interfaces::rwm::*;
    use self::interfaces::rxkb::*;
    wayland_scanner::generate_client_code!("./protocol/river-window-management-v1.xml");
    wayland_scanner::generate_client_code!("./protocol/river-xkb-bindings-v1.xml");
    wayland_scanner::generate_client_code!("./protocol/river-layer-shell-v1.xml");
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
    SpawnLauncher,
    ShowHotkeys,
    Close,
    FocusNext,
    Move,
    Resize,
    Exit,
    TagCycle,
    OpenTagPicker,
    TagSwitch,
}

/// Human-readable cheat-sheet shown by `Action::ShowHotkeys` (`Mod4+?`).
/// Hand-maintained alongside `init_new_seats`'s bindings below — there are
/// few enough of these that a shared declarative table isn't worth the
/// indirection (YAGNI); keep this list in sync when adding a binding.
const HOTKEY_HELP: &[&str] = &[
    "Mod4+Space       Spawn terminal (foot)",
    "Mod4+R           App launcher (fuzzel)",
    "Mod4+Q           Close focused window",
    "Mod4+N           Cycle focus",
    "Mod4+Tab         Cycle tag",
    "Mod4+A           Tag manager (assign tags to focused window)",
    "Mod4+S           Switch tag",
    "Mod4+?           Show this hotkey list",
    "Mod4+Esc         Exit session",
    "Mod4+LeftClick   Move window",
    "Mod4+RightClick  Resize window",
];

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
    /// Story 2.6: optional, unlike `river_xkb` - binding this global is what
    /// tells `river` to allow layer-shell clients (eg waybar's status bar)
    /// to map surfaces at all, instead of closing them immediately. Absence
    /// is not fatal; it just means layer-shell surfaces can't map, same as
    /// `buoy-wm`'s behavior before this story.
    river_layer_shell: Option<RiverLayerShellV1>,
    wm: WindowManager,
}

#[derive(Debug, Default)]
struct WindowManager {
    windows: VecDeque<Window>,
    outputs: HashMap<ObjectId, Output>,
    seats: HashMap<ObjectId, Seat>,
    wm_core: Arc<Mutex<WmCore>>,
    /// Story 2.9 Task 1: every bound `wl_output` global, keyed by the
    /// registry `name: u32` it was bound for — populated as soon as each
    /// global is advertised, independent of (and generally *before*) the
    /// `river_output_v1::WlOutput` event that later references the same
    /// registry name (Task 2). Never removed; a `wl_output` global going
    /// away isn't handled by this story (see Technical notes' scope
    /// boundary) — YAGNI until a real hotplug-removal story needs it.
    pending_wl_outputs: HashMap<u32, wl_output::WlOutput>,
    /// Story 2.9 Task 1: each bound `wl_output`'s real connector name (e.g.
    /// `"eDP-1"`), keyed by that `wl_output` proxy's own id, populated
    /// whenever its `Name` event fires. Deliberately a separate map from
    /// `pending_wl_outputs` (keyed by object id, not registry name) so a
    /// name can be looked up fresh at the point it's actually needed
    /// (`WindowManager::output_name`) without ever caching "no name yet" as
    /// a permanent negative — the event's arrival time relative to
    /// `river_output_v1::WlOutput` is not guaranteed (Technical notes).
    wl_output_names: HashMap<ObjectId, String>,
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
    /// The output's top-left corner in the compositor's global coordinate
    /// space, from `river_output_v1`'s `position` event. Defaults to
    /// `(0, 0)` until the first such event arrives (Story 2.8 Task 1) —
    /// same "plain data field, no decision logic of its own" carve-out as
    /// every other all-zero-initialized field on this struct.
    position: (i32, i32),
    /// The output's width/height extent from `position`, from
    /// `river_output_v1`'s `dimensions` event. Defaults to `(0, 0)` until
    /// the first such event arrives (Story 2.8 Task 1).
    dimensions: (i32, i32),
    /// The id of this output's correlated `wl_output` proxy (Story 2.9 Task
    /// 2), set once `river_output_v1`'s `WlOutput` event resolves the
    /// registry name it carries to a `wl_output` binding in
    /// `WindowManager::pending_wl_outputs`. `None` until that event
    /// arrives, or permanently if the correlation somehow fails (AC 2's
    /// documented fallback). Deliberately just the object id, not the real
    /// connector name string directly — the corresponding `wl_output`'s own
    /// `Name` event isn't guaranteed to have arrived yet at correlation
    /// time (Technical notes), so the name itself must always be looked up
    /// fresh from `WindowManager::wl_output_names` when actually needed
    /// (`output_name`), never cached here.
    wl_output_object_id: Option<ObjectId>,
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
    /// The pointer's last-known position in the compositor's global
    /// coordinate space, from `river_seat_v1`'s `pointer_position` event
    /// (protocol `since="2"`). `None` until the first such event arrives
    /// for this seat (Story 2.8 Task 2) — the AC 2 fallback case
    /// `WindowManager::active_output_id` handles explicitly.
    pointer_position: Option<(i32, i32)>,
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
        // Story 2.7 Task 5.2: re-resolve fullscreen assignment for every
        // pinned terminal after `manage_seats` may have switched a tag onto
        // a different output (or off every output) via a raw keybind -
        // still within this same manage sequence, satisfying
        // `river_window_v1.fullscreen`'s manage-sequence-only constraint.
        self.recompute_pinned_terminal_fullscreen();
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

        // Story 2.7 Task 3: `river_window_v1.hide`/`show` "modif[y]
        // rendering state and may only be made as part of a render
        // sequence" per the protocol, so this visibility recompute belongs
        // here, not in `handle_manage_start`.
        self.recompute_window_visibility();

        proxy.render_finish();
    }

    /// Story 2.8 Task 5: closes the gap Story 1.7's Dev Agent Record
    /// flagged - removing a real output previously never reached
    /// `wm_core`, leaving a permanent ghost entry eligible forever after to
    /// be selected by `active_output_id`. Sequencing matters here: the
    /// `wm_core` removal pass (this method's first block) must finish, and
    /// its lock must be released, *before* `active_output_id()` is called
    /// below - `active_output_id` reads `self.outputs`, which must already
    /// reflect only the post-removal survivor set (Task 3), and
    /// `std::sync::Mutex` is not reentrant (same "lock, mutate, drop, fresh
    /// borrow" pattern `manage_seats` already establishes for this reason).
    fn remove_outputs(&mut self) {
        let mut orphaned_tags: Vec<TagId> = Vec::new();
        {
            let mut wm_core = ipc::lock_recovering(&self.wm_core);
            let wl_output_names = &mut self.wl_output_names;
            self.outputs.retain(|_, output| {
                if output.removed {
                    output.proxy.destroy();
                    // Code review follow-up (Story 2.9): prune the
                    // matching `wl_output_names` entry too, mirroring the
                    // `wm_core.unregister_output` cleanup just below for
                    // the same removal event — otherwise this map grows
                    // without bound across dock/undock cycles for the life
                    // of this long-running process.
                    if let Some(wl_output_object_id) = &output.wl_output_object_id {
                        wl_output_names.remove(wl_output_object_id);
                    }
                    // This output_id was registered in wm_core the moment
                    // the real output appeared (Event::Output) and only
                    // this call site ever removes it, so `Err(UnknownOutput)`
                    // is believed structurally unreachable; log rather than
                    // silently swallow, so a future regression stays
                    // visible (NFR2), same pattern as remove_windows'
                    // unregister_view call.
                    match wm_core.unregister_output(output.output_id) {
                        Ok(Some(tag_id)) => orphaned_tags.push(tag_id),
                        Ok(None) => {}
                        Err(e) => eprintln!(
                            "Failed to unregister output {:?} from wm_core: {e:?}",
                            output.output_id
                        ),
                    }
                    return false;
                }
                true
            });
        }

        // Reroute any tag that was displayed on a just-removed output onto
        // the (post-removal) active output, reusing `switch_tag` - the same
        // ADR-005-enforcing mutator every other tag-assignment path already
        // goes through, not a second, parallel implementation. If no output
        // survives, `active_output_id()` returns `None` and the orphaned
        // tag(s) are simply displayed nowhere, which `is_view_visible`
        // already handles correctly (hidden, not a crash) - AC 5.
        //
        // Code review follow-up (Story 2.8): `switch_tag` unconditionally
        // overwrites its target output's `current_tag` - calling it
        // whenever *any* output survives, without checking whether that
        // output already had its own tag displayed, would silently destroy
        // whatever the user was actually looking at (e.g. closing the
        // laptop lid while docked would blow away the external monitor's
        // current tag and replace it with the laptop's orphaned one). Only
        // reroute onto the active output if it isn't already displaying
        // something; otherwise leave the orphaned tag displayed nowhere,
        // same safe "hidden, not a crash" outcome as the no-survivor case.
        if let Some(active_output_id) = self.active_output_id() {
            let mut wm_core = ipc::lock_recovering(&self.wm_core);
            // Accepted, narrow edge case (documented rather than silently
            // mishandled, mirroring Story 2.7's documented-FIFO-race
            // precedent): if more than one output was removed in the same
            // pass and each had a different displayed tag, only the first
            // one processed finds the active output still empty and gets
            // placed there - every subsequent one then sees it occupied
            // (by the one just placed) and is left displayed nowhere,
            // rather than blindly overwriting it.
            for tag_id in orphaned_tags {
                if wm_core.output_current_tag(active_output_id).is_some() {
                    continue;
                }
                log_wm_core_err(
                    wm_core.switch_tag(active_output_id, tag_id),
                    "Failed to reroute orphaned tag onto active output",
                );
            }
        }
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
        // Computed before the `&mut self.wm_core` borrow below begins, same
        // reasoning as `manage_seats`'s own `active_output_id` precedent
        // (Story 1.7): `active_output_id()` is a whole-`&self` method call
        // (it reads `self.outputs`), which cannot run during the loop's
        // mutable `wm_core` borrow even though the fields are disjoint.
        // Story 2.7 Task 4: this is the "active output" a freshly-created
        // non-pinned window auto-tags onto.
        let active_output_id = self.active_output_id();
        let mut wm_core = ipc::lock_recovering(&self.wm_core);
        for window in self.windows.iter_mut().filter(|w| w.new) {
            let view_id = wm_core.register_view(&window.app_id);
            window.view_id = Some(view_id);
            if window.app_id == PINNED_TERM_APP_ID {
                log_wm_core_err(
                    wm_core.set_view_floating(view_id, false),
                    "Failed to set pinned terminal non-floating",
                );
                log_wm_core_err(
                    wm_core.lower_view(view_id),
                    "Failed to lower pinned terminal in stacking order",
                );
                // `lower_view` above only updates `wm_core`'s own abstract
                // bookkeeping (Story 1.5) - it never touches the real
                // scene-graph node. Without this call the pinned terminal's
                // freshly-mapped node lands wherever the compositor's
                // default insertion order puts it (often near the top),
                // which is why floating windows mapped before it could end
                // up rendered *below* it despite their own `place_top()`
                // call further down in this function (FR4/FR5).
                window.node.place_bottom();
                // Story 2.7 Task 2.3: recover which tag this pinned
                // terminal was spawned for (the protocol gives no way to
                // know from the mapped window alone - it only carries
                // `app_id == PINNED_TERM_APP_ID`). Popping from an empty
                // queue (no corresponding pending spawn) shouldn't happen
                // given the spawn-then-map ordering, but is handled
                // gracefully rather than panicking (NFR2): log and leave
                // the window untagged, which Task 1's bootstrap exception
                // (`is_view_visible`) keeps visible rather than
                // permanently hidden.
                match wm_core.pop_pending_pinned_terminal_tag() {
                    Some(tag_id) => {
                        log_wm_core_err(
                            wm_core.toggle_view_tag(view_id, tag_id),
                            "Failed to tag newly-mapped pinned terminal",
                        );
                        // Story 2.7 Task 5.1: fullscreen is window
                        // management state and may only be requested as
                        // part of a manage sequence (per the protocol's
                        // own `river_window_v1.fullscreen` description) -
                        // `init_new_windows` runs inside
                        // `handle_manage_start`, so this is the correct
                        // place for it. If the tag isn't currently shown
                        // on any output, do nothing here: Task 3's
                        // render-sequence visibility pass
                        // (`recompute_window_visibility`) will hide it via
                        // the same `is_view_visible` decision every other
                        // view uses, rather than this call site
                        // special-casing a hide.
                        if let Some(output_id) = wm_core.output_showing_tag(tag_id)
                            && let Some(output_proxy) =
                                output_proxy_for_id(&self.outputs, output_id)
                        {
                            window.proxy.fullscreen(output_proxy);
                        }
                    }
                    None => {
                        eprintln!(
                            "Pinned terminal window mapped with no pending spawn tag queued; leaving untagged"
                        );
                        // Code review follow-up (Story 2.7): an untagged
                        // window is shown by `is_view_visible`'s bootstrap
                        // exception, so give it the same explicit
                        // position/dimensions the non-pinned branch below
                        // gives every new window, rather than leaving it
                        // with whatever undefined geometry the compositor
                        // happens to pick — this is a stray window (no
                        // known caller maps `app_id == PINNED_TERM_APP_ID`
                        // without going through this WM's own spawn
                        // tracking), but it must still render sanely if it
                        // ever occurs.
                        window
                            .set_position(DEFAULT_FLOATING_GEOMETRY.x, DEFAULT_FLOATING_GEOMETRY.y);
                        window.proxy.propose_dimensions(
                            DEFAULT_FLOATING_GEOMETRY.width,
                            DEFAULT_FLOATING_GEOMETRY.height,
                        );
                    }
                }
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
                // Story 2.7 Task 4: auto-tag a freshly-created (non-pinned)
                // window with the active output's current tag (dwm
                // convention) so it's immediately visible on the tag the
                // user is looking at, rather than silently invisible until
                // manually tagged via the assign-mode picker. If no output
                // is registered yet, or the active output has no current
                // tag yet (e.g. no tag exists at all), leave the window
                // untagged - Task 1's bootstrap exception keeps it visible.
                if let Some(output_id) = active_output_id
                    && let Some(tag_id) = wm_core.output_current_tag(output_id)
                {
                    log_wm_core_err(
                        wm_core.toggle_view_tag(view_id, tag_id),
                        "Failed to auto-tag new window with active output's current tag",
                    );
                }
            }
            window.new = false;
        }
    }

    /// Story 2.7 Task 5.2: re-resolves and re-applies fullscreen/exit-
    /// fullscreen for every mapped pinned terminal, one per tag it's been
    /// associated with (Task 2). Called unconditionally at the end of every
    /// `handle_manage_start`, the same "simplest-correct first cut, recompute
    /// broadly rather than track precisely which tag-state change to react
    /// to" strategy `recompute_window_visibility` uses for hide/show (Task
    /// 3) - the two recomputations can't be the same function because the
    /// protocol restricts `fullscreen`/`exit_fullscreen` to a manage
    /// sequence (this method's caller) while `show`/`hide` are restricted to
    /// a render sequence (`recompute_window_visibility`'s caller). This
    /// covers `switch_tag`/`cycle_tag` reassigning a tag to a different
    /// output (or off every output) regardless of whether that happened via
    /// the raw keybind (`manage_seats`, same manage sequence as this call)
    /// or asynchronously via the IPC-driven picker (picked up the next time
    /// any manage sequence runs - see `WmCore::pending_pinned_terminal_tags`'s
    /// doc comment for why that's the shared, cross-thread-correct queue).
    fn recompute_pinned_terminal_fullscreen(&mut self) {
        let wm_core = ipc::lock_recovering(&self.wm_core);
        for window in self.windows.iter() {
            if window.app_id != PINNED_TERM_APP_ID {
                continue;
            }
            let Some(view_id) = window.view_id else {
                continue;
            };
            let tag_id = match wm_core.view_tags(view_id) {
                Ok(tags) => tags.first().copied(),
                Err(e) => {
                    eprintln!(
                        "Failed to resolve pinned terminal's tags for view {view_id:?}: {e:?}"
                    );
                    None
                }
            };
            match tag_id.and_then(|tag_id| wm_core.output_showing_tag(tag_id)) {
                Some(output_id) => {
                    if let Some(output_proxy) = output_proxy_for_id(&self.outputs, output_id) {
                        window.proxy.fullscreen(output_proxy);
                    }
                }
                // Not in scope (see this story's Technical notes): the
                // brief non-fullscreen frame this may cause immediately
                // before Task 3's next render sequence hides the window is
                // accepted, not tuned away - correctness (it ends up
                // hidden, not stuck fullscreen-but-invisible) matters more
                // here than frame-perfection.
                None => window.proxy.exit_fullscreen(),
            }
        }
    }

    /// Story 2.7 Task 3: the broad visibility recomputation pass - for
    /// every mapped window, decides show vs. hide via `WmCore::
    /// is_view_visible` (Task 1's pure decision) and issues the matching
    /// `river_window_v1` request. `hide`/`show` "modif[y] rendering state
    /// and may only be made as part of a render sequence" per the
    /// protocol, so this is called from `handle_render_start`, not
    /// `handle_manage_start` - the same manage-vs-render sequence
    /// constraint Story 1.4/1.6's `set_position`/`place_top` calls satisfy
    /// by being called from within the correct sequence's handler. Runs
    /// unconditionally every render sequence (simplest-correct first cut
    /// per this story's Task 3.1: NFR1's 50ms budget isn't at risk for a
    /// handful of windows) rather than tracking precisely which tag/output
    /// mutation call site needs to trigger it - this covers every case in
    /// the AC's list (`toggle_view_tag` via the assign-mode picker or
    /// Task 4's auto-tag, `switch_tag`/`cycle_tag` via keybind or the
    /// switch-mode picker, window registration/removal) uniformly.
    fn recompute_window_visibility(&mut self) {
        let wm_core = ipc::lock_recovering(&self.wm_core);
        for window in self.windows.iter() {
            let Some(view_id) = window.view_id else {
                continue;
            };
            match wm_core.is_view_visible(view_id) {
                Ok(true) => window.proxy.show(),
                Ok(false) => window.proxy.hide(),
                Err(e) => {
                    eprintln!("Failed to resolve visibility for view {view_id:?}: {e:?}")
                }
            }
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
        // Story 2.2 gap #4: `Mod4+A` ("Assign") opens the tag-manager
        // picker, following this file's existing single-letter-mnemonic
        // convention.
        const A: u32 = 0x61;
        // Story 2.4: `Mod4+S` ("Switch") opens the same picker in
        // switch mode, same single-letter-mnemonic convention as `A`.
        const S: u32 = 0x73;
        // `Mod4+R` ("Run") launches fuzzel's own desktop-entry launcher
        // mode, same single-letter-mnemonic convention as `A`/`S`.
        const R: u32 = 0x72;
        // `Mod4+Shift+?` shows the hotkey cheat-sheet. Unlike the other
        // bindings above, the keysym here is the *shifted* symbol the
        // layout actually produces when Shift is held (xkbcommon has no
        // separate unshifted `?` keysym) — `Modifiers` must include
        // `shift` too, or this binding would never match the real
        // Shift-held keysym the compositor reports.
        const QUESTION: u32 = 0x3f;
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
                seat.create_xkb_binding(river_xkb, qh, mods, A, Action::OpenTagPicker);
                seat.create_xkb_binding(river_xkb, qh, mods, S, Action::TagSwitch);
                seat.create_xkb_binding(river_xkb, qh, mods, R, Action::SpawnLauncher);
                seat.create_xkb_binding(
                    river_xkb,
                    qh,
                    mods.union(Modifiers::Shift),
                    QUESTION,
                    Action::ShowHotkeys,
                );
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

    /// The "active output" for keybind-driven tag actions and new-window
    /// auto-tagging: the registered output whose rectangle
    /// (`position`/`dimensions`) contains a seat's last-known pointer
    /// position (Story 2.8) — the same "focused monitor follows mouse"
    /// convention dwm/i3/sway-style WMs use, and the protocol's only
    /// first-class always-current signal for this
    /// (`river_seat_v1.pointer_position`). If multiple seats disagree (an
    /// edge case this single-user WM practically never hits), the lowest
    /// matching `OutputId` wins — deterministic regardless of `self.seats`'
    /// `HashMap` iteration order (code review follow-up: a first-match-
    /// while-iterating approach here would silently depend on hash-bucket
    /// order despite claiming determinism), same standard ADR-005 already
    /// set for the fallback below.
    ///
    /// Falls back to the old Story 1.7 placeholder — the lowest-`OutputId`
    /// (first-registered) output — when no seat has yet reported a pointer
    /// position (startup, before the first `pointer_position` event), or
    /// when every known pointer position falls outside every currently-
    /// registered output's rectangle (e.g. geometry events haven't arrived
    /// yet for some output, or the pointer's last-known position referenced
    /// an output that's since been removed — see `remove_outputs`). This is
    /// no longer a documented permanent placeholder (Story 1.7's "no live
    /// protocol signal to compute from yet" no longer applies) — it's now
    /// the deterministic fallback for the genuinely-ambiguous/not-yet-known
    /// cases only.
    fn active_output_id(&self) -> Option<OutputId> {
        let pointed_output_id = self
            .seats
            .values()
            .filter_map(|seat| {
                let (px, py) = seat.pointer_position?;
                self.outputs.values().find(|output| {
                    let (ox, oy) = output.position;
                    let (ow, oh) = output.dimensions;
                    // Half-open bounds: `dimensions` is a width/height
                    // extent from `position`, so the rectangle's far edge
                    // (`position + dimensions`) is exclusive, matching how
                    // `river_output_v1`'s `position`/`dimensions` events
                    // are documented.
                    (ox..ox + ow).contains(&px) && (oy..oy + oh).contains(&py)
                })
            })
            .map(|output| output.output_id)
            .min();
        pointed_output_id.or_else(|| self.outputs.values().map(|o| o.output_id).min())
    }

    /// Story 2.9 Task 2: resolves a `wm-core` [`OutputId`] to its real
    /// Wayland connector name (e.g. `"eDP-1"`), for `fuzzel --output=` (Task
    /// 5). Same linear-scan-by-`output_id` shape `output_proxy_for_id`
    /// already uses (`outputs` is keyed by `ObjectId`, not `OutputId`).
    /// Returns `None` at any missing step — no matching `Output`, no
    /// correlated `wl_output` object yet (or ever), or that `wl_output`'s
    /// own `Name` event hasn't arrived yet — never panics (NFR2); AC 2's
    /// "no `--output` flag at all" fallback covers every `None` case
    /// uniformly. Always looks `wl_output_names` up fresh rather than
    /// caching a name on `Output` itself, since the `Name` event's arrival
    /// relative to correlation time is not guaranteed (Technical notes).
    fn output_name(&self, output_id: OutputId) -> Option<&str> {
        let output = self.outputs.values().find(|o| o.output_id == output_id)?;
        let wl_output_object_id = output.wl_output_object_id.as_ref()?;
        self.wl_output_names
            .get(wl_output_object_id)
            .map(String::as_str)
    }

    fn manage_seats(&mut self, wm_proxy: &RiverWindowManagerV1) {
        // Computed before the `&mut self.wm_core` borrow below begins:
        // `active_output_id` is a whole-`&self` method call (it reads
        // `self.outputs`), which cannot run *during* the loop's mutable
        // `wm_core` borrow even though the two fields are disjoint (Story
        // 1.7).
        let active_output_id = self.active_output_id();
        // Story 2.9 Task 3: resolved alongside `active_output_id` above,
        // before the `wm_core` mutable borrow below begins, same reasoning
        // — `output_name` is a whole-`&self` method call (it reads
        // `self.outputs`/`self.wl_output_names`). Owned (`String`, not
        // `&str`) so it outlives the borrow of `self` that `output_name`
        // itself requires, letting it be passed down as `&str` via
        // `.as_deref()` at the `do_action` call site below.
        let active_output_name = active_output_id
            .and_then(|id| self.output_name(id))
            .map(str::to_owned);
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
            if let Some(tag_id) = seat.do_action(
                &mut self.windows,
                wm_proxy,
                wm_core,
                active_output_id,
                active_output_name.as_deref(),
            ) {
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
            position: (0, 0),
            dimensions: (0, 0),
            wl_output_object_id: None,
        }
    }
}

/// Resolves the real `river_output_v1` proxy for a `wm-core` [`OutputId`],
/// by scanning `outputs`' values for a matching `Output::output_id` (the
/// map's own keys are `ObjectId`s, not `OutputId`s - `wm-core` never sees
/// Wayland object ids). A free function rather than a `WindowManager`
/// method (Story 2.7 Tasks 2/5): it borrows only the `outputs` field
/// directly, so callers already holding a disjoint mutable borrow of
/// `self.windows` (`init_new_windows`,
/// `recompute_pinned_terminal_fullscreen`) can call it without the whole-
/// `self` re-borrow a method call would require (same reasoning as
/// `active_output_id` needing to be computed before such a loop begins).
fn output_proxy_for_id(
    outputs: &HashMap<ObjectId, Output>,
    output_id: OutputId,
) -> Option<&RiverOutputV1> {
    outputs
        .values()
        .find(|output| output.output_id == output_id)
        .map(|output| &output.proxy)
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
            pointer_position: None,
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
    /// `WindowManager::active_output_id`).
    fn do_action(
        &mut self,
        windows: &mut VecDeque<Window>,
        wm_proxy: &RiverWindowManagerV1,
        wm_core: &mut WmCore,
        active_output_id: Option<OutputId>,
        // Story 2.9 Task 3: the active output's real Wayland connector name
        // (e.g. `"eDP-1"`), resolved by `manage_seats` alongside
        // `active_output_id` above via `WindowManager::output_name`. `None`
        // whenever that name isn't yet known (Technical notes) — both
        // `tag-picker` spawn arms below treat that the same as
        // `active_output_id` being `None`: append no extra argument at all,
        // rather than a bogus/empty one (AC 2).
        active_output_name: Option<&str>,
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
            // `Mod4+R`: same fire-and-forget spawn shape as `SpawnFoot`
            // above. Bare `fuzzel` (no `--dmenu`) runs its own built-in
            // desktop-entry launcher, so no argument wiring is needed.
            Action::SpawnLauncher => {
                match std::process::Command::new("fuzzel")
                    .env_remove("WAYLAND_DEBUG")
                    .spawn()
                {
                    Ok(_) => {}
                    Err(e) => eprintln!("Failed to spawn fuzzel launcher: {e}"),
                }
                None
            }
            Action::ShowHotkeys => {
                // `HOTKEY_HELP` is small and fixed (well under the ~64KiB
                // default pipe buffer), so writing it synchronously here
                // can't block waiting for fuzzel to drain its stdin —
                // unlike `tag-picker`'s own `run_fuzzel`, whose checklist
                // input can grow arbitrarily large and needs a writer
                // thread for that reason.
                match std::process::Command::new("fuzzel")
                    .arg("--dmenu")
                    .arg("--prompt")
                    .arg("Hotkeys: ")
                    .stdin(std::process::Stdio::piped())
                    .env_remove("WAYLAND_DEBUG")
                    .spawn()
                {
                    Ok(mut child) => {
                        if let Some(mut stdin) = child.stdin.take() {
                            use std::io::Write;
                            if let Err(e) = writeln!(stdin, "{}", HOTKEY_HELP.join("\n")) {
                                eprintln!("Failed to write hotkey list to fuzzel's stdin: {e}");
                            }
                        }
                    }
                    Err(e) => eprintln!("Failed to spawn fuzzel for hotkey list: {e}"),
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
            // Story 2.2: fire-and-forget process spawn, no `wm_core` access.
            // `tag-picker` resolves the focused view itself via its own
            // `get-state` IPC call, so this arm never switches an output's
            // active tag and thus never triggers `manage_seats`'
            // pinned-terminal-spawn signal.
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
                        let mut command = std::process::Command::new(tag_picker_path(&wm_exe));
                        // Story 2.9 Task 3.3: the active output's real
                        // connector name, so `tag-picker` can in turn tell
                        // `fuzzel --output=<name>` which monitor to render
                        // on (Task 5) — never appended when unknown,
                        // preserving today's zero-args default exactly (AC
                        // 2).
                        if let Some(name) = active_output_name {
                            command.arg(name);
                        }
                        match command.env_remove("WAYLAND_DEBUG").spawn() {
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
            // Story 2.4: `Mod4+S` ("Switch") spawns the same `tag-picker`
            // binary in switch mode, passing the WM's own deterministic
            // `active_output_id` resolution across the process boundary as
            // a CLI argument — `tag-picker` never re-derives "the active
            // output" itself (Task 1.2). Same fire-and-forget spawn shape
            // as `Action::OpenTagPicker` above, plus the same
            // `None`-output defensive no-op shape as `Action::TagCycle`.
            // Always returns `None`: this arm never itself mutates
            // `wm_core` or triggers `manage_seats`' pinned-terminal-spawn
            // signal — the eventual `switch-tag` IPC call and its
            // pinned-terminal follow-up (Task 2) both happen later,
            // asynchronously, once the user picks a tag in the spawned
            // process.
            Action::TagSwitch => {
                match active_output_id {
                    Some(output_id) => match std::env::current_exe() {
                        Ok(wm_exe) => {
                            let mut command = std::process::Command::new(tag_picker_path(&wm_exe));
                            command.arg("switch").arg(output_id.0.to_string());
                            // Story 2.9 Task 3.4: same trailing-name
                            // convention as `Action::OpenTagPicker` above —
                            // appended after the existing two args, only
                            // when known, preserving today's two-arg
                            // `switch <id>` shape exactly when it isn't (AC
                            // 2).
                            if let Some(name) = active_output_name {
                                command.arg(name);
                            }
                            match command.env_remove("WAYLAND_DEBUG").spawn() {
                                Ok(_) => {}
                                Err(e) => {
                                    eprintln!("Failed to spawn tag-picker in switch mode: {e}")
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to resolve wm's own executable path: {e}")
                        }
                    },
                    None => {
                        eprintln!("Tag-switch keybind pressed but no output is registered yet")
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

    // Code review follow-up (Story 2.7): `windows.back()` is the most-
    // recently-interacted-with window WM-wide, with no tag/visibility
    // filtering of its own — nothing about tag switching (`cycle_tag`/
    // `switch_tag`) reorders `self.windows` or reassigns focus. Before this
    // fix, a tag switch could leave Wayland keyboard focus and
    // `wm_core::focused_view` pointed at a window `recompute_window_visibility`
    // hides on the very next render sequence, with nothing else ever
    // refocusing a window actually visible on the new tag — the user's
    // keypresses would go nowhere until they clicked something. Filtering
    // `windows.back()` through `is_view_visible` here means a hidden
    // back-of-stack window is treated the same as no window at all (focus
    // cleared) rather than wrongly re-affirmed as focused.
    fn focus_top(&mut self, windows: &VecDeque<Window>, wm_core: &mut WmCore) {
        let target = windows.back().filter(|window| {
            let view_id = window.view_id.expect(
                "every window reaches focus_top only after init_new_windows registered it earlier in the same handle_manage_start call",
            );
            match wm_core.is_view_visible(view_id) {
                Ok(visible) => visible,
                Err(e) => {
                    eprintln!(
                        "Failed to check visibility for view {view_id:?} in wm_core: {e:?}"
                    );
                    true
                }
            }
        });
        match target {
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
            const RIVER_LAYER_SHELL_V1_VERSION: u32 = 1;
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
                "river_layer_shell_v1" => {
                    // Optional (Story 2.6): unlike the two globals above, a
                    // version mismatch here is not fatal - just skip binding
                    // and fall back to `buoy-wm`'s pre-Story-2.6 behavior
                    // (layer-shell surfaces can't map).
                    if version < RIVER_LAYER_SHELL_V1_VERSION {
                        eprintln!(
                            "Server supports river_layer_shell_v1 v{version}, but we need at least v{RIVER_LAYER_SHELL_V1_VERSION} - layer-shell surfaces (eg a waybar status bar) will not be able to map"
                        );
                        return;
                    }
                    let layer_shell = registry.bind::<RiverLayerShellV1, _, _>(
                        name,
                        RIVER_LAYER_SHELL_V1_VERSION,
                        qh,
                        (),
                    );
                    state.river_layer_shell = Some(layer_shell);
                }
                // Story 2.9 Task 1: bind every `wl_output` global as soon
                // as it's advertised, regardless of which `river_output_v1`
                // it will later correlate to (Task 2) — the protocol
                // guarantees a `wl_output` global is advertised before any
                // `river_output_v1::WlOutput` event that references it by
                // this same registry `name`, but binding eagerly here,
                // keyed by that `name`, is what makes the later lookup
                // possible at all. Capped at v4 (adds the `name` event this
                // story needs) — a lower server version isn't fatal, same
                // optional-global shape as `river_layer_shell_v1` above,
                // not the required-global shape `river_window_manager_v1`/
                // `river_xkb_bindings_v1` use: this is purely best-effort
                // cosmetic data for `fuzzel --output=`, not core WM
                // function.
                "wl_output" => {
                    const WL_OUTPUT_NAME_VERSION: u32 = 4;
                    let wl_output = registry.bind::<wl_output::WlOutput, _, _>(
                        name,
                        version.min(WL_OUTPUT_NAME_VERSION),
                        qh,
                        (),
                    );
                    state.wm.pending_wl_outputs.insert(name, wl_output);
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
            // Story 2.9 Task 2: `name` here is the *registry* name the
            // corresponding `wl_output` global was advertised with (per the
            // event's own doc comment) — not a connector string. Look it up
            // in `pending_wl_outputs` (Task 1, populated as each `wl_output`
            // global was bound, guaranteed by the protocol to have already
            // happened by this point) to find which bound `wl_output`
            // object correlates to this `Output`, and remember *that
            // object's own id* — not a name string yet, since its own
            // `Name` event isn't guaranteed to have arrived yet (Technical
            // notes). If somehow not found (the global wasn't bound —
            // shouldn't happen per the protocol's ordering guarantee, but
            // handled gracefully per NFR2), leave it `None`; AC 2's
            // fallback covers this.
            //
            // Code review follow-up (Story 2.9): `.remove()`, not `.get()`
            // — `pending_wl_outputs` exists only to bridge the gap between
            // a `wl_output` global being advertised and this correlation
            // event arriving; once correlated, the entry serves no further
            // purpose (`output_name` reads `wl_output_names` from here on,
            // keyed by the object id just captured, not this map). Leaving
            // it behind after every successful correlation — which is
            // effectively every output, ever — grew this map without
            // bound for the life of the long-running WM process.
            Event::WlOutput { name } => {
                if let Some(wl_output_proxy) = state.wm.pending_wl_outputs.remove(&name) {
                    output.wl_output_object_id = Some(wl_output_proxy.id());
                }
            }
            // Story 2.8 Task 1: track the output's real global-coordinate
            // rectangle, previously discarded — `active_output_id`
            // (Task 3) now reads these to find which output the pointer is
            // actually over.
            Event::Position { x, y } => output.position = (x, y),
            Event::Dimensions { width, height } => output.dimensions = (width, height),
        }
    }
}

/// Story 2.9 Task 1.2: real event handling (not `delegate_noop!` —
/// `wl_output`'s `Name` event is exactly the data this story needs), for
/// every `wl_output` global bound in the `wl_registry::Event::Global`
/// handler above. Only `Name` is handled; `Geometry`/`Mode`/`Scale`/`Done`/
/// `Description` are redundant with `river_output_v1`'s own
/// `position`/`dimensions` events (Story 2.8) or simply not needed here
/// (Technical notes' scope boundary) and are intentionally ignored.
impl Dispatch<wl_output::WlOutput, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            state.wm.wl_output_names.insert(proxy.id(), name);
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
            // Story 2.8 Task 2: track the pointer's last-known global
            // position, previously discarded — `active_output_id`
            // (Task 3) uses this to find which output the pointer is
            // actually over, instead of the old lowest-`OutputId`
            // placeholder.
            Event::PointerPosition { x, y } => seat.pointer_position = Some((x, y)),
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
// Story 2.6: the global itself has no events (get_output/get_seat, whose
// child objects DO have events, are out of this story's scope - see
// story-2-6.md's Technical notes).
wayland_client::delegate_noop!(AppData: ignore RiverLayerShellV1);

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
