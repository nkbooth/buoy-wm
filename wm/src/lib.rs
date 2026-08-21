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

//! Everything `buoy-wm` is built from that does not itself hold a Wayland
//! connection.
//!
//! The crate is a library plus a thin `[[bin]]` (`src/main.rs`) rather than
//! a bin-only target, and the reason is testability rather than reuse: a
//! binary target contributes no doctests and cannot be `use`d from a
//! `tests/` crate, so every `///` example in the tree was uncompiled prose
//! and no test anywhere could cross a process boundary (audit finding
//! T-04). The `tests/` directories that now exist are the whole point of
//! the split.
//!
//! What is *not* behind this boundary is as deliberate. `src/main.rs` keeps
//! the `wayland-scanner`-generated `river` protocol bindings, the Wayland
//! `Dispatch` impls, the proxy-carrying `Window`/`Output`/`Seat` structs and
//! `main` itself. The bindings are the reason the rest of that list cannot
//! follow the modules below yet: `wayland-scanner` marks every generated
//! `Event` enum `#[non_exhaustive]`, which is exhaustive to match on inside
//! its own crate and not across a crate boundary — so moving `river` here
//! while the ten `Dispatch::event` matches stay in the binary would force
//! ten `_ =>` arms and turn "the vendored protocol XML grew an event" from
//! a compile error into a silently ignored message. They move together or
//! not at all. Nothing in this file has to change when they do.

pub mod config;
pub mod ipc;
pub mod wm_core;
