// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! `View`: an in-memory record of a window's tag membership, geometry,
//! floating state, and focus.

use super::ids::ViewId;
use super::tag_set::TagSet;

/// A window's on-screen position and size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The fixed default position/size applied to every newly registered
/// non-pinned view (FR5).
///
/// This is a fixed constant rather than computed from output geometry
/// because `data-model.md`'s `Output` entity has no width/height fields
/// yet — output-aware placement (e.g. centering) is out of scope until a
/// future story extends that entity (see Story 1.6's Technical notes for
/// the full rationale).
pub const DEFAULT_FLOATING_GEOMETRY: Geometry = Geometry {
    x: 100,
    y: 100,
    width: 800,
    height: 600,
};

/// An in-memory record for a registered window (view).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub id: ViewId,
    pub app_id: String,
    pub tags: TagSet,
    pub floating: bool,
    pub geometry: Geometry,
    pub focused: bool,
}
