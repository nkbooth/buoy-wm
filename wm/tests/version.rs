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

//! `buoy-wm --version` answers before the binary touches anything.
//!
//! `release.yml`'s `verify` job runs it from the installed package in a
//! clean container with no compositor (NFR6), so the flag has to be
//! checked before the Wayland connect. The environment is cleared, so the
//! process can find no Wayland socket, IPC socket or config file and
//! cannot spawn anything off `PATH`; `CARGO_BIN_EXE_*` runs the real
//! executable, as `ipc_boundary.rs` does.

use std::process::{Command, Output};

const NAME: &str = "buoy-wm";

/// The real binary with an empty environment but for the coverage
/// runtime's output path: under `cargo llvm-cov` a cleared
/// `LLVM_PROFILE_FILE` makes the instrumented binary write its profile
/// into the working directory and complain on stderr when it cannot.
fn clean_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_buoy-wm"));
    command.env_clear();
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    command
}

fn run_version(command: &mut Command) -> Output {
    command
        .arg("--version")
        .output()
        .expect("run the real buoy-wm")
}

fn assert_prints_the_version(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{} {}\n", NAME, env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

#[test]
fn version_is_printed_with_an_empty_environment() {
    let output = run_version(&mut clean_command());
    assert_prints_the_version(&output);
}

#[test]
fn version_is_printed_before_any_wayland_connect() {
    let output =
        run_version(clean_command().env("WAYLAND_DISPLAY", "/nonexistent/buoy-wm-version-test"));
    assert_prints_the_version(&output);
}
