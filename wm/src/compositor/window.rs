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

//! The two proxy-carrying records this WM keeps per mapped window and per
//! connected output, and the geometry questions asked of them.
//!
//! # Rationale
//!
//! Every field is `pub(crate)`: these are the binary's own plain data,
//! read and written by the `Dispatch` handlers that receive the events and
//! by the aggregate that manages them. The visibility is wider than ideal
//! and exactly as wide as it was when all of this shared one file — the
//! narrower alternative is to split each record into a proxy-free data
//! half, which is future work (audit finding T-01 phase 3).

use std::collections::HashMap;

use buoy_wm::wm_core::ids::{OutputId, ViewId};
use wayland_backend::client::ObjectId;
use wayland_client::QueueHandle;

use crate::AppData;
use crate::compositor::river::river_node_v1::RiverNodeV1;
use crate::compositor::river::river_output_v1::RiverOutputV1;
use crate::compositor::river::river_seat_v1::RiverSeatV1;
use crate::compositor::river::river_window_v1::{Edges, RiverWindowV1};

#[derive(Debug)]
pub(crate) struct Window {
    pub(crate) proxy: RiverWindowV1,
    pub(crate) node: RiverNodeV1,
    pub(crate) new: bool,
    pub(crate) closed: bool,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) pointer_move_requested: Option<RiverSeatV1>,
    pub(crate) pointer_resize_requested: Option<RiverSeatV1>,
    pub(crate) pointer_resize_requested_edges: Edges,
    pub(crate) app_id: String,
    pub(crate) view_id: Option<ViewId>,
}

#[derive(Debug)]
pub(crate) struct Output {
    pub(crate) proxy: RiverOutputV1,
    pub(crate) removed: bool,
    pub(crate) output_id: OutputId,
    /// The output's top-left corner in the compositor's global coordinate
    /// space, from `river_output_v1`'s `position` event. Defaults to
    /// `(0, 0)` until the first such event arrives (Story 2.8 Task 1) —
    /// same "plain data field, no decision logic of its own" carve-out as
    /// every other all-zero-initialized field on this struct.
    pub(crate) position: (i32, i32),
    /// The output's width/height extent from `position`, from
    /// `river_output_v1`'s `dimensions` event. Defaults to `(0, 0)` until
    /// the first such event arrives (Story 2.8 Task 1).
    pub(crate) dimensions: (i32, i32),
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
    pub(crate) wl_output_object_id: Option<ObjectId>,
}

impl Window {
    pub(crate) fn new(proxy: RiverWindowV1, qh: &QueueHandle<AppData>) -> Self {
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

    pub(crate) fn set_position(&mut self, x: i32, y: i32) {
        self.node.set_position(x, y);
        self.x = x;
        self.y = y;
    }
}

impl Output {
    pub(crate) fn new(proxy: RiverOutputV1, output_id: OutputId) -> Self {
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

/// Whether `point` falls inside the output rectangle at `position` with
/// `dimensions`.
///
/// Half-open bounds: `dimensions` is a width/height extent from `position`,
/// so the rectangle's far edge (`position + dimensions`) is exclusive,
/// matching how `river_output_v1`'s `position`/`dimensions` events are
/// documented. Extracted from `active_output_id`'s closure, which needs live
/// `Output` proxies: the off-by-one this avoids was documented by comment
/// and asserted by nothing (audit finding T-01).
pub(crate) fn output_contains(
    position: (i32, i32),
    dimensions: (i32, i32),
    point: (i32, i32),
) -> bool {
    let (ox, oy) = position;
    let (ow, oh) = dimensions;
    let (px, py) = point;
    (ox..ox.saturating_add(ow)).contains(&px) && (oy..oy.saturating_add(oh)).contains(&py)
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
/// Returns the whole [`Output`], not just its proxy: the pinned terminal is
/// sized to fill its output by hand rather than via
/// `river_window_v1.fullscreen`, so callers need the real
/// `position`/`dimensions` as well — see
/// [`crate::WindowManager::recompute_pinned_terminal_geometry`] for why
/// that is the mechanism.
pub(crate) fn output_for_id(
    outputs: &HashMap<ObjectId, Output>,
    output_id: OutputId,
) -> Option<&Output> {
    outputs
        .values()
        .find(|output| output.output_id == output_id)
}

/// The output an action acts on: the id
/// [`crate::WindowManager::active_output_id`] resolved, plus that output's real
/// Wayland connector name (e.g. `"eDP-1"`) when it is known.
///
/// One parameter rather than two because `name` is derived *from* `id` (via
/// [`crate::WindowManager::output_name`]), so "a name but no id" is a state that
/// cannot occur — yet every arm of [`crate::Seat::do_action`] used to receive two
/// independent `Option`s and defend against it separately (audit finding
/// J-04). `name` stays optional: it is genuinely unknown during the startup
/// window before the connector name has arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActiveOutput<'a> {
    pub(crate) id: OutputId,
    pub(crate) name: Option<&'a str>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The half-open rule `active_output_id` documents: a pointer sitting on
    /// an output's far edge belongs to the *next* output, so two adjacent
    /// outputs never both claim the same column.
    #[test]
    fn an_output_rectangle_excludes_its_far_edge_and_includes_its_origin() {
        let position = (1920, 0);
        let dimensions = (1920, 1080);

        assert!(output_contains(position, dimensions, (1920, 0)));
        assert!(output_contains(position, dimensions, (3839, 1079)));
        assert!(
            !output_contains(position, dimensions, (3840, 0)),
            "the far edge belongs to whatever output starts there"
        );
        assert!(!output_contains(position, dimensions, (1920, 1080)));
        assert!(!output_contains(position, dimensions, (1919, 0)));
    }

    #[test]
    fn an_output_extent_at_the_i32_boundary_does_not_wrap_into_excluding_everything() {
        // A compositor-supplied `dimensions` this large is nonsense, but
        // `position + dimensions` overflowing would make the range empty and
        // silently orphan every pointer position (audit finding F-03's
        // class).
        assert!(output_contains(
            (i32::MAX - 1, 0),
            (10, 10),
            (i32::MAX - 1, 5)
        ));
        assert!(!output_contains((0, 0), (0, 0), (0, 0)));
    }
}
