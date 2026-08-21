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

//! `Mod4+S`, end to end, in one process boundary crossing.
//!
//! Everything switch mode decides is unit-tested in `picker`/`mode`/`wire`,
//! and until the `[lib]` split there was no way to test that `main` wires
//! those decisions to each other in the right order — the whole file sat at
//! 0.00% coverage and the module doc comment carved itself out of the
//! project's TDD rule as a result (audit finding T-01). This drives the real
//! binary: a real socket it must find from `XDG_RUNTIME_DIR`, a real
//! launcher process it must spawn and feed, and the two requests it must
//! send in order.
//!
//! `fuzzel` is stubbed by putting an executable named `fuzzel` first on
//! `PATH` — the binary resolves the launcher by name on purpose (see
//! `run_fuzzel`), so this needs no seam that production does not have.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A `0700` directory standing in for `$XDG_RUNTIME_DIR`, plus somewhere to
/// put the stub launcher. Removed with everything in it however the test
/// ends, which is why it is a `Drop` guard (audit finding T-03's shape).
struct Fixture(PathBuf);

impl Fixture {
    fn create(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("buoy-it-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("bin")).expect("create the fixture directories");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("make the fixture runtime directory private");
        Self(path)
    }

    fn runtime_dir(&self) -> &Path {
        &self.0
    }

    fn socket_path(&self) -> PathBuf {
        self.0.join("buoy-wm.sock")
    }

    fn bin_dir(&self) -> PathBuf {
        self.0.join("bin")
    }

    fn argv_log(&self) -> PathBuf {
        self.0.join("launcher-argv")
    }

    /// Installs an executable `fuzzel` that records its own argv, drains
    /// stdin, and prints `selection` — the one row the user "picked".
    fn install_stub_launcher(&self, selection: &str) {
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {argv}\ncat > /dev/null\nprintf '{selection}\\n'\n",
            argv = self.argv_log().display(),
        );
        let launcher = self.bin_dir().join("fuzzel");
        std::fs::write(&launcher, script).expect("write the stub launcher");
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755))
            .expect("make the stub launcher executable");
    }

    /// `PATH` with the stub launcher's directory first.
    fn path_env(&self) -> String {
        let inherited = std::env::var("PATH").unwrap_or_default();
        format!("{}:{inherited}", self.bin_dir().display())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Kills the spawned binary however the test ends, so a failed assertion
/// cannot leave a picker holding an overlay-layer launcher behind.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Accepts one connection within `deadline` or panics. `std` has no accept
/// timeout, and a blocking accept against a child that died at startup
/// would hang the whole test run rather than fail one test.
fn accept_within(listener: &UnixListener, deadline: Duration) -> UnixStream {
    listener
        .set_nonblocking(true)
        .expect("make the fixture listener non-blocking");
    let start = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .expect("restore blocking I/O on the accepted connection");
                return stream;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("accept failed: {e}"),
        }
        assert!(
            start.elapsed() < deadline,
            "the real binary never connected within {deadline:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn switch_mode_sends_switch_tag_for_the_row_the_launcher_returned() {
    let fixture = Fixture::create("switch-mode");
    // The row `render_switch_list` would have printed for tag 1, returned
    // verbatim the way `fuzzel` returns a whole accepted line.
    fixture.install_stub_launcher("code\t1");
    let listener = UnixListener::bind(fixture.socket_path()).expect("bind the fake wm socket");

    let child = Command::new(env!("CARGO_BIN_EXE_buoy-tag-picker"))
        .args(["switch", "7", "DP-1"])
        .env("XDG_RUNTIME_DIR", fixture.runtime_dir())
        .env("PATH", fixture.path_env())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the real buoy-tag-picker");
    let mut child = ChildGuard(child);

    let connection = accept_within(&listener, Duration::from_secs(10));
    let mut writer = connection
        .try_clone()
        .expect("split the fixture connection for writing");
    let mut reader = BufReader::new(connection);
    let mut request = String::new();

    reader
        .read_line(&mut request)
        .expect("read the first request");
    assert_eq!(
        request.trim_end(),
        r#"{"type":"get-state"}"#,
        "switch mode's first request is not get-state"
    );
    writeln!(
        writer,
        r#"{{"type":"state","tags":[{{"id":0,"name":"web"}},{{"id":1,"name":"code"}}],"views":[],"outputs":[{{"id":7,"current_tag":0}}],"focused_view":null}}"#
    )
    .and_then(|()| writer.flush())
    .expect("answer get-state");

    request.clear();
    reader
        .read_line(&mut request)
        .expect("read the request the launcher's selection produced");
    assert_eq!(
        request.trim_end(),
        r#"{"type":"switch-tag","output_id":7,"tag_id":1}"#,
        "the picked row did not become a switch-tag for that row's id on that output"
    );
    writeln!(writer, r#"{{"type":"ok"}}"#)
        .and_then(|()| writer.flush())
        .expect("acknowledge switch-tag");

    let status = child.0.wait().expect("wait for the picker to exit");
    assert!(
        status.success(),
        "an accepted switch left the picker exiting non-zero: {status:?}"
    );

    // Story 2.9's `--output=` targeting, checked where it actually matters:
    // in the argv of the process that got spawned.
    let argv = std::fs::read_to_string(fixture.argv_log()).expect("read the launcher's argv");
    assert!(
        argv.lines().any(|arg| arg == "--output=DP-1"),
        "the launcher was not told which monitor to render on: {argv:?}"
    );
    assert!(
        argv.lines().any(|arg| arg == "--dmenu"),
        "the launcher was not driven in dmenu mode: {argv:?}"
    );
}

#[test]
fn a_malformed_invocation_fails_before_it_needs_a_socket() {
    let output = Command::new(env!("CARGO_BIN_EXE_buoy-tag-picker"))
        .arg("nonsense")
        // Deliberately no `XDG_RUNTIME_DIR`: argument shape is checked
        // before anything touches the socket, so this must fail on the
        // argument rather than on the missing path.
        .env_remove("XDG_RUNTIME_DIR")
        .output()
        .expect("run the real buoy-tag-picker");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("nonsense"),
        "the failure did not name the argument it rejected: {stderr}"
    );
    assert!(
        !stderr.contains("XDG_RUNTIME_DIR"),
        "argument parsing reached the socket path first: {stderr}"
    );
}
