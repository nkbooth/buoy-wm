// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! Newtype identifiers for `wm-core`'s registered entities.

/// Identifies a registered [`View`](super::view::View). IDs are unique for
/// the lifetime of a `WmCore` instance and are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViewId(pub u64);

/// Identifies a registered tag. Backed by `u8` because the valid range is
/// `0..64` per ADR-006 (tag registry is capped at 64 entries, one bit per
/// tag in a `u64` [`TagSet`](super::tag_set::TagSet)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TagId(pub u8);

/// Identifies a registered output (display). IDs are unique for the
/// lifetime of a `WmCore` instance and are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OutputId(pub u64);
