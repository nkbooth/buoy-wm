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

//! `buoy-status-bar`: a companion binary driving one waybar `custom/tag` module
//! per physical output (Story 2.5). Invoked as `buoy-status-bar <output_id>`,
//! `output_id` being `wm`'s own raw `OutputId` (see `wire`'s module doc
//! comment and the story's Description "output-identity mapping gap" for
//! why no connector-name mapping is attempted here). Connects to `wm`'s
//! IPC socket and polls `get-state` on its own internal timer
//! (`POLL_INTERVAL`), printing one JSON line to stdout per waybar's
//! `waybar-custom(5)` "script that loops itself" contract (no
//! `interval`/`signal` needed on the waybar side) whenever the rendered
//! state actually changes. This module is live-socket and stdout I/O glue
//! only — every decision it composes (`parse_output_id`, `resolve_bar_line`,
//! `format_waybar_line`, `changed`) is unit-tested in `bar_line`/`wire`
//! and this module's own `tests` (the socket path's own resolution rule
//! is tested in `buoy-common`);
//! nothing in `try_get_state`/the poll loop has its own RED/GREEN tests
//! (same carve-out class as `buoy-tag-picker/src/main.rs`'s own connection
//! glue).

use buoy_status_bar::{bar_line, wire};

use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use buoy_common::framing::{Line, MAX_LINE_BYTES, read_line_bounded};
use buoy_common::log_err;
use buoy_common::socket_path::NO_RUNTIME_DIR_MESSAGE;

/// `buoy-status-bar`'s poll cadence (Task 1.4): comfortably under typical
/// human just-noticeable-lag for a passive display, far below any
/// NFR1-relevant threshold (NFR1 bounds `wm`'s own action-handling
/// latency, not this passive bar's refresh cadence). Not user-configurable
/// in v1 (YAGNI — a single hobby user, no stated need for tuning).
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Parses `buoy-status-bar`'s sole CLI argument, the target output's numeric
/// `OutputId`. Exactly one numeric argument is accepted; zero, more than
/// one, or a non-numeric argument is a startup error — the same
/// fail-closed-on-malformed-invocation discipline `buoy-tag-picker`'s own
/// `mode::parse_args` established (Story 2.4 Task 1.1), checked **before**
/// `main` ever touches the socket.
fn parse_output_id(args: &[String]) -> Result<u64, String> {
    match args {
        [id] => id
            .parse::<u64>()
            .map_err(|_| format!("invalid output id: {id}")),
        _ => Err(format!("usage: buoy-status-bar <output_id>, got: {args:?}")),
    }
}

/// Read/write deadline for a single poll tick's socket I/O (code review
/// follow-up: without this, a `wm` that accepts the connection but never
/// writes back — deadlocked, wedged, or under heavy load — hangs
/// `read_line` forever, wedging this whole process with no "disconnected"
/// state and no retry). Set well under `POLL_INTERVAL` (250ms) so a
/// timeout still resolves in time for the loop to retry on its very next
/// tick rather than blocking past it.
const SOCKET_IO_TIMEOUT: Duration = Duration::from_millis(100);

/// Connects to `socket_path` and applies `SOCKET_IO_TIMEOUT` to both the
/// read and write halves before handing the stream back — split out of
/// `try_get_state` so the timeout configuration itself is unit-testable
/// (code review follow-up) without needing a live `wm` on the other end.
/// Every failure is a described `Err`, never a panic (NFR2).
fn connect_with_timeout(socket_path: &Path) -> Result<UnixStream, String> {
    let stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("cannot connect to {}: {e}", socket_path.display()))?;
    // The mirror of the server's own peer check: a `wm` not running as
    // this user is not this user's `wm`, and rendering its idea of the
    // current tag into the desktop's own chrome is exactly the spoof a
    // squatted socket path buys.
    buoy_common::peer::authenticate_peer(&stream)
        .map_err(|rejection| format!("refusing to talk to {socket_path:?}: {rejection}"))?;
    stream
        .set_read_timeout(Some(SOCKET_IO_TIMEOUT))
        .map_err(|e| format!("cannot set the socket read timeout: {e}"))?;
    stream
        .set_write_timeout(Some(SOCKET_IO_TIMEOUT))
        .map_err(|e| format!("cannot set the socket write timeout: {e}"))?;
    Ok(stream)
}

/// Records `error` as the current failure, returning whether it is worth a
/// log line — that is, whether it differs from the failure already
/// reported.
///
/// `try_get_state` collapsed seven distinct failures into one `None` and
/// one disconnected glyph, retried four times a second forever with
/// nothing written anywhere, so a wrong socket path or a wire-schema drift
/// was undebuggable by design (audit finding G-05). Logging every tick
/// would be worse than the silence it replaces, so a failure is logged on
/// transition only.
fn note_error(last_error: &mut Option<String>, error: &str) -> bool {
    if last_error.as_deref() == Some(error) {
        return false;
    }
    *last_error = Some(error.to_string());
    true
}

/// Clears the recorded failure after a successful poll, so a failure that
/// comes back after a recovery is logged again rather than swallowed.
fn note_success(last_error: &mut Option<String>) {
    *last_error = None;
}

/// The `get-state` connection, kept open across polls.
///
/// The bar used to connect fresh every tick: four connect / accept / thread
/// spawn / whole-world-clone / close cycles per second per output, for the
/// whole login session, against the same mutex the Wayland dispatch thread
/// needs to render a frame (audit finding B-02). The server's
/// `handle_connection_inner` already loops on one stream and deliberately
/// keeps it open, and its idle read deadline is generous for exactly this
/// reason, so reusing the connection is the cheaper half of that fix and it
/// needs nothing from the server.
///
/// A failed poll drops the connection so the next tick reconnects — which is
/// what keeps the "fails this tick, retries next tick" behaviour every
/// failure mode had before, including a `wm` that restarted underneath us.
struct Poller {
    socket_path: PathBuf,
    open: Option<Connection>,
}

/// One open connection's write and read halves.
struct Connection {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Poller {
    fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            open: None,
        }
    }

    /// Sends `get-state` — reconnecting first if there is no live
    /// connection — and reads/parses exactly one response line.
    ///
    /// Every failure — connect, peer rejection, setting the I/O timeout,
    /// write, flush, read, parse, or an unexpected non-`State` response —
    /// renders as the same disconnected glyph, which is the AC, but each one
    /// says which it was (audit finding G-05). The read/write timeout
    /// (`SOCKET_IO_TIMEOUT`, applied by [`connect_with_timeout`]) bounds a
    /// stalled `wm` to the same "fails this tick, retries next tick" path as
    /// every other failure mode.
    fn get_state(&mut self) -> Result<(Vec<wire::TagDto>, Vec<wire::OutputDto>), String> {
        let result = self.poll_once();
        if result.is_err() {
            // Whatever went wrong, this connection is not trusted for the
            // next tick: a half-read response would desynchronise every
            // poll after it.
            self.open = None;
        }
        result
    }

    fn poll_once(&mut self) -> Result<(Vec<wire::TagDto>, Vec<wire::OutputDto>), String> {
        if self.open.is_none() {
            let stream = connect_with_timeout(&self.socket_path)?;
            let writer = stream
                .try_clone()
                .map_err(|e| format!("cannot split the connection for writing: {e}"))?;
            self.open = Some(Connection {
                writer,
                reader: BufReader::new(stream),
            });
        }
        match self.open.as_mut() {
            Some(connection) => request_state(connection),
            // Unreachable: populated immediately above. Written as a total
            // match rather than an `expect` because this binary never
            // panics on any input shape (NFR2), and "unreachable in
            // practice, still given a defined answer" is the same posture
            // `bar_line::resolve_bar_line`'s unknown-tag arm takes.
            None => Err("the polling connection vanished as it was opened".to_string()),
        }
    }
}

/// One `get-state` exchange on an already-open connection.
fn request_state(
    connection: &mut Connection,
) -> Result<(Vec<wire::TagDto>, Vec<wire::OutputDto>), String> {
    let request = wire::serialize_request(&wire::Request::GetState);
    let mut send = || -> std::io::Result<()> {
        connection.writer.write_all(request.as_bytes())?;
        connection.writer.write_all(b"\n")?;
        connection.writer.flush()
    };
    send().map_err(|e| format!("cannot send get-state: {e}"))?;

    // Bounded, not a bare `read_line`: at 4 polls a second for the whole
    // session, an unbounded read is an unbounded allocation every 250 ms
    // against whatever is actually on the other end (audit finding E-04).
    let line = match read_line_bounded(&mut connection.reader)
        .map_err(|e| format!("cannot read the get-state response: {e}"))?
    {
        Line::Complete(bytes) => bytes,
        Line::Oversize => {
            return Err(format!("response line over the {MAX_LINE_BYTES}-byte cap"));
        }
        Line::Eof => return Err("the connection closed with no response".to_string()),
    };

    match wire::parse_response(&line) {
        Ok(wire::Response::State { tags, outputs }) => Ok((tags, outputs)),
        Ok(other) => Err(format!("unexpected response to get-state: {other:?}")),
        Err(_) => Err("the get-state response did not parse".to_string()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let output_id = match parse_output_id(&args) {
        Ok(output_id) => output_id,
        Err(message) => {
            log_err!("{message}");
            std::process::exit(1);
        }
    };

    let socket_path = match buoy_common::socket_path::default_socket_path() {
        Some(socket_path) => socket_path,
        None => {
            // Nothing to poll and nothing that will make one appear, so
            // this is the one condition that ends the process rather than
            // rendering as the disconnected state: a bar that retries a
            // path it does not have, four times a second forever, hides a
            // configuration error instead of reporting it.
            log_err!("{NO_RUNTIME_DIR_MESSAGE}");
            std::process::exit(1);
        }
    };
    let mut poller = Poller::new(socket_path);
    let mut last_printed: Option<String> = None;

    // Loops for the lifetime of the process — deliberately never calls
    // `std::process::exit` past this point (Task 1.4/6.3): a poll failure
    // renders as the disconnected state and retries next tick with no
    // backoff, and a stdout write/flush failure (e.g. waybar itself
    // restarting) is logged to stderr rather than treated as fatal, so
    // this binary needs no `restart-interval` in the user's waybar config.
    let mut last_error: Option<String> = None;

    loop {
        let bar_line = match poller.get_state() {
            Ok((tags, outputs)) => {
                note_success(&mut last_error);
                Some(bar_line::resolve_bar_line(output_id, &outputs, &tags))
            }
            Err(e) => {
                if note_error(&mut last_error, &e) {
                    log_err!("{e}");
                }
                None
            }
        };
        let line = bar_line::format_waybar_line(bar_line);

        if bar_line::changed(last_printed.as_deref(), &line) {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            if let Err(e) = writeln!(handle, "{line}").and_then(|()| handle.flush()) {
                log_err!("failed to write to stdout: {e}");
            } else {
                last_printed = Some(line);
            }
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seven distinct failures used to collapse into one `None` and the
    /// same disconnected glyph, retried at 4 Hz forever with nothing
    /// written anywhere. Logging every tick would be worse than silence,
    /// so a failure is logged when it *changes*.
    #[test]
    fn a_failure_is_logged_once_and_not_again_until_it_changes() {
        let mut last: Option<String> = None;
        assert!(note_error(&mut last, "connect refused"));
        assert!(!note_error(&mut last, "connect refused"));
        assert!(note_error(&mut last, "read timed out"));
        assert!(!note_error(&mut last, "read timed out"));
    }

    #[test]
    fn a_recovery_lets_the_same_failure_be_logged_again() {
        let mut last: Option<String> = None;
        assert!(note_error(&mut last, "connect refused"));
        note_success(&mut last);
        assert!(note_error(&mut last, "connect refused"));
    }

    #[test]
    fn parse_output_id_accepts_a_single_numeric_argument() {
        assert_eq!(parse_output_id(&["3".into()]), Ok(3u64));
    }

    #[test]
    fn parse_output_id_rejects_zero_arguments() {
        assert!(parse_output_id(&[]).is_err());
    }

    #[test]
    fn parse_output_id_rejects_more_than_one_argument() {
        assert!(parse_output_id(&["3".into(), "extra".into()]).is_err());
    }

    #[test]
    fn parse_output_id_rejects_a_non_numeric_argument() {
        assert!(parse_output_id(&["not-a-number".into()]).is_err());
    }

    // Code review follow-up: `try_get_state` used to have no read/write
    // timeout at all, so a `wm` that accepted the connection but never
    // wrote a response wedged this process in `read_line` forever. This
    // confirms `connect_with_timeout` actually configures both timeouts on
    // the returned stream, deterministically and without needing a live,
    // stalling peer — the fast/cheap alternative to a slow live-hang
    // reproduction the story's Code Review Follow-up calls for.
    #[test]
    fn connect_with_timeout_sets_the_configured_read_and_write_timeouts() {
        use std::os::unix::net::UnixListener;

        let socket = SocketGuard(std::env::temp_dir().join(format!(
            "buoy-status-bar-connect-with-timeout-test-{}.sock",
            std::process::id()
        )));
        let _ = std::fs::remove_file(socket.path());
        let _listener = UnixListener::bind(socket.path()).expect("failed to bind test socket");

        let stream =
            connect_with_timeout(socket.path()).expect("connect_with_timeout should succeed");

        assert_eq!(stream.read_timeout().unwrap(), Some(SOCKET_IO_TIMEOUT));
        assert_eq!(stream.write_timeout().unwrap(), Some(SOCKET_IO_TIMEOUT));
    }

    /// Removes a test socket inode when the test ends, including on a
    /// panicking assertion — the trailing `remove_file` this replaces did
    /// not run on the one path that most needs the cleanup (audit finding
    /// T-03).
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
}
