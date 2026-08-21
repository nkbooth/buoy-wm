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

mod bar_line;
mod wire;

use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use buoy_common::framing::{Line, MAX_LINE_BYTES, read_line_bounded};
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
/// A failure setting either timeout (`.ok()?`) is folded into the same
/// "connection failed this tick" path as a failed `connect` itself — never
/// a panic (NFR2).
fn connect_with_timeout(socket_path: &Path) -> Option<UnixStream> {
    let stream = UnixStream::connect(socket_path).ok()?;
    // The mirror of the server's own peer check: a `wm` not running as
    // this user is not this user's `wm`, and rendering its idea of the
    // current tag into the desktop's own chrome is exactly the spoof a
    // squatted socket path buys.
    buoy_common::peer::authenticate_peer(&stream).ok()?;
    stream.set_read_timeout(Some(SOCKET_IO_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(SOCKET_IO_TIMEOUT)).ok()?;
    Some(stream)
}

/// Connects fresh to `socket_path` (Task 1.4's "reconnect every tick"
/// design), sends `get-state`, and reads/parses exactly one response line.
/// Any failure at any step — connect, setting the I/O timeout, write,
/// flush, read, parse, or an unexpected non-`State` response — returns
/// `None` rather than panicking; the caller treats `None` as "poll failed
/// this tick," never distinguishing *why* it failed (Task 1.4/AC: the
/// disconnected state covers every such failure uniformly). The read/write
/// timeout (`SOCKET_IO_TIMEOUT`, applied by `connect_with_timeout`) bounds
/// a stalled `wm` (connection accepted but never written to) to the same
/// "fails this tick, retries next tick" path as every other failure mode,
/// rather than blocking `read_line` forever.
fn try_get_state(socket_path: &Path) -> Option<(Vec<wire::TagDto>, Vec<wire::OutputDto>)> {
    let mut stream = connect_with_timeout(socket_path)?;
    let request = wire::serialize_request(&wire::Request::GetState);
    stream.write_all(request.as_bytes()).ok()?;
    stream.write_all(b"\n").ok()?;
    stream.flush().ok()?;

    let mut reader = BufReader::new(stream);
    // Bounded, not a bare `read_line`: at 4 polls a second for the whole
    // session, an unbounded read is an unbounded allocation every 250 ms
    // against whatever is actually on the other end (audit finding E-04).
    let line = match read_line_bounded(&mut reader).ok()? {
        Line::Complete(bytes) => bytes,
        Line::Oversize => {
            eprintln!("buoy-status-bar: response line over the {MAX_LINE_BYTES}-byte cap");
            return None;
        }
        // EOF: peer closed the connection with no response.
        Line::Eof => return None,
    };

    match wire::parse_response(&line).ok()? {
        wire::Response::State { tags, outputs } => Some((tags, outputs)),
        _ => None,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let output_id = match parse_output_id(&args) {
        Ok(output_id) => output_id,
        Err(message) => {
            eprintln!("buoy-status-bar: {message}");
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
            eprintln!("buoy-status-bar: {NO_RUNTIME_DIR_MESSAGE}");
            std::process::exit(1);
        }
    };
    let mut last_printed: Option<String> = None;

    // Loops for the lifetime of the process — deliberately never calls
    // `std::process::exit` past this point (Task 1.4/6.3): a poll failure
    // renders as the disconnected state and retries next tick with no
    // backoff, and a stdout write/flush failure (e.g. waybar itself
    // restarting) is logged to stderr rather than treated as fatal, so
    // this binary needs no `restart-interval` in the user's waybar config.
    loop {
        let bar_line = try_get_state(&socket_path)
            .map(|(tags, outputs)| bar_line::resolve_bar_line(output_id, &outputs, &tags));
        let line = bar_line::format_waybar_line(bar_line);

        if bar_line::changed(last_printed.as_deref(), &line) {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            if let Err(e) = writeln!(handle, "{line}").and_then(|()| handle.flush()) {
                eprintln!("buoy-status-bar: failed to write to stdout: {e}");
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

        let socket_path = std::env::temp_dir().join(format!(
            "buoy-status-bar-connect-with-timeout-test-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&socket_path);
        let _listener = UnixListener::bind(&socket_path).expect("failed to bind test socket");

        let stream =
            connect_with_timeout(&socket_path).expect("connect_with_timeout should succeed");

        assert_eq!(stream.read_timeout().unwrap(), Some(SOCKET_IO_TIMEOUT));
        assert_eq!(stream.write_timeout().unwrap(), Some(SOCKET_IO_TIMEOUT));

        let _ = std::fs::remove_file(&socket_path);
    }
}
