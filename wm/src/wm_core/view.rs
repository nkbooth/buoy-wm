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

//! `View`: an in-memory record of a window's tag membership, geometry and
//! floating state.

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
///
/// Deliberately carries no focus flag of its own. It used to, mirroring
/// [`WmCore::focused_view`](super::state::WmCore) — two representations of
/// one fact, one write path, and every production read went to the other
/// one. `unregister_view` cleared the core's field but not the removed
/// `View`'s, which was harmless only because the `View` was dropped in the
/// same statement; the first future path that moved or cloned a `View`
/// would have inherited a stale `focused: true` (audit finding K-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub id: ViewId,
    pub app_id: String,
    pub tags: TagSet,
    pub floating: bool,
    pub geometry: Geometry,
}
