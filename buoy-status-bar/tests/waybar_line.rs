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

//! The workspace's first test that crosses a process boundary.
//!
//! Every other test in this repo runs inside one process: the 14 IPC server
//! tests bind a real socket but drive both ends from the same test binary,
//! so nothing anywhere exercised a real `buoy-*` executable — argument
//! parsing, socket resolution from the real environment, the poll loop and
//! the stdout contract waybar actually consumes were all reachable only by
//! logging in (audit finding T-04). This binds a socket, points
//! `XDG_RUNTIME_DIR` at it, `exec`s the real `buoy-status-bar` through
//! Cargo's `CARGO_BIN_EXE_*` (no new dependency, no path guessing), and
//! reads the line waybar would read.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// A `0700` directory that stands in for `$XDG_RUNTIME_DIR`, removed with
/// everything in it when the test ends — including on failure, which is
/// why this is a `Drop` guard and not a call at the end of the test body
/// (the shape audit finding T-03 established for the in-process tests).
struct RuntimeDir(PathBuf);

impl RuntimeDir {
    fn create(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("buoy-it-{}-{label}", std::process::id()));
        // A leftover from a killed previous run would otherwise make the
        // bind fail with EADDRINUSE and read as a product bug.
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create the test runtime directory");
        // `buoy_common::socket_path::verify_private_dir` is what the WM
        // applies to this directory in production; matching it here keeps
        // the fixture honest rather than merely working.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("make the test runtime directory private");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn socket_path(&self) -> PathBuf {
        self.0.join("buoy-wm.sock")
    }
}

impl Drop for RuntimeDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Kills the spawned binary when the test ends however it ends. Without
/// this a failing assertion leaves a `buoy-status-bar` polling a deleted
/// socket four times a second for as long as the test runner lives.
struct ChildGuard(Child);

impl ChildGuard {
    /// Whatever the child has written to stderr by now, for a failure
    /// message. Called only on a path that is already failing, so the read
    /// blocking until the pipe closes is exactly what is wanted: the child
    /// is killed first.
    fn stderr_so_far(&mut self) -> String {
        use std::io::Read;

        let _ = self.0.kill();
        let mut text = String::new();
        if let Some(stderr) = self.0.stderr.as_mut() {
            let _ = stderr.read_to_string(&mut text);
        }
        text
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// One `state` response, written by hand rather than built with
/// `buoy-wm`'s serializer on purpose: this test's job is the *bytes* on the
/// wire and the bytes on stdout. That the two crates' Rust types still
/// agree about those bytes is a different question, pinned separately by
/// `wm/tests/wire_compatibility.rs`.
const STATE_RESPONSE: &str = r#"{"type":"state","tags":[{"id":0,"name":"web"},{"id":1,"name":"code"}],"views":[],"outputs":[{"id":7,"current_tag":1}],"focused_view":null}"#;

/// Accepts one connection within `deadline`, or panics with what the child
/// wrote to stderr instead. `std` has no accept timeout, so this is a
/// non-blocking accept plus a bounded wait — a blocking accept against a
/// child that failed at startup would hang the whole test run rather than
/// fail one test.
fn accept_within(
    listener: &UnixListener,
    child: &mut ChildGuard,
    deadline: std::time::Duration,
) -> std::os::unix::net::UnixStream {
    listener
        .set_nonblocking(true)
        .expect("make the fixture listener non-blocking");
    let start = std::time::Instant::now();
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
            "the real binary never connected within {deadline:?}; its stderr: {}",
            child.stderr_so_far()
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn the_real_binary_prints_the_waybar_line_for_its_own_output() {
    let runtime_dir = RuntimeDir::create("waybar-line");
    let listener = UnixListener::bind(runtime_dir.socket_path()).expect("bind the fake wm socket");

    let child = Command::new(env!("CARGO_BIN_EXE_buoy-status-bar"))
        // Output 7 is the one the fake WM reports a current tag for; the
        // response deliberately carries a second tag the bar must not pick,
        // and a second output id it must not read.
        .arg("7")
        .env("XDG_RUNTIME_DIR", runtime_dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the real buoy-status-bar");
    let mut child = ChildGuard(child);

    // Served on this thread rather than a helper thread: the exchange is
    // strictly sequential (the bar sends, then blocks reading), so a thread
    // would only move a failed assertion somewhere it cannot fail the test.
    let connection = accept_within(&listener, &mut child, std::time::Duration::from_secs(10));
    let mut writer = connection
        .try_clone()
        .expect("split the fixture connection for writing");
    let mut reader = BufReader::new(connection);
    let mut request = String::new();
    reader
        .read_line(&mut request)
        .expect("read the request the real binary sent");
    assert_eq!(
        request.trim_end(),
        r#"{"type":"get-state"}"#,
        "the bar asked for something other than get-state"
    );
    writeln!(writer, "{STATE_RESPONSE}")
        .and_then(|()| writer.flush())
        .expect("answer the request");

    let stdout = child.0.stdout.take().expect("capture the child's stdout");
    let mut first_line = String::new();
    BufReader::new(stdout)
        .read_line(&mut first_line)
        .expect("read the first waybar line the real binary printed");

    // A literal, not `format_waybar_line`'s own output: an assertion built
    // from the function under test would pass on any consistent mistake,
    // and this string is a contract with `~/.config/waybar`, not with this
    // crate (audit finding T-02's lesson applied to a new test).
    assert_eq!(
        first_line.trim_end(),
        r#"{"text":"code","class":"normal"}"#,
        "the line waybar would consume is not the one output 7's current tag calls for"
    );
}

#[test]
fn the_real_binary_polls_again_on_the_connection_it_already_has() {
    // One connect/accept/thread-spawn/close cycle per tick per output, four
    // times a second for the whole login session, against the same mutex the
    // Wayland dispatch thread needs — the normal-load half of audit finding
    // B-02. The server deliberately keeps a connection open across requests,
    // so the fix is for the bar to use it.
    let runtime_dir = RuntimeDir::create("connection-reuse");
    let listener = UnixListener::bind(runtime_dir.socket_path()).expect("bind the fake wm socket");

    let child = Command::new(env!("CARGO_BIN_EXE_buoy-status-bar"))
        .arg("7")
        .env("XDG_RUNTIME_DIR", runtime_dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the real buoy-status-bar");
    let mut child = ChildGuard(child);

    let connection = accept_within(&listener, &mut child, std::time::Duration::from_secs(10));
    let mut writer = connection
        .try_clone()
        .expect("split the fixture connection for writing");
    let mut reader = BufReader::new(connection);

    // Two full request/response exchanges on the one accepted connection. If
    // the bar reconnects per tick, the second `read_line` sees EOF on this
    // stream instead of a request, because the bar closed it.
    for poll in 1..=2 {
        let mut request = String::new();
        let read = reader
            .read_line(&mut request)
            .expect("read a request from the connection already accepted");
        assert!(
            read > 0,
            "poll {poll} arrived on a new connection: the bar closed the one it had"
        );
        assert_eq!(request.trim_end(), r#"{"type":"get-state"}"#);
        writeln!(writer, "{STATE_RESPONSE}")
            .and_then(|()| writer.flush())
            .unwrap_or_else(|e| panic!("answer poll {poll}: {e}"));
    }

    // Reuse must not cost recovery: a `wm` that restarted is a closed
    // connection, and the bar has to reconnect on the next tick rather than
    // render disconnected forever.
    drop(writer);
    drop(reader);
    let reconnected = accept_within(&listener, &mut child, std::time::Duration::from_secs(10));
    let mut writer = reconnected
        .try_clone()
        .expect("split the reconnected fixture connection");
    let mut reader = BufReader::new(reconnected);
    let mut request = String::new();
    reader
        .read_line(&mut request)
        .expect("read the request that followed the reconnect");
    assert_eq!(request.trim_end(), r#"{"type":"get-state"}"#);
    writeln!(writer, "{STATE_RESPONSE}")
        .and_then(|()| writer.flush())
        .expect("answer the poll after the reconnect");
}
