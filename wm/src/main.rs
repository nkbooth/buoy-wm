// SPDX-FileCopyrightText: © 2026 Nick Booth
// Portions © 2026 Julian Andrews, originally distributed under 0BSD as
// part of tinyrwm <https://codeberg.org/river/tinyrwm>.
// SPDX-License-Identifier: RPL-1.5
//
// Unless explicitly acquired and licensed from Licensor under another
// license, the contents of this file are subject to the Reciprocal Public
// License ("RPL") Version 1.5, or subsequent versions as allowed by the
// RPL, and You may not copy or use this file in either source code or
// executable form, except in compliance with the terms and conditions of
// the RPL.
//
// All software distributed under the RPL is provided strictly on an "AS
// IS" basis, WITHOUT WARRANTY OF ANY KIND, EITHER EXPRESS OR IMPLIED, AND
// LICENSOR HEREBY DISCLAIMS ALL SUCH WARRANTIES, INCLUDING WITHOUT
// LIMITATION, ANY WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
// PURPOSE, QUIET ENJOYMENT, OR NON-INFRINGEMENT. See the RPL for specific
// language governing rights and limitations under the RPL.

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
    river_input_device_v1::RiverInputDeviceV1,
    river_input_manager_v1::RiverInputManagerV1,
    river_layer_shell_v1::RiverLayerShellV1,
    river_libinput_config_v1::RiverLibinputConfigV1,
    river_libinput_device_v1::RiverLibinputDeviceV1,
    river_libinput_result_v1::RiverLibinputResultV1,
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

        pub(super) mod rinput {
            // Needs `wl_output` in scope for `map_to_output`, and nothing
            // from `rwm` — input management is a standalone tree, unlike
            // `rxkb`/`rlayer` which both hang off `river_seat_v1`.
            pub use wayland_client::protocol::__interfaces::*;
            wayland_scanner::generate_interfaces!("./protocol/river-input-management-v1.xml");
        }

        pub(super) mod rlibinput {
            // `river_libinput_device_v1.input_device` carries a
            // `river_input_device_v1`, so this must be generated after (and
            // importing) `rinput`.
            use super::rinput::*;
            wayland_scanner::generate_interfaces!("./protocol/river-libinput-config-v1.xml");
        }
    }

    use self::interfaces::rinput::*;
    use self::interfaces::rlayer::*;
    use self::interfaces::rlibinput::*;
    use self::interfaces::rwm::*;
    use self::interfaces::rxkb::*;
    wayland_scanner::generate_client_code!("./protocol/river-window-management-v1.xml");
    wayland_scanner::generate_client_code!("./protocol/river-xkb-bindings-v1.xml");
    wayland_scanner::generate_client_code!("./protocol/river-layer-shell-v1.xml");
    wayland_scanner::generate_client_code!("./protocol/river-input-management-v1.xml");
    wayland_scanner::generate_client_code!("./protocol/river-libinput-config-v1.xml");
}

mod config;
mod ipc;
mod wm_core;

use config::{Action, Config};
use wm_core::ids::{OutputId, TagId, ViewId};
use wm_core::state::{
    WmCore, WmCoreError, is_pinned_term_app_id, pinned_term_app_id, tag_id_from_pinned_app_id,
};
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

/// Formats a protocol enum for a log line without `WEnum`'s wrapper.
///
/// `{:?}` on a `WEnum` prints `Value(Disabled)`, leaking a detail of how
/// wayland-rs models "this could be a value the client's copy of the
/// protocol has never heard of" into output a person reads. An unknown
/// value still has to say so — it means river and this binary disagree
/// about the protocol — but it says it in words rather than a wrapper.
fn wenum_label<T: Debug>(value: wayland_client::WEnum<T>) -> String {
    // Matched on the variants rather than via `into_result`, whose `Err`
    // carries a pre-formatted "Unknown numeric value N for enum ..." string
    // — the raw number is the useful half, and the type name is already
    // implied by the log line it lands in.
    match value {
        wayland_client::WEnum::Value(known) => format!("{known:?}"),
        wayland_client::WEnum::Unknown(raw) => format!("unknown ({raw})"),
    }
}

/// Every child this WM spawns is fire-and-forget — nothing ever reads an
/// exit status. Without a `wait` each finished child lingers as a zombie
/// for the lifetime of the session, and this process is a long-lived
/// session daemon, so they accumulate (code-review follow-up). Rather than
/// tracking children per call site, keep one list and opportunistically
/// reap whatever has finished each time a new child is spawned.
static SPAWNED_CHILDREN: Mutex<Vec<std::process::Child>> = Mutex::new(Vec::new());

/// Records `child` for reaping and clears out any that have already
/// exited. Recovers from a poisoned lock the same way [`ipc::
/// lock_recovering`] does — losing track of a child leaks a zombie, which
/// is never worth taking down the session for (NFR2).
fn track_child(child: std::process::Child) {
    let mut children = SPAWNED_CHILDREN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    children.retain_mut(|tracked| !matches!(tracked.try_wait(), Ok(Some(_))));
    children.push(child);
}

/// Spawns `command` fire-and-forget, logging a failure as `"Failed to spawn
/// {what}: {e}"`. `WAYLAND_DEBUG` is removed from every child's environment
/// — the added noise makes debugging the window manager itself impractical.
///
/// Returns whether the child was actually created. Most callers spawn
/// something whose failure costs the user one keypress and ignore this; the
/// pinned terminal is the exception, because its spawn has already been
/// recorded as having happened (audit finding D-01).
fn spawn_tracked(command: &mut std::process::Command, what: &str) -> bool {
    match command.env_remove("WAYLAND_DEBUG").spawn() {
        Ok(child) => {
            track_child(child);
            true
        }
        Err(e) => {
            eprintln!("Failed to spawn {what}: {e}");
            false
        }
    }
}

/// Sends the one `river_libinput_device_v1` request `setting` stands for.
///
/// Each request allocates a `river_libinput_result_v1` whose udata is a
/// description of what was attempted, so an `unsupported`/`invalid` reply
/// names the device and setting it came from instead of arriving bare.
/// Neither input protocol mentions manage sequences, so these can be sent
/// as soon as a device is known rather than inside a transaction.
fn apply_libinput_setting(
    device: &RiverLibinputDeviceV1,
    setting: config::LibinputSetting,
    device_name: &str,
    qh: &QueueHandle<AppData>,
) {
    use config::LibinputSetting;
    use river::river_libinput_device_v1::{
        ClickMethod as WireClickMethod, DwtState, NaturalScrollState,
        TapButtonMap as WireTapButtonMap, TapState,
    };
    let label = |what: &str| format!("libinput {device_name:?}: {what}");
    match setting {
        LibinputSetting::Tap(enabled) => {
            let state = if enabled {
                TapState::Enabled
            } else {
                TapState::Disabled
            };
            device.set_tap(state, qh, label("tap"));
        }
        LibinputSetting::TapButtonMap(map) => {
            let wire = match map {
                config::TapButtonMap::Lrm => WireTapButtonMap::Lrm,
                config::TapButtonMap::Lmr => WireTapButtonMap::Lmr,
            };
            device.set_tap_button_map(wire, qh, label("tap_button_map"));
        }
        LibinputSetting::ClickMethod(method) => {
            let wire = match method {
                config::ClickMethod::None => WireClickMethod::None,
                config::ClickMethod::ButtonAreas => WireClickMethod::ButtonAreas,
                config::ClickMethod::Clickfinger => WireClickMethod::Clickfinger,
            };
            device.set_click_method(wire, qh, label("click_method"));
        }
        LibinputSetting::NaturalScroll(enabled) => {
            let state = if enabled {
                NaturalScrollState::Enabled
            } else {
                NaturalScrollState::Disabled
            };
            device.set_natural_scroll(state, qh, label("natural_scroll"));
        }
        LibinputSetting::DisableWhileTyping(enabled) => {
            let state = if enabled {
                DwtState::Enabled
            } else {
                DwtState::Disabled
            };
            device.set_dwt(state, qh, label("disable_while_typing"));
        }
        LibinputSetting::AccelSpeed(speed) => {
            // The protocol carries doubles as a native-endian byte array —
            // Wayland has no float type, and this protocol's own preamble
            // documents `type="array" summary="double"` as exactly that.
            device.set_accel_speed(speed.to_ne_bytes().to_vec(), qh, label("accel_speed"));
        }
    }
}

/// Translates a config modifier set into the protocol's bitfield.
fn river_modifiers(mods: &[config::Modifier]) -> Modifiers {
    mods.iter().fold(Modifiers::empty(), |acc, modifier| {
        acc | match modifier {
            config::Modifier::Super => Modifiers::Mod4,
            config::Modifier::Ctrl => Modifiers::Ctrl,
            config::Modifier::Alt => Modifiers::Mod1,
            config::Modifier::Shift => Modifiers::Shift,
        }
    })
}

/// Linux input event codes for the three pointer buttons river's
/// `create_pointer_binding` takes, from `linux/input-event-codes.h`.
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

/// Translates a config pointer button into its Linux input event code.
fn input_event_code(button: config::Button) -> u32 {
    match button {
        config::Button::Left => BTN_LEFT,
        config::Button::Right => BTN_RIGHT,
        config::Button::Middle => BTN_MIDDLE,
    }
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
    /// Story 2.6: optional, unlike `river_xkb` - binding this global is what
    /// tells `river` to allow layer-shell clients (eg waybar's status bar)
    /// to map surfaces at all, instead of closing them immediately. Absence
    /// is not fatal; it just means layer-shell surfaces can't map, same as
    /// `buoy-wm`'s behavior before this story.
    river_layer_shell: Option<RiverLayerShellV1>,
    /// Optional, like `river_layer_shell`: without it no input device can be
    /// named, so nothing gets configured and every device keeps libinput's
    /// defaults — which is exactly buoy's behavior before `[[input]]`
    /// existed, not a reason to refuse the session.
    river_input_manager: Option<RiverInputManagerV1>,
    /// Optional for the same reason. river only advertises this alongside
    /// `river_input_manager_v1`, but nothing in the protocol promises that,
    /// so the two are tracked (and degraded) independently.
    river_libinput_config: Option<RiverLibinputConfigV1>,
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
    /// Each `river_input_device_v1`'s name, keyed by that proxy's object id,
    /// populated when its `name` event arrives. Held separately from
    /// `libinput_devices` because the two objects are created by two
    /// different globals and the protocol makes no promise about which
    /// arrives first — same ordering trap as `wl_output_names` above.
    input_device_names: HashMap<ObjectId, String>,
    /// Each `river_libinput_device_v1`, keyed by its own object id.
    libinput_devices: HashMap<ObjectId, LibinputDevice>,
    /// The user's `~/.config/buoy/config.toml`, or [`Config::default`]'s
    /// built-in equivalent when there is no such file. Loaded once at
    /// startup — re-reading it on change would mean tearing down and
    /// recreating every live binding object, which no story needs yet.
    config: Config,
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
    /// The action a binding fired since the last `do_action`, if any.
    /// `Option` rather than a `None` enum variant so a parameterized action
    /// can be taken by value without cloning its payload.
    pending_action: Option<Action>,
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
    /// Code review follow-up: whether the pinned terminal currently holds
    /// this seat's real keyboard focus by deliberate choice (a direct
    /// click), as opposed to `focus_top()`'s default `windows.back()`
    /// resolution. Unlike the one-shot local flag this replaced, this
    /// persists across `manage_seats` calls — the pinned terminal is
    /// permanently excluded from `self.windows`' back-of-queue reordering
    /// (FR4: always bottom of z-order), so once it's focused,
    /// `windows.back()` never becomes it. Without this persisted flag,
    /// `focus_top()` would silently steal focus back to whatever WAS at
    /// `windows.back()` on every manage sequence *after* the one where the
    /// terminal was clicked — not just the very next click — since nothing
    /// remembered the terminal was deliberately focused a moment earlier
    /// (confirmed live: focus visibly jumped away from the terminal on the
    /// next keystroke-driven manage sequence, not just eventually).
    ///
    /// Scoped to the terminal's *visibility*: `manage_seats` expires this
    /// the moment the focused window stops being visible, because a latch
    /// that outlives its window suppresses the very `focus_top()` call that
    /// would repair focus. Switching tags away from a deliberately-clicked
    /// terminal is exactly that case, and left focus stranded on the
    /// outgoing tag's now-hidden terminal.
    terminal_intentionally_focused: bool,
}

/// A libinput-configurable device, and the correlation state needed before
/// its `[[input]]` entry can be applied.
#[derive(Debug)]
struct LibinputDevice {
    proxy: RiverLibinputDeviceV1,
    /// The `river_input_device_v1` this configures, from the `input_device`
    /// event. `None` until that arrives — the name lives on that object, so
    /// there is nothing to match a config entry against until then.
    input_device_id: Option<ObjectId>,
    /// Whether the config has already been applied. The two halves of the
    /// device's identity arrive in an unspecified order, so both arrival
    /// paths attempt to configure and this is what keeps it to once.
    configured: bool,
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
        // Code review follow-up: captured before `init_new_windows` below
        // flips every new window's `new` flag to `false` - `manage_seats`
        // needs to know whether a genuinely new window showed up this pass
        // (which should always claim focus, even from a deliberately-
        // focused pinned terminal) versus nothing having changed at all.
        let any_new_windows = self.windows.iter().any(|w| w.new);
        self.init_new_windows();
        self.init_new_seats(river_xkb, qh);
        self.manage_windows();
        self.manage_seats(proxy, any_new_windows);
        // Re-resolve size/position for every pinned terminal after
        // `manage_seats` may have switched a tag onto a different output
        // (or off every output) via a raw keybind - still within this same
        // manage sequence, satisfying `propose_dimensions`'
        // manage-sequence-only constraint.
        self.recompute_pinned_terminal_geometry();
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
            // Story 2.7 Task 2.3, reworked for audit finding D-01(b): the
            // tag this pinned terminal was spawned for is encoded in its
            // own `app_id`, so it is recovered here rather than popped from
            // a queue whose order depended on which of two threads' spawns
            // happened to map first.
            if let Some(tag_id) = tag_id_from_pinned_app_id(&window.app_id) {
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
                // An `app_id` naming a tag that is not registered can only
                // come from a client spoofing the convention (audit finding
                // D-02), so it is handled gracefully rather than panicking
                // (NFR2): log and leave the window untagged, which Task 1's
                // bootstrap exception (`is_view_visible`) keeps visible
                // rather than permanently hidden.
                match wm_core.toggle_view_tag(view_id, tag_id) {
                    Ok(()) => {
                        // Code review follow-up: no longer uses
                        // `river_window_v1.fullscreen()` (Story 2.7's
                        // original mechanism) - live testing found that a
                        // truly fullscreen window moves into river's own
                        // `fullscreen_tree` scene layer, which sits ABOVE
                        // the ordinary `wm_tree` layer every other window
                        // lives in (confirmed via river's own architecture
                        // docs). `place_top()`/`place_bottom()` only
                        // reorder nodes *within* a layer, so no amount of
                        // WM-side stacking control can make an ordinary
                        // window (Zen, Chromium, anything) render above a
                        // truly-fullscreen one - they render behind it,
                        // always, regardless of `place_top()`. Sizing the
                        // pinned terminal manually instead
                        // (`set_position`/`propose_dimensions`, to exactly
                        // fill the output) keeps it an ordinary `wm_tree`
                        // window, so `place_bottom()` (already called
                        // above) and every other window's `place_top()`
                        // behave exactly as documented. If the tag isn't
                        // currently shown on any output, do nothing here:
                        // Task 3's render-sequence visibility pass
                        // (`recompute_window_visibility`) will hide it via
                        // the same `is_view_visible` decision every other
                        // view uses, rather than this call site
                        // special-casing a hide.
                        if let Some(output_id) = wm_core.output_showing_tag(tag_id)
                            && let Some(output) = output_for_id(&self.outputs, output_id)
                        {
                            window.set_position(output.position.0, output.position.1);
                            window
                                .proxy
                                .propose_dimensions(output.dimensions.0, output.dimensions.1);
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to tag newly-mapped pinned terminal onto {tag_id:?}: {e:?}; leaving untagged"
                        );
                        // Code review follow-up (Story 2.7): an untagged
                        // window is shown by `is_view_visible`'s bootstrap
                        // exception, so give it the same explicit
                        // position/dimensions the non-pinned branch below
                        // gives every new window, rather than leaving it
                        // with whatever undefined geometry the compositor
                        // happens to pick — this is a stray window (no
                        // pinned terminal this WM spawned names an
                        // unregistered tag), but it must still render
                        // sanely if it ever occurs.
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

    /// Re-resolves and re-applies the pinned terminal's size/position, one
    /// per tag it's been associated with (Story 2.7 Task 2). Called
    /// unconditionally at the end of every `handle_manage_start`, the same
    /// "simplest-correct first cut, recompute broadly rather than track
    /// precisely which tag-state change to react to" strategy
    /// `recompute_window_visibility` uses for hide/show (Story 2.7 Task 3).
    /// This covers `switch_tag`/`cycle_tag` reassigning a tag to a
    /// different output (or off every output) regardless of whether that
    /// happened via the raw keybind (`manage_seats`, same manage sequence
    /// as this call) or asynchronously via the IPC-driven picker (picked up
    /// the next time any manage sequence runs).
    ///
    /// Code review follow-up: no longer uses `river_window_v1.fullscreen()`
    /// (Story 2.7's original mechanism, renamed from
    /// `recompute_pinned_terminal_fullscreen`) - see `init_new_windows`'s
    /// pinned-terminal branch for the full explanation of why a truly
    /// fullscreen window can never render below an ordinary one regardless
    /// of `place_top()` (river's own scene graph puts fullscreen windows in
    /// a separate, higher layer). `set_position`/`propose_dimensions` sized
    /// to exactly fill the output achieve the same visual result (the
    /// terminal fills the screen) while keeping it an ordinary `wm_tree`
    /// window other windows correctly render above via their own
    /// `place_top()`.
    fn recompute_pinned_terminal_geometry(&mut self) {
        let wm_core = ipc::lock_recovering(&self.wm_core);
        for window in self.windows.iter_mut() {
            if !is_pinned_term_app_id(&window.app_id) {
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
            // If the tag isn't currently shown on any output, do nothing:
            // `recompute_window_visibility`'s next render sequence hides
            // the window via the same `is_view_visible` decision every
            // other view uses, rather than this call site special-casing
            // it - leaving the last-known position/dimensions in place is
            // harmless since the window is hidden regardless.
            if let Some(output) = tag_id
                .and_then(|tag_id| wm_core.output_showing_tag(tag_id))
                .and_then(|output_id| output_for_id(&self.outputs, output_id))
            {
                window.set_position(output.position.0, output.position.1);
                window
                    .proxy
                    .propose_dimensions(output.dimensions.0, output.dimensions.1);
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
        // Bound to a `let` rather than matched inline: the arm below
        // re-locks the same mutex to roll the claim back, and a scrutinee
        // temporary would still be holding the guard there.
        let claim = ipc::lock_recovering(&self.wm_core).claim_pinned_terminal_spawn(tag_id);
        match claim {
            Ok(Some(session_name)) => spawn_pinned_terminal_or_release_claim(
                &self.wm_core,
                tag_id,
                &self.config.defaults.terminal,
                &self
                    .config
                    .defaults
                    .pinned_terminal_argv(&pinned_term_app_id(tag_id), &session_name),
            ),
            Ok(None) => {}
            Err(e) => {
                eprintln!("Failed to check pinned-terminal spawn state for tag {tag_id:?}: {e:?}")
            }
        }
    }

    /// Registers every binding the loaded config declares on each new seat.
    ///
    /// `Config::parse` has already rejected any key name that doesn't
    /// resolve, so `keysym()` returning `None` here means the config was
    /// bypassed entirely (only `Config::default` can do that, and its own
    /// names are covered by a test). Skip-and-log rather than panic keeps
    /// one bad binding from taking down the session (NFR2).
    fn init_new_seats(&mut self, river_xkb: &RiverXkbBindingsV1, qh: &QueueHandle<AppData>) {
        for seat in self.seats.values_mut() {
            if !seat.new {
                continue;
            }
            for keybind in &self.config.keybinds {
                let Some(keysym) = keybind.keysym() else {
                    eprintln!("Skipping keybind with unresolvable key `{}`", keybind.key);
                    continue;
                };
                seat.create_xkb_binding(
                    river_xkb,
                    qh,
                    river_modifiers(&keybind.mods),
                    keysym,
                    keybind.action.clone(),
                );
            }
            for mousebind in &self.config.mousebinds {
                seat.create_pointer_binding(
                    qh,
                    river_modifiers(&mousebind.mods),
                    input_event_code(mousebind.button),
                    mousebind.action.clone(),
                );
            }
            seat.new = false;
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
    /// 5). Same linear-scan-by-`output_id` shape `output_for_id`
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

    /// The device name behind a `river_libinput_device_v1`, once both its
    /// `input_device` event and that device's `name` event have arrived.
    fn libinput_device_name(&self, libinput_id: &ObjectId) -> Option<&str> {
        let input_device_id = self
            .libinput_devices
            .get(libinput_id)?
            .input_device_id
            .as_ref()?;
        self.input_device_names
            .get(input_device_id)
            .map(String::as_str)
    }

    /// The name of the device behind `libinput_id`, but only when the user
    /// actually has an `[[input]]` entry for it — the gate that keeps
    /// device-state logging to devices the config speaks about.
    fn configured_device_name(&self, libinput_id: &ObjectId) -> Option<&str> {
        let name = self.libinput_device_name(libinput_id)?;
        self.config.input_for(name).map(|_| name)
    }

    /// Applies each device's matching `[[input]]` entry, once.
    ///
    /// Called from both arrival paths — the device's `name` and its
    /// `input_device` correlation — because the protocol does not order them
    /// relative to each other. Whichever lands second is the one that finds
    /// the device ready; `configured` keeps a device from being reconfigured
    /// when the other path fires later for an unrelated device.
    ///
    /// A device with no matching entry is marked configured too: leaving
    /// libinput's defaults alone is a decision, not unfinished work, and
    /// re-deciding it on every later event would be pointless.
    fn configure_libinput_devices(&mut self, qh: &QueueHandle<AppData>) {
        let ready: Vec<(ObjectId, Option<Vec<config::LibinputSetting>>, String)> = self
            .libinput_devices
            .iter()
            .filter(|(_, device)| !device.configured)
            .filter_map(|(id, _)| {
                let name = self.libinput_device_name(id)?.to_string();
                let settings = self.config.input_for(&name).map(|input| input.settings());
                Some((id.clone(), settings, name))
            })
            .collect();
        for (id, settings, name) in ready {
            // Borrowed separately from the scan above: `libinput_device_name`
            // needs `&self` while sending the requests needs the device
            // mutably, and the two cannot overlap.
            let Some(device) = self.libinput_devices.get_mut(&id) else {
                continue;
            };
            device.configured = true;
            let Some(settings) = settings else {
                continue;
            };
            eprintln!(
                "libinput {name:?}: applying {} setting(s) from [[input]]",
                settings.len()
            );
            for setting in settings {
                apply_libinput_setting(&device.proxy, setting, &name, qh);
            }
        }
    }

    fn manage_seats(&mut self, wm_proxy: &RiverWindowManagerV1, any_new_windows: bool) {
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
            // A `window_interaction` event names a window the compositor
            // may already have destroyed by the time this manage sequence
            // runs: that event and the `closed` event that drops the window
            // from `self.windows` race, and the protocol promises no
            // ordering between them. `expect`ing this lookup took the whole
            // session down over a click that had merely outlived its target
            // (NFR2), so a miss now skips only the raise/reorder below and
            // lets the rest of this pass — focus-latch expiry, `focus_top`,
            // `do_action`, op handling — run exactly as it would have.
            'interacted: {
                let Some(window_proxy) = seat.interacted.take() else {
                    break 'interacted;
                };
                let Some(i) = self
                    .windows
                    .iter()
                    .position(|window| window.proxy == window_proxy)
                else {
                    eprintln!(
                        "Ignoring interaction with unmanaged window {:?} (already closed)",
                        window_proxy.id()
                    );
                    break 'interacted;
                };
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
                if is_pinned_term_app_id(&window.app_id) {
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
                    seat.terminal_intentionally_focused = true;
                } else {
                    self.windows.push_back(window);
                    // A different window just legitimately claimed focus —
                    // the pinned terminal no longer holds it, regardless of
                    // whether it did a moment ago.
                    seat.terminal_intentionally_focused = false;
                }
            }
            // Code review follow-up: `seat.terminal_intentionally_focused`
            // (see its own doc comment) replaces a one-shot local flag that
            // only survived for the single pass in which the pinned
            // terminal was clicked — on every *later* pass, with no new
            // click and no new window, `windows.back()` still pointed at
            // whatever was focused *before* the terminal, so the old
            // unconditional `focus_top()` call below silently stole focus
            // right back. `any_new_windows` still forces `focus_top()` to
            // run even while the terminal holds intentional focus — a
            // freshly-mapped window should always be able to claim focus.
            // The latch only earns its keep while the terminal it was set
            // for is actually on screen. A tag switch hides that terminal
            // without touching focus, so leaving the latch set would
            // suppress `focus_top` indefinitely and strand keyboard focus on
            // a window the user can no longer see — the reported tag-switch
            // symptom, and the half `focus_top`'s own visibility scan cannot
            // fix on its own, since it never gets called.
            if seat.terminal_intentionally_focused
                && !seat.focused_view_is_visible(&self.windows, wm_core)
            {
                seat.terminal_intentionally_focused = false;
            }
            if any_new_windows || !seat.terminal_intentionally_focused {
                seat.terminal_intentionally_focused = false;
                seat.focus_top(&self.windows, wm_core, None);
            }
            // `do_action` runs *after* the focus block above, so a binding
            // that switches tags (`Action::TagCycle`) hides the focused
            // window a step too late for this pass's `focus_top` to have
            // noticed. Nothing schedules another pass on its own — the main
            // loop is a `blocking_dispatch`, so focus would stay on the
            // outgoing tag's window until some unrelated compositor event
            // arrived, and the user's next keypress would land on a window
            // they can't see. Reordering the two isn't an option: `do_action`
            // reads the focus this pass's `focus_top` just resolved (that's
            // what makes click-then-`Mod4+Q` close the right window), so the
            // switch is detected after the fact instead, by the active
            // output's current tag changing, and focus repaired in the same
            // pass.
            let tag_before = active_output_id.and_then(|id| wm_core.output_current_tag(id));
            if let Some(tag_id) = seat.do_action(
                &mut self.windows,
                wm_proxy,
                wm_core,
                active_output_id,
                active_output_name.as_deref(),
                &self.config,
            ) {
                pending_terminal_spawns.push(tag_id);
            }
            let tag_after = active_output_id.and_then(|id| wm_core.output_current_tag(id));
            if tag_after != tag_before {
                // A deliberate terminal focus does not survive the tag it was
                // made on, same expiry rule as the latch check above.
                seat.terminal_intentionally_focused = false;
                seat.focus_top(&self.windows, wm_core, active_output_id);
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

/// Resolves the real `Output` record for a `wm-core` [`OutputId`], by
/// scanning `outputs`' values for a matching `Output::output_id` (the map's
/// own keys are `ObjectId`s, not `OutputId`s - `wm-core` never sees Wayland
/// object ids). A free function rather than a `WindowManager` method
/// (Story 2.7 Tasks 2/5): it borrows only the `outputs` field directly, so
/// callers already holding a disjoint mutable borrow of `self.windows`
/// (`init_new_windows`, `recompute_pinned_terminal_geometry`) can call it
/// without the whole-`self` re-borrow a method call would require (same
/// reasoning as `active_output_id` needing to be computed before such a
/// loop begins).
///
/// Code review follow-up: renamed from `output_proxy_for_id` (which
/// returned only the `RiverOutputV1` proxy) - callers need the output's
/// real `position`/`dimensions` too now that the pinned terminal is sized
/// to fill its output manually rather than via `river_window_v1.fullscreen`
/// (see `recompute_pinned_terminal_geometry`'s doc comment for why).
fn output_for_id(outputs: &HashMap<ObjectId, Output>, output_id: OutputId) -> Option<&Output> {
    outputs
        .values()
        .find(|output| output.output_id == output_id)
}

/// Code review follow-up (Story 2.2, finding #1): resolves the `buoy-tag-picker`
/// binary's path as a sibling of the WM's own running executable, rather
/// than trusting `$PATH` — nothing in this repo installs the built
/// `buoy-tag-picker` binary onto `PATH`, and both binaries land in the same
/// Cargo workspace `target/{profile}/` directory, so `wm_exe`'s parent
/// directory is exactly where `buoy-tag-picker` lives too. Falls back to the
/// bare name if `wm_exe` unexpectedly has no parent (e.g. a bare filename
/// with no directory component) — `Command::spawn()` will then fail the
/// same `$PATH`-dependent way the old code always did, handled by the
/// existing error-logging call site rather than invented here.
fn tag_picker_path(wm_exe: &Path) -> PathBuf {
    match wm_exe.parent() {
        Some(dir) => dir.join("buoy-tag-picker"),
        None => PathBuf::from("buoy-tag-picker"),
    }
}

/// Spawns a tag's pinned terminal as `<terminal> <argv...>`, where `argv`
/// is [`Config::pinned_terminal_argv`]'s already-substituted result — by
/// default foot's `-a pinned-term-<tag id> zellij attach --create
/// <session>`.
///
/// Both the program and its argv are configurable because the flag that
/// sets a window's app-id is terminal-specific (code-review follow-up:
/// hardcoding foot's `-a` meant configuring `terminal` broke every pinned
/// terminal silently, and permanently — the spawn succeeds, the tag is
/// marked spawned, and the claim is idempotent so it never retries).
/// `Config::parse` requires the argv to carry `{app_id}`, since
/// [`pinned_term_app_id`] is how the rest of this WM recognizes the window
/// and recovers which tag it belongs to.
///
/// Arguments are passed individually to `Command`, never through a shell,
/// so an arbitrary tag name in the session carries no injection risk.
// Called from `ensure_pinned_terminal_spawned`, which gained its own
// production call site in `manage_seats` in Story 1.7.
fn spawn_pinned_terminal(terminal: &str, argv: &[String]) -> bool {
    spawn_tracked(
        std::process::Command::new(terminal).args(argv),
        &format!("pinned terminal `{terminal}`"),
    )
}

/// Spawns a tag's pinned terminal and, if the spawn fails, releases the
/// claim [`WmCore::claim_pinned_terminal_spawn`] already committed for it.
///
/// The claim has to be committed before the spawn — it is what makes the
/// spawn happen at most once — so rolling it back is the only thing keeping
/// a missing terminal binary, a mid-upgrade replacement, or a transient
/// `EMFILE` from costing that tag its pinned terminal for the whole session
/// and mis-tagging every pinned terminal that maps after it (audit finding
/// D-01). Both spawn sites — the Wayland thread's keybind path and the IPC
/// thread's `switch-tag` path — go through here for that reason.
fn spawn_pinned_terminal_or_release_claim(
    wm_core: &Mutex<WmCore>,
    tag_id: TagId,
    terminal: &str,
    argv: &[String],
) {
    if spawn_pinned_terminal(terminal, argv) {
        return;
    }
    log_wm_core_err(
        ipc::lock_recovering(wm_core).release_pinned_terminal_claim(tag_id),
        "Failed to release the pinned-terminal claim after a failed spawn",
    );
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
            pending_action: None,
            op: SeatOp::None,
            op_dx: 0,
            op_dy: 0,
            op_release: false,
            pointer_position: None,
            terminal_intentionally_focused: false,
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
        // `buoy-tag-picker` spawn arms below treat that the same as
        // `active_output_id` being `None`: append no extra argument at all,
        // rather than a bogus/empty one (AC 2).
        active_output_name: Option<&str>,
        config: &Config,
    ) -> Option<TagId> {
        let pending_action = self.pending_action.take()?;
        match pending_action {
            // Don't pass WAYLAND_DEBUG on to children, the added noise makes
            // debugging the window manager itself impractical.
            Action::Terminal => {
                spawn_tracked(
                    &mut std::process::Command::new(&config.defaults.terminal),
                    &format!("terminal `{}`", config.defaults.terminal),
                );
                None
            }
            // Runs through `sh -c` so a bind can carry a whole command line —
            // arguments, pipes, `~` expansion — instead of just a bare
            // program name. The string comes from the user's own config
            // file, so shell interpretation is the intent here, not an
            // injection vector: anyone who can edit it can already run
            // anything as this user.
            Action::Exec(command_line) => {
                spawn_tracked(
                    std::process::Command::new("sh")
                        .arg("-c")
                        .arg(&command_line),
                    &format!("`{command_line}`"),
                );
                None
            }
            // Create-on-demand: a named-tag bind is meant to be pressed
            // before the tag exists (`Super+1` = "email" on a fresh
            // session), so a missing tag is created rather than treated as
            // an error.
            //
            // The name is resolved before `create_tag` is reached, but not
            // to prevent duplicates — `TagRegistry::create_tag` is already
            // idempotent by name and returns the existing id (code-review
            // follow-up corrected an earlier comment claiming otherwise).
            // It is so that the overwhelmingly common case, pressing a bind
            // for a tag that already exists, never calls a `&mut WmCore`
            // mutator at all: this project's retrospective identifies new
            // call sites onto shared-state mutators as its highest-risk
            // change shape, so a read stays a read.
            Action::SwitchTag(name) => {
                let Some(output_id) = active_output_id else {
                    eprintln!("Tag keybind for `{name}` pressed but no output is registered yet");
                    return None;
                };
                let tag_id = match wm_core.tag_id_by_name(&name) {
                    Some(tag_id) => tag_id,
                    None => match wm_core.create_tag(name.clone()) {
                        Ok(tag_id) => tag_id,
                        Err(e) => {
                            eprintln!("Failed to create tag `{name}`: {e:?}");
                            return None;
                        }
                    },
                };
                match wm_core.switch_tag(output_id, tag_id) {
                    // Mirrors `Action::TagCycle`: the returned tag id is the
                    // signal `manage_seats` uses to spawn this tag's pinned
                    // terminal on first use.
                    Ok(()) => Some(tag_id),
                    Err(e) => {
                        eprintln!("Failed to switch to tag `{name}`: {e:?}");
                        None
                    }
                }
            }
            // `Mod4+R`: same fire-and-forget spawn shape as `SpawnFoot`
            // above. Bare `fuzzel` (no `--dmenu`) runs its own built-in
            // desktop-entry launcher, so no argument wiring is needed.
            Action::Launcher => {
                // "overlay" (not the default "top") renders above a
                // fullscreen window too (fuzzel.ini(5)) - kept as
                // defense-in-depth even though the pinned terminal no
                // longer uses real protocol fullscreen (see
                // `recompute_pinned_terminal_geometry`'s doc comment).
                //
                // Code review follow-up: also pass `--output=<name>`, the
                // same real connector name `Action::OpenTagPicker`/
                // `TagSwitch` already pass to `buoy-tag-picker` (Story 2.9) -
                // this arm spawns `fuzzel` directly, bypassing `buoy-tag-picker`
                // entirely, so it never got that fix. Without it, `fuzzel`
                // fell back to "let the compositor choose", which could
                // pick a disabled/off output when docked (kanshi disables
                // the laptop panel) - the launcher would map with real
                // keyboard focus and accept input, but paint to a screen
                // nothing shows on. No flag at all when the name isn't yet
                // known, same as every other `--output=` call site.
                let mut command = std::process::Command::new(&config.defaults.launcher);
                command.arg("--layer=overlay");
                if let Some(name) = active_output_name {
                    command.arg(format!("--output={name}"));
                }
                spawn_tracked(
                    &mut command,
                    &format!("launcher `{}`", config.defaults.launcher),
                );
                None
            }
            Action::Hotkeys => {
                // The cheat-sheet used to be a fixed 11-entry constant,
                // comfortably under the ~64KiB default pipe buffer, which
                // is what made a synchronous write safe here. It is now
                // generated from the user's own bindings and has no bound
                // at all, so a large enough config could fill the pipe and
                // block this — the WM's only thread — until fuzzel drained
                // it, freezing all window management. Hand the write to a
                // thread, exactly as `buoy-tag-picker`'s `run_fuzzel` already
                // does for its arbitrarily-long checklist (code-review
                // follow-up).
                //
                // `fuzzel` is deliberately not `config.defaults.launcher`:
                // it is driven as a dmenu-style pager here, with
                // fuzzel-specific flags, not as the user's chosen launcher.
                // See `Action::Launcher`'s comments above for `--layer=
                // overlay` and `--output=<name>`.
                let mut command = std::process::Command::new("fuzzel");
                command
                    .arg("--dmenu")
                    .arg("--layer=overlay")
                    .arg("--prompt")
                    .arg("Hotkeys: ");
                if let Some(name) = active_output_name {
                    command.arg(format!("--output={name}"));
                }
                match command
                    .stdin(std::process::Stdio::piped())
                    .env_remove("WAYLAND_DEBUG")
                    .spawn()
                {
                    Ok(mut child) => {
                        if let Some(mut stdin) = child.stdin.take() {
                            let help = config.hotkey_help().join("\n");
                            std::thread::spawn(move || {
                                use std::io::Write;
                                if let Err(e) = writeln!(stdin, "{help}") {
                                    eprintln!("Failed to write hotkey list to fuzzel: {e}");
                                }
                            });
                        }
                        track_child(child);
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
                        .is_some_and(|window| is_pinned_term_app_id(&window.app_id));
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
                // focus_window/place_top proxy calls against the top of
                // `wm_core`'s stacking order — which `cycle_focus`' own
                // `raise_view` just set to this same window — and this
                // reorder keeps `windows` agreeing with it.
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
                    if is_pinned_term_app_id(&windows[i].app_id) {
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
                        // Unscoped: `cycle_focus` has already chosen the
                        // target and only considers visible views, so this
                        // call is re-affirming that choice, not searching.
                        self.focus_top(windows, wm_core, None);
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
            Action::CycleTag => match active_output_id {
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
            // `buoy-tag-picker` resolves the focused view itself via its own
            // `get-state` IPC call, so this arm never switches an output's
            // active tag and thus never triggers `manage_seats`'
            // pinned-terminal-spawn signal.
            //
            // Code review follow-up (finding #1): spawning `"buoy-tag-picker"` by
            // bare name relied on `$PATH`, but nothing in this repo installs
            // the built binary there — in a real session this silently
            // ENOENTs and `Mod4+A` does nothing. Resolve the sibling
            // binary's path relative to the WM's own running executable
            // instead (`tag_picker_path`); if `current_exe()` itself fails,
            // log and skip spawning rather than guessing a path or
            // panicking (NFR2). Once resolved, the spawn/error-handling
            // shape is otherwise byte-for-byte the same as
            // `Action::SpawnFoot`'s above.
            Action::TagPicker => {
                match std::env::current_exe() {
                    Ok(wm_exe) => {
                        let mut command = std::process::Command::new(tag_picker_path(&wm_exe));
                        // Story 2.10 Task 3: the active output's id,
                        // threaded across the process boundary so
                        // assign-mode can switch the active output to a
                        // picked/created tag when no window is focused
                        // (Tasks 4/5) — mirrors `Action::TagSwitch`'s own
                        // `<output_id> [<output_name>]` argument order
                        // below. Only appended when an output is actually
                        // registered; `None` (a startup-race edge case
                        // Task 2 makes rare but doesn't eliminate) spawns
                        // with zero args, same as today's behavior.
                        if let Some(output_id) = active_output_id {
                            command.arg(output_id.0.to_string());
                            // Story 2.9 Task 3.3: the active output's real
                            // connector name, so `buoy-tag-picker` can in turn
                            // tell `fuzzel --output=<name>` which monitor
                            // to render on (Task 5) — never appended when
                            // unknown, preserving today's argument shape
                            // exactly (AC 2).
                            if let Some(name) = active_output_name {
                                command.arg(name);
                            }
                        }
                        spawn_tracked(&mut command, "buoy-tag-picker");
                    }
                    Err(e) => {
                        eprintln!("Failed to resolve wm's own executable path: {e}")
                    }
                }
                None
            }
            // Story 2.4: `Mod4+S` ("Switch") spawns the same `buoy-tag-picker`
            // binary in switch mode, passing the WM's own deterministic
            // `active_output_id` resolution across the process boundary as
            // a CLI argument — `buoy-tag-picker` never re-derives "the active
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
                            spawn_tracked(&mut command, "buoy-tag-picker in switch mode");
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
    // `switch_tag`) reorders `self.windows` or reassigns focus. Before that
    // fix, a tag switch could leave Wayland keyboard focus and
    // `wm_core::focused_view` pointed at a window `recompute_window_visibility`
    // hides on the very next render sequence.
    //
    // Filtering `windows.back()` alone only got as far as *dropping* focus in
    // that case, though: one candidate was tested, and a hidden one cleared
    // focus outright instead of handing it to something the user can actually
    // see, so a tag switch away from the focused window left keypresses going
    // nowhere until the user clicked. `wm_core::topmost_visible_view` replaces
    // that single-candidate test with a back-to-front scan for the highest
    // *visible* view, which lands on the new tag's pinned terminal as the last
    // resort (`lower_view` keeps it at the front/bottom of the stacking
    // order). `self.windows` is deliberately not the order scanned: it is only
    // ever appended to and reordered on interaction, so a pinned terminal
    // mapped after a floating window sits *behind* it there, while
    // `wm_core`'s `stacking_order` is the order both `lower_view` and
    // `raise_view` actively maintain.
    /// `scope` restricts the search to one output's currently-displayed
    /// tag. The tag-switch repair passes the output it just switched, so
    /// focus can't land on a window the user isn't looking at (code-review
    /// follow-up); the routine every-pass call passes `None`, where any
    /// visible window is a legitimate target.
    fn focus_top(
        &mut self,
        windows: &VecDeque<Window>,
        wm_core: &mut WmCore,
        scope: Option<OutputId>,
    ) {
        // A visible view always has a matching `Window`: `remove_windows`
        // drops a window from `self.windows` and unregisters its view from
        // `wm_core` inside the same pass, so the two can't diverge across
        // this call. `and_then` therefore degrades to the same "nothing to
        // focus" clear as an empty stacking order rather than panicking
        // (NFR2), which also retires this function's two `expect`s.
        let topmost = match scope {
            Some(output_id) => wm_core.topmost_visible_view_on(output_id),
            None => wm_core.topmost_visible_view(),
        };
        let target = topmost.and_then(|view_id| {
            windows
                .iter()
                .find(|window| window.view_id == Some(view_id))
                .map(|window| (view_id, window))
        });
        match target {
            Some((view_id, window)) => {
                self.proxy.focus_window(&window.proxy);
                // FR4: the pinned terminal must always render at the bottom,
                // so — unlike every other window — it takes focus without
                // being raised. Same split `manage_seats`' click-to-focus
                // path already makes, and the case this scan newly reaches:
                // before, focus_top could only ever land on `windows.back()`,
                // which the pinned terminal is never pushed to.
                if !is_pinned_term_app_id(&window.app_id) {
                    window.node.place_top();
                }
                self.focused = Some(window.proxy.clone());
                // set_focus is expected to succeed here: view_id came from
                // this same `wm_core`'s stacking order, so it should not be
                // unknown to it; log rather than silently swallow an Err, so
                // a future regression that does hit it stays visible (NFR2,
                // error propagation), same pattern as remove_windows'
                // unregister_view logging.
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

    /// Whether this seat's current focus target is still visible. Drives
    /// `terminal_intentionally_focused`'s expiry in `manage_seats` — a seat
    /// focused on nothing, on a window that has since been removed, or on a
    /// window hidden by a tag switch all answer `false`, which releases the
    /// latch so `focus_top` runs and repairs focus.
    fn focused_view_is_visible(&self, windows: &VecDeque<Window>, wm_core: &WmCore) -> bool {
        let Some(focused) = &self.focused else {
            return false;
        };
        windows
            .iter()
            .find(|window| &window.proxy == focused)
            .and_then(|window| window.view_id)
            .and_then(|view_id| wm_core.is_view_visible(view_id).ok())
            .unwrap_or(false)
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
            const RIVER_INPUT_MANAGER_V1_VERSION: u32 = 1;
            const RIVER_LIBINPUT_CONFIG_V1_VERSION: u32 = 1;
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
                "river_input_manager_v1" => {
                    // Optional, same as `river_layer_shell_v1`: a river
                    // without it just means no `[[input]]` entry can be
                    // applied, which is the pre-`[[input]]` status quo.
                    if version < RIVER_INPUT_MANAGER_V1_VERSION {
                        eprintln!(
                            "Server supports river_input_manager_v1 v{version}, but we need at least v{RIVER_INPUT_MANAGER_V1_VERSION} - [[input]] device configuration will not be applied"
                        );
                        return;
                    }
                    let input_manager = registry.bind::<RiverInputManagerV1, _, _>(
                        name,
                        RIVER_INPUT_MANAGER_V1_VERSION,
                        qh,
                        (),
                    );
                    state.river_input_manager = Some(input_manager);
                }
                "river_libinput_config_v1" => {
                    if version < RIVER_LIBINPUT_CONFIG_V1_VERSION {
                        eprintln!(
                            "Server supports river_libinput_config_v1 v{version}, but we need at least v{RIVER_LIBINPUT_CONFIG_V1_VERSION} - [[input]] device configuration will not be applied"
                        );
                        return;
                    }
                    let libinput_config = registry.bind::<RiverLibinputConfigV1, _, _>(
                        name,
                        RIVER_LIBINPUT_CONFIG_V1_VERSION,
                        qh,
                        (),
                    );
                    state.river_libinput_config = Some(libinput_config);
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
                let mut wm_core_guard = ipc::lock_recovering(&state.wm.wm_core);
                let output_id = wm_core_guard.register_output();
                // Story 2.10: bootstrap a "default" tag + switch this
                // output to it + spawn its pinned terminal, the first
                // time *any* output registers on a completely empty tag
                // registry (AC 1) — the same three effects
                // `Action::TagCycle`'s keybind path already produces for
                // an existing tag, just triggered once automatically at
                // startup. Gated on the registry being empty rather than
                // "is this the very first output ever" so a multi-output
                // login can't race on which output counts as first (see
                // this story's Technical notes); never re-triggers once
                // any tag exists (AC 2), since `tag_count()` can never
                // return to zero again once non-zero (ADR-006: no
                // tag-deletion operation exists). Both `create_tag` and
                // `switch_tag` are structurally near-impossible to fail
                // here (a fresh registry, a just-registered output), but
                // log-and-continue rather than panic either way (NFR2).
                let bootstrapped_tag_id = if wm_core_guard.tag_count() == 0 {
                    match wm_core_guard.create_tag(state.wm.config.defaults.default_tag.clone()) {
                        Ok(tag_id) => {
                            if let Err(e) = wm_core_guard.switch_tag(output_id, tag_id) {
                                eprintln!(
                                    "Failed to switch bootstrap output {output_id:?} to default tag: {e:?}"
                                );
                            }
                            Some(tag_id)
                        }
                        Err(e) => {
                            eprintln!("Failed to create bootstrap default tag: {e:?}");
                            None
                        }
                    }
                } else {
                    None
                };
                // Story 2.1: drop the lock before
                // `ensure_pinned_terminal_spawned` re-locks the same
                // mutex below — same "lock, mutate, drop, fresh borrow"
                // sequencing `manage_seats` already establishes, since
                // `std::sync::Mutex` is not reentrant.
                drop(wm_core_guard);
                state.wm.outputs.insert(id.id(), Output::new(id, output_id));
                if let Some(tag_id) = bootstrapped_tag_id {
                    state.wm.ensure_pinned_terminal_spawned(tag_id);
                }
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
            Event::ShellSurfaceInteraction { .. } => {}
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
            Event::Pressed => seat.pending_action = Some(binding.action.clone()),
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
            .expect("pointer_binding not found");
        match event {
            Event::Pressed => seat.pending_action = Some(binding.action.clone()),
            Event::Released => {}
        }
    }
}

impl Dispatch<RiverInputManagerV1, ()> for AppData {
    fn event(
        _state: &mut Self,
        _proxy: &RiverInputManagerV1,
        event: <RiverInputManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use river::river_input_manager_v1::Event;
        match event {
            // The `river_input_device_v1` this carries is registered by its
            // own `name` event rather than here: a device with no name yet
            // cannot be matched against an `[[input]]` entry, so there is
            // nothing useful to record at creation time.
            Event::InputDevice { .. } => {}
            // Only sent in response to a `stop` request, which this WM never
            // makes - it wants input devices for the whole session.
            Event::Finished => {}
        }
    }

    wayland_client::event_created_child!(AppData, RiverInputManagerV1, [
        river::river_input_manager_v1::EVT_INPUT_DEVICE_OPCODE => (RiverInputDeviceV1, ())
    ]);
}

impl Dispatch<RiverInputDeviceV1, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverInputDeviceV1,
        event: <RiverInputDeviceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use river::river_input_device_v1::Event;
        match event {
            Event::Name { name } => {
                state.wm.input_device_names.insert(proxy.id(), name);
                // The libinput half may already be waiting on this name.
                state.wm.configure_libinput_devices(qh);
            }
            Event::Removed => {
                state.wm.input_device_names.remove(&proxy.id());
                proxy.destroy();
            }
            Event::Type { .. } => {}
        }
    }
}

impl Dispatch<RiverLibinputConfigV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _proxy: &RiverLibinputConfigV1,
        event: <RiverLibinputConfigV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use river::river_libinput_config_v1::Event;
        match event {
            Event::LibinputDevice { id } => {
                state.wm.libinput_devices.insert(
                    id.id(),
                    LibinputDevice {
                        proxy: id,
                        input_device_id: None,
                        configured: false,
                    },
                );
            }
            // As with the input manager: never requested, so never sent.
            Event::Finished => {}
        }
    }

    wayland_client::event_created_child!(AppData, RiverLibinputConfigV1, [
        river::river_libinput_config_v1::EVT_LIBINPUT_DEVICE_OPCODE => (RiverLibinputDeviceV1, ())
    ]);
}

impl Dispatch<RiverLibinputDeviceV1, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverLibinputDeviceV1,
        event: <RiverLibinputDeviceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use river::river_libinput_device_v1::Event;
        match event {
            Event::InputDevice { device } => {
                if let Some(libinput_device) = state.wm.libinput_devices.get_mut(&proxy.id()) {
                    libinput_device.input_device_id = Some(device.id());
                }
                // The name may already have arrived on the other object.
                state.wm.configure_libinput_devices(qh);
            }
            Event::Removed => {
                state.wm.libinput_devices.remove(&proxy.id());
                proxy.destroy();
            }
            // Of this protocol's ~40 report events, only the two that answer
            // "why did my touchpad not do what I expected" are surfaced:
            // what tap is actually set to, and which click method decides
            // whether a bottom-right press is a right click. Reported only
            // for a device the user configured — every device sends these,
            // so logging them all would bury the one that matters under
            // keyboards and lid switches. river re-sends `*_current` after a
            // successful `set_*`, which makes the second line for a
            // configured device the confirmation that it took effect.
            Event::TapCurrent { state: tap_state } => {
                if let Some(name) = state.wm.configured_device_name(&proxy.id()) {
                    eprintln!(
                        "libinput {name:?}: tap-to-click is {}",
                        wenum_label(tap_state)
                    );
                }
            }
            Event::ClickMethodCurrent { method } => {
                if let Some(name) = state.wm.configured_device_name(&proxy.id()) {
                    eprintln!("libinput {name:?}: click method is {}", wenum_label(method));
                }
            }
            _ => {}
        }
    }
}

/// Every `set_*` request allocates one of these to report back on. The
/// `String` payload is the human-readable "what was set on which device" so
/// a rejection names itself instead of arriving as a bare `invalid`.
impl Dispatch<RiverLibinputResultV1, String> for AppData {
    fn event(
        _state: &mut Self,
        _proxy: &RiverLibinputResultV1,
        event: <RiverLibinputResultV1 as Proxy>::Event,
        data: &String,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use river::river_libinput_result_v1::Event;
        // A silently-dropped `unsupported` is how a setting that simply does
        // not work on this device looks identical to one that was never
        // configured, so both failure modes are named here. `success` stays
        // quiet - one line per applied setting per startup is noise.
        match event {
            Event::Success => {}
            Event::Unsupported => {
                eprintln!("{data}: unsupported by this device, ignored");
            }
            Event::Invalid => {
                eprintln!("{data}: invalid value, ignored");
            }
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

    // A broken config file is reported and then ignored in favour of the
    // built-in defaults, rather than aborting startup: this WM is the only
    // thing that can put a screen in front of the user to fix the typo
    // with, so refusing to start over a bad keybind would lock them out of
    // their own session. Loaded before the first roundtrip binds any seat,
    // since `init_new_seats` registers whatever this produces.
    app_data.wm.config = match Config::load() {
        Ok(config) => config,
        Err(e) => {
            // `load` only returns `Err` for a file it actually found, so
            // the path is always resolvable here; the fallback label is
            // belt-and-braces rather than a reachable case.
            let path = config::config_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "the config file".to_string());
            eprintln!("Failed to load {path}: {e}\nFalling back to built-in defaults.");
            Config::default()
        }
    };

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
    if let Err(e) = ipc::server::spawn(
        Arc::clone(&app_data.wm.wm_core),
        &socket_path,
        app_data.wm.config.defaults.clone(),
    ) {
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
    fn wenum_label_prints_a_known_value_without_the_wrapper() {
        use river::river_libinput_device_v1::TapState;
        assert_eq!(
            wenum_label(wayland_client::WEnum::Value(TapState::Disabled)),
            "Disabled"
        );
    }

    /// An unrecognized value means river and this binary disagree about the
    /// protocol, which has to stay visible rather than being smoothed into
    /// something that looks like a real setting.
    #[test]
    fn wenum_label_names_an_unknown_value_and_keeps_the_raw_number() {
        use river::river_libinput_device_v1::TapState;
        assert_eq!(
            wenum_label::<TapState>(wayland_client::WEnum::Unknown(7)),
            "unknown (7)"
        );
    }

    /// Audit finding D-01: `claim_pinned_terminal_spawn` commits the claim
    /// before any process exists, so a spawn that never happens would
    /// otherwise leave the tag marked spawned forever, with no retry.
    /// Exercises the real failure branch — the terminal path does not
    /// exist, so `Command::spawn` returns `ENOENT` and no process is
    /// created.
    #[test]
    fn a_failed_pinned_terminal_spawn_releases_the_claim() {
        let wm_core = Mutex::new(WmCore::new());
        let tag_id = ipc::lock_recovering(&wm_core).create_tag("web").unwrap();
        let session_name = ipc::lock_recovering(&wm_core)
            .claim_pinned_terminal_spawn(tag_id)
            .unwrap()
            .expect("a freshly created tag has not claimed its spawn yet");

        spawn_pinned_terminal_or_release_claim(
            &wm_core,
            tag_id,
            "/nonexistent/buoy-wm-test-no-such-terminal",
            &[session_name],
        );

        let mut core = ipc::lock_recovering(&wm_core);
        assert_eq!(
            core.claim_pinned_terminal_spawn(tag_id),
            Ok(Some("tag-web".to_string())),
            "a failed spawn left the tag marked spawned, so it can never retry"
        );
    }

    #[test]
    fn tag_picker_path_resolves_to_sibling_of_debug_wm_exe() {
        assert_eq!(
            tag_picker_path(Path::new("/workspaces/buoy-wm/target/debug/wm")),
            PathBuf::from("/workspaces/buoy-wm/target/debug/buoy-tag-picker")
        );
    }

    #[test]
    fn tag_picker_path_resolves_to_sibling_of_release_wm_exe() {
        assert_eq!(
            tag_picker_path(Path::new("/workspaces/buoy-wm/target/release/wm")),
            PathBuf::from("/workspaces/buoy-wm/target/release/buoy-tag-picker")
        );
    }

    #[test]
    fn tag_picker_path_falls_back_to_bare_name_when_wm_exe_has_no_parent() {
        // `Path::parent()` returns `None` only when the path terminates in a
        // root or prefix (or is empty) — `/` is the concrete case that hits
        // the fallback branch, not just a single-component relative path
        // (whose `.parent()` is `Some("")`, which `join` already reduces to
        // the bare name anyway).
        assert_eq!(
            tag_picker_path(Path::new("/")),
            PathBuf::from("buoy-tag-picker")
        );
    }
}
