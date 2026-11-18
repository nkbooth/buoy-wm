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

//! `buoy-wm --print-default-config` prints the shipped example, from the
//! real binary, touching nothing.
//!
//! The process gets an unreachable Wayland socket and a config directory
//! holding a broken config file: if the flag connected, read the config
//! or started the WM, the run would fail or report on stderr. The stdout
//! is compared with `docs/config.example.toml` minus its licence header,
//! so the shipped file is held to the binary rather than to the library.

use std::path::PathBuf;
use std::process::Command;

const EXAMPLE: &str = include_str!("../../docs/config.example.toml");

/// The real binary with an empty environment but for the coverage
/// runtime's output path, as in `version.rs`.
fn clean_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_buoy-wm"));
    command.env_clear();
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    command
}

/// A config home holding a `buoy/config.toml` that does not parse,
/// removed when dropped.
struct BrokenConfigHome(PathBuf);

impl BrokenConfigHome {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("buoy-print-default-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("buoy")).expect("create the config directory");
        std::fs::write(path.join("buoy/config.toml"), "this is [not toml")
            .expect("write the broken config");
        Self(path)
    }
}

impl Drop for BrokenConfigHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_flag_prints_the_shipped_example_without_the_licence_header() {
    let home = BrokenConfigHome::new();
    let output = clean_command()
        .arg("--print-default-config")
        .env(
            "WAYLAND_DISPLAY",
            "/nonexistent/buoy-wm-print-default-config",
        )
        .env("HOME", &home.0)
        .env("XDG_CONFIG_HOME", &home.0)
        .output()
        .expect("run the real buoy-wm");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert_eq!(stderr, "");
    let (_header, body) = EXAMPLE
        .split_once("\n\n")
        .expect("docs/config.example.toml has a header block and a blank line");
    assert_eq!(String::from_utf8_lossy(&output.stdout), body);
}

/// Only the first argument is a flag, as with `--version`: in second place
/// it is ignored and buoy-wm starts, which with no compositor fails to
/// connect.
#[test]
fn the_flag_in_second_place_is_not_a_request() {
    let output = clean_command()
        .args(["something", "--print-default-config"])
        .env(
            "WAYLAND_DISPLAY",
            "/nonexistent/buoy-wm-print-default-config",
        )
        .output()
        .expect("run the real buoy-wm");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_ne!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    // The connect is what failed, so startup really ran past the flags.
    assert!(stderr.contains("NoCompositor"), "stderr: {stderr}");
}
