// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! The Unix domain socket accept loop and per-connection handling.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::config::Defaults;
use crate::ipc::dispatch::handle_request;
use crate::ipc::lock_recovering;
use crate::ipc::protocol::{Response, parse_request, serialize_response};
use crate::wm_core::state::WmCore;

/// The maximum accepted length of one request line, per the AC's "oversized
/// (>64 KiB in one line)" bullet. A line at or beyond this length without a
/// terminating newline is treated as malformed input (NFR2: bounds the
/// read buffer instead of growing it forever for a hostile/broken client).
const MAX_LINE_BYTES: usize = 64 * 1024;

/// Binds a Unix domain socket at `socket_path` and spawns a dedicated
/// accept-loop thread that hands each connection off to its own thread
/// (thread-per-connection — see the story's Technical notes "Concurrency
/// model"). The bind itself happens synchronously in the calling thread,
/// before any thread is spawned, so a caller can connect immediately after
/// this function returns `Ok` with no race or `sleep` needed. A stale file
/// left behind by a previous run (crash, or simply a non-socket file) is
/// removed best-effort before binding — see the story's Technical notes
/// "Stale socket handling" for why blind removal is acceptable here (this
/// module does not distinguish a stale file from a second, still-running
/// WM instance).
///
/// `defaults` carries the pinned terminal's program and argv — the config
/// values, passed in rather than read here so this module keeps its only
/// dependency on the WM being the shared `wm_core` handle.
pub fn spawn(
    wm_core: Arc<Mutex<WmCore>>,
    socket_path: &Path,
    defaults: Defaults,
) -> std::io::Result<JoinHandle<()>> {
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path)?;
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))?;

    Ok(std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let wm_core = Arc::clone(&wm_core);
                    let defaults = defaults.clone();
                    std::thread::spawn(move || handle_connection(stream, wm_core, &defaults));
                }
                Err(e) => eprintln!("ipc: accept error: {e}"),
            }
        }
    }))
}

/// Reads and dispatches requests from one connection until the client
/// disconnects or sends malformed input. Wrapped in `catch_unwind` (see the
/// story's Technical notes "Defense in depth: per-connection panic
/// containment") so a hypothetical panic anywhere in this function's body
/// — including while the shared mutex is locked — is caught and logged,
/// closing only this one connection. This is defense-in-depth, not the
/// primary crash-safety mechanism: thread-per-connection plus Rust's
/// default `panic = "unwind"` already confine an unhandled panic to its
/// own thread, and [`lock_recovering`](crate::ipc::lock_recovering) is
/// what actually keeps a poisoned mutex from crashing the *next* locker.
fn handle_connection(stream: UnixStream, wm_core: Arc<Mutex<WmCore>>, defaults: &Defaults) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_connection_inner(stream, &wm_core, defaults);
    }));
    if let Err(e) = result {
        eprintln!("ipc: connection handler panicked (contained): {e:?}");
    }
}

/// Writes `response` as one JSON line followed by `\n`, flushing
/// afterward. Returns `false` (caller should close the connection) if the
/// write itself fails — a write failure means the peer is already gone,
/// nothing more to do on this connection.
fn write_response(stream: &mut UnixStream, response: &Response) -> bool {
    let line = serialize_response(response);
    stream.write_all(line.as_bytes()).is_ok()
        && stream.write_all(b"\n").is_ok()
        && stream.flush().is_ok()
}

fn handle_connection_inner(stream: UnixStream, wm_core: &Arc<Mutex<WmCore>>, defaults: &Defaults) {
    let mut writer = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ipc: failed to clone connection for writing: {e}");
            return;
        }
    };
    let mut reader = BufReader::new(stream);

    loop {
        let mut buf = Vec::new();
        let read_result = reader
            .by_ref()
            .take(MAX_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut buf);

        let n = match read_result {
            Ok(n) => n,
            Err(e) => {
                eprintln!("ipc: connection read error, closing: {e}");
                return;
            }
        };
        if n == 0 {
            return; // EOF: client disconnected
        }

        let had_newline = buf.last() == Some(&b'\n');
        if !had_newline && buf.len() > MAX_LINE_BYTES {
            // Oversized line with no newline in sight: malformed input,
            // per the AC's ">64 KiB in one line" bullet. Reject and close
            // rather than continuing to read an unbounded amount.
            write_response(
                &mut writer,
                &Response::Error {
                    message: "malformed request".into(),
                },
            );
            return;
        }
        if !had_newline {
            // Stream ended mid-line without a newline and within the size
            // cap: treat as EOF (nothing more the client will send).
            return;
        }

        let line_bytes = &buf[..buf.len() - 1]; // strip the trailing '\n'

        match parse_request(line_bytes) {
            Err(_) => {
                write_response(
                    &mut writer,
                    &Response::Error {
                        message: "malformed request".into(),
                    },
                );
                return; // malformed/unparseable input: reset the connection
            }
            Ok(request) => {
                let (response, pending_spawn) = {
                    let mut core = lock_recovering(wm_core);
                    handle_request(&mut core, request)
                };
                if !write_response(&mut writer, &response) {
                    return; // peer gone; nothing more to do
                }
                // Code-review follow-up (Story 2.4): the real
                // `crate::spawn_pinned_terminal` process spawn happens
                // here — after the response is already on the wire and the
                // `wm-core` mutex released — rather than inside
                // `handle_request` itself, so a slow or failing spawn can
                // never block the client waiting on its response, and so
                // `dispatch::handle_request`'s own unit tests stay free of
                // real process spawns. `crate::spawn_pinned_terminal` is a
                // private fn at the binary crate's root module; this module
                // (a descendant of the crate root) may call it directly
                // with no visibility changes, same as `dispatch.rs`
                // previously did.
                if let Some(session_name) = pending_spawn {
                    crate::spawn_pinned_terminal(
                        &defaults.terminal,
                        &defaults.pinned_terminal_argv(
                            crate::wm_core::state::PINNED_TERM_APP_ID,
                            &session_name,
                        ),
                    );
                }
                // well-formed request, wm-core-level Ok/Error: connection
                // stays open for further requests.
            }
        }
    }
}

/// Resolves the Unix domain socket's filesystem path from explicit,
/// injectable parameters — not read from `std::env` directly inside this
/// function, so it stays a pure, testable decision (the "test the
/// decision, not the I/O" split every prior story's `wm-core`/`main.rs`
/// boundary already follows).
///
/// Real-deployment case: `$XDG_RUNTIME_DIR/buoy-wm.sock`
/// (`architecture/deployment.md` confirms the WM runs under a real `river`
/// login session, which always sets `XDG_RUNTIME_DIR`). Devcontainer/
/// sandbox-convenience fallback: `/tmp/buoy-wm-<user>.sock`, using `user`
/// then `logname` then the literal `"unknown"` as the disambiguating
/// suffix — `XDG_RUNTIME_DIR` is typically unset in that environment.
pub fn resolve_socket_path(
    xdg_runtime_dir: Option<&str>,
    user: Option<&str>,
    logname: Option<&str>,
) -> PathBuf {
    // A set-but-empty `XDG_RUNTIME_DIR` (a real systemd/container pattern)
    // must fall through to the /tmp case too, not join onto an empty base
    // and produce a relative `buoy-wm.sock` path.
    match xdg_runtime_dir.filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("buoy-wm.sock"),
        None => PathBuf::from("/tmp").join(format!(
            "buoy-wm-{}.sock",
            user.or(logname).unwrap_or("unknown")
        )),
    }
}

/// Thin, untested (I/O-reading, not logic) wrapper around
/// [`resolve_socket_path`] that reads the real environment variables —
/// what `main.rs` calls to get the real socket path.
pub fn default_socket_path() -> PathBuf {
    resolve_socket_path(
        std::env::var("XDG_RUNTIME_DIR").ok().as_deref(),
        std::env::var("USER").ok().as_deref(),
        std::env::var("LOGNAME").ok().as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use crate::wm_core::state::WmCore;

    /// A minimal newline-delimited-JSON test client over a real
    /// `UnixStream`. Keeps one `BufReader` alive for the connection's
    /// lifetime (rather than re-wrapping the stream per call), so no
    /// buffered-but-unread bytes are ever silently dropped between calls.
    struct TestClient {
        write: UnixStream,
        read: BufReader<UnixStream>,
    }

    impl TestClient {
        fn connect(path: &std::path::Path) -> Self {
            let stream = UnixStream::connect(path).expect("connect to test socket");
            let read = BufReader::new(stream.try_clone().expect("clone stream for reading"));
            Self {
                write: stream,
                read,
            }
        }

        fn send_line(&mut self, line: &str) {
            self.write.write_all(line.as_bytes()).unwrap();
            self.write.write_all(b"\n").unwrap();
        }

        fn send_raw(&mut self, bytes: &[u8]) {
            self.write.write_all(bytes).unwrap();
        }

        /// Reads one newline-delimited line. `None` means EOF (the
        /// connection was closed by the server).
        fn read_line(&mut self) -> Option<String> {
            let mut buf = String::new();
            let n = self
                .read
                .read_line(&mut buf)
                .expect("read from test socket");
            if n == 0 {
                None
            } else {
                Some(buf.trim_end_matches('\n').to_string())
            }
        }
    }

    /// A monotonically increasing counter, so concurrent test runs never
    /// collide on the same temp socket path.
    static TEST_SOCKET_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_socket_path() -> std::path::PathBuf {
        let n = TEST_SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("buoy-wm-test-{}-{}.sock", std::process::id(), n))
    }

    fn spawn_test_server_with_core(core: WmCore) -> (std::path::PathBuf, Arc<Mutex<WmCore>>) {
        let socket_path = unique_socket_path();
        let wm_core = Arc::new(Mutex::new(core));
        // `/bin/true` rather than a real terminal: a request that claims a
        // pinned-terminal spawn reaches a real `Command::spawn` from these
        // tests, and an inert no-op keeps that from opening windows on the
        // machine running the suite.
        spawn(
            Arc::clone(&wm_core),
            &socket_path,
            Defaults {
                terminal: "/bin/true".to_string(),
                ..Defaults::default()
            },
        )
        .expect("server must spawn successfully");
        (socket_path, wm_core)
    }

    fn spawn_test_server() -> (std::path::PathBuf, Arc<Mutex<WmCore>>) {
        spawn_test_server_with_core(WmCore::new())
    }

    #[test]
    fn get_state_round_trip_returns_state_response() {
        let (socket_path, _wm_core) = spawn_test_server();
        let mut client = TestClient::connect(&socket_path);
        client.send_line(r#"{"type":"get-state"}"#);
        let response = client.read_line().expect("expected a response line");
        assert!(response.contains(r#""type":"state""#));
    }

    #[test]
    fn toggle_tag_round_trip_mutates_shared_wm_core() {
        let mut core = WmCore::new();
        let view_id = core.register_view("foot");
        let (socket_path, _wm_core) = spawn_test_server_with_core(core);

        let mut client = TestClient::connect(&socket_path);
        client.send_line(r#"{"type":"create-tag","name":"web"}"#);
        let create_response = client.read_line().expect("expected tag-created response");
        assert!(create_response.contains(r#""type":"tag-created""#));
        let tag_id: u8 = {
            let marker = "\"tag_id\":";
            let start = create_response.find(marker).unwrap() + marker.len();
            let rest = &create_response[start..];
            let end = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            rest[..end].parse().unwrap()
        };

        client.send_line(&format!(
            r#"{{"type":"toggle-tag","view_id":{},"tag_id":{}}}"#,
            view_id.0, tag_id
        ));
        let toggle_response = client.read_line().expect("expected ok response");
        assert!(toggle_response.contains(r#""type":"ok""#));

        client.send_line(r#"{"type":"get-state"}"#);
        let state_response = client.read_line().expect("expected state response");
        assert!(
            state_response.contains(&format!(
                r#""id":{},"app_id":"foot","tags":[{}]"#,
                view_id.0, tag_id
            )),
            "state response must reflect the mutation applied via a separate request: {state_response}"
        );
    }

    #[test]
    fn malformed_json_gets_error_response_then_connection_closes() {
        let (socket_path, _wm_core) = spawn_test_server();
        let mut client = TestClient::connect(&socket_path);
        client.send_line("not json");
        let response = client.read_line().expect("expected an error response");
        assert!(response.contains(r#""type":"error""#));
        assert_eq!(
            client.read_line(),
            None,
            "connection must be closed after a malformed request"
        );
    }

    #[test]
    fn oversized_line_without_newline_is_rejected_not_grown_forever() {
        let (socket_path, _wm_core) = spawn_test_server();
        let mut client = TestClient::connect(&socket_path);
        let oversized = vec![b'a'; 70 * 1024];
        client.send_raw(&oversized);

        let first = client.read_line();
        if let Some(line) = &first {
            assert!(line.contains(r#""type":"error""#));
        }
        assert_eq!(
            client.read_line(),
            None,
            "connection must be closed after an oversized, newline-less line"
        );
    }

    #[test]
    fn invalid_utf8_bytes_get_error_response_not_a_panic() {
        let (socket_path, _wm_core) = spawn_test_server();
        let mut client = TestClient::connect(&socket_path);
        client.send_raw(&[0xFF, 0xFE]);
        client.send_raw(b"\n");
        let response = client.read_line().expect("expected an error response");
        assert!(response.contains(r#""type":"error""#));
    }

    #[test]
    fn one_malformed_connection_does_not_affect_a_second_concurrent_good_connection() {
        let (socket_path, _wm_core) = spawn_test_server();
        let mut bad_client = TestClient::connect(&socket_path);
        let mut good_client = TestClient::connect(&socket_path);

        bad_client.send_line("not json");
        good_client.send_line(r#"{"type":"get-state"}"#);

        let bad_response = bad_client.read_line().expect("expected an error response");
        assert!(bad_response.contains(r#""type":"error""#));
        let good_response = good_client
            .read_line()
            .expect("the good connection's response must be unaffected");
        assert!(good_response.contains(r#""type":"state""#));

        // The server itself (and a subsequent third connection) must still
        // be alive after the malformed connection was closed.
        let mut third_client = TestClient::connect(&socket_path);
        third_client.send_line(r#"{"type":"get-state"}"#);
        let third_response = third_client
            .read_line()
            .expect("server must still be alive");
        assert!(third_response.contains(r#""type":"state""#));
    }

    #[test]
    fn stale_socket_file_from_a_previous_run_does_not_block_startup() {
        let socket_path = unique_socket_path();
        std::fs::write(&socket_path, b"stale, not a socket").unwrap();
        let wm_core = Arc::new(Mutex::new(WmCore::new()));
        let result = spawn(
            wm_core,
            &socket_path,
            Defaults {
                terminal: "/bin/true".to_string(),
                ..Defaults::default()
            },
        );
        assert!(
            result.is_ok(),
            "a stale non-socket file must not block startup: {result:?}"
        );
    }

    #[test]
    fn socket_file_has_owner_only_permissions_after_spawn() {
        let (socket_path, _wm_core) = spawn_test_server();
        let mode = std::fs::metadata(&socket_path)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn well_formed_request_referencing_unknown_ids_keeps_connection_open() {
        let (socket_path, _wm_core) = spawn_test_server();
        let mut client = TestClient::connect(&socket_path);
        client.send_line(r#"{"type":"toggle-tag","view_id":9999,"tag_id":63}"#);
        let error_response = client.read_line().expect("expected an error response");
        assert!(error_response.contains(r#""type":"error""#));

        client.send_line(r#"{"type":"get-state"}"#);
        let state_response = client
            .read_line()
            .expect("connection must stay open for a well-formed follow-up request");
        assert!(state_response.contains(r#""type":"state""#));
    }

    #[test]
    fn resolve_socket_path_uses_xdg_runtime_dir_when_set() {
        assert_eq!(
            resolve_socket_path(Some("/run/user/1000"), None, None),
            std::path::PathBuf::from("/run/user/1000/buoy-wm.sock")
        );
    }

    #[test]
    fn resolve_socket_path_falls_back_to_tmp_user_when_xdg_runtime_dir_unset() {
        assert_eq!(
            resolve_socket_path(None, Some("nick"), None),
            std::path::PathBuf::from("/tmp/buoy-wm-nick.sock")
        );
    }

    #[test]
    fn resolve_socket_path_falls_back_to_logname_when_user_also_unset() {
        assert_eq!(
            resolve_socket_path(None, None, Some("nick")),
            std::path::PathBuf::from("/tmp/buoy-wm-nick.sock")
        );
    }

    #[test]
    fn resolve_socket_path_falls_back_to_literal_unknown_when_nothing_is_set() {
        assert_eq!(
            resolve_socket_path(None, None, None),
            std::path::PathBuf::from("/tmp/buoy-wm-unknown.sock")
        );
    }

    #[test]
    fn resolve_socket_path_treats_empty_xdg_runtime_dir_as_unset() {
        // A set-but-empty XDG_RUNTIME_DIR (a real systemd/container
        // pattern) must fall through to the /tmp case, not join onto an
        // empty base and produce a relative "buoy-wm.sock" path.
        assert_eq!(
            resolve_socket_path(Some(""), Some("nick"), None),
            std::path::PathBuf::from("/tmp/buoy-wm-nick.sock")
        );
    }
}
