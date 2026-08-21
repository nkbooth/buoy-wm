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

//! Cross-binary invariants for the `buoy-wm` workspace.
//!
//! Every item here is *behavioural* and shared by more than one of the
//! three binaries: if `buoy-wm` and its companions disagree about it, the
//! symptom is "the picker does nothing", not a compile error. That is the
//! whole admission criterion for this crate, and ADR-009 in
//! `docs/adrs.md` is where it and its consequences are recorded — notably
//! that the wire *types* stay deliberately per-crate.

pub mod framing;
pub mod log;
pub mod notify;
pub mod peer;
pub mod socket_path;
