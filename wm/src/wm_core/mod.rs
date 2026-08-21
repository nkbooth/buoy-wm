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

//! `wm-core`: the single in-memory source of truth for window/tag
//! membership, view geometry/floating state, focus, output current-tag,
//! the tag registry, stacking/render order, and terminal-spawned status.
//!
//! This module is pure state + logic with no I/O (see `components.md`).
//! Every decision it makes is driven either by the binary's Wayland
//! dispatch handlers (`compositor::dispatch`) or by `ipc::dispatch`, and it depends on neither — so
//! its whole public API is exercisable by unit tests with no compositor and
//! no socket, which is why the test coverage here is what it is.

/// The maximum number of tags a [`tag::TagRegistry`] may hold, and the
/// exclusive upper bound on any [`ids::TagId`] a [`tag_set::TagSet`] can
/// represent.
///
/// Fixed by `u64`'s width, not a tunable: ADR-006 stores tag membership as
/// one bit per tag in a `u64` bitset. Lives here beside the types it
/// constrains rather than being restated as a bare `64` at each guard
/// (audit finding J-06).
pub const MAX_TAGS: u8 = u64::BITS as u8;

pub mod ids;
pub mod output;
pub mod state;
pub mod tag;
pub mod tag_set;
pub mod view;
