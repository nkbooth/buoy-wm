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

//! The aggregate: every window, output and seat this WM is managing, and
//! the manage/render sequences that act on all of them at once.
//!
//! # Rationale
//!
//! [`WindowManager::handle_manage_start`] and
//! [`WindowManager::handle_render_start`] are the two points where river
//! hands control over, and the protocol decides which requests may be made
//! from which — `hide`/`show` in a render sequence, `propose_dimensions`
//! in a manage sequence — so the ordering inside these methods is a
//! protocol constraint rather than a preference.
//!
//! The other constraint that shapes this module is that
//! [`buoy_wm::ipc::lock_recovering`] is not reentrant. Every method that
//! locks `wm_core` and then calls something that locks it again drops its
//! guard first, explicitly, and says so; `manage_seats` and
//! `ensure_pinned_terminal_spawned` are the pair that establishes the
//! pattern.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use buoy_common::{log_err, log_info};
use buoy_wm::config::{self, Config};
use buoy_wm::ipc;
use buoy_wm::wm_core::ids::{OutputId, TagId};
use buoy_wm::wm_core::state::{WmCore, is_pinned_term_app_id, tag_id_from_pinned_app_id};
use buoy_wm::wm_core::view::{DEFAULT_FLOATING_GEOMETRY, Geometry};
use wayland_backend::client::ObjectId;
use wayland_client::{Proxy, QueueHandle, protocol::wl_output};

use crate::compositor::child::reap_finished_children;
use crate::compositor::drag::{dragged_origin, resize_origin};
use crate::compositor::launch::spawn_pinned_terminal_or_release_claim;
use crate::compositor::report::log_wm_core_err;
use crate::compositor::river::river_input_manager_v1::RiverInputManagerV1;
use crate::compositor::river::river_layer_shell_v1::RiverLayerShellV1;
use crate::compositor::river::river_libinput_config_v1::RiverLibinputConfigV1;
use crate::compositor::river::river_libinput_device_v1::RiverLibinputDeviceV1;
use crate::compositor::river::river_window_manager_v1::RiverWindowManagerV1;
use crate::compositor::river::river_xkb_bindings_v1::RiverXkbBindingsV1;
use crate::compositor::seat::{Seat, SeatOp};
use crate::compositor::window::{ActiveOutput, Output, Window, output_contains, output_for_id};
use crate::compositor::wire::{apply_libinput_setting, input_event_code, river_modifiers};

#[derive(Debug, Default)]
pub(crate) struct AppData {
    pub(crate) river_wm: Option<RiverWindowManagerV1>,
    pub(crate) river_xkb: Option<RiverXkbBindingsV1>,
    /// Story 2.6: optional, unlike `river_xkb` - binding this global is what
    /// tells `river` to allow layer-shell clients (eg waybar's status bar)
    /// to map surfaces at all, instead of closing them immediately. Absence
    /// is not fatal; it just means layer-shell surfaces can't map, same as
    /// `buoy-wm`'s behavior before this story.
    pub(crate) river_layer_shell: Option<RiverLayerShellV1>,
    /// Optional, like `river_layer_shell`: without it no input device can be
    /// named, so nothing gets configured and every device keeps libinput's
    /// defaults — which is exactly buoy's behavior before `[[input]]`
    /// existed, not a reason to refuse the session.
    pub(crate) river_input_manager: Option<RiverInputManagerV1>,
    /// Optional for the same reason. river only advertises this alongside
    /// `river_input_manager_v1`, but nothing in the protocol promises that,
    /// so the two are tracked (and degraded) independently.
    pub(crate) river_libinput_config: Option<RiverLibinputConfigV1>,
    pub(crate) wm: WindowManager,
}

#[derive(Debug, Default)]
pub(crate) struct WindowManager {
    pub(crate) windows: VecDeque<Window>,
    pub(crate) outputs: HashMap<ObjectId, Output>,
    pub(crate) seats: HashMap<ObjectId, Seat>,
    pub(crate) wm_core: Arc<Mutex<WmCore>>,
    /// Story 2.9 Task 1: every bound `wl_output` global, keyed by the
    /// registry `name: u32` it was bound for — populated as soon as each
    /// global is advertised, independent of (and generally *before*) the
    /// `river_output_v1::WlOutput` event that later references the same
    /// registry name (Task 2). Never removed; a `wl_output` global going
    /// away isn't handled by this story (see Technical notes' scope
    /// boundary) — YAGNI until a real hotplug-removal story needs it.
    pub(crate) pending_wl_outputs: HashMap<u32, wl_output::WlOutput>,
    /// Story 2.9 Task 1: each bound `wl_output`'s real connector name (e.g.
    /// `"eDP-1"`), keyed by that `wl_output` proxy's own id, populated
    /// whenever its `Name` event fires. Deliberately a separate map from
    /// `pending_wl_outputs` (keyed by object id, not registry name) so a
    /// name can be looked up fresh at the point it's actually needed
    /// (`WindowManager::output_name`) without ever caching "no name yet" as
    /// a permanent negative — the event's arrival time relative to
    /// `river_output_v1::WlOutput` is not guaranteed (Technical notes).
    pub(crate) wl_output_names: HashMap<ObjectId, String>,
    /// Each `river_input_device_v1`'s name, keyed by that proxy's object id,
    /// populated when its `name` event arrives. Held separately from
    /// `libinput_devices` because the two objects are created by two
    /// different globals and the protocol makes no promise about which
    /// arrives first — same ordering trap as `wl_output_names` above.
    pub(crate) input_device_names: HashMap<ObjectId, String>,
    /// Each `river_libinput_device_v1`, keyed by its own object id.
    pub(crate) libinput_devices: HashMap<ObjectId, LibinputDevice>,
    /// The user's `~/.config/buoy/config.toml`, or [`Config::default`]'s
    /// built-in equivalent when there is no such file. Loaded once at
    /// startup — re-reading it on change would mean tearing down and
    /// recreating every live binding object, which no story needs yet.
    pub(crate) config: Config,
}

/// A libinput-configurable device, and the correlation state needed before
/// its `[[input]]` entry can be applied.
#[derive(Debug)]
pub(crate) struct LibinputDevice {
    pub(crate) proxy: RiverLibinputDeviceV1,
    /// The `river_input_device_v1` this configures, from the `input_device`
    /// event. `None` until that arrives — the name lives on that object, so
    /// there is nothing to match a config entry against until then.
    pub(crate) input_device_id: Option<ObjectId>,
    /// Whether the config has already been applied. The two halves of the
    /// device's identity arrive in an unspecified order, so both arrival
    /// paths attempt to configure and this is what keeps it to once.
    pub(crate) configured: bool,
}

impl WindowManager {
    pub(crate) fn handle_manage_start(
        &mut self,
        proxy: &RiverWindowManagerV1,
        river_xkb: &RiverXkbBindingsV1,
        qh: &QueueHandle<AppData>,
    ) {
        // This is already the "reclaim things that died" phase, and it runs
        // whether or not anything was spawned — unlike `track_child`, which
        // is the only other reaper (audit finding F-05).
        reap_finished_children();
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

    pub(crate) fn handle_render_start(&mut self, proxy: &RiverWindowManagerV1) {
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
                        let (x, y) = dragged_origin((*start_x, *start_y), (seat.op_dx, seat.op_dy));
                        window.set_position(x, y);
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
                        let start = Geometry {
                            x: *start_x,
                            y: *start_y,
                            width: *start_width,
                            height: *start_height,
                        };
                        let (x, y) = resize_origin(start, *edges, (window.width, window.height));
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
                        Err(e) => log_err!(
                            "Failed to unregister output {:?} from wm_core: {e}",
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
                            log_err!("Failed to unregister view {id:?} from wm_core: {e}");
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
                        // Sized to fill its output by hand rather than made
                        // truly fullscreen: river keeps fullscreen windows
                        // in a higher scene layer than ordinary ones, and
                        // `place_top()`/`place_bottom()` only reorder within
                        // a layer — so a fullscreen pinned terminal would
                        // render above every other window no matter what
                        // this WM did, which is the opposite of FR4.
                        //
                        // A tag not currently shown on any output is left
                        // alone: the render-sequence visibility pass hides
                        // it through the same `is_view_visible` decision
                        // every other view goes through, so no hide needs
                        // special-casing here.
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
                        log_err!(
                            "Failed to tag newly-mapped pinned terminal onto {tag_id:?}: {e}; leaving untagged"
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
    /// Deliberately not `river_window_v1.fullscreen()`. River puts a truly
    /// fullscreen window in a separate, higher scene layer, and
    /// `place_top()`/`place_bottom()` only reorder within a layer — so no
    /// amount of WM-side stacking could make an ordinary window render
    /// above the pinned terminal, which FR4 requires. Sizing it manually to
    /// exactly fill its output looks the same to the user and keeps it an
    /// ordinary window.
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
                    log_err!("Failed to resolve pinned terminal's tags for view {view_id:?}: {e}");
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
    /// `river_window_v1` request. `hide`/`show` "modif\[y\] rendering state
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
                    log_err!("Failed to resolve visibility for view {view_id:?}: {e}")
                }
            }
        }
    }

    /// Composes Story 1.5's Tasks 2-3: claims the lazy-spawn-once
    /// pinned-terminal slot for `tag_id` and, if this is the first claim,
    /// spawns it. Called from `manage_seats`, once per tag that a seat's
    /// `Action::CycleTag` just switched an output onto (Story 1.7) —
    /// deliberately called after (not during) the seat loop that holds
    /// `wm_core`'s mutable borrow.
    pub(crate) fn ensure_pinned_terminal_spawned(&mut self, tag_id: TagId) {
        // Bound to a `let` rather than matched inline: the arm below
        // re-locks the same mutex to roll the claim back, and a scrutinee
        // temporary would still be holding the guard there.
        let claim = ipc::lock_recovering(&self.wm_core).claim_pinned_terminal_spawn(tag_id);
        match claim {
            Ok(Some(session_name)) => spawn_pinned_terminal_or_release_claim(
                &self.wm_core,
                tag_id,
                &self.config.defaults,
                &session_name,
            ),
            Ok(None) => {}
            Err(e) => {
                log_err!("Failed to check pinned-terminal spawn state for tag {tag_id:?}: {e}")
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
                    log_err!("Skipping keybind with unresolvable key `{}`", keybind.key);
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
            // Both proxies were captured in an *earlier* event, so the seat
            // they name may have been destroyed by `remove_seats` in the
            // meantime. `expect`ing the lookup carried the same
            // outlived-target hazard that once took the session down over a
            // click (see `manage_seats`' `'interacted` block); a miss costs
            // one drag instead (audit finding F-01).
            if let Some(seat_proxy) = window.pointer_move_requested.take() {
                match self.seats.get_mut(&seat_proxy.id()) {
                    Some(seat) => seat.pointer_move(window),
                    None => log_err!(
                        "Ignoring a move request from seat {:?}, which is no \
                         longer registered",
                        seat_proxy.id()
                    ),
                }
            }
            if let Some(seat_proxy) = window.pointer_resize_requested.take() {
                match self.seats.get_mut(&seat_proxy.id()) {
                    Some(seat) => {
                        seat.pointer_resize(window, window.pointer_resize_requested_edges)
                    }
                    None => log_err!(
                        "Ignoring a resize request from seat {:?}, which is no \
                         longer registered",
                        seat_proxy.id()
                    ),
                }
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
                self.outputs
                    .values()
                    .find(|output| output_contains(output.position, output.dimensions, (px, py)))
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
    pub(crate) fn configured_device_name(&self, libinput_id: &ObjectId) -> Option<&str> {
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
    pub(crate) fn configure_libinput_devices(&mut self, qh: &QueueHandle<AppData>) {
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
            log_info!(
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
        // Paired into one value here rather than threaded through
        // `do_action` as two `Option`s that cannot actually disagree — see
        // [`ActiveOutput`] (audit finding J-04).
        let active_output = active_output_id.map(|id| ActiveOutput {
            id,
            name: active_output_name.as_deref(),
        });
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
                    log_info!(
                        "Ignoring interaction with unmanaged window {:?} (already closed)",
                        window_proxy.id()
                    );
                    break 'interacted;
                };
                // `i` came from the `position()` immediately above with no
                // intervening mutation, so `None` is unreachable — written
                // as a fallible match anyway so the census of panic sites on
                // this thread stays empty (audit finding F-01).
                let Some(window) = self.windows.remove(i) else {
                    break 'interacted;
                };
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
                    log_err!("Failed to raise view {view_id:?} in wm_core stacking order: {e}");
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
                        log_err!("Failed to set focus for view {view_id:?} in wm_core: {e}");
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
            // that switches tags (`Action::CycleTag`) hides the focused
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
                active_output,
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
