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

//! The Unix domain socket accept loop and per-connection handling.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::config::Defaults;
use crate::ipc::dispatch::handle_request;
use crate::ipc::lock_recovering;
use crate::ipc::protocol::{ParseError, Request, Response, parse_request, serialize_response};
use crate::wm_core::state::WmCore;

/// The maximum accepted length of one request line, per the AC's "oversized
/// (>64 KiB in one line)" bullet. A line at or beyond this length without a
/// terminating newline is treated as malformed input (NFR2: bounds the
/// read buffer instead of growing it forever for a hostile/broken client).
const MAX_LINE_BYTES: usize = 64 * 1024;

/// The longest peer-supplied detail this module will put in one log line.
/// Bounds the journal cost of a malformed request to a constant.
const MAX_LOGGED_DETAIL_BYTES: usize = 512;

/// Consecutive accept failures between log lines. The first failure of a
/// run is always logged; after that, `EMFILE` is a condition, not an
/// event, and one line per attempt would fill the journal faster than the
/// backoff sleep can slow it down.
const ACCEPT_ERROR_LOG_INTERVAL: u32 = 100;

/// Resource limits the accept loop and each connection enforce. Held in a
/// struct rather than read from the constants directly so the tests can
/// drive the very same code paths with deadlines that fit inside a test
/// run — a 600-second read deadline is not otherwise testable.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// Connections served concurrently. Excess connections are dropped
    /// immediately rather than queued: both real clients tolerate refusal
    /// (`buoy-status-bar` retries on its next poll, `buoy-tag-picker`
    /// exits with a message), and queueing would just move the unbounded
    /// thread growth somewhere less visible.
    max_connections: usize,
    /// Stack reserved per connection thread. Each handler holds a
    /// `BufReader` and one request line, so the platform default (8 MiB of
    /// address space) buys nothing.
    connection_stack_size: usize,
    /// How long a response write may block before the peer is treated as
    /// gone. Every response is one small line, so a peer that has not
    /// drained it in this long is not reading at all.
    write_timeout: Duration,
    /// How long a connection may sit idle between requests before it is
    /// closed. Deliberately *not* symmetric with `write_timeout`:
    /// `buoy-tag-picker` keeps one connection open across successive
    /// `fuzzel` invocations in assign mode, so this deadline has to cover
    /// human think-time or the picker breaks mid-use.
    idle_read_timeout: Duration,
    /// How long the accept loop sleeps after a failed `accept`. Turns a
    /// persistent `EMFILE` from a hot loop burning a core inside the
    /// window manager into a slow retry.
    accept_error_backoff: Duration,
    /// How many `create-tag` requests one connection may make before it is
    /// closed. The 64-tag registry is finite and, by ADR-006, never
    /// reclaimed, so it is the one resource a peer can spend permanently
    /// (audit finding E-02).
    ///
    /// Counted per connection rather than per process because the quota
    /// bounds a burst, not a session: a user who legitimately needs
    /// another tag presses `Super+S` again, which is a new connection.
    max_tag_creations_per_connection: usize,
}

impl Limits {
    /// The values the real WM runs with. See each field for why.
    const PRODUCTION: Self = Self {
        max_connections: 16,
        connection_stack_size: 256 * 1024,
        write_timeout: Duration::from_secs(5),
        idle_read_timeout: Duration::from_secs(600),
        accept_error_backoff: Duration::from_millis(100),
        // Eight rather than one: no legitimate flow creates more than one
        // tag per connection — `buoy-tag-picker`'s switch mode creates at
        // most one and then terminates, assign mode creates none, and the
        // keybind path never touches the socket — but the picker reopens on
        // a tag-cap rejection, so the headroom keeps the quota away from
        // any real sequence while still costing an abuser a fresh
        // connection for every eight registry slots.
        max_tag_creations_per_connection: 8,
    };
}

/// The live-connection counter behind [`Limits::max_connections`], plus a
/// one-shot flag that keeps the refusal log line from becoming the very
/// flood the accept-loop backoff exists to prevent: a peer hammering a
/// full server gets one line per capacity episode, not one per attempt.
struct ConnectionCount {
    active: AtomicUsize,
    at_capacity_reported: AtomicBool,
}

impl ConnectionCount {
    const fn new() -> Self {
        Self {
            active: AtomicUsize::new(0),
            at_capacity_reported: AtomicBool::new(false),
        }
    }

    /// Reserves one of `max` slots, returning the guard that releases it on
    /// drop. `None` means the server is at capacity and the caller must
    /// drop the connection.
    fn try_acquire(self: &Arc<Self>, max: usize) -> Option<ConnectionSlot> {
        if self.active.fetch_add(1, Ordering::AcqRel) >= max {
            self.active.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(ConnectionSlot {
            count: Arc::clone(self),
        })
    }

    /// Logs "at capacity" at most once per episode, so the refusal path
    /// cannot be used to write to the journal at connect speed.
    fn report_at_capacity(&self, max: usize) {
        if !self.at_capacity_reported.swap(true, Ordering::AcqRel) {
            eprintln!("ipc: at capacity ({max} connections); refusing new connections");
        }
    }
}

/// Releases one connection slot when a connection handler ends — including
/// when it ends by way of the `catch_unwind` in [`handle_connection`],
/// which is why this is a `Drop` guard rather than a decrement at the end
/// of the handler body.
struct ConnectionSlot {
    count: Arc<ConnectionCount>,
}

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.count.active.fetch_sub(1, Ordering::AcqRel);
        // A freed slot starts a new capacity episode, so the next refusal
        // is worth one line again.
        self.count
            .at_capacity_reported
            .store(false, Ordering::Release);
    }
}

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
///
/// The accept loop is resource-bounded: at most [`Limits::max_connections`]
/// connections are served at once, excess connections are dropped, a
/// failed `accept` backs off instead of spinning, and a connection thread
/// that cannot be created closes that one connection instead of
/// unwinding the accept-loop thread. That last point is load-bearing —
/// nothing restarts this thread, so losing it means IPC is dead (pickers
/// and every status bar) for the rest of the login session.
pub fn spawn(
    wm_core: Arc<Mutex<WmCore>>,
    socket_path: &Path,
    defaults: Defaults,
) -> std::io::Result<JoinHandle<()>> {
    spawn_with_limits(wm_core, socket_path, defaults, Limits::PRODUCTION)
}

/// [`spawn`] with the resource limits supplied explicitly, so the tests can
/// exercise the cap, the write timeout, the idle read deadline and the
/// thread-spawn failure path with values that fit inside a test run.
fn spawn_with_limits(
    wm_core: Arc<Mutex<WmCore>>,
    socket_path: &Path,
    defaults: Defaults,
    limits: Limits,
) -> std::io::Result<JoinHandle<()>> {
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path)?;
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))?;

    // `Builder::spawn` rather than `thread::spawn`: a thread that cannot be
    // created is an `Err` the caller can log and degrade on, not a panic in
    // the middle of WM startup.
    std::thread::Builder::new()
        .name("buoy-ipc-accept".to_string())
        .spawn(move || accept_loop(listener, wm_core, defaults, limits))
}

/// Accepts connections forever, handing each to its own thread while the
/// connection budget allows. Never returns: `UnixListener::incoming()`
/// never yields `None`.
fn accept_loop(
    listener: UnixListener,
    wm_core: Arc<Mutex<WmCore>>,
    defaults: Defaults,
    limits: Limits,
) {
    let connections = Arc::new(ConnectionCount::new());
    let mut consecutive_errors: u32 = 0;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                consecutive_errors = 0;
                match connections.try_acquire(limits.max_connections) {
                    Some(slot) => launch_connection(stream, slot, &wm_core, &defaults, limits),
                    // Dropping `stream` closes it, which both real clients
                    // handle: the status bar retries next poll, the picker
                    // reports and exits.
                    None => connections.report_at_capacity(limits.max_connections),
                }
            }
            Err(e) => {
                consecutive_errors = consecutive_errors.saturating_add(1);
                if consecutive_errors == 1 || consecutive_errors % ACCEPT_ERROR_LOG_INTERVAL == 0 {
                    eprintln!(
                        "ipc: accept error (consecutive: {consecutive_errors}), backing off: {e}"
                    );
                }
                // A persistent EMFILE/ENFILE would otherwise make this a
                // hot loop writing stderr as fast as journald accepts it,
                // inside the window manager process.
                std::thread::sleep(limits.accept_error_backoff);
            }
        }
    }
}

/// Moves one accepted connection onto its own thread. A failure to create
/// that thread closes the connection and leaves the accept loop running;
/// `slot` is released either way, because a failed `Builder::spawn` drops
/// the closure it was given.
fn launch_connection(
    stream: UnixStream,
    slot: ConnectionSlot,
    wm_core: &Arc<Mutex<WmCore>>,
    defaults: &Defaults,
    limits: Limits,
) {
    let wm_core = Arc::clone(wm_core);
    let defaults = defaults.clone();
    let spawned = std::thread::Builder::new()
        .name("buoy-ipc-conn".to_string())
        .stack_size(limits.connection_stack_size)
        .spawn(move || {
            // `slot` lives in the thread body so its release survives
            // `handle_connection`'s `catch_unwind`.
            let _slot = slot;
            handle_connection(stream, wm_core, &defaults, limits);
        });
    if let Err(e) = spawned {
        eprintln!("ipc: cannot spawn connection thread, dropping connection: {e}");
    }
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
fn handle_connection(
    stream: UnixStream,
    wm_core: Arc<Mutex<WmCore>>,
    defaults: &Defaults,
    limits: Limits,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_connection_inner(stream, &wm_core, defaults, limits);
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

/// Whether `error` is a read/write deadline expiring rather than a real
/// I/O failure. Linux surfaces `SO_RCVTIMEO`/`SO_SNDTIMEO` as `EAGAIN`,
/// which maps to `WouldBlock`, but other platforms use `TimedOut`.
fn is_timeout(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

/// Renders `error` for a log line, escaped and length-capped.
///
/// [`ParseError::InvalidJson`] carries `serde_json`'s message, which quotes
/// the peer's own text verbatim — so this is deliberately built with
/// `{:?}` and never `{}`. `Display` would let anything that can reach the
/// socket write newlines and forged `ipc: ` prefixes straight into the
/// journal, turning a missing-log-line bug into a log-injection primitive.
fn describe_parse_error(error: &ParseError) -> String {
    match error {
        ParseError::InvalidUtf8 => "invalid utf-8".to_string(),
        ParseError::InvalidJson(detail) => truncate_for_log(&format!("{detail:?}")),
    }
}

/// Caps an already-escaped log payload at [`MAX_LOGGED_DETAIL_BYTES`],
/// cutting on a `char` boundary so the result stays valid UTF-8.
fn truncate_for_log(escaped: &str) -> String {
    if escaped.len() <= MAX_LOGGED_DETAIL_BYTES {
        return escaped.to_string();
    }
    let mut end = MAX_LOGGED_DETAIL_BYTES;
    while !escaped.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &escaped[..end])
}

/// A stable, never-peer-controlled label for `request`'s type. Exhaustive
/// by construction, so a new [`Request`] variant is a compile error here
/// rather than an unlabelled log line.
fn request_kind(request: &Request) -> &'static str {
    match request {
        Request::GetState => "get-state",
        Request::ToggleTag { .. } => "toggle-tag",
        Request::CreateTag { .. } => "create-tag",
        Request::SwitchTag { .. } => "switch-tag",
    }
}

/// Logs the outcome of one dispatched request.
///
/// Rejections are always logged — they are the abuse signal that was
/// missing entirely, and the connection deliberately stays open after one,
/// which makes the wire errors an id-space oracle worth a journal trail.
/// Successes are logged only for the three mutating requests: `get-state`
/// is polled by every status bar four times a second per output, so
/// logging it would drown everything else, including these rejections.
///
/// `message` is server-generated, but printed with `{:?}` anyway — the
/// same rule as [`describe_parse_error`], applied by default rather than
/// per-site, so a future wire error that embeds peer text cannot quietly
/// become injectable.
fn log_dispatch_outcome(kind: &'static str, response: &Response) {
    match response {
        Response::Error { message } => eprintln!("ipc: {kind} rejected: {message:?}"),
        Response::Ok | Response::TagCreated { .. } => eprintln!("ipc: {kind} applied"),
        Response::State { .. } => {}
    }
}

fn handle_connection_inner(
    stream: UnixStream,
    wm_core: &Arc<Mutex<WmCore>>,
    defaults: &Defaults,
    limits: Limits,
) {
    // Set before the `try_clone` below: these are socket-level options, so
    // one call covers the reader and the writer view of the same socket.
    if let Err(e) = stream.set_write_timeout(Some(limits.write_timeout)) {
        eprintln!("ipc: cannot set connection write timeout, closing: {e}");
        return;
    }
    if let Err(e) = stream.set_read_timeout(Some(limits.idle_read_timeout)) {
        eprintln!("ipc: cannot set connection read deadline, closing: {e}");
        return;
    }

    let mut writer = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ipc: failed to clone connection for writing: {e}");
            return;
        }
    };
    let mut reader = BufReader::new(stream);
    let mut tag_creations: usize = 0;

    loop {
        let mut buf = Vec::new();
        let read_result = reader
            .by_ref()
            .take(MAX_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut buf);

        let n = match read_result {
            Ok(n) => n,
            Err(e) if is_timeout(&e) => {
                eprintln!(
                    "ipc: closing connection idle for more than {:?}",
                    limits.idle_read_timeout
                );
                return;
            }
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
            eprintln!(
                "ipc: rejecting request line over the {MAX_LINE_BYTES}-byte cap; closing connection"
            );
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
            Err(e) => {
                eprintln!(
                    "ipc: rejecting malformed request: {}",
                    describe_parse_error(&e)
                );
                write_response(
                    &mut writer,
                    &Response::Error {
                        message: "malformed request".into(),
                    },
                );
                return; // malformed/unparseable input: reset the connection
            }
            Ok(request) => {
                let kind = request_kind(&request);
                if matches!(request, Request::CreateTag { .. }) {
                    tag_creations = tag_creations.saturating_add(1);
                    if tag_creations > limits.max_tag_creations_per_connection {
                        eprintln!(
                            "ipc: create-tag quota ({}) spent on one connection; closing",
                            limits.max_tag_creations_per_connection
                        );
                        write_response(
                            &mut writer,
                            &Response::Error {
                                message: "create-tag quota exceeded on this connection".into(),
                            },
                        );
                        return;
                    }
                }
                let (response, pending_spawn) = {
                    let mut core = lock_recovering(wm_core);
                    handle_request(&mut core, request)
                };
                log_dispatch_outcome(kind, &response);
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
                if let Some(pending) = pending_spawn {
                    crate::spawn_pinned_terminal_or_release_claim(
                        wm_core,
                        pending.tag_id,
                        &defaults.terminal,
                        &defaults.pinned_terminal_argv(
                            &crate::wm_core::state::pinned_term_app_id(pending.tag_id),
                            &pending.session_name,
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
/// (the WM runs under a real `river` login session, which always sets
/// `XDG_RUNTIME_DIR`). Devcontainer/
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
            // Generous, but finite, on both directions: with no
            // client-side deadline a server bug that stops answering — or
            // stops reading — turns the whole suite into a hang rather
            // than one failing test.
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("set test client read timeout");
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .expect("set test client write timeout");
            let read = BufReader::new(stream.try_clone().expect("clone stream for reading"));
            Self {
                write: stream,
                read,
            }
        }

        fn send_line(&mut self, line: &str) {
            self.try_send_line(line).expect("write to test socket");
        }

        /// `send_line` for the cases where the server closing on us mid-burst
        /// is the behaviour under test rather than a failure.
        fn try_send_line(&mut self, line: &str) -> std::io::Result<()> {
            self.write.write_all(line.as_bytes())?;
            self.write.write_all(b"\n")
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

    /// Removes a test server's socket inode when the test ends, including
    /// on a panicking assertion, so a resource-limit test run does not
    /// litter the temp directory with one socket per case.
    struct SocketGuard(std::path::PathBuf);

    impl SocketGuard {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for SocketGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn spawn_test_server_with_limits(
        core: WmCore,
        limits: Limits,
    ) -> (SocketGuard, Arc<Mutex<WmCore>>) {
        let socket_path = unique_socket_path();
        let wm_core = Arc::new(Mutex::new(core));
        spawn_with_limits(
            Arc::clone(&wm_core),
            &socket_path,
            Defaults {
                terminal: "/bin/true".to_string(),
                ..Defaults::default()
            },
            limits,
        )
        .expect("server must spawn successfully");
        (SocketGuard(socket_path), wm_core)
    }

    /// What a one-shot `get-state` probe on a fresh connection observed.
    /// `Refused` and `TimedOut` are kept apart deliberately: a refused
    /// connection is the connection cap doing its job, while a timed-out
    /// one means the accept loop itself stopped accepting — the failure
    /// mode H-01 exists to prevent, and one that would otherwise show up
    /// as a hung test suite rather than a failing test.
    #[derive(Debug)]
    enum ProbeResult {
        Served(String),
        Refused,
        TimedOut,
    }

    fn probe_get_state(path: &std::path::Path) -> ProbeResult {
        let mut stream = UnixStream::connect(path).expect("connect to test socket");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set probe read timeout");
        if stream.write_all(b"{\"type\":\"get-state\"}\n").is_err() {
            return ProbeResult::Refused;
        }
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => ProbeResult::Refused,
            Ok(_) => ProbeResult::Served(line),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                ProbeResult::TimedOut
            }
            Err(_) => ProbeResult::Refused,
        }
    }

    /// Polls [`probe_get_state`] until the server serves a connection.
    /// Slot release is asynchronous — the handler thread has to observe the
    /// peer's EOF or its own write timeout first — so a single unretried
    /// probe would be a race.
    fn wait_for_served_get_state(path: &std::path::Path) -> String {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match probe_get_state(path) {
                ProbeResult::Served(line) => return line,
                other => assert!(
                    std::time::Instant::now() < deadline,
                    "server never freed a connection slot (last probe: {other:?})"
                ),
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn production_read_deadline_is_far_longer_than_the_write_timeout() {
        // The asymmetry is load-bearing, not incidental: `buoy-tag-picker`
        // holds one connection open across successive `fuzzel` invocations
        // in assign mode, so the read side has to tolerate human
        // think-time while the write side must not tolerate a peer that
        // has stopped reading. A future edit that collapses the two into
        // one value breaks assign mode, so pin the relationship here.
        assert!(
            Limits::PRODUCTION.idle_read_timeout > Limits::PRODUCTION.write_timeout * 10,
            "read deadline must dwarf the write timeout"
        );
    }

    #[test]
    fn connections_beyond_the_cap_are_dropped_and_their_slots_are_reused() {
        let limits = Limits {
            max_connections: 2,
            ..Limits::PRODUCTION
        };
        let (socket, _wm_core) = spawn_test_server_with_limits(WmCore::new(), limits);

        // Both slots are held by connections whose handler thread is
        // parked in `read_until` waiting for a second request.
        let mut first = TestClient::connect(socket.path());
        first.send_line(r#"{"type":"get-state"}"#);
        assert!(
            first
                .read_line()
                .expect("first slot served")
                .contains(r#""type":"state""#)
        );
        let mut second = TestClient::connect(socket.path());
        second.send_line(r#"{"type":"get-state"}"#);
        assert!(
            second
                .read_line()
                .expect("second slot served")
                .contains(r#""type":"state""#)
        );

        assert!(
            matches!(probe_get_state(socket.path()), ProbeResult::Refused),
            "a connection beyond the cap must be dropped, not queued or served"
        );

        drop(first);
        assert!(wait_for_served_get_state(socket.path()).contains(r#""type":"state""#));
        drop(second);
    }

    #[test]
    fn write_timeout_frees_a_slot_held_by_a_peer_that_never_reads() {
        let limits = Limits {
            max_connections: 1,
            write_timeout: Duration::from_millis(100),
            ..Limits::PRODUCTION
        };
        // A fat state makes each `get-state` response large enough that a
        // few hundred unread replies overrun the socket's send buffer,
        // which is the only way to park the handler in `write_all` at all.
        // The bulk comes from view `app_id`s rather than tag names, which
        // are capped at `MAX_TAG_NAME_BYTES` (audit finding E-01) and
        // cannot be padded arbitrarily.
        let mut core = WmCore::new();
        for i in 0..64 {
            core.create_tag(format!("{}{i}", "t".repeat(60)))
                .expect("fill the tag registry");
        }
        for i in 0..64 {
            core.register_view(&format!("{}{i}", "a".repeat(200)));
        }
        let (socket, _wm_core) = spawn_test_server_with_limits(core, limits);

        let mut greedy = TestClient::connect(socket.path());
        // The server closing on us part-way through the burst *is* the
        // write timeout working, so a failed write here is a pass.
        for _ in 0..500 {
            if greedy.try_send_line(r#"{"type":"get-state"}"#).is_err() {
                break;
            }
        }

        // Without a write timeout the handler blocks in `write_all`
        // forever and never releases the only slot, so this call would
        // exhaust its deadline instead of returning.
        assert!(wait_for_served_get_state(socket.path()).contains(r#""type":"state""#));
        drop(greedy);
    }

    /// Audit finding E-02: the 64-tag registry is finite and, by ADR-006,
    /// never reclaimed, so one connection must not be able to spend it.
    /// The quota counts attempts, not successes, and closes the connection
    /// once it is spent.
    #[test]
    fn create_tag_requests_beyond_the_per_connection_quota_are_refused() {
        let limits = Limits {
            max_tag_creations_per_connection: 2,
            ..Limits::PRODUCTION
        };
        let (socket, wm_core) = spawn_test_server_with_limits(WmCore::new(), limits);

        let mut client = TestClient::connect(socket.path());
        for i in 0..2 {
            client.send_line(&format!(r#"{{"type":"create-tag","name":"tag{i}"}}"#));
            assert!(
                client
                    .read_line()
                    .expect("within quota")
                    .contains(r#""type":"tag-created""#)
            );
        }

        client.send_line(r#"{"type":"create-tag","name":"one-too-many"}"#);
        let refusal = client.read_line().expect("a refusal, not a closed socket");
        assert!(
            refusal.contains("quota"),
            "expected a quota refusal, got {refusal:?}"
        );
        assert_eq!(
            client.read_line(),
            None,
            "the connection must be closed once its quota is spent"
        );

        let core = lock_recovering(&wm_core);
        assert_eq!(
            core.tag_count(),
            2,
            "a refused create-tag must not reach the registry"
        );
    }

    /// The quota is per connection, so a client that legitimately needs
    /// another tag can open one — it bounds a single peer's burst, it does
    /// not lock the registry.
    #[test]
    fn a_fresh_connection_gets_a_fresh_create_tag_quota() {
        let limits = Limits {
            max_tag_creations_per_connection: 1,
            ..Limits::PRODUCTION
        };
        let (socket, wm_core) = spawn_test_server_with_limits(WmCore::new(), limits);

        for i in 0..3 {
            let mut client = TestClient::connect(socket.path());
            client.send_line(&format!(r#"{{"type":"create-tag","name":"tag{i}"}}"#));
            assert!(
                client
                    .read_line()
                    .expect("first create-tag on a fresh connection")
                    .contains(r#""type":"tag-created""#)
            );
        }

        assert_eq!(lock_recovering(&wm_core).tag_count(), 3);
    }

    #[test]
    fn an_idle_connection_is_closed_after_the_read_deadline() {
        let limits = Limits {
            idle_read_timeout: Duration::from_millis(150),
            ..Limits::PRODUCTION
        };
        let (socket, _wm_core) = spawn_test_server_with_limits(WmCore::new(), limits);

        let mut client = TestClient::connect(socket.path());
        client.send_line(r#"{"type":"get-state"}"#);
        assert!(
            client
                .read_line()
                .expect("served once")
                .contains(r#""type":"state""#)
        );
        assert_eq!(
            client.read_line(),
            None,
            "an idle connection must be closed once its read deadline passes, \
             rather than parking a thread for the rest of the session"
        );
    }

    #[test]
    fn a_pause_between_requests_far_longer_than_the_write_timeout_keeps_the_connection() {
        // Assign mode's shape: one connection, several requests, human
        // think-time in between. The read deadline is what must govern
        // here, never the write timeout.
        let limits = Limits {
            write_timeout: Duration::from_millis(50),
            ..Limits::PRODUCTION
        };
        let (socket, _wm_core) = spawn_test_server_with_limits(WmCore::new(), limits);

        let mut client = TestClient::connect(socket.path());
        client.send_line(r#"{"type":"get-state"}"#);
        assert!(
            client
                .read_line()
                .expect("first request served")
                .contains(r#""type":"state""#)
        );
        std::thread::sleep(Duration::from_millis(250));
        client.send_line(r#"{"type":"get-state"}"#);
        assert!(
            client
                .read_line()
                .expect("connection must survive a pause many times the write timeout")
                .contains(r#""type":"state""#)
        );
    }

    #[test]
    fn a_connection_thread_that_cannot_be_spawned_does_not_kill_the_accept_loop() {
        // Requesting a 1 TiB stack is refused by the kernel with the same
        // EAGAIN a thread-exhausted host produces, which is the only way
        // to reach `Builder::spawn`'s error arm without touching process
        // rlimits from inside a shared test binary.
        let limits = Limits {
            connection_stack_size: 1 << 40,
            ..Limits::PRODUCTION
        };
        let (socket, _wm_core) = spawn_test_server_with_limits(WmCore::new(), limits);

        assert!(
            matches!(probe_get_state(socket.path()), ProbeResult::Refused),
            "an unspawnable connection must be dropped, not panic the accept loop"
        );
        assert!(
            matches!(probe_get_state(socket.path()), ProbeResult::Refused),
            "the accept loop must still be accepting after a spawn failure"
        );
    }

    #[test]
    fn request_kind_labels_every_variant() {
        assert_eq!(
            request_kind(&crate::ipc::protocol::Request::GetState),
            "get-state"
        );
        assert_eq!(
            request_kind(&crate::ipc::protocol::Request::ToggleTag {
                view_id: 1,
                tag_id: 0
            }),
            "toggle-tag"
        );
        assert_eq!(
            request_kind(&crate::ipc::protocol::Request::CreateTag { name: "x".into() }),
            "create-tag"
        );
        assert_eq!(
            request_kind(&crate::ipc::protocol::Request::SwitchTag {
                output_id: 1,
                tag_id: 0
            }),
            "switch-tag"
        );
    }

    #[test]
    fn parse_error_log_detail_escapes_peer_supplied_newlines() {
        let error = ParseError::InvalidJson("harmless\nipc: forged log line".to_string());
        let detail = describe_parse_error(&error);
        assert!(
            !detail.contains('\n'),
            "a socket peer must not be able to write a second journal line: {detail}"
        );
        assert!(
            detail.contains("\\n"),
            "the newline must survive as an escape: {detail}"
        );
    }

    #[test]
    fn parse_error_log_detail_is_length_capped() {
        let error = ParseError::InvalidJson("x".repeat(MAX_LOGGED_DETAIL_BYTES * 4));
        let detail = describe_parse_error(&error);
        assert!(
            detail.len() <= MAX_LOGGED_DETAIL_BYTES + 3,
            "one malformed request must not write an unbounded journal line ({} bytes)",
            detail.len()
        );
    }

    #[test]
    fn parse_error_log_detail_names_the_invalid_utf8_case() {
        assert_eq!(
            describe_parse_error(&ParseError::InvalidUtf8),
            "invalid utf-8"
        );
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
