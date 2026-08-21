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

//! What the `buoy-wm` library exposes, exercised from outside the crate.
//!
//! `ipc::server`'s own 14 tests are `#[cfg(test)]` siblings and can reach
//! anything in the crate; this one can reach only the public API, which is
//! the property that keeps the boundary honest as `src/main.rs` is broken up
//! behind it. It is also the only test that runs the real `buoy-wm`
//! executable.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use buoy_wm::ipc::server;
use buoy_wm::wm_core::state::WmCore;

/// A `0700` directory to bind the socket in, removed with everything in it
/// however the test ends (audit finding T-03's shape).
struct Fixture(PathBuf);

impl Fixture {
    fn create(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("buoy-it-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create the fixture directory");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("make the fixture directory private");
        Self(path)
    }

    fn socket_path(&self) -> PathBuf {
        self.0.join("buoy-wm.sock")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_public_api_is_enough_to_serve_a_switch_tag_end_to_end() {
    let fixture = Fixture::create("ipc-boundary");
    let mut core = WmCore::default();
    let tag = core
        .create_tag("code")
        .expect("an empty registry accepts a tag");
    let output = core.register_output();
    let core = Arc::new(Mutex::new(core));

    // The pinned-terminal spawn is an injected effect, so this counts the
    // requests without any process being created (audit finding J-02).
    let spawns = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&spawns);
    let _accept_loop = server::spawn(
        Arc::clone(&core),
        &fixture.socket_path(),
        Arc::new(move |pending| {
            assert_eq!(pending.session_name, "tag-code");
            counted.fetch_add(1, Ordering::AcqRel);
        }),
    )
    .expect("bind the socket and start the accept loop");

    let stream = UnixStream::connect(fixture.socket_path()).expect("connect to the server");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("bound the test's own wait");
    let mut writer = stream.try_clone().expect("split for writing");
    let mut reader = BufReader::new(stream);

    writeln!(
        writer,
        r#"{{"type":"switch-tag","output_id":{},"tag_id":{}}}"#,
        output.0, tag.0
    )
    .and_then(|()| writer.flush())
    .expect("send switch-tag");

    let mut response = String::new();
    reader.read_line(&mut response).expect("read the response");
    assert_eq!(response.trim_end(), r#"{"type":"ok"}"#);

    // The spawn is deliberately performed after the response is on the
    // wire, so it is observable only once the answer has arrived — and it
    // still has to have happened.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while spawns.load(Ordering::Acquire) == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        spawns.load(Ordering::Acquire),
        1,
        "switching to a tag with no terminal yet did not claim one spawn"
    );

    writeln!(writer, r#"{{"type":"get-state"}}"#)
        .and_then(|()| writer.flush())
        .expect("send get-state on the same connection");
    response.clear();
    reader.read_line(&mut response).expect("read the state");
    assert!(
        response.contains(r#""current_tag":0"#),
        "the state does not show the switch that was just applied: {response}"
    );
}

#[test]
fn the_binary_refuses_to_start_without_a_compositor_rather_than_exiting_quietly() {
    // `buoy-wm` is `exec`'d by river as the login session leader, so a
    // startup failure that exits zero with an empty stderr is a black
    // screen with nothing to diagnose it from. This is the one thing that
    // can be asserted about the real executable without a compositor.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_buoy-wm"))
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .env_remove("XDG_RUNTIME_DIR")
        .output()
        .expect("run the real buoy-wm");

    assert!(
        !output.status.success(),
        "buoy-wm reported success with no compositor to manage"
    );
    assert!(
        !output.stderr.is_empty(),
        "buoy-wm failed to start and said nothing about why"
    );
}
