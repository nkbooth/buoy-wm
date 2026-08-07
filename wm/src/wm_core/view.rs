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
