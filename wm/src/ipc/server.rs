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

use std::io::{BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use buoy_common::framing::{Line, MAX_LINE_BYTES, read_line_bounded};
use buoy_common::peer::{PeerIdentity, authenticate_peer};
use buoy_common::{log_err, log_info};

use crate::ipc::dispatch::{PendingPinnedSpawn, handle_request};
use crate::ipc::protocol::{ParseError, Request, Response, parse_request, serialize_response};
use crate::ipc::{lock_recovering, panic_message};
use crate::wm_core::state::WmCore;

/// The longest peer-supplied detail this module will put in one log line.
/// Bounds the journal cost of a malformed request to a constant.
const MAX_LOGGED_DETAIL_BYTES: usize = 512;

/// Performs the pinned-terminal spawn a dispatched `switch-tag` claimed.
///
/// An injected effect rather than a call up into the crate root, which was
/// the workspace's one bidirectional module dependency: a leaf transport
/// module reaching into the composition root to launch a process (audit
/// finding J-02). Three things follow. The dependency is now visible in
/// [`spawn`]'s signature instead of hiding inside a function body; this
/// module no longer needs `crate::config` or anything from the Wayland
/// root, so it can move to a library crate; and the tests can assert that
/// a spawn was *requested* without a process ever being created.
///
/// `Arc` because one accept loop hands the same effect to every connection
/// thread, and it outlives all of them. Called with the shared `wm-core`
/// mutex released — see the call site.
pub type SpawnPinnedTerminal = Arc<dyn Fn(&PendingPinnedSpawn) + Send + Sync>;

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
            log_err!("at capacity ({max} connections); refusing new connections");
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

/// Clears `socket_path` so `bind` can have it, refusing rather than
/// clobbering whatever is already there.
///
/// This used to be `let _ = std::fs::remove_file(socket_path)` — the one
/// production `let _ =` in the workspace that discarded a meaningful
/// error, and load-bearing rather than defensive because nothing unlinked
/// the socket on shutdown (audit findings C-04 and B-04). It would follow
/// a symlink, delete an arbitrary file planted at the path, and — worst —
/// could not tell a stale inode from **a second, still-running instance**,
/// whose socket it would happily unlink, leaving the first WM listening on
/// an inode nobody can reach while every picker and status bar silently
/// attached to the second.
///
/// So: probe first. A path that answers `connect` belongs to a live
/// instance and is left alone; a path whose inode is not a socket is left
/// alone too, because a window manager has no business deleting files it
/// was merely pointed at. Only a genuine dead socket is removed.
fn clear_socket_path(socket_path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::FileTypeExt;

    let metadata = match std::fs::symlink_metadata(socket_path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if UnixStream::connect(socket_path).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!(
                "another buoy-wm instance is already listening on {}",
                socket_path.display()
            ),
        ));
    }
    if !metadata.file_type().is_socket() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "{} exists and is not a socket; refusing to remove it",
                socket_path.display()
            ),
        ));
    }
    std::fs::remove_file(socket_path)
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
/// `spawn_pinned_terminal` is the one effect this module has that is not
/// reading or writing the socket; see [`SpawnPinnedTerminal`] for why it
/// arrives as a parameter.
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
    spawn_pinned_terminal: SpawnPinnedTerminal,
) -> std::io::Result<JoinHandle<()>> {
    spawn_with_limits(
        wm_core,
        socket_path,
        spawn_pinned_terminal,
        Limits::PRODUCTION,
    )
}

/// [`spawn`] with the resource limits supplied explicitly, so the tests can
/// exercise the cap, the write timeout, the idle read deadline and the
/// thread-spawn failure path with values that fit inside a test run.
fn spawn_with_limits(
    wm_core: Arc<Mutex<WmCore>>,
    socket_path: &Path,
    spawn_pinned_terminal: SpawnPinnedTerminal,
    limits: Limits,
) -> std::io::Result<JoinHandle<()>> {
    clear_socket_path(socket_path)?;
    let listener = UnixListener::bind(socket_path)?;
    // Belt and braces over the private parent directory
    // `buoy_common::socket_path::verify_private_dir` insists on: `bind`
    // creates the inode at `0777 & ~umask`, and a chmod cannot revoke a
    // connection already won in that window, so this mode is a second
    // line rather than the control. On failure the inode is removed again
    // — leaving a socket file with no accept loop behind it was how a
    // failed chmod used to present (audit finding C-03).
    if let Err(e) = std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600)) {
        drop(listener);
        let _ = std::fs::remove_file(socket_path);
        return Err(e);
    }

    // `Builder::spawn` rather than `thread::spawn`: a thread that cannot be
    // created is an `Err` the caller can log and degrade on, not a panic in
    // the middle of WM startup.
    std::thread::Builder::new()
        .name("buoy-ipc-accept".to_string())
        .spawn(move || accept_loop(listener, wm_core, spawn_pinned_terminal, limits))
}

/// Accepts connections forever, handing each to its own thread while the
/// connection budget allows. Never returns: `UnixListener::incoming()`
/// never yields `None`.
fn accept_loop(
    listener: UnixListener,
    wm_core: Arc<Mutex<WmCore>>,
    spawn_pinned_terminal: SpawnPinnedTerminal,
    limits: Limits,
) {
    let connections = Arc::new(ConnectionCount::new());
    let mut consecutive_errors: u32 = 0;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                consecutive_errors = 0;
                match connections.try_acquire(limits.max_connections) {
                    Some(slot) => {
                        launch_connection(stream, slot, &wm_core, &spawn_pinned_terminal, limits)
                    }
                    // Dropping `stream` closes it, which both real clients
                    // handle: the status bar retries next poll, the picker
                    // reports and exits.
                    None => connections.report_at_capacity(limits.max_connections),
                }
            }
            Err(e) => {
                consecutive_errors = consecutive_errors.saturating_add(1);
                if consecutive_errors == 1
                    || consecutive_errors.is_multiple_of(ACCEPT_ERROR_LOG_INTERVAL)
                {
                    log_err!("accept error (consecutive: {consecutive_errors}), backing off: {e}");
                }
                // A persistent EMFILE/ENFILE would otherwise make this a
                // hot loop writing stderr as fast as journald accepts it,
                // inside the window manager process.
                std::thread::sleep(limits.accept_error_backoff);
            }
        }
    }

    // Documented as unreachable — `UnixListener::incoming()` never yields
    // `None` — and therefore exactly the kind of thing that would end IPC
    // for the rest of the login session with no record at all if the
    // documentation were ever wrong (audit finding B-03).
    log_err!("the accept loop ended, so IPC is down until buoy-wm restarts");
}

/// Moves one accepted connection onto its own thread. A failure to create
/// that thread closes the connection and leaves the accept loop running;
/// `slot` is released either way, because a failed `Builder::spawn` drops
/// the closure it was given.
fn launch_connection(
    stream: UnixStream,
    slot: ConnectionSlot,
    wm_core: &Arc<Mutex<WmCore>>,
    spawn_pinned_terminal: &SpawnPinnedTerminal,
    limits: Limits,
) {
    let wm_core = Arc::clone(wm_core);
    let spawn_pinned_terminal = Arc::clone(spawn_pinned_terminal);
    let spawned = std::thread::Builder::new()
        .name("buoy-ipc-conn".to_string())
        .stack_size(limits.connection_stack_size)
        .spawn(move || {
            // `slot` lives in the thread body so its release survives
            // `handle_connection`'s `catch_unwind`.
            let _slot = slot;
            handle_connection(stream, wm_core, &spawn_pinned_terminal, limits);
        });
    if let Err(e) = spawned {
        log_err!("cannot spawn connection thread, dropping connection: {e}");
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
/// own thread, and [`lock_recovering`] is
/// what actually keeps a poisoned mutex from crashing the *next* locker.
///
/// The `Err` arm below is deliberately uncovered, and this note is the
/// record of that decision rather than an oversight (audit finding T-05).
/// Reaching it needs a panic inside `handle_connection_inner`, and the only
/// ways to arrange one are a `#[cfg(test)]` [`Request`] variant that panics
/// — a test-only branch in a cross-process protocol, which this codebase
/// does not do anywhere — or a fault injection point in the read path that
/// would itself be the untested code. The payload decode is covered on its
/// own ([`panic_message`]), so what is untested here is `catch_unwind`
/// returning `Err`, which is `std`'s contract rather than this module's.
fn handle_connection(
    stream: UnixStream,
    wm_core: Arc<Mutex<WmCore>>,
    spawn_pinned_terminal: &SpawnPinnedTerminal,
    limits: Limits,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_connection_inner(stream, &wm_core, spawn_pinned_terminal, limits);
    }));
    if let Err(payload) = result {
        log_err!(
            "connection handler panicked (contained): {}",
            panic_message(&*payload)
        );
    }
}

/// Writes `response` as one JSON line followed by `\n`, flushing
/// afterward. Returns `false` (caller should close the connection) if the
/// write itself fails.
///
/// A failure that is not a hangup is logged. The three `is_ok()` calls this
/// replaced threw away *why*, on the strength of a doc comment asserting
/// that a write failure always means the peer is gone — true for
/// `BrokenPipe`, false for `ENOBUFS`, `EINTR` and an expired write
/// deadline, each of which says something about this server rather than
/// about the peer (audit finding F-05). The read side at
/// `handle_connection_inner` already logs; this makes the two consistent.
fn write_response(stream: &mut UnixStream, response: &Response) -> bool {
    let line = serialize_response(response);
    for chunk in [line.as_bytes(), b"\n"] {
        if let Err(e) = stream.write_all(chunk) {
            report_write_failure(&e);
            return false;
        }
    }
    if let Err(e) = stream.flush() {
        report_write_failure(&e);
        return false;
    }
    true
}

/// Logs a failed response write unless it is just the peer having hung up.
fn report_write_failure(error: &std::io::Error) {
    if !is_peer_hangup(error) {
        log_err!("could not write a response: {error}");
    }
}

/// Whether a failed write says nothing more than "the peer is gone".
///
/// Only `BrokenPipe` does. Everything else — a full socket buffer, an
/// interrupted syscall, the write deadline expiring — is a condition worth
/// a log line (audit finding F-05).
fn is_peer_hangup(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::BrokenPipe
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

/// A peer's identity as one log field, so a refusal names the process that
/// earned it rather than only the refusal (audit finding G-01).
fn describe_peer(peer: PeerIdentity) -> String {
    format!("pid {} uid {}", peer.pid, peer.uid)
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
fn log_dispatch_outcome(peer: PeerIdentity, kind: &'static str, response: &Response) {
    match response {
        Response::Error { message } => {
            log_err!("{kind} from {} rejected: {message:?}", describe_peer(peer))
        }
        Response::Ok | Response::TagCreated { .. } => log_info!("{kind} applied"),
        Response::State { .. } => {}
    }
}

/// One connection's memo of the last `get-state` answer it produced, valid
/// for as long as [`WmCore::generation`] has not moved.
///
/// Per connection, deliberately, and that is the whole design: `get-state`
/// used to deep-clone every tag name and every view's `app_id` — and do an
/// O(tags) scan per view — while holding the mutex the Wayland dispatch
/// thread needs six or seven times per manage/render sequence against a
/// 50 ms budget, and `buoy-status-bar` asks for it four times a second per
/// output for the whole login session (audit finding B-02). A cache shared
/// between connections would need its own lock, and a second lock taken
/// anywhere near this one is how a deadlock gets written: `lock_recovering`
/// is not reentrant, and the accept loop's whole purpose is to keep the
/// compositor's event loop unblocked. A per-connection memo needs no
/// synchronisation at all, and now that both real clients hold their
/// connection open across requests it covers the entire steady state.
struct StateCache {
    generation: u64,
    response: Rc<Response>,
}

/// Dispatches `request`, answering `get-state` from `cache` when nothing has
/// changed since it was filled.
///
/// The mutex is held for the `generation` read and, on a miss, for the
/// snapshot — and released before the caller writes to the socket, exactly
/// as the uncached version was. Nothing else is locked, in either order.
fn dispatch(
    wm_core: &Arc<Mutex<WmCore>>,
    request: Request,
    cache: &mut Option<StateCache>,
) -> (Rc<Response>, Option<PendingPinnedSpawn>) {
    if matches!(request, Request::GetState) {
        let mut core = lock_recovering(wm_core);
        let generation = core.generation();
        if let Some(cached) = cache.as_ref().filter(|c| c.generation == generation) {
            return (Rc::clone(&cached.response), None);
        }
        // `handle_request` rather than `snapshot()` directly, so there is one
        // code path from `Request` to `Response` and this arm cannot drift
        // from the uncached one. `get-state` never claims a spawn, and
        // passing whatever it returned through says so without asserting it.
        let (response, pending_spawn) = handle_request(&mut core, request);
        drop(core);
        let response = Rc::new(response);
        *cache = Some(StateCache {
            generation,
            response: Rc::clone(&response),
        });
        return (response, pending_spawn);
    }
    let mut core = lock_recovering(wm_core);
    let (response, pending_spawn) = handle_request(&mut core, request);
    drop(core);
    (Rc::new(response), pending_spawn)
}

// 102 code lines against the 100-line gate, and the two over are the
// authentication preamble that has to run before anything is read. Left
// whole rather than split for the sake of the number: the read/dispatch/
// write sequence below is one transaction and every proposed cut so far
// moved lines without moving the decision. Tracked as debt, not accepted
// as a pattern (audit finding T-06). Was four over until the `get-state`
// cache moved the dispatch block into [`dispatch`] (finding B-02).
#[allow(clippy::too_many_lines)]
fn handle_connection_inner(
    stream: UnixStream,
    wm_core: &Arc<Mutex<WmCore>>,
    spawn_pinned_terminal: &SpawnPinnedTerminal,
    limits: Limits,
) {
    // Before anything is read, let alone dispatched: the socket's mode was
    // the only access control on this whole surface, so whoever won a race
    // on the path or the mode got the full request set — including the
    // `switch-tag` that makes the WM spawn a process (audit finding C-06).
    // `SO_PEERCRED` is stamped by the kernel at `connect(2)` and cannot be
    // forged from userspace, which is what makes it independent of any
    // filesystem race. Fails closed on unreadable credentials.
    let peer = match authenticate_peer(&stream) {
        Ok(peer) => peer,
        Err(rejection) => {
            log_err!("refusing connection: {rejection}");
            return;
        }
    };

    // Set before the `try_clone` below: these are socket-level options, so
    // one call covers the reader and the writer view of the same socket.
    if let Err(e) = stream.set_write_timeout(Some(limits.write_timeout)) {
        log_err!("cannot set connection write timeout, closing: {e}");
        return;
    }
    if let Err(e) = stream.set_read_timeout(Some(limits.idle_read_timeout)) {
        log_err!("cannot set connection read deadline, closing: {e}");
        return;
    }

    let mut writer = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            log_err!("failed to clone connection for writing: {e}");
            return;
        }
    };
    let mut reader = BufReader::new(stream);
    let mut tag_creations: usize = 0;
    let mut state_cache: Option<StateCache> = None;

    loop {
        let line = match read_line_bounded(&mut reader) {
            Ok(line) => line,
            Err(e) if is_timeout(&e) => {
                log_info!(
                    "closing connection idle for more than {:?}",
                    limits.idle_read_timeout
                );
                return;
            }
            Err(e) => {
                log_err!("connection read error, closing: {e}");
                return;
            }
        };

        let line_bytes = match line {
            Line::Complete(bytes) => bytes,
            // EOF, cleanly or mid-line: nothing more the client will send.
            Line::Eof => return,
            Line::Oversize => {
                log_err!(
                    "rejecting a request line from {} over the {MAX_LINE_BYTES}-byte cap; closing connection",
                    describe_peer(peer)
                );
                write_response(
                    &mut writer,
                    &Response::Error {
                        message: "malformed request".into(),
                    },
                );
                return;
            }
        };

        match parse_request(&line_bytes) {
            Err(e) => {
                log_err!(
                    "rejecting a malformed request from {}: {}",
                    describe_peer(peer),
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
                        log_err!(
                            "create-tag quota ({}) spent on one connection by {}; closing",
                            limits.max_tag_creations_per_connection,
                            describe_peer(peer)
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
                let (response, pending_spawn) = dispatch(wm_core, request, &mut state_cache);
                log_dispatch_outcome(peer, kind, &response);
                if !write_response(&mut writer, &response) {
                    return; // peer gone; nothing more to do
                }
                // The spawn happens here — after the response is already
                // on the wire and after the `wm-core` mutex was released —
                // rather than inside `handle_request`, so a slow or failing
                // spawn can never block the client waiting on its response.
                // The released mutex is load-bearing twice over: the effect
                // re-locks it to roll back a claim whose spawn failed, and
                // `lock_recovering` is not reentrant.
                if let Some(pending) = pending_spawn {
                    spawn_pinned_terminal(&pending);
                }
                // well-formed request, wm-core-level Ok/Error: connection
                // stays open for further requests.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `get-state` cache, exercised directly rather than through a
    /// socket: the observable difference between a hit and a miss is which
    /// allocation comes back, and `Rc::ptr_eq` is the only thing that can
    /// see that. Through a socket the two are byte-identical by design,
    /// which is exactly why a round-trip test would assert nothing here
    /// (audit finding T-02's lesson).
    #[test]
    fn a_second_get_state_with_nothing_changed_returns_the_answer_already_built() {
        let core = Arc::new(Mutex::new(WmCore::default()));
        let mut cache = None;

        let (first, _) = dispatch(&core, Request::GetState, &mut cache);
        let (second, _) = dispatch(&core, Request::GetState, &mut cache);

        assert!(
            Rc::ptr_eq(&first, &second),
            "the second poll rebuilt the whole world instead of reusing it"
        );
    }

    #[test]
    fn a_mutation_through_the_same_connection_invalidates_the_cached_answer() {
        let core = Arc::new(Mutex::new(WmCore::default()));
        let mut cache = None;

        let (before, _) = dispatch(&core, Request::GetState, &mut cache);
        dispatch(
            &core,
            Request::CreateTag {
                name: "web".to_string(),
            },
            &mut cache,
        );
        let (after, _) = dispatch(&core, Request::GetState, &mut cache);

        assert!(!Rc::ptr_eq(&before, &after));
        assert_ne!(*before, *after, "the new tag is not in the answer");
    }

    /// The one that matters: the Wayland dispatch thread mutates the same
    /// `WmCore` on every keybind and every manage sequence, and it knows
    /// nothing about any connection's cache. A cache keyed on anything but
    /// the core's own generation would keep serving tags that are gone.
    #[test]
    fn a_mutation_from_outside_this_connection_invalidates_the_cached_answer() {
        let core = Arc::new(Mutex::new(WmCore::default()));
        let mut cache = None;

        let (before, _) = dispatch(&core, Request::GetState, &mut cache);
        lock_recovering(&core)
            .create_tag("web")
            .expect("an empty registry accepts a tag");
        let (after, _) = dispatch(&core, Request::GetState, &mut cache);

        assert!(
            !Rc::ptr_eq(&before, &after),
            "a change nobody told this connection about was served from cache"
        );
        assert_ne!(*before, *after);
    }
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

        /// [`TestClient::read_line`] parsed as JSON, for the assertions
        /// that are about a response's *fields* rather than its type
        /// discriminant — a substring match on serialized output pins the
        /// field order too (audit finding T-05).
        fn read_json(&mut self) -> serde_json::Value {
            let line = self.read_line().expect("expected a response line");
            serde_json::from_str(&line).expect("every response is one JSON object")
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

    /// A pinned-terminal spawn effect that records what it was asked for
    /// instead of doing it.
    ///
    /// The previous fixture passed `terminal = "/bin/true"` so that the
    /// real `Command::spawn` these tests reach would at least not open a
    /// window on the machine running the suite — which meant the spawn path
    /// was exercised but nothing could be asserted about it, and the whole
    /// `switch-tag` follow-up sat uncovered (audit findings T-05 and J-02).
    #[derive(Clone, Default)]
    struct SpawnRecorder(Arc<Mutex<Vec<PendingPinnedSpawn>>>);

    impl SpawnRecorder {
        fn effect(&self) -> SpawnPinnedTerminal {
            let recorded = Arc::clone(&self.0);
            Arc::new(move |pending: &PendingPinnedSpawn| {
                recorded
                    .lock()
                    .expect("the recorder mutex is only ever held to push one value")
                    .push(pending.clone())
            })
        }

        fn requested(&self) -> Vec<PendingPinnedSpawn> {
            self.0
                .lock()
                .expect("the recorder mutex is only ever held to push one value")
                .clone()
        }
    }

    /// A running test server: the socket it listens on, the core it
    /// mutates, and every spawn it asked for.
    ///
    /// One value rather than a tuple, so the socket inode's removal is tied
    /// to the test's scope — eight sockets used to survive every suite run
    /// (audit finding T-03) — and so reaching the spawn record does not
    /// need a third tuple element at every call site.
    struct TestServer {
        socket: SocketGuard,
        wm_core: Arc<Mutex<WmCore>>,
        spawns: SpawnRecorder,
    }

    impl TestServer {
        fn start() -> Self {
            Self::with_core(WmCore::new())
        }

        fn with_core(core: WmCore) -> Self {
            Self::with_limits(core, Limits::PRODUCTION)
        }

        /// [`TestServer::start`] with the resource limits supplied
        /// explicitly, so the connection cap, the write timeout, the idle
        /// read deadline and the thread-spawn failure path can be exercised
        /// with values that fit inside a test run.
        fn with_limits(core: WmCore, limits: Limits) -> Self {
            let socket = SocketGuard(unique_socket_path());
            let wm_core = Arc::new(Mutex::new(core));
            let spawns = SpawnRecorder::default();
            spawn_with_limits(Arc::clone(&wm_core), socket.path(), spawns.effect(), limits)
                .expect("server must spawn successfully");
            Self {
                socket,
                wm_core,
                spawns,
            }
        }

        fn path(&self) -> &std::path::Path {
            self.socket.path()
        }
    }

    /// Removes a test server's socket inode when the test ends, including
    /// on a panicking assertion, so a suite run does not litter the temp
    /// directory with one socket per case.
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

    /// Audit finding F-05: `write_response` threw away the `io::Error` it
    /// acted on, on the strength of a doc comment claiming a write failure
    /// always means the peer is gone. True for `BrokenPipe`, false for
    /// `ENOBUFS`, `EINTR` and a write deadline — which is a server-side
    /// problem worth a line.
    #[test]
    fn only_a_hangup_is_an_unremarkable_write_failure() {
        assert!(is_peer_hangup(&std::io::Error::from(
            std::io::ErrorKind::BrokenPipe
        )));
        assert!(!is_peer_hangup(&std::io::Error::from(
            std::io::ErrorKind::Interrupted
        )));
        assert!(!is_peer_hangup(&std::io::Error::from(
            std::io::ErrorKind::WouldBlock
        )));
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
        let server = TestServer::with_limits(WmCore::new(), limits);

        // Both slots are held by connections whose handler thread is
        // parked in `read_until` waiting for a second request.
        let mut first = TestClient::connect(server.path());
        first.send_line(r#"{"type":"get-state"}"#);
        assert!(
            first
                .read_line()
                .expect("first slot served")
                .contains(r#""type":"state""#)
        );
        let mut second = TestClient::connect(server.path());
        second.send_line(r#"{"type":"get-state"}"#);
        assert!(
            second
                .read_line()
                .expect("second slot served")
                .contains(r#""type":"state""#)
        );

        assert!(
            matches!(probe_get_state(server.path()), ProbeResult::Refused),
            "a connection beyond the cap must be dropped, not queued or served"
        );

        drop(first);
        assert!(wait_for_served_get_state(server.path()).contains(r#""type":"state""#));
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
        let server = TestServer::with_limits(core, limits);

        let mut greedy = TestClient::connect(server.path());
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
        assert!(wait_for_served_get_state(server.path()).contains(r#""type":"state""#));
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
        let server = TestServer::with_limits(WmCore::new(), limits);

        let mut client = TestClient::connect(server.path());
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

        let core = lock_recovering(&server.wm_core);
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
        let server = TestServer::with_limits(WmCore::new(), limits);

        for i in 0..3 {
            let mut client = TestClient::connect(server.path());
            client.send_line(&format!(r#"{{"type":"create-tag","name":"tag{i}"}}"#));
            assert!(
                client
                    .read_line()
                    .expect("first create-tag on a fresh connection")
                    .contains(r#""type":"tag-created""#)
            );
        }

        assert_eq!(lock_recovering(&server.wm_core).tag_count(), 3);
    }

    #[test]
    fn an_idle_connection_is_closed_after_the_read_deadline() {
        let limits = Limits {
            idle_read_timeout: Duration::from_millis(150),
            ..Limits::PRODUCTION
        };
        let server = TestServer::with_limits(WmCore::new(), limits);

        let mut client = TestClient::connect(server.path());
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
        let server = TestServer::with_limits(WmCore::new(), limits);

        let mut client = TestClient::connect(server.path());
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
        let server = TestServer::with_limits(WmCore::new(), limits);

        assert!(
            matches!(probe_get_state(server.path()), ProbeResult::Refused),
            "an unspawnable connection must be dropped, not panic the accept loop"
        );
        assert!(
            matches!(probe_get_state(server.path()), ProbeResult::Refused),
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
        let server = TestServer::start();
        let mut client = TestClient::connect(server.path());
        client.send_line(r#"{"type":"get-state"}"#);
        let response = client.read_line().expect("expected a response line");
        assert!(response.contains(r#""type":"state""#));
    }

    /// Audit finding T-05: this test used to assert on a substring of the
    /// serialized line (`"id":3,"app_id":"foot","tags":[0]`) and to scrape
    /// the created tag id out with `find("\"tag_id\":")` — both of which
    /// break on a field reorder that changes no behaviour, and the scraper
    /// panicked unhelpfully when it did. Parsed as JSON instead, which is
    /// the shape the protocol actually promises.
    #[test]
    fn toggle_tag_round_trip_mutates_shared_wm_core() {
        let mut core = WmCore::new();
        let view_id = core.register_view("foot");
        let server = TestServer::with_core(core);

        let mut client = TestClient::connect(server.path());
        client.send_line(r#"{"type":"create-tag","name":"web"}"#);
        let created = client.read_json();
        assert_eq!(created["type"], "tag-created");
        let tag_id = created["tag_id"]
            .as_u64()
            .expect("tag-created carries the new tag's id");

        client.send_line(&format!(
            r#"{{"type":"toggle-tag","view_id":{},"tag_id":{tag_id}}}"#,
            view_id.0
        ));
        assert_eq!(client.read_json()["type"], "ok");

        client.send_line(r#"{"type":"get-state"}"#);
        let state = client.read_json();
        assert_eq!(state["type"], "state");
        assert_eq!(
            state["views"],
            serde_json::json!([{ "id": view_id.0, "app_id": "foot", "tags": [tag_id] }]),
            "the state must reflect a mutation applied through a separate request"
        );
    }

    /// Audit findings T-05 and J-02: `switch-tag`'s pinned-terminal
    /// follow-up is the only effect a socket peer can cause outside
    /// `wm-core`, and it had no coverage at all — the fixture substituted
    /// `/bin/true` for the real terminal precisely so that nothing could be
    /// observed about it.
    ///
    /// The second `get-state` is what makes this deterministic rather than
    /// a poll: one connection is served by one thread, so the spawn — which
    /// happens after the first response is written — has certainly run by
    /// the time the second response comes back.
    #[test]
    fn a_switch_tag_asks_for_the_tags_pinned_terminal_exactly_once() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_id = core.create_tag("web").expect("create the tag to switch to");
        let server = TestServer::with_core(core);
        let mut client = TestClient::connect(server.path());
        let switch = format!(
            r#"{{"type":"switch-tag","output_id":{},"tag_id":{}}}"#,
            output_id.0, tag_id.0
        );

        client.send_line(&switch);
        assert!(
            client
                .read_line()
                .expect("expected an ok response")
                .contains(r#""type":"ok""#)
        );
        client.send_line(r#"{"type":"get-state"}"#);
        client.read_line().expect("expected a state response");

        assert_eq!(
            server.spawns.requested(),
            vec![PendingPinnedSpawn {
                tag_id,
                session_name: "tag-web".to_string(),
            }],
            "the claimed spawn must be requested, with the tag that claimed it"
        );

        // The claim is idempotent by design, so a second switch to the same
        // tag must not spawn a second terminal into the same zellij session.
        client.send_line(&switch);
        client.read_line().expect("expected a second ok response");
        client.send_line(r#"{"type":"get-state"}"#);
        client.read_line().expect("expected a state response");
        assert_eq!(
            server.spawns.requested().len(),
            1,
            "a tag's pinned terminal is spawned at most once per session"
        );
    }

    /// A request that changes nothing must not ask for a process either:
    /// this is the arm that would spawn a terminal for a tag the switch
    /// itself rejected.
    #[test]
    fn a_switch_tag_that_fails_asks_for_no_spawn() {
        let server = TestServer::start();
        let mut client = TestClient::connect(server.path());
        client.send_line(r#"{"type":"switch-tag","output_id":9999,"tag_id":63}"#);
        assert!(
            client
                .read_line()
                .expect("expected an error response")
                .contains(r#""type":"error""#)
        );
        client.send_line(r#"{"type":"get-state"}"#);
        client.read_line().expect("expected a state response");
        assert!(server.spawns.requested().is_empty());
    }

    #[test]
    fn malformed_json_gets_error_response_then_connection_closes() {
        let server = TestServer::start();
        let mut client = TestClient::connect(server.path());
        client.send_line("not json");
        let response = client.read_line().expect("expected an error response");
        assert!(response.contains(r#""type":"error""#));
        assert_eq!(
            client.read_line(),
            None,
            "connection must be closed after a malformed request"
        );
    }

    /// Audit finding T-02: this assertion used to sit inside an
    /// `if let Some(line) = &first`, and coverage proved the branch never
    /// ran — so the regression test for the codebase's oversized-line
    /// defence asserted nothing about the server's answer, and a
    /// `panic`-and-drop would have satisfied it equally.
    ///
    /// The contract, committed to unconditionally: the peer is told its
    /// request was refused, and then the connection closes.
    #[test]
    fn oversized_line_without_newline_is_rejected_not_grown_forever() {
        let server = TestServer::start();
        let mut client = TestClient::connect(server.path());
        let oversized = vec![b'a'; 70 * 1024];
        client.send_raw(&oversized);

        assert!(
            client
                .read_line()
                .expect("the server must answer before it closes")
                .contains(r#""type":"error""#),
            "an oversized line must be refused in words, not by silence"
        );
        assert_eq!(
            client.read_line(),
            None,
            "connection must be closed after an oversized, newline-less line"
        );
    }

    /// The other half of the same contract, and the reason the test above
    /// can be unconditional: [`MAX_LINE_BYTES`] is a size cap, not a
    /// timeout, so a request that fits — right up to the last byte before
    /// the cap — is still answered normally.
    #[test]
    fn a_request_line_just_under_the_cap_is_still_answered() {
        let server = TestServer::start();
        let mut client = TestClient::connect(server.path());
        // Padding inside an unknown JSON field: parseable, ignored by
        // `parse_request`'s `deny_unknown_fields`-free shape, and sized so
        // the line plus its newline is exactly at the cap.
        let request = r#"{"type":"get-state"}"#;
        let padding = " ".repeat(MAX_LINE_BYTES - request.len() - 1);
        client.send_line(&format!("{request}{padding}"));

        assert!(
            client
                .read_line()
                .expect("a line under the cap must be answered")
                .contains(r#""type":"state""#)
        );
    }

    #[test]
    fn invalid_utf8_bytes_get_error_response_not_a_panic() {
        let server = TestServer::start();
        let mut client = TestClient::connect(server.path());
        client.send_raw(&[0xFF, 0xFE]);
        client.send_raw(b"\n");
        let response = client.read_line().expect("expected an error response");
        assert!(response.contains(r#""type":"error""#));
    }

    #[test]
    fn one_malformed_connection_does_not_affect_a_second_concurrent_good_connection() {
        let server = TestServer::start();
        let mut bad_client = TestClient::connect(server.path());
        let mut good_client = TestClient::connect(server.path());

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
        let mut third_client = TestClient::connect(server.path());
        third_client.send_line(r#"{"type":"get-state"}"#);
        let third_response = third_client
            .read_line()
            .expect("server must still be alive");
        assert!(third_response.contains(r#""type":"state""#));
    }

    /// Binds `socket_path` with a real server, for the cases where the
    /// *outcome* of `spawn` is what is under test.
    fn try_spawn_at(socket_path: &std::path::Path) -> std::io::Result<JoinHandle<()>> {
        spawn(
            Arc::new(Mutex::new(WmCore::new())),
            socket_path,
            SpawnRecorder::default().effect(),
        )
    }

    #[test]
    fn a_stale_socket_inode_from_a_previous_run_does_not_block_startup() {
        let guard = SocketGuard(unique_socket_path());
        // A listener that is dropped leaves the inode behind with nothing
        // accepting on it, which is exactly what a crashed WM leaves.
        drop(UnixListener::bind(guard.path()).expect("bind the stale socket"));

        let result = try_spawn_at(guard.path());
        assert!(
            result.is_ok(),
            "a stale socket inode must not block startup: {result:?}"
        );
    }

    /// The failure mode this replaces: a blind `remove_file` unlinked a
    /// *live* instance's socket, leaving the first WM listening on an
    /// unreachable inode while every picker and status bar silently
    /// attached to the second.
    #[test]
    fn a_second_instance_refuses_to_start_and_leaves_the_first_ones_socket_alone() {
        let server = TestServer::start();

        let error = try_spawn_at(server.path())
            .expect_err("a live instance's socket must not be taken over");
        assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);

        let mut client = TestClient::connect(server.path());
        client.send_line(r#"{"type":"get-state"}"#);
        assert!(
            client
                .read_line()
                .expect("the first instance must still be serving")
                .contains(r#""type":"state""#)
        );
    }

    /// Refusing to start is the right outcome for an unexpected inode: the
    /// alternative is a window manager that deletes whatever file it finds
    /// at a path it was handed.
    #[test]
    fn a_plain_file_at_the_socket_path_is_refused_rather_than_deleted() {
        let guard = SocketGuard(unique_socket_path());
        std::fs::write(guard.path(), b"not a socket").expect("plant a plain file");

        assert!(try_spawn_at(guard.path()).is_err());
        assert_eq!(
            std::fs::read(guard.path()).expect("the planted file must survive"),
            b"not a socket"
        );
    }

    #[test]
    fn a_symlink_at_the_socket_path_is_refused_and_its_target_survives() {
        let guard = SocketGuard(unique_socket_path());
        let victim = unique_socket_path();
        std::fs::write(&victim, b"precious").expect("write the victim file");
        std::os::unix::fs::symlink(&victim, guard.path()).expect("plant a symlink");

        assert!(try_spawn_at(guard.path()).is_err());
        assert_eq!(
            std::fs::read(&victim).expect("the symlink target must survive"),
            b"precious"
        );
        let _ = std::fs::remove_file(&victim);
    }

    #[test]
    fn socket_file_has_owner_only_permissions_after_spawn() {
        let server = TestServer::start();
        let mode = std::fs::metadata(server.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn well_formed_request_referencing_unknown_ids_keeps_connection_open() {
        let server = TestServer::start();
        let mut client = TestClient::connect(server.path());
        client.send_line(r#"{"type":"toggle-tag","view_id":9999,"tag_id":63}"#);
        let error_response = client.read_line().expect("expected an error response");
        assert!(error_response.contains(r#""type":"error""#));

        client.send_line(r#"{"type":"get-state"}"#);
        let state_response = client
            .read_line()
            .expect("connection must stay open for a well-formed follow-up request");
        assert!(state_response.contains(r#""type":"state""#));
    }
}
