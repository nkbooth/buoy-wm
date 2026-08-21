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

//! Everything the `buoy-wm` binary is, below `main` itself.
//!
//! # Rationale
//!
//! `wm/src/main.rs` had grown to 3,651 lines with seven independent
//! reasons to change — the vendored protocol codegen, config-to-protocol
//! translation, child-process lifecycle, the `WindowManager` aggregate,
//! keybind execution, thirteen `Dispatch` impls and startup wiring (audit
//! finding J-10). This module is that file split along the seams its own
//! comments already marked, leaving `main.rs` holding only the composition
//! root.
//!
//! The split stops at the crate boundary on purpose. These modules are
//! part of the *binary*, not of `buoy_wm`'s library, because
//! [`river`]'s generated `Event` enums are `#[non_exhaustive]` — see that
//! module for why the `Dispatch` impls cannot cross a crate boundary
//! without losing exhaustive matching. Within one crate a file boundary
//! costs nothing, which is why this is a directory and not a second crate.

pub mod river;
