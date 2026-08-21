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

//! `buoy-tag-picker`: a companion binary spawned by `wm`'s `Mod4+A` ("assign
//! mode", Story 2.2 Task 7) and `Mod4+S` ("switch mode", Story 2.4 Task 7)
//! keybinds. Connects to `wm`'s IPC socket and fetches state, then either
//! drives a sequential `fuzzel --dmenu` toggle-and-reopen loop (assign
//! mode) or a single-shot select-or-cancel invocation (switch mode).
//!
//! The loop shape is forced, not chosen: `fuzzel` 1.14.1 has neither a
//! native checkbox toggle nor a `--multi` flag, so the only interaction its
//! real flag surface supports is one single-select invocation per toggle,
//! reopened with refreshed glyphs until the user cancels (ADR-004).
//!
//! Switch mode is deliberately *not* built on that loop. There is nothing
//! to toggle — membership is not the concept, and selecting is itself the
//! terminal action. Expressing it as "toggle then immediately break" would
//! drag along `pending_rejected_name`, a create-then-apply retry flag and a
//! membership vector that mean nothing here and would need defensively
//! never touching. Two small independent functions sharing
//! `connect_and_get_state` and one launcher is the better shape.
//! This module is now only what genuinely needs the real world: resolving
//! and connecting to the socket, and the process's one exit. What gets sent
//! and in what order moved to `session`, which is generic over its I/O and
//! its launcher and therefore testable; how the launcher is spawned moved to
//! `launcher`. The carve-out this comment used to claim — "nothing here has
//! its own RED/GREEN tests" — is no longer a carve-out for the file's
//! decisions, only for its I/O (audit finding T-01).

use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use buoy_common::log_err;
use buoy_common::socket_path::NO_RUNTIME_DIR_MESSAGE;
use buoy_tag_picker::launcher::Fuzzel;
use buoy_tag_picker::mode;
use buoy_tag_picker::session::{
    AssignContext, Link, StateSnapshot, run_assign_mode, run_switch_mode,
};

/// Read/write deadline for this process's socket I/O.
const SOCKET_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Reports `message` where the user will actually see it, then exits
/// non-zero.
///
/// This process is spawned by a keybind with no terminal, so its stderr
/// goes wherever the display manager sends it — from the user's seat, every
/// failure here was invisible, and the ten `exit(1)` sites made
/// `Super+A`/`Super+S` look like dead keys (audit finding G-03). There is
/// one such site now, called from `main` alone: everything below returns
/// its failure instead of ending the process from inside, which is what
/// made the flows testable (audit finding T-01).
fn die_visibly(message: &str) -> ! {
    log_err!("{message}");
    // Spawned, not waited for: this process is about to exit, and the
    // notification daemon takes the request over D-Bus before `notify-send`
    // itself finishes.
    if let Err(e) = buoy_common::notify::notify_send_command(message).spawn() {
        log_err!("could not notify the user about the failure above: {e}");
    }
    std::process::exit(1);
}

/// How long to wait before connect attempt `attempt + 1`, or `None` when
/// there are no attempts left.
///
/// The WM binds its IPC socket only after its first Wayland roundtrip, so
/// there is a real window at login during which the socket does not exist
/// and a keypress gets `ECONNREFUSED` (audit finding E-05). Connect is the
/// only step retried, because it is the only one whose retry is
/// unambiguously safe: `toggle-tag` is not idempotent, so a general retry
/// layer would need scoping away from it.
fn retry_delay(attempt: u32) -> Option<Duration> {
    const ATTEMPTS: u32 = 3;
    if attempt + 1 >= ATTEMPTS {
        return None;
    }
    Some(Duration::from_millis(50 << attempt))
}

/// Connects to `socket_path`, retrying a refused connection on
/// [`retry_delay`]'s schedule.
fn connect_with_retry(socket_path: &std::path::Path) -> std::io::Result<UnixStream> {
    let mut attempt = 0;
    loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => return Ok(stream),
            Err(e) => match retry_delay(attempt) {
                Some(delay) => {
                    std::thread::sleep(delay);
                    attempt += 1;
                }
                None => return Err(e),
            },
        }
    }
}

/// Resolves the socket path, connects, authenticates the peer, applies the
/// I/O deadlines and fetches the first `get-state` — everything both modes
/// need before doing anything mode-specific.
fn connect_and_get_state()
-> Result<(Link<UnixStream, BufReader<UnixStream>>, StateSnapshot), String> {
    let Some(socket_path) = buoy_common::socket_path::default_socket_path() else {
        return Err(NO_RUNTIME_DIR_MESSAGE.to_string());
    };
    let stream = connect_with_retry(&socket_path)
        .map_err(|e| format!("cannot reach buoy-wm at {socket_path:?}: {e}"))?;
    // A `wm` that is not running as this user is not this user's `wm`. The
    // mirror of the server's own check, and the reason a squatted socket
    // path is a failed connection rather than a silent capture of every
    // tag name, every window's app id, and every selection made here.
    buoy_common::peer::authenticate_peer(&stream).map_err(|rejection| {
        format!("refusing to talk to whatever is listening at {socket_path:?}: {rejection}")
    })?;
    // This process had no socket deadline at all, so a wedged or squatted
    // peer left it blocked forever while holding a `--layer=overlay`
    // fuzzel window with keyboard focus over the whole screen (audit
    // finding E-04). Safe to keep short, unlike the server's own read
    // deadline: every read here immediately follows a send, and the
    // human-time wait happens inside the launcher, never on the socket.
    for (label, applied) in [
        ("read", stream.set_read_timeout(Some(SOCKET_IO_TIMEOUT))),
        ("write", stream.set_write_timeout(Some(SOCKET_IO_TIMEOUT))),
    ] {
        applied.map_err(|e| format!("cannot set the socket {label} timeout: {e}"))?;
    }
    let writer = stream
        .try_clone()
        .map_err(|e| format!("cannot split the connection for writing: {e}"))?;
    let mut link = Link::new(writer, BufReader::new(stream));
    let state = link.get_state()?;
    Ok((link, state))
}

/// Everything `main` does, with its failures returned rather than exited on
/// — so there is exactly one `std::process::exit` in this binary and it is
/// the last thing that happens.
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = mode::parse_args(&args)?;
    let mut launcher = Fuzzel;

    match mode {
        // Story 2.10 Task 5: `run_assign_mode` opens with or without a
        // focused view now (`wm`'s `Action::OpenAssignPicker` no longer
        // requires one either) — `should_open_picker`'s guard, checked
        // inside `run_assign_mode` itself, only refuses when *neither* a
        // view nor an output is known.
        mode::Mode::Assign {
            output_id,
            output_name,
        } => {
            let (mut link, state) = connect_and_get_state()?;
            let view_id = state.focused_view;
            run_assign_mode(
                &mut link,
                &mut launcher,
                AssignContext {
                    state,
                    view_id,
                    output_id,
                    output_name,
                },
            )
        }
        // Switch mode has no focused-view precondition at all — pressing
        // `Mod4+S` works with no window focused and none existing.
        mode::Mode::Switch {
            output_id,
            output_name,
        } => {
            let (mut link, state) = connect_and_get_state()?;
            run_switch_mode(
                &mut link,
                &mut launcher,
                &state.tags,
                output_id,
                output_name.as_deref(),
            )
        }
    }
}

fn main() {
    if let Err(message) = run() {
        die_visibly(&message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Audit finding E-05: the WM binds its socket after its first Wayland
    /// roundtrip, so a keypress in that window used to give the picker
    /// ECONNREFUSED and an immediate exit — `Super+A` silently doing
    /// nothing, once, unreproducibly.
    #[test]
    fn the_connect_retry_schedule_backs_off_and_then_gives_up() {
        assert_eq!(retry_delay(0), Some(Duration::from_millis(50)));
        assert_eq!(retry_delay(1), Some(Duration::from_millis(100)));
        assert_eq!(retry_delay(2), None);
    }
}
