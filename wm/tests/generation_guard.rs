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

//! The one invariant behind the cached `get-state` response that a future
//! edit can break silently.
//!
//! `WmCore::generation` is what lets a reader decide a snapshot it already
//! has is still current (audit finding B-02), and it only works if *every*
//! mutator moves it. `state.rs`'s own
//! `every_mutator_moves_the_generation_forward` covers the nineteen that
//! exist; it cannot cover the twentieth, because a test enumerating mutators
//! by hand is exactly the kind of list that falls behind — and the cost of
//! falling behind here is a client shown tags that no longer exist.
//!
//! So this counts them in the source instead. It lives in `tests/` rather
//! than beside the code it reads for a mundane reason: the needles would
//! otherwise be found in the test that looks for them.

/// Lines that declare a method taking `&mut self`, ignoring comments — a doc
/// comment mentioning the receiver in backticks is not a method.
fn mutable_receivers(source: &str) -> Vec<&str> {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .filter(|line| line.contains("&mut self"))
        .collect()
}

#[test]
fn every_mutable_method_on_wm_core_bumps_the_generation() {
    let source = include_str!("../src/wm_core/state.rs");
    let receivers = mutable_receivers(source);
    let bumps = source.matches("self.bump_generation();").count();

    // `bump_generation` itself takes `&mut self` and does not call itself,
    // which is the whole of the `+ 1`.
    assert_eq!(
        receivers.len(),
        bumps + 1,
        "{} methods take `&mut self` but only {bumps} calls to \
         `bump_generation` exist. A new mutator that does not bump it serves \
         `get-state` from a snapshot of state that has already changed. If a \
         new `&mut self` helper genuinely should not bump — because its \
         caller already did — say so here and adjust the count.\nreceivers: \
         {receivers:#?}",
        receivers.len(),
    );
}
