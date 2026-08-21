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

//! The decisions `buoy-tag-picker` makes, separated from the process spawning
//! and live-socket I/O that `src/main.rs` does with them.
//!
//! A library plus a thin `[[bin]]` rather than a bin-only target: Cargo
//! collects doctests only from library targets, and a `tests/` crate can
//! only `use` a library, so with no `[lib]` here nothing could exercise
//! this binary across a process boundary and every `///` example was
//! uncompiled prose (audit finding T-04).

pub mod mode;
pub mod picker;
pub mod wire;
