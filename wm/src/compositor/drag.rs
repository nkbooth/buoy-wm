// SPDX-FileCopyrightText: © 2026 Nick Booth
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

//! The arithmetic behind a pointer move or resize drag.
//!
//! # Rationale
//!
//! Pure functions over `Geometry` and the protocol's `Edges` bitfield, so
//! the two properties that matter can be asserted rather than reasoned
//! about: every operation saturates (the deltas arrive over the wire and
//! `[profile.release]` sets no `overflow-checks`), and an edge bitmask this
//! build has never heard of costs the user one drag rather than the whole
//! session (audit findings F-01, F-03).

use buoy_common::log_err;
use buoy_wm::wm_core::view::Geometry;

use crate::compositor::report::wenum_label;
use crate::compositor::river::river_window_v1::Edges;

/// The smallest width or height this WM will ever propose for a window.
///
/// A resize drag can legitimately carry the pointer past the window's
/// opposite edge, and `river_window_management_v1` defines no behaviour for
/// a zero or negative extent — so the arithmetic is floored here rather
/// than left to whatever the compositor does with a nonsense size.
const MIN_WINDOW_EXTENT: i32 = 1;

/// The width and height a resize drag has reached: `start`'s extents moved
/// by `delta` on whichever of `edges` is being dragged, floored at
/// [`MIN_WINDOW_EXTENT`].
///
/// Saturating, not wrapping. `delta` arrives over the wire as
/// `river_seat_v1.op_delta` and `[profile.release]` sets no
/// `overflow-checks`, so release builds wrapped silently — and because the
/// floor is applied after the arithmetic, a wrapped `800 - i32::MIN` sails
/// through it as a huge positive extent (audit finding F-03).
pub(crate) fn resize_extents(start: Geometry, edges: Edges, delta: (i32, i32)) -> (i32, i32) {
    let (mut width, mut height) = (start.width, start.height);
    if edges.contains(Edges::Left) {
        width = width.saturating_sub(delta.0);
    }
    if edges.contains(Edges::Right) {
        width = width.saturating_add(delta.0);
    }
    if edges.contains(Edges::Top) {
        height = height.saturating_sub(delta.1);
    }
    if edges.contains(Edges::Bottom) {
        height = height.saturating_add(delta.1);
    }
    (width.max(MIN_WINDOW_EXTENT), height.max(MIN_WINDOW_EXTENT))
}

/// Where a move drag has taken a window's top-left corner. Saturating for
/// the same reason as [`resize_extents`].
pub(crate) fn dragged_origin(start: (i32, i32), delta: (i32, i32)) -> (i32, i32) {
    (
        start.0.saturating_add(delta.0),
        start.1.saturating_add(delta.1),
    )
}

/// Where a resize drag's top-left corner has to sit given the extents the
/// window actually took (`current`).
///
/// Only the left and top edges move the origin: dragging one of those has
/// to keep the *opposite* edge where the drag started, which means the
/// origin absorbs whatever the extent gained. Saturating for the same
/// reason as [`resize_extents`].
pub(crate) fn resize_origin(start: Geometry, edges: Edges, current: (i32, i32)) -> (i32, i32) {
    let (mut x, mut y) = (start.x, start.y);
    if edges.contains(Edges::Left) {
        x = x.saturating_add(start.width.saturating_sub(current.0));
    }
    if edges.contains(Edges::Top) {
        y = y.saturating_add(start.height.saturating_sub(current.1));
    }
    (x, y)
}

/// The edges a `pointer_resize_requested` event names, or `None` when this
/// build's copy of the protocol does not recognise the bitmask.
///
/// A `WEnum` arriving off the wire is protocol version skew, not a logic
/// invariant this code can prove: `expect`ing it meant the next river
/// upgrade that widens the edge bitfield would take the whole login
/// session down over one resize (audit finding F-01). Dropping the request
/// costs the user a drag; panicking costs them everything unsaved.
pub(crate) fn requested_resize_edges(edges: wayland_client::WEnum<Edges>) -> Option<Edges> {
    match edges.into_result() {
        Ok(recognised) => Some(recognised),
        Err(_) => {
            log_err!(
                "Ignoring a resize request naming edges this build does not \
                 recognise: {}",
                wenum_label(edges)
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Audit finding F-03: `dx`/`dy` arrive over the wire, `[profile
    /// .release]` sets no `overflow-checks`, and the `MIN_WINDOW_EXTENT`
    /// floor is applied *after* the arithmetic — so a wrap turns a drag
    /// past the opposite edge into a huge positive extent the floor cannot
    /// see.
    #[test]
    fn a_resize_delta_at_the_i32_boundary_saturates_instead_of_wrapping() {
        let start = Geometry {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        assert_eq!(
            resize_extents(start, Edges::Left.union(Edges::Top), (i32::MIN, i32::MIN)),
            (i32::MAX, i32::MAX)
        );
        assert_eq!(
            resize_extents(
                start,
                Edges::Right.union(Edges::Bottom),
                (i32::MAX, i32::MAX)
            ),
            (i32::MAX, i32::MAX)
        );
    }

    /// The protocol defines no behaviour for a zero or negative extent, and
    /// a resize drag can legitimately carry the pointer past the opposite
    /// edge.
    #[test]
    fn a_resize_drag_past_the_opposite_edge_is_floored_not_negative() {
        let start = Geometry {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        assert_eq!(
            resize_extents(start, Edges::Right.union(Edges::Bottom), (-5000, -5000)),
            (MIN_WINDOW_EXTENT, MIN_WINDOW_EXTENT)
        );
    }

    #[test]
    fn a_resize_delta_grows_the_dragged_edge_only() {
        let start = Geometry {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        assert_eq!(resize_extents(start, Edges::Right, (30, 40)), (830, 600));
        assert_eq!(resize_extents(start, Edges::Top, (30, 40)), (800, 560));
    }

    #[test]
    fn a_move_delta_at_the_i32_boundary_saturates_instead_of_wrapping() {
        assert_eq!(
            dragged_origin((i32::MAX, i32::MIN), (i32::MAX, i32::MIN)),
            (i32::MAX, i32::MIN)
        );
        assert_eq!(dragged_origin((100, 100), (-30, 40)), (70, 140));
    }

    /// Dragging the left or top edge has to move the origin as well as the
    /// extent, so the *opposite* edge stays under the cursor's start point.
    #[test]
    fn a_resize_origin_follows_only_the_left_and_top_edges() {
        let start = Geometry {
            x: 100,
            y: 100,
            width: 800,
            height: 600,
        };
        assert_eq!(
            resize_origin(start, Edges::Left.union(Edges::Top), (700, 500)),
            (200, 200)
        );
        assert_eq!(
            resize_origin(start, Edges::Right.union(Edges::Bottom), (700, 500)),
            (100, 100)
        );
    }

    #[test]
    fn a_resize_origin_saturates_on_an_extent_at_the_i32_boundary() {
        let start = Geometry {
            x: 0,
            y: 0,
            width: i32::MIN,
            height: i32::MIN,
        };
        assert_eq!(
            resize_origin(start, Edges::Left.union(Edges::Top), (i32::MAX, i32::MAX)),
            (i32::MIN, i32::MIN)
        );
    }

    /// Audit finding F-01: a `WEnum` straight off the wire is protocol
    /// version skew, not a logic invariant, so an edge bitmask this build
    /// has never heard of must cost one resize rather than the session.
    #[test]
    fn an_unrecognised_resize_edge_bitmask_is_dropped_rather_than_fatal() {
        assert_eq!(
            requested_resize_edges(wayland_client::WEnum::Unknown(0xdead)),
            None
        );
    }

    #[test]
    fn a_recognised_resize_edge_bitmask_is_passed_through() {
        assert_eq!(
            requested_resize_edges(wayland_client::WEnum::Value(
                Edges::Left.union(Edges::Bottom)
            )),
            Some(Edges::Left.union(Edges::Bottom))
        );
    }
}
