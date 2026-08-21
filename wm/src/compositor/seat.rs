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

//! One seat, its bindings, and what a pressed binding does.
//!
//! # Rationale
//!
//! [`Seat::do_action`] is the whole user-facing surface of this window
//! manager: thirteen actions, of which exactly two can switch a tag and so
//! return the pinned-terminal spawn signal the aggregate acts on after the
//! seat loop ends. It was a 365-line match with seven parameters until
//! audit finding J-04; what is left here is the dispatch table plus the
//! handlers that need this seat's own focus state, with everything that
//! merely starts a process in [`crate::compositor::launch`].

use std::collections::{HashMap, VecDeque};

use buoy_common::log_err;
use buoy_wm::config::{Action, Config};
use buoy_wm::wm_core::ids::{OutputId, TagId};
use buoy_wm::wm_core::state::{WmCore, is_pinned_terminal_app_id};
use buoy_wm::wm_core::view::Geometry;
use wayland_backend::client::ObjectId;
use wayland_client::{Proxy, QueueHandle};

use crate::compositor::drag::resize_extents;
use crate::compositor::launch::{
    spawn_exec, spawn_hotkey_sheet, spawn_launcher, spawn_switch_picker, spawn_tag_picker,
    spawn_terminal,
};
use crate::compositor::manager::AppData;
use crate::compositor::report::notify_user;
use crate::compositor::river::river_pointer_binding_v1::RiverPointerBindingV1;
use crate::compositor::river::river_seat_v1::{Modifiers, RiverSeatV1};
use crate::compositor::river::river_window_manager_v1::RiverWindowManagerV1;
use crate::compositor::river::river_window_v1::{Edges, RiverWindowV1};
use crate::compositor::river::river_xkb_binding_v1::RiverXkbBindingV1;
use crate::compositor::river::river_xkb_bindings_v1::RiverXkbBindingsV1;
use crate::compositor::window::{ActiveOutput, Window};

/// Records `action` as the binding press the next manage sequence will act
/// on, returning whichever press it displaced.
///
/// The slot holds exactly one action, and `blocking_dispatch` dispatches
/// *every* queued event before returning to the manage sequence that drains
/// it — so two bindings firing in one round (two fast keypresses, or a key
/// press plus a pointer-button press) used to discard the first with no
/// trace at all (audit finding K-01).
///
/// Deliberately reports rather than queues. Whether river ever batches two
/// binding events into one dispatch round is unverified, and a `VecDeque`
/// sized for a condition that may never occur is speculative machinery with
/// its own ordering questions; a log line that never fires costs nothing
/// and answers the question after one session of fast typing. If this line
/// turns up in a journal, the queue is the next step.
pub(crate) fn set_pending_action(slot: &mut Option<Action>, action: Action) -> Option<Action> {
    let displaced = slot.replace(action);
    if let Some(dropped) = &displaced {
        // `{:?}` is safe here: `Action`'s hand-written `Debug` redacts
        // `Exec`'s command line (audit finding G-05).
        log_err!("Dropped keybind action {dropped:?}: a second binding fired before it was run");
    }
    displaced
}

#[derive(Debug, Clone)]
pub(crate) enum SeatOp {
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

#[derive(Debug)]
pub(crate) struct Seat {
    pub(crate) proxy: RiverSeatV1,
    pub(crate) new: bool,
    pub(crate) removed: bool,
    pub(crate) focused: Option<RiverWindowV1>,
    pub(crate) hovered: Option<RiverWindowV1>,
    pub(crate) interacted: Option<RiverWindowV1>,
    pub(crate) xkb_bindings: HashMap<ObjectId, XkbBinding>,
    pub(crate) pointer_bindings: HashMap<ObjectId, PointerBinding>,
    /// The action a binding fired since the last `do_action`, if any.
    /// `Option` rather than a `None` enum variant so a parameterized action
    /// can be taken by value without cloning its payload.
    ///
    /// Capacity one, which is a real limit and not just a shape: write it
    /// through [`set_pending_action`] so a displaced press is reported
    /// rather than lost (audit finding K-01).
    pub(crate) pending_action: Option<Action>,
    pub(crate) op: SeatOp,
    pub(crate) op_dx: i32,
    pub(crate) op_dy: i32,
    pub(crate) op_release: bool,
    /// The pointer's last-known position in the compositor's global
    /// coordinate space, from `river_seat_v1`'s `pointer_position` event
    /// (protocol `since="2"`). `None` until the first such event arrives
    /// for this seat (Story 2.8 Task 2) — the AC 2 fallback case
    /// `WindowManager::active_output_id` handles explicitly.
    pub(crate) pointer_position: Option<(i32, i32)>,
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
    pub(crate) terminal_intentionally_focused: bool,
}

#[derive(Debug)]
pub(crate) struct XkbBinding {
    pub(crate) proxy: RiverXkbBindingV1,
    pub(crate) action: Action,
}

#[derive(Debug)]
pub(crate) struct PointerBinding {
    pub(crate) proxy: RiverPointerBindingV1,
    pub(crate) action: Action,
}

/// Switches `active_output` onto the tag named `name`, creating it if it
/// does not exist yet. Returns the tag actually switched to, which is
/// [`Seat::do_action`]'s pinned-terminal spawn signal.
///
/// # Rationale
///
/// Create-on-demand because a named-tag bind is meant to be pressed before
/// the tag exists (`Super+1` = "email" on a fresh session), so a missing
/// tag is created rather than treated as an error.
///
/// The name is resolved before [`WmCore::create_tag`] is reached, but not to
/// prevent duplicates — `create_tag` is already idempotent by name and
/// returns the existing id. It is so that the overwhelmingly common case,
/// pressing a bind for a tag that already exists, never calls a
/// `&mut WmCore` mutator at all: this project's retrospective identifies new
/// call sites onto shared-state mutators as its highest-risk change shape,
/// so a read stays a read.
fn switch_to_named_tag(
    wm_core: &mut WmCore,
    active_output: Option<ActiveOutput<'_>>,
    name: &str,
) -> Option<TagId> {
    let Some(active) = active_output else {
        notify_user(&format!(
            "The keybind for tag `{name}` did nothing: no output is registered yet."
        ));
        return None;
    };
    let tag_id = match wm_core.tag_id_by_name(name) {
        Some(tag_id) => tag_id,
        None => match wm_core.create_tag(name.to_owned()) {
            Ok(tag_id) => tag_id,
            Err(e) => {
                log_err!("Failed to create tag `{name}`: {e}");
                return None;
            }
        },
    };
    match wm_core.switch_tag(active.id, tag_id) {
        Ok(()) => Some(tag_id),
        Err(e) => {
            log_err!("Failed to switch to tag `{name}`: {e}");
            None
        }
    }
}

/// Advances `active_output` to its next tag, returning the tag switched to —
/// [`Seat::do_action`]'s pinned-terminal spawn signal.
fn cycle_active_tag(
    wm_core: &mut WmCore,
    active_output: Option<ActiveOutput<'_>>,
) -> Option<TagId> {
    let Some(active) = active_output else {
        notify_user("The tag-cycle keybind did nothing: no output is registered yet.");
        return None;
    };
    match wm_core.cycle_tag(active.id) {
        Ok(tag_id) => tag_id,
        Err(e) => {
            log_err!("Failed to cycle tag on output {:?}: {e}", active.id);
            None
        }
    }
}

impl Seat {
    pub(crate) fn new(proxy: RiverSeatV1) -> Self {
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

    pub(crate) fn create_xkb_binding(
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

    pub(crate) fn create_pointer_binding(
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
    /// action just switched `active_output` onto `tag_id` — the signal
    /// [`super::manager::WindowManager::manage_seats`] uses, after this
    /// seat loop ends, to ensure that tag's pinned terminal is spawned by
    /// [`super::manager::WindowManager::ensure_pinned_terminal_spawned`].
    ///
    /// Exactly two of the thirteen actions can produce that signal, which is
    /// why they are the only two arms below that return a value; every other
    /// arm is a side effect and falls through to `None`. Before audit
    /// finding J-04 that fact was buried in a 365-line body where each arm
    /// spelled out its own `None`.
    pub(crate) fn do_action(
        &mut self,
        windows: &mut VecDeque<Window>,
        wm_proxy: &RiverWindowManagerV1,
        wm_core: &mut WmCore,
        active_output: Option<ActiveOutput<'_>>,
        config: &Config,
    ) -> Option<TagId> {
        let pending_action = self.pending_action.take()?;
        match pending_action {
            Action::CycleTag => return cycle_active_tag(wm_core, active_output),
            Action::SwitchTag(name) => {
                return switch_to_named_tag(wm_core, active_output, &name);
            }
            Action::Terminal => spawn_terminal(config),
            Action::Exec(command_line) => spawn_exec(&command_line),
            Action::Launcher => spawn_launcher(config, active_output),
            Action::Hotkeys => spawn_hotkey_sheet(config, active_output),
            Action::OpenAssignPicker => spawn_tag_picker(None, active_output),
            Action::OpenSwitchPicker => spawn_switch_picker(active_output),
            Action::Close => self.close_focused(windows),
            Action::FocusNext => self.focus_next_window(windows, wm_core),
            Action::Move => self.move_hovered_window(windows),
            Action::Resize => self.resize_hovered_window(windows),
            Action::Exit => wm_proxy.exit_session(),
        }
        None
    }

    /// Closes this seat's focused window, unless it is a pinned terminal.
    ///
    /// # Rationale
    ///
    /// The exclusion is checked against this seat's *own* real focus target
    /// (`self.focused`'s [`Window::app_id`]) rather than
    /// `WmCore::closable_focused_view`'s single WM-wide focused view: with
    /// multiple seats the global field can reflect a different seat's focus
    /// by the time this runs (last seat processed in
    /// [`super::manager::WindowManager::manage_seats`] wins), which could
    /// let the pinned terminal be closed via this seat's request even
    /// though it isn't this seat's real focus, or spuriously block a
    /// legitimate close.
    fn close_focused(&self, windows: &VecDeque<Window>) {
        let Some(window_proxy) = self.focused.as_ref() else {
            return;
        };
        let is_pinned_terminal = windows
            .iter()
            .find(|window| &window.proxy == window_proxy)
            .is_some_and(|window| is_pinned_terminal_app_id(&window.app_id));
        if !is_pinned_terminal {
            window_proxy.close();
        }
    }

    /// Moves focus to the next window in `wm_core`'s cycle order and brings
    /// it to the top of the real z-order.
    ///
    /// # Rationale
    ///
    /// [`WmCore::cycle_focus`]' returned view is the single source of truth
    /// for which window is next: it is looked up in `windows` and moved to
    /// the back rather than rotating `windows` independently and letting the
    /// two mechanisms diverge.
    ///
    /// The pinned-terminal check is defensive — `cycle_focus` already
    /// excludes it from its candidates — so that this function is correct on
    /// its own terms rather than only by accident of behavior elsewhere. If
    /// the pinned terminal ever were returned it gets real keyboard focus
    /// but not the reorder, because FR4 keeps it at the bottom.
    fn focus_next_window(&mut self, windows: &mut VecDeque<Window>, wm_core: &mut WmCore) {
        let Some(next_view_id) = wm_core.cycle_focus() else {
            return;
        };
        let Some(i) = windows
            .iter()
            .position(|window| window.view_id == Some(next_view_id))
        else {
            return;
        };
        if is_pinned_terminal_app_id(&windows[i].app_id) {
            let window = &windows[i];
            self.proxy.focus_window(&window.proxy);
            self.focused = Some(window.proxy.clone());
            if let Err(e) = wm_core.set_focus(next_view_id) {
                log_err!("Failed to set focus for view {next_view_id:?} in wm_core: {e}");
            }
        } else if let Some(window) = windows.remove(i) {
            // `i` came from a `position()` on this same deque with no
            // intervening mutation, so `None` is unreachable; written
            // fallibly so no panic site remains on this thread (audit
            // finding F-01).
            windows.push_back(window);
            // Unscoped: `cycle_focus` has already chosen the target and only
            // considers visible views, so this call is re-affirming that
            // choice, not searching.
            self.focus_top(windows, wm_core, None);
        }
    }

    /// Starts a pointer move of the hovered window.
    fn move_hovered_window(&mut self, windows: &VecDeque<Window>) {
        if let Some(window) = self.hovered_idle_window(windows, "move") {
            self.pointer_move(window);
        }
    }

    /// Starts a pointer resize of the hovered window from its bottom-right
    /// corner.
    fn resize_hovered_window(&mut self, windows: &VecDeque<Window>) {
        if let Some(window) = self.hovered_idle_window(windows, "resize") {
            self.pointer_resize(window, Edges::Bottom.union(Edges::Right));
        }
    }

    /// The hovered window, when this seat has one, it is still managed, and
    /// no drag is already in progress. `what` names the operation being
    /// declined, for the log line.
    ///
    /// # Rationale
    ///
    /// `self.hovered` was captured by a `PointerEnter` event that races the
    /// `closed` event dropping the window from `windows`, with no ordering
    /// promised between them — the same outlived-target hazard the
    /// `'interacted` block in
    /// [`super::manager::WindowManager::manage_seats`] documents. A miss
    /// loses this drag rather than the session (audit finding F-01).
    fn hovered_idle_window<'w>(
        &self,
        windows: &'w VecDeque<Window>,
        what: &str,
    ) -> Option<&'w Window> {
        let (Some(window_proxy), SeatOp::None) = (self.hovered.as_ref(), &self.op) else {
            return None;
        };
        let hovered = windows.iter().find(|window| &window.proxy == window_proxy);
        if hovered.is_none() {
            log_err!(
                "Ignoring a {what} of the hovered window, which is no longer \
                 managed (already closed)"
            );
        }
        hovered
    }

    pub(crate) fn op_end(&mut self) {
        if let SeatOp::Resize { window_proxy, .. } = &self.op {
            window_proxy.inform_resize_end();
        }
        self.proxy.op_end();
        self.op = SeatOp::None;
    }

    pub(crate) fn op_manage(&mut self) {
        match &self.op {
            SeatOp::None | SeatOp::Move { .. } => {}
            SeatOp::Resize {
                window_proxy,
                start_x,
                start_y,
                start_width,
                start_height,
                edges,
            } => {
                let start = Geometry {
                    x: *start_x,
                    y: *start_y,
                    width: *start_width,
                    height: *start_height,
                };
                let (width, height) = resize_extents(start, *edges, (self.op_dx, self.op_dy));
                window_proxy.propose_dimensions(width, height);
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
    pub(crate) fn focus_top(
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
                if !is_pinned_terminal_app_id(&window.app_id) {
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
                    log_err!("Failed to set focus for view {view_id:?} in wm_core: {e}");
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
    pub(crate) fn focused_view_is_visible(
        &self,
        windows: &VecDeque<Window>,
        wm_core: &WmCore,
    ) -> bool {
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

    pub(crate) fn pointer_move(&mut self, window: &Window) {
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

    pub(crate) fn pointer_resize(&mut self, window: &Window, edges: Edges) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Audit finding K-01: the slot holds one action and
    /// `blocking_dispatch` dispatches every queued event before the manage
    /// sequence that drains it, so two binding presses in one round left no
    /// trace of the first. Whether river ever batches two is unverified —
    /// this reports the loss rather than queueing for it.
    #[test]
    fn a_second_binding_press_before_the_first_is_acted_on_is_reported() {
        let mut slot = None;
        assert_eq!(set_pending_action(&mut slot, Action::CycleTag), None);
        assert_eq!(
            set_pending_action(&mut slot, Action::FocusNext),
            Some(Action::CycleTag)
        );
        assert_eq!(slot, Some(Action::FocusNext));
    }
}
