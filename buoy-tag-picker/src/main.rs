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
//! drag along `pending_rejected_name`, `pending_create_apply_failed` and a
//! membership vector that mean nothing here and would need defensively
//! never touching. Two small independent functions sharing
//! `connect_and_get_state`/`run_fuzzel` is the better shape. This
//! module is process-spawn and live-socket I/O glue only — every decision
//! it makes (what to render, how to parse `fuzzel`'s output, how to
//! detect cancel, which mode argv selects) is unit-tested in
//! `picker`/`wire`/`mode`; nothing here has its own RED/GREEN tests
//! (same carve-out class as `wm/src/main.rs`'s Wayland-`Dispatch` glue).

mod mode;
mod picker;
mod wire;

use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::time::Duration;

use buoy_common::framing::{Line, MAX_LINE_BYTES, read_line_bounded};
use buoy_common::log_err;
use buoy_common::socket_path::NO_RUNTIME_DIR_MESSAGE;

/// Assign mode's `fuzzel` placeholder text. Deliberately says nothing about
/// creating: as of Story 2.13 this mode only toggles membership in tags
/// that already exist.
const ASSIGN_PLACEHOLDER: &str = "type to filter";

/// Switch mode's `fuzzel` placeholder text. The contrast with
/// [`ASSIGN_PLACEHOLDER`] is the point — this is the one mode that turns a
/// typed, non-matching name into a new tag (Story 2.13), and the
/// placeholder is where that is discoverable.
const SWITCH_PLACEHOLDER: &str = "type to filter, or a new name to create";

/// Reports `message` where the user will actually see it, then exits
/// non-zero.
///
/// This process is spawned by a keybind with no terminal, so its stderr
/// goes wherever the display manager sends it — from the user's seat, every
/// failure here was invisible, and the ten `exit(1)` sites made
/// `Super+A`/`Super+S` look like dead keys (audit finding G-03).
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

/// Writes `request` as one JSON line followed by `\n`, flushing
/// afterward. Returns `false` if the write failed (peer gone).
fn send_request(writer: &mut UnixStream, request: &wire::Request) -> bool {
    let line = wire::serialize_request(request);
    writer.write_all(line.as_bytes()).is_ok()
        && writer.write_all(b"\n").is_ok()
        && writer.flush().is_ok()
}

/// Reads and parses one newline-delimited response line. `None` covers
/// EOF (peer closed the connection), a line over the framing cap, a read
/// that hit the socket deadline, and a response that fails to parse — all
/// of them "nothing usable came back," and every call site treats them the
/// same way (log and stop).
///
/// The cap is not paranoia about our own WM: against a squatted socket
/// path this is the only thing bounding what an unrelated process can make
/// this process allocate (audit finding E-04).
fn read_response(reader: &mut BufReader<UnixStream>) -> Option<wire::Response> {
    match read_line_bounded(reader) {
        Ok(Line::Complete(bytes)) => wire::parse_response(&bytes).ok(),
        Ok(Line::Oversize) => {
            log_err!("response line over the {MAX_LINE_BYTES}-byte cap");
            None
        }
        Ok(Line::Eof) | Err(_) => None,
    }
}

/// Reports a `wire::Response::Error` message on stderr.
///
/// `{:?}`, never `{}`: `message` arrives from whatever is on the other end
/// of the socket, so `Display` would let a squatted socket path (or a
/// wm-side error that embeds peer text) write control characters and
/// forged lines into the terminal or journal.
fn report_server_error(message: &str) {
    log_err!("{message:?}");
}

/// Sends `switch-tag` for `output_id`/`tag_id` and exits the process on
/// any failure to send or on any non-`Ok` response — `switch-tag` is
/// always the last thing any of its three call sites do (Code review
/// follow-up, Story 2.10: this exact send/read/report block was
/// duplicated three times over — [`run_switch_mode`]'s `Selected` arm,
/// and both of `run_assign_mode`'s no-focused-view fallback arms — past
/// this project's own three-strike DRY threshold). Never returns on
/// failure; callers only need to call this and then `break`/return on the
/// implicit success path.
fn send_switch_tag_or_exit(
    writer: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    output_id: u64,
    tag_id: u8,
) {
    if !send_request(writer, &wire::Request::SwitchTag { output_id, tag_id }) {
        die_visibly("buoy-wm stopped listening before the tag switch was sent");
    }
    match read_response(reader) {
        Some(wire::Response::Ok) => {}
        Some(wire::Response::Error { message }) => {
            report_server_error(&message);
            die_visibly("buoy-wm refused the tag switch; see the journal for its reason");
        }
        other => {
            die_visibly(&format!(
                "buoy-wm answered the tag switch with something unexpected: {other:?}"
            ));
        }
    }
}

/// Spawns one `fuzzel --dmenu` invocation, writes `input` to its stdin,
/// and waits for it to exit.
///
/// The binary name is deliberately hardcoded rather than read from
/// `defaults.launcher`: `fuzzel` is driven here as a dmenu-style pager
/// with fuzzel-specific flags (`--with-nth`, `--nth-delimiter`,
/// `--accept-nth`'s absence), not as the user's chosen launcher, and this
/// process has no access to the WM's config anyway. `wm`'s own
/// `Action::Hotkeys` arm hardcodes it for the same reason. The consequence
/// is worth stating: a user without `fuzzel` installed gets a silent no-op
/// from `Super+A`/`Super+S` (audit finding J-09).
///
/// Real flags cross-checked against `fuzzel(1)`
/// (Technical notes' "Spike finding"): `--with-nth=1` displays only the
/// checkbox-glyph+name column (`--nth-delimiter` is the tab this module's
/// stdin rows use as the field separator).
///
/// Code review follow-up: deliberately does **not** pass `--accept-nth`
/// (this project used `--accept-nth=2` from Story 2.2 through Story 2.10,
/// intending it to print just the bare tag id column on selection).
/// Confirmed live and cross-checked against upstream (Codeberg
/// `dnkl/fuzzel` issues #670/#671): `--accept-nth=N` prints the *literal
/// string* `"{N}"` instead of the real value whenever there is no actual
/// N:th column to extract from the accepted line — exactly what happens
/// for a typed, non-matching custom entry (the create-tag path), which by
/// definition has no tab-delimited columns at all. Every tag ever created
/// by typing a new name was silently misnamed to the literal text `"{2}"`
/// as a result. `picker::parse_fuzzel_output`/`parse_switch_selection`
/// now parse the *full* raw returned line themselves (splitting on the
/// tab delimiter when one is present) instead of trusting `fuzzel` to
/// have already extracted just the id — this is what the `--with-nth=1`
/// example in `fuzzel(1)` itself documents as the actual behavior with no
/// `--accept-nth` present: "the full input line is printed on stdout."
/// When `initial_search` is `Some(text)`, `--search=<text>`
/// pre-fills fuzzel's input box (Story 2.3: restores the rejected name into
/// view on a cap-rejection reopen — a real, documented flag). `placeholder`
/// is the greyed-out prompt text, which differs by mode (Story 2.13): only
/// switch mode can create a tag, so only it advertises that. When
/// `output_name` is `Some(name)`, `--output=<name>` targets the real
/// Wayland connector `wm` resolved as the active output (Story 2.9); `None`
/// omits the flag entirely, leaving fuzzel's own "let the compositor
/// choose" default in effect, same as this story's baseline behavior.
/// Returns `Ok((exit_success, stdout_as_lossy_utf8))` for an invocation
/// that ran, and `Err(message)` when it could not be spawned or waited
/// for. Those used to be the same value: `(false, String::new())` covered
/// both "`fuzzel` is not installed" and "the user pressed Escape", so a
/// missing dependency presented as `Super+A` doing nothing at all, with a
/// zero exit code (audit finding F-02). The `parse_*` functions'
/// defensive reading of `false` is correct and unchanged — the bug was
/// upstream, in what `false` had been made to mean.
///
/// Code review follow-up (finding #2): stdin is written from a dedicated
/// thread, concurrently with the main thread's `wait_with_output()`, rather
/// than writing all of stdin before waiting — the previous shape could
/// deadlock if `input` ever exceeded the OS pipe buffer (~64KB) while
/// `fuzzel` was itself blocked writing a full stdout buffer, since neither
/// side would ever be read to unblock the other. Not reachable today at the
/// 64-tag cap with realistic names, but not bounded/documented either; this
/// is the standard `std::process::Command` pattern for avoiding that class
/// of deadlock.
fn run_fuzzel(
    input: &str,
    initial_search: Option<&str>,
    output_name: Option<&str>,
    placeholder: &str,
) -> Result<(bool, String), String> {
    run_dmenu(FUZZEL, input, initial_search, output_name, placeholder)
}

/// The launcher binary. Hardcoded at the one call site above rather than
/// read from `defaults.launcher` — see [`run_fuzzel`] — and taken as a
/// parameter by [`run_dmenu`] only so the tests can drive the spawn-failed,
/// dismissed and accepted paths without a real `fuzzel` or a Wayland
/// session.
const FUZZEL: &str = "fuzzel";

/// [`run_fuzzel`] with the program named explicitly.
fn run_dmenu(
    program: &str,
    input: &str,
    initial_search: Option<&str>,
    output_name: Option<&str>,
    placeholder: &str,
) -> Result<(bool, String), String> {
    let mut command = Command::new(program);
    command
        .arg("--dmenu")
        // "overlay" (not the default "top") renders above a fullscreen
        // window too (fuzzel.ini(5)) - kept as defense-in-depth even
        // though the pinned terminal no longer uses real protocol
        // fullscreen (see `wm`'s `recompute_pinned_terminal_geometry`).
        .arg("--layer=overlay")
        .arg("--with-nth=1")
        .arg("--nth-delimiter=\t")
        .arg(format!("--placeholder={placeholder}"));
    if let Some(text) = initial_search {
        command.arg(format!("--search={text}"));
    }
    // Story 2.9 Task 5: tells fuzzel which real monitor to render on (its
    // own manual: "-o, --output=OUTPUT ... default: let the compositor
    // choose output"). Only appended when known — `None` here means `wm`
    // hadn't yet resolved a connector name for the active output (Story 2.9
    // AC 2), and fuzzel's own default ("let the compositor choose") is
    // exactly today's pre-Story-2.9 behavior, so omitting the flag entirely
    // (never an empty/malformed `--output=`) preserves it exactly.
    //
    // Code review follow-up (Story 2.9): also guard against an empty
    // string specifically, not just `None` — nothing upstream currently
    // validates that a resolved connector name is non-empty, and an empty
    // `--output=` argument would be exactly the malformed flag this AC
    // rules out. Defends the boundary directly rather than trusting every
    // caller to have already checked.
    if let Some(name) = output_name.filter(|name| !name.is_empty()) {
        command.arg(format!("--output={name}"));
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run `{program}`: {e}"))?;

    // Take stdin and write it on its own thread so this thread is free to
    // call `wait_with_output()` concurrently — if `input` is large enough
    // to fill the stdin pipe buffer while `fuzzel` is blocked on a full
    // stdout buffer of its own, writing to completion before waiting would
    // deadlock both sides.
    let stdin_writer = child.stdin.take().map(|mut stdin| {
        let input = input.to_owned();
        std::thread::spawn(move || {
            if let Err(e) = stdin.write_all(input.as_bytes()) {
                log_err!("failed to write to fuzzel's stdin: {e}");
            }
            // Explicit drop closes fuzzel's stdin so it sees EOF and can
            // exit its input-reading phase.
            drop(stdin);
        })
    });

    let result = child
        .wait_with_output()
        .map(|output| {
            (
                output.status.success(),
                String::from_utf8_lossy(&output.stdout).into_owned(),
            )
        })
        .map_err(|e| format!("cannot wait for `{program}`: {e}"));

    // `wait_with_output` already implies the write side is done or moot
    // (the child exited). Joined anyway to catch the one thing the closure
    // can do that nothing else would report: panic. A *write* error is
    // already surfaced by the closure's own `log_err!` above — the comment
    // that used to sit here claimed the join was what kept it from being
    // silently dropped, which `let _ = handle.join()` was never doing
    // (audit finding F-05).
    if let Some(handle) = stdin_writer {
        if handle.join().is_err() {
            // The payload is not decoded here: this process installs no
            // panic hook, so the default one has already written the
            // message and its `file:line` to stderr. What was missing was
            // any statement that the *input* is therefore incomplete.
            log_err!("the thread feeding `{program}` its input panicked; its input is incomplete");
        }
    }

    result
}

/// Read/write deadline for this process's socket I/O.
const SOCKET_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Connects to `wm`'s IPC socket, sends `get-state`, and returns the
/// writer/reader pair plus the parsed state (Task 6.1's mechanical
/// extraction — shared boilerplate both assign and switch mode need
/// before doing anything mode-specific). Exits the process on any
/// connection/request/response failure, exactly as `main`'s own inline
/// version did before this extraction — no behavioral change.
fn connect_and_get_state() -> (
    UnixStream,
    BufReader<UnixStream>,
    Vec<wire::TagDto>,
    Vec<wire::ViewDto>,
    Option<u64>,
) {
    let Some(socket_path) = buoy_common::socket_path::default_socket_path() else {
        die_visibly(NO_RUNTIME_DIR_MESSAGE);
    };
    let stream = match connect_with_retry(&socket_path) {
        Ok(stream) => stream,
        Err(e) => die_visibly(&format!("cannot reach buoy-wm at {socket_path:?}: {e}")),
    };
    // A `wm` that is not running as this user is not this user's `wm`. The
    // mirror of the server's own check, and the reason a squatted socket
    // path is a failed connection rather than a silent capture of every
    // tag name, every window's app id, and every selection made here.
    if let Err(rejection) = buoy_common::peer::authenticate_peer(&stream) {
        die_visibly(&format!(
            "refusing to talk to whatever is listening at {socket_path:?}: {rejection}"
        ));
    }
    // This process had no socket deadline at all, so a wedged or squatted
    // peer left it blocked forever while holding a `--layer=overlay`
    // fuzzel window with keyboard focus over the whole screen (audit
    // finding E-04). Safe to keep short, unlike the server's own read
    // deadline: every read here immediately follows a send, and the
    // human-time wait happens inside `run_fuzzel`, never on the socket.
    for (label, applied) in [
        ("read", stream.set_read_timeout(Some(SOCKET_IO_TIMEOUT))),
        ("write", stream.set_write_timeout(Some(SOCKET_IO_TIMEOUT))),
    ] {
        if let Err(e) = applied {
            die_visibly(&format!("cannot set the socket {label} timeout: {e}"));
        }
    }
    let mut writer = match stream.try_clone() {
        Ok(writer) => writer,
        Err(e) => die_visibly(&format!("cannot split the connection for writing: {e}")),
    };
    let mut reader = BufReader::new(stream);

    if !send_request(&mut writer, &wire::Request::GetState) {
        die_visibly("buoy-wm stopped listening before the state request was sent");
    }
    let (tags, views, focused_view) = match read_response(&mut reader) {
        Some(wire::Response::State {
            tags,
            views,
            focused_view,
        }) => (tags, views, focused_view),
        Some(other) => die_visibly(&format!(
            "buoy-wm answered the state request with something unexpected: {other:?}"
        )),
        None => die_visibly("buoy-wm sent nothing usable in answer to the state request"),
    };

    (writer, reader, tags, views, focused_view)
}

/// Assign mode's toggle-and-reopen loop (Story 2.2), extracted from `main`
/// in Story 2.4 Task 6.1.
///
/// Story 2.10 Task 5: the picker opens with no view focused (`view_id:
/// None`) too, not just the original always-a-view case. With a view
/// focused, a pick toggles that tag's membership and the picker reopens.
/// With no view focused, "toggle this tag's membership" has no target, so
/// the pick falls back to switching the active output (`output_id`, `wm`'s
/// own spawn-time resolution — see `mode::Mode::Assign`) to the picked tag
/// instead, then stops — a switch is a one-shot terminal action, not a
/// toggle-and-reopen, exactly like [`run_switch_mode`].
///
/// Story 2.13: this mode no longer creates tags. A typed name that matches
/// no row now parses as `Cancelled` (see
/// `picker::parse_fuzzel_output`), and the create-and-reopen machinery
/// that used to live here — the cap-rejection row, the create-then-toggle
/// chain, the local `tags` mirror update and the "created but not applied"
/// notice row — moved wholesale to [`run_switch_mode`], which is where
/// creating a tag belongs: a new tag is a place you go, not a label you
/// attach. `tags` is consequently read-only for the whole call now.
///
/// Code review follow-up (Story 2.10): `picker::should_open_picker` is
/// checked once, up front, before any wire traffic at all. It also makes
/// every `output_id.expect(...)` below safe: once past this guard, a `None`
/// view guarantees a `Some` output for the rest of this call.
fn run_assign_mode(
    mut writer: UnixStream,
    mut reader: BufReader<UnixStream>,
    tags: Vec<wire::TagDto>,
    views: Vec<wire::ViewDto>,
    view_id: Option<u64>,
    output_id: Option<u64>,
    output_name: Option<&str>,
) {
    if !picker::should_open_picker(view_id, output_id) {
        log_err!("no window focused and no output known");
        std::process::exit(0);
    }

    let mut current_tags: Vec<u8> = view_id
        .and_then(|view_id| views.iter().find(|v| v.id == view_id))
        .map(|v| v.tags.clone())
        .unwrap_or_default();

    let known_ids: Vec<u8> = tags.iter().map(|t| t.id).collect();

    loop {
        let entries = picker::build_checklist_entries(&tags, &current_tags);
        let input = picker::render_fuzzel_input(&entries);
        // No `initial_search` — that mechanism exists only to restore a
        // name rejected at the tag cap, which is a create-path concept and
        // therefore switch mode's now (Story 2.13).
        let (exit_success, stdout) = match run_fuzzel(&input, None, output_name, ASSIGN_PLACEHOLDER)
        {
            Ok(outcome) => outcome,
            Err(message) => die_visibly(&message),
        };

        match picker::parse_fuzzel_output(exit_success, &stdout, &known_ids) {
            picker::PickerAction::Cancelled => break,
            picker::PickerAction::Toggled(tag_id) => match view_id {
                Some(view_id) => {
                    if !send_request(&mut writer, &wire::Request::ToggleTag { view_id, tag_id }) {
                        log_err!("failed to send toggle-tag request");
                        break;
                    }
                    match read_response(&mut reader) {
                        Some(wire::Response::Ok) => {
                            picker::toggle_local_membership(&mut current_tags, tag_id);
                        }
                        Some(wire::Response::Error { message }) => {
                            report_server_error(&message);
                            break;
                        }
                        other => {
                            log_err!("unexpected response to toggle-tag: {other:?}");
                            break;
                        }
                    }
                }
                // Story 2.10 Task 5: no view is focused, so there is
                // nothing to toggle membership on — switch the active
                // output to `tag_id` instead, handled the same shape as
                // `run_switch_mode`'s own `Selected` arm, then stop.
                // `output_id` is guaranteed `Some` here: the
                // `should_open_picker` guard before this loop already
                // refused to run at all unless a view or an output was
                // known, and this arm only runs when the view is `None`.
                None => {
                    let output_id = output_id
                        .expect("should_open_picker guarantees an output when no view is focused");
                    send_switch_tag_or_exit(&mut writer, &mut reader, output_id, tag_id);
                    break;
                }
            },
        }
    }
}

/// Switch mode's flow (Story 2.4 Task 6.2): renders a plain, un-checkboxed
/// list of every registry tag and opens `fuzzel`. Selecting a row sends
/// `switch-tag` and returns; dismissing returns with no request sent at
/// all.
///
/// Story 2.13 adds the create path, moved here from assign mode: a typed
/// name matching no row is sent as `create-tag`, and the resulting tag id
/// is switched to immediately — creating a tag and going there are one
/// action, because creating one is how you start working somewhere new.
/// `create-tag` is idempotent by exact name on `wm`'s side, so typing a
/// name that already exists resolves to that tag's id and switches to it,
/// with no duplicate registry entry and no special case here.
///
/// The loop reopens for exactly one reason: a `create-tag` rejected at the
/// 64-tag registry cap (ADR-006), which is the only outcome that has
/// neither performed the action nor been dismissed, and whose reason is
/// invisible to a keybind-spawned process with no attached terminal — so
/// it is re-surfaced as a prepended row with the rejected name restored
/// into the input box. Every other outcome, create or not, still
/// terminates on the first pass: one output shows exactly one tag (FR2),
/// so a switch is terminal by definition.
fn run_switch_mode(
    mut writer: UnixStream,
    mut reader: BufReader<UnixStream>,
    tags: Vec<wire::TagDto>,
    output_id: u64,
    output_name: Option<&str>,
) {
    let known_ids: Vec<u8> = tags.iter().map(|t| t.id).collect();

    // `Some(name)` when the most recent `create-tag` attempt was rejected
    // for hitting the 64-tag cap: the next `fuzzel` reopen prepends the
    // rejection row and restores `name` into the input box via `--search`
    // (Story 2.3 Task 4.2, moved here by Story 2.13).
    let mut pending_rejected_name: Option<String> = None;

    loop {
        let mut input = String::new();
        if pending_rejected_name.is_some() {
            input.push_str(&picker::render_rejection_row());
        }
        input.push_str(&picker::render_switch_list(&tags));
        let (exit_success, stdout) = match run_fuzzel(
            &input,
            pending_rejected_name.as_deref(),
            output_name,
            SWITCH_PLACEHOLDER,
        ) {
            Ok(outcome) => outcome,
            Err(message) => die_visibly(&message),
        };

        match picker::parse_switch_selection(exit_success, &stdout, &known_ids) {
            picker::SwitchAction::Cancelled => break,
            picker::SwitchAction::Selected(tag_id) => {
                send_switch_tag_or_exit(&mut writer, &mut reader, output_id, tag_id);
                break;
            }
            picker::SwitchAction::CreateTag(name) => {
                if !send_request(
                    &mut writer,
                    &wire::Request::CreateTag { name: name.clone() },
                ) {
                    log_err!("failed to send create-tag request");
                    break;
                }
                match read_response(&mut reader) {
                    // A created tag that can't then be switched to is
                    // reported and exits non-zero inside
                    // `send_switch_tag_or_exit` — deliberately the same
                    // failure posture a *selected existing* tag has had
                    // since Story 2.4, rather than a second, different one
                    // for the same request.
                    Some(wire::Response::TagCreated { tag_id }) => {
                        send_switch_tag_or_exit(&mut writer, &mut reader, output_id, tag_id);
                        break;
                    }
                    Some(wire::Response::Error { message }) => {
                        if message == picker::REJECTION_MESSAGE {
                            pending_rejected_name = Some(name);
                        } else {
                            report_server_error(&message);
                            break;
                        }
                    }
                    other => {
                        log_err!("unexpected response to create-tag: {other:?}");
                        break;
                    }
                }
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match mode::parse_args(&args) {
        Ok(mode) => mode,
        Err(message) => die_visibly(&message),
    };

    match mode {
        // Story 2.10 Task 5: `run_assign_mode` opens with or without a
        // focused view now (`wm`'s `Action::OpenTagPicker` no longer
        // requires one either) — `should_open_picker`'s guard, checked
        // inside `run_assign_mode` itself, only refuses when *neither* a
        // view nor an output is known.
        mode::Mode::Assign {
            output_id,
            output_name,
        } => {
            let (writer, reader, tags, views, focused_view) = connect_and_get_state();
            run_assign_mode(
                writer,
                reader,
                tags,
                views,
                focused_view,
                output_id,
                output_name.as_deref(),
            );
        }
        // Switch mode has no focused-view precondition at all — pressing
        // `Mod4+S` works with no window focused and none existing.
        mode::Mode::Switch {
            output_id,
            output_name,
        } => {
            let (writer, reader, tags, _views, _focused_view) = connect_and_get_state();
            run_switch_mode(writer, reader, tags, output_id, output_name.as_deref());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Audit finding F-02: a launcher that could not be spawned and a user
    /// who pressed Escape were byte-for-byte identical, so `fuzzel` not
    /// being installed presented as "`Super+A` does nothing" — including a
    /// zero exit code, so even a scripted caller could not tell.
    #[test]
    fn a_launcher_that_cannot_be_spawned_is_an_error_not_a_cancellation() {
        let error = run_dmenu("buoy-no-such-launcher-binary", "", None, None, "")
            .expect_err("a missing launcher must not look like a dismissal");
        assert!(error.contains("buoy-no-such-launcher-binary"), "{error}");
    }

    #[test]
    fn a_launcher_the_user_dismissed_is_a_completed_invocation() {
        let (accepted, output) =
            run_dmenu("/bin/false", "", None, None, "").expect("/bin/false ran");
        assert!(!accepted);
        assert!(output.is_empty());
    }

    #[test]
    fn a_launcher_that_exits_zero_is_an_accepted_invocation() {
        let (accepted, _output) =
            run_dmenu("/bin/true", "", None, None, "").expect("/bin/true ran");
        assert!(accepted);
    }

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
