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

//! The thirteen `Dispatch` impls that receive every river event, plus the
//! three interfaces that deliberately receive none.
//!
//! # Rationale
//!
//! Every `match event` here is exhaustive, and that is the whole reason
//! [`super::river`] and this module are in the binary rather than behind
//! `buoy_wm`'s library boundary: `wayland-scanner` marks each generated
//! `Event` enum `#[non_exhaustive]`, which is exhaustive to match on
//! inside its own crate and not across a crate boundary. Split them and
//! every match below needs a `_ =>` arm, turning "the vendored protocol
//! XML grew an event" from a compile error into a silently ignored
//! message. Two arms are `_ =>` on purpose — `RiverLibinputDeviceV1` has
//! roughly forty report events and this WM surfaces two — and the
//! difference between those and a wildcard added by a refactor is the
//! point.
//!
//! These handlers stay thin on purpose (`CONTRIBUTING.md`): a handler
//! records what arrived and defers every decision to something testable.
//! Where one does more than that — the tag bootstrap on the first output —
//! the reason is sequencing that has nowhere else to live.

use buoy_common::{log_err, log_info};
use buoy_wm::ipc;
use wayland_backend::client::ObjectId;
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    protocol::{wl_output, wl_registry},
};

use crate::compositor::drag::requested_resize_edges;
use crate::compositor::manager::{AppData, LibinputDevice};
use crate::compositor::report::wenum_label;
use crate::compositor::river;
use crate::compositor::river::{
    river_input_device_v1::RiverInputDeviceV1, river_input_manager_v1::RiverInputManagerV1,
    river_layer_shell_v1::RiverLayerShellV1, river_libinput_config_v1::RiverLibinputConfigV1,
    river_libinput_device_v1::RiverLibinputDeviceV1,
    river_libinput_result_v1::RiverLibinputResultV1, river_node_v1::RiverNodeV1,
    river_output_v1::RiverOutputV1, river_pointer_binding_v1::RiverPointerBindingV1,
    river_seat_v1::RiverSeatV1, river_window_manager_v1::RiverWindowManagerV1,
    river_window_v1::RiverWindowV1, river_xkb_binding_v1::RiverXkbBindingV1,
    river_xkb_bindings_v1::RiverXkbBindingsV1,
};
use crate::compositor::seat::{Seat, set_pending_action};
use crate::compositor::window::{Output, Window};

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
                        log_err!(
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
                        log_err!(
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
                        log_err!(
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
                        log_err!(
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
                        log_err!(
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
                log_err!("Error: Another WM is already running");
                std::process::exit(1);
            }
            Event::Finished => std::process::exit(0),
            Event::ManageStart => {
                // Deliberately still a panic (audit finding F-01). `main`
                // exits before the first `blocking_dispatch` if this global
                // is absent and nothing ever clears it, so a `None` here is
                // a real broken invariant — and the only alternative,
                // returning without `manage_finish()`, would wedge river's
                // manage sequence and freeze the desktop instead of ending
                // it, which is strictly worse.
                let river_xkb = state
                    .river_xkb
                    .as_ref()
                    .expect("river_xkb_bindings_v1 was checked present before the event loop");
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
                // `Action::CycleTag`'s keybind path already produces for
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
                                log_err!(
                                    "Failed to switch bootstrap output {output_id:?} to default tag: {e}"
                                );
                            }
                            Some(tag_id)
                        }
                        Err(e) => {
                            log_err!("Failed to create bootstrap default tag: {e}");
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
                if let Some(recognised) = requested_resize_edges(edges) {
                    window.pointer_resize_requested = Some(seat);
                    window.pointer_resize_requested_edges = recognised;
                }
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
        // `remove_outputs` destroys this proxy and drops the map entry
        // during a manage sequence while `river_output_v1` events for it
        // arrive independently, so a miss here is a race rather than a
        // broken invariant — the same graceful-return shape
        // `RiverWindowV1`'s dispatcher above already uses (audit finding
        // F-01).
        let Some(output) = state.wm.outputs.get_mut(&proxy.id()) else {
            log_err!(
                "Ignoring an event for output {:?}, which is no longer registered",
                proxy.id()
            );
            return;
        };
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
        // Same race as `RiverOutputV1` above: `remove_seats` destroys the
        // proxy and drops the entry mid-manage-sequence while events for it
        // are still in flight (audit finding F-01).
        let Some(seat) = state.wm.seats.get_mut(&proxy.id()) else {
            log_err!(
                "Ignoring an event for seat {:?}, which is no longer registered",
                proxy.id()
            );
            return;
        };
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
        // `remove_seats` destroys every binding proxy along with its seat,
        // so a keypress already queued when the seat went away resolves to
        // neither — a race, not a broken invariant (audit finding F-01).
        let Some(seat) = state.wm.seats.get_mut(data) else {
            log_err!("Ignoring a keybind press for seat {data:?}, which is no longer registered");
            return;
        };
        let Some(binding) = seat.xkb_bindings.get(&proxy.id()) else {
            log_err!(
                "Ignoring a press of keybinding {:?}, which is no longer registered",
                proxy.id()
            );
            return;
        };
        match event {
            Event::Pressed => {
                set_pending_action(&mut seat.pending_action, binding.action.clone());
            }
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
        // Same race as `RiverXkbBindingV1` above (audit finding F-01).
        let Some(seat) = state.wm.seats.get_mut(data) else {
            log_err!("Ignoring a mousebind press for seat {data:?}, which is no longer registered");
            return;
        };
        let Some(binding) = seat.pointer_bindings.get(&proxy.id()) else {
            log_err!(
                "Ignoring a press of mousebinding {:?}, which is no longer registered",
                proxy.id()
            );
            return;
        };
        match event {
            Event::Pressed => {
                set_pending_action(&mut seat.pending_action, binding.action.clone());
            }
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
                    log_info!(
                        "libinput {name:?}: tap-to-click is {}",
                        wenum_label(tap_state)
                    );
                }
            }
            Event::ClickMethodCurrent { method } => {
                if let Some(name) = state.wm.configured_device_name(&proxy.id()) {
                    log_info!("libinput {name:?}: click method is {}", wenum_label(method));
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
        // Nothing destroys this proxy here and nothing should: all three
        // events are `type="destructor"` in
        // `river-libinput-config-v1.xml`, so river disposes of the object as
        // it sends one and the interface has no `destroy` request to call.
        // Audit finding F-05 read this as a leaked protocol object per
        // setting per device; it is not.
        //
        // A silently-dropped `unsupported` is how a setting that simply does
        // not work on this device looks identical to one that was never
        // configured, so both failure modes are named here. `success` stays
        // quiet - one line per applied setting per startup is noise.
        match event {
            Event::Success => {}
            Event::Unsupported => {
                log_err!("{data}: unsupported by this device, ignored");
            }
            Event::Invalid => {
                log_err!("{data}: invalid value, ignored");
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
