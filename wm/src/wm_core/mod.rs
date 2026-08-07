// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! `wm-core`: the single in-memory source of truth for window/tag
//! membership, view geometry/floating state, focus, output current-tag,
//! the tag registry, stacking/render order, and terminal-spawned status.
//!
//! This module is pure state + logic with no I/O (see `components.md`).
//! It is not yet wired to the vendored Dispatch handlers in `main.rs`
//! (that wiring is Story 1.4's job) — its public API is exercised
//! directly by unit tests in this crate.

pub mod ids;
pub mod output;
pub mod state;
pub mod tag;
pub mod tag_set;
pub mod view;
