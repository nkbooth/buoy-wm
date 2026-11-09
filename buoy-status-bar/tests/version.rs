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

//! `buoy-status-bar --version` answers before the output id is parsed.
//!
//! `release.yml`'s `verify` job runs it from the installed package in a
//! clean container (NFR6); with no `XDG_RUNTIME_DIR` the bar would
//! otherwise exit 1 looking for the WM's socket, so a pass here means the
//! flag was handled first.

use std::process::Command;

#[test]
fn version_is_printed_with_an_empty_environment() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_buoy-status-bar"));
    command.arg("--version").env_clear();
    // Under `cargo llvm-cov` a cleared LLVM_PROFILE_FILE makes the
    // instrumented binary write its profile into the working directory
    // and complain on stderr when it cannot; it is the one variable kept.
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    let output = command.output().expect("run the real buoy-status-bar");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("buoy-status-bar {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}
