// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! `tag-picker`: a companion binary spawned by `wm`'s `Mod4+A` ("assign
//! mode", Story 2.2 Task 7) and `Mod4+S` ("switch mode", Story 2.4 Task 7)
//! keybinds. Connects to `wm`'s IPC socket and fetches state, then either
//! drives a sequential `fuzzel --dmenu` toggle-and-reopen loop (assign
//! mode — see `docs/planning/epics/story-2-2.md`'s Technical notes "Spike
//! finding" for why this loop shape, not a single native multi-select, is
//! the only interaction fuzzel's real flag surface supports) or a single-
//! shot select-or-cancel invocation (switch mode — see
//! `docs/planning/epics/story-2-4.md`'s Technical notes "Switch mode's
//! single-shot shape vs. assign mode's toggle-and-reopen loop"). This
//! module is process-spawn and live-socket I/O glue only — every decision
//! it makes (what to render, how to parse `fuzzel`'s output, how to
//! detect cancel, which mode argv selects) is unit-tested in
//! `checklist`/`wire`/`mode`; nothing here has its own RED/GREEN tests
//! (same carve-out class as `wm/src/main.rs`'s Wayland-`Dispatch` glue).

mod checklist;
mod mode;
mod socket_path;
mod wire;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};

/// Writes `request` as one JSON line followed by `\n`, flushing
/// afterward. Returns `false` if the write failed (peer gone).
fn send_request(writer: &mut UnixStream, request: &wire::Request) -> bool {
    let line = wire::serialize_request(request);
    writer.write_all(line.as_bytes()).is_ok()
        && writer.write_all(b"\n").is_ok()
        && writer.flush().is_ok()
}

/// Reads and parses one newline-delimited response line. `None` covers
/// both EOF (peer closed the connection) and a response that fails to
/// parse — both are "nothing usable came back," and every call site
/// treats them the same way (log and stop).
fn read_response(reader: &mut BufReader<UnixStream>) -> Option<wire::Response> {
    let mut buf = String::new();
    match reader.read_line(&mut buf) {
        Ok(0) => None,
        Ok(_) => wire::parse_response(buf.trim_end_matches('\n').as_bytes()).ok(),
        Err(_) => None,
    }
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
        eprintln!("tag-picker: failed to send switch-tag request");
        std::process::exit(1);
    }
    match read_response(reader) {
        Some(wire::Response::Ok) => {}
        Some(wire::Response::Error { message }) => {
            eprintln!("tag-picker: {message}");
            std::process::exit(1);
        }
        other => {
            eprintln!("tag-picker: unexpected response to switch-tag: {other:?}");
            std::process::exit(1);
        }
    }
}

/// Spawns one `fuzzel --dmenu` invocation, writes `input` to its stdin,
/// and waits for it to exit. Real flags cross-checked against `fuzzel(1)`
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
/// as a result. `checklist::parse_fuzzel_output`/`parse_switch_selection`
/// now parse the *full* raw returned line themselves (splitting on the
/// tab delimiter when one is present) instead of trusting `fuzzel` to
/// have already extracted just the id — this is what the `--with-nth=1`
/// example in `fuzzel(1)` itself documents as the actual behavior with no
/// `--accept-nth` present: "the full input line is printed on stdout."
/// When `initial_search` is `Some(text)`, `--search=<text>`
/// pre-fills fuzzel's input box (Story 2.3: restores the rejected name into
/// view on a cap-rejection reopen — a real, documented flag). When
/// `output_name` is `Some(name)`, `--output=<name>` targets the real
/// Wayland connector `wm` resolved as the active output (Story 2.9); `None`
/// omits the flag entirely, leaving fuzzel's own "let the compositor
/// choose" default in effect, same as this story's baseline behavior.
/// Returns
/// `(exit_success, stdout_as_lossy_utf8)`; a failure to spawn or wait is
/// treated as a failed/cancelled invocation rather than panicking (never
/// exercised live in this sandbox — no `fuzzel` binary and no Wayland
/// session, see the story's Task 8.3 note).
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
) -> (bool, String) {
    let mut command = Command::new("fuzzel");
    command
        .arg("--dmenu")
        // "overlay" (not the default "top") renders above a fullscreen
        // window too (fuzzel.ini(5)) - kept as defense-in-depth even
        // though the pinned terminal no longer uses real protocol
        // fullscreen (see `wm`'s `recompute_pinned_terminal_geometry`).
        .arg("--layer=overlay")
        .arg("--with-nth=1")
        .arg("--nth-delimiter=\t")
        .arg("--placeholder=type to filter, or a new name to create");
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
    let mut child = match command.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("tag-picker: failed to spawn fuzzel: {e}");
            return (false, String::new());
        }
    };

    // Take stdin and write it on its own thread so this thread is free to
    // call `wait_with_output()` concurrently — if `input` is large enough
    // to fill the stdin pipe buffer while `fuzzel` is blocked on a full
    // stdout buffer of its own, writing to completion before waiting would
    // deadlock both sides.
    let stdin_writer = child.stdin.take().map(|mut stdin| {
        let input = input.to_owned();
        std::thread::spawn(move || {
            if let Err(e) = stdin.write_all(input.as_bytes()) {
                eprintln!("tag-picker: failed to write to fuzzel's stdin: {e}");
            }
            // Explicit drop closes fuzzel's stdin so it sees EOF and can
            // exit its input-reading phase.
            drop(stdin);
        })
    });

    let result = match child.wait_with_output() {
        Ok(output) => (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        ),
        Err(e) => {
            eprintln!("tag-picker: failed to wait for fuzzel: {e}");
            (false, String::new())
        }
    };

    // `wait_with_output` already implies the write side is done or moot
    // (the child exited), but join anyway so a stdin-write error above is
    // never silently dropped before this function returns.
    if let Some(handle) = stdin_writer {
        let _ = handle.join();
    }

    result
}

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
    let socket_path = socket_path::default_socket_path();
    let stream = match UnixStream::connect(&socket_path) {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("tag-picker: failed to connect to {socket_path:?}: {e}");
            std::process::exit(1);
        }
    };
    let mut writer = match stream.try_clone() {
        Ok(writer) => writer,
        Err(e) => {
            eprintln!("tag-picker: failed to clone connection for writing: {e}");
            std::process::exit(1);
        }
    };
    let mut reader = BufReader::new(stream);

    if !send_request(&mut writer, &wire::Request::GetState) {
        eprintln!("tag-picker: failed to send get-state request");
        std::process::exit(1);
    }
    let (tags, views, focused_view) = match read_response(&mut reader) {
        Some(wire::Response::State {
            tags,
            views,
            focused_view,
        }) => (tags, views, focused_view),
        Some(other) => {
            eprintln!("tag-picker: unexpected response to get-state: {other:?}");
            std::process::exit(1);
        }
        None => {
            eprintln!("tag-picker: no usable response to get-state");
            std::process::exit(1);
        }
    };

    (writer, reader, tags, views, focused_view)
}

/// Assign mode's existing toggle-and-reopen loop (Story 2.2/2.3), moved
/// out of `main` verbatim (Task 6.1) — no behavioral change, verified by
/// every existing `checklist`/`wire` unit test (untouched by this
/// extraction) staying green.
///
/// Story 2.10 Task 5: the picker now opens with no view focused
/// (`view_id: None`) too, not just the original always-a-view case. With a
/// view focused, every outcome below is byte-for-byte identical to
/// pre-Story-2.10 behavior (toggle membership, reopen). With no view
/// focused, "toggle this tag's membership" has no target, so both outcome
/// arms fall back to switching the active output (`output_id`, `wm`'s own
/// spawn-time resolution — see `mode::Mode::Assign`) to the picked/created
/// tag instead, then stop — a switch is a one-shot terminal action, not a
/// toggle-and-reopen, exactly like [`run_switch_mode`].
///
/// Code review follow-up (Story 2.10): `checklist::should_open_picker` is
/// checked once, up front, before any wire traffic at all — reinstated
/// (generalized to accept *either* a view or an output) after its removal
/// let a `CreateTag` request succeed and permanently register a tag (no
/// delete-tag API exists) in the rare case where neither is known, only to
/// then have nowhere to apply or switch it. This single check also makes
/// every `output_id.expect(...)` below safe: once past this guard, a
/// `None` view guarantees a `Some` output for the rest of this call.
fn run_assign_mode(
    mut writer: UnixStream,
    mut reader: BufReader<UnixStream>,
    mut tags: Vec<wire::TagDto>,
    views: Vec<wire::ViewDto>,
    view_id: Option<u64>,
    output_id: Option<u64>,
    output_name: Option<&str>,
) {
    if !checklist::should_open_picker(view_id, output_id) {
        eprintln!("tag-picker: no window focused and no output known");
        std::process::exit(0);
    }

    let mut current_tags: Vec<u8> = view_id
        .and_then(|view_id| views.iter().find(|v| v.id == view_id))
        .map(|v| v.tags.clone())
        .unwrap_or_default();

    // `Some(name)` when the most recent `create-tag` attempt was rejected
    // for hitting the 64-tag cap: the next `fuzzel` reopen prepends the
    // rejection row and restores `name` into the input box via `--search`
    // (Story 2.3 Task 4.2).
    let mut pending_rejected_name: Option<String> = None;

    // `true` when the most recent `create-tag` succeeded but the
    // immediately-chained `toggle-tag` failed (Code review follow-up,
    // finding 2): the next `fuzzel` reopen prepends a message row telling
    // the user the tag exists but wasn't applied, since stderr is invisible
    // to a keybind-spawned process with no attached terminal.
    let mut pending_create_apply_failed = false;

    loop {
        let known_ids: Vec<u8> = tags.iter().map(|t| t.id).collect();
        let mut input = String::new();
        if pending_rejected_name.is_some() {
            input.push_str(&checklist::render_rejection_row());
        }
        if pending_create_apply_failed {
            input.push_str(&checklist::render_create_apply_failed_row());
        }
        let entries = checklist::build_checklist_entries(&tags, &current_tags);
        input.push_str(&checklist::render_fuzzel_input(&entries));
        let (exit_success, stdout) =
            run_fuzzel(&input, pending_rejected_name.as_deref(), output_name);

        match checklist::parse_fuzzel_output(exit_success, &stdout, &known_ids) {
            checklist::PickerAction::Cancelled => break,
            checklist::PickerAction::Toggled(tag_id) => {
                pending_rejected_name = None;
                pending_create_apply_failed = false;
                match view_id {
                    Some(view_id) => {
                        if !send_request(&mut writer, &wire::Request::ToggleTag { view_id, tag_id })
                        {
                            eprintln!("tag-picker: failed to send toggle-tag request");
                            break;
                        }
                        match read_response(&mut reader) {
                            Some(wire::Response::Ok) => {
                                checklist::toggle_local_membership(&mut current_tags, tag_id);
                            }
                            Some(wire::Response::Error { message }) => {
                                eprintln!("tag-picker: {message}");
                                break;
                            }
                            other => {
                                eprintln!(
                                    "tag-picker: unexpected response to toggle-tag: {other:?}"
                                );
                                break;
                            }
                        }
                    }
                    // Story 2.10 Task 5: no view is focused, so there is
                    // nothing to toggle membership on — switch the active
                    // output to `tag_id` instead, handled the same shape
                    // as `run_switch_mode`'s own `Selected` arm, then stop.
                    // `output_id` is guaranteed `Some` here: the
                    // `should_open_picker` guard before this loop already
                    // refused to run at all unless a view or an output was
                    // known, and this arm only runs when the view is
                    // `None`.
                    None => {
                        let output_id = output_id.expect(
                            "should_open_picker guarantees an output when no view is focused",
                        );
                        send_switch_tag_or_exit(&mut writer, &mut reader, output_id, tag_id);
                        break;
                    }
                }
            }
            checklist::PickerAction::CreateTag(name) => {
                if !send_request(
                    &mut writer,
                    &wire::Request::CreateTag { name: name.clone() },
                ) {
                    eprintln!("tag-picker: failed to send create-tag request");
                    break;
                }
                match read_response(&mut reader) {
                    Some(wire::Response::TagCreated { tag_id }) => {
                        // Code review follow-up (finding 3): only mirror a
                        // genuinely new tag id — `create-tag` is idempotent
                        // by name (finding 1), so a name-collision resolves
                        // to an id already present in `tags`, and re-pushing
                        // it would duplicate that tag's row on every
                        // subsequent reopen this session.
                        if checklist::should_add_to_tag_mirror(tag_id, &tags) {
                            tags.push(wire::TagDto {
                                id: tag_id,
                                name: name.clone(),
                            });
                        }
                        pending_rejected_name = None;

                        match view_id {
                            Some(view_id) => {
                                // Code review follow-up (finding 1): "ensure
                                // applied," not "blindly toggle" — a name-collision
                                // with an already-applied existing tag must not be
                                // toggled, or `toggle_view_tag`'s add-if-absent/
                                // remove-if-present semantics would remove it.
                                if checklist::should_toggle_after_create(tag_id, &current_tags) {
                                    if !send_request(
                                        &mut writer,
                                        &wire::Request::ToggleTag { view_id, tag_id },
                                    ) {
                                        // Code review follow-up (finding 2): the tag
                                        // now permanently exists registry-side
                                        // (no delete-tag API, ADR-006/v1 scope) but
                                        // was never applied. Surface this via the
                                        // picker UI itself on the next reopen rather
                                        // than silently breaking — stderr is
                                        // invisible to a keybind-spawned process
                                        // with no attached terminal.
                                        eprintln!(
                                            "tag-picker: tag {tag_id} created but failed to send chained toggle-tag request"
                                        );
                                        pending_create_apply_failed = true;
                                    } else {
                                        match read_response(&mut reader) {
                                            Some(wire::Response::Ok) => {
                                                checklist::toggle_local_membership(
                                                    &mut current_tags,
                                                    tag_id,
                                                );
                                                pending_create_apply_failed = false;
                                            }
                                            Some(wire::Response::Error { message }) => {
                                                eprintln!(
                                                    "tag-picker: tag {tag_id} created but chained toggle-tag failed: {message}"
                                                );
                                                pending_create_apply_failed = true;
                                            }
                                            other => {
                                                eprintln!(
                                                    "tag-picker: tag {tag_id} created but chained toggle-tag got unexpected response: {other:?}"
                                                );
                                                pending_create_apply_failed = true;
                                            }
                                        }
                                    }
                                } else {
                                    // Name-collision with an already-applied
                                    // existing tag: already applied, nothing left
                                    // to do or report.
                                    pending_create_apply_failed = false;
                                }
                            }
                            // Story 2.10 Task 5: no view is focused, so
                            // there is no membership to toggle at all —
                            // chain a switch of the active output onto the
                            // freshly created tag instead, handled the
                            // same shape as the forked `Toggled` arm above,
                            // then stop. `output_id` is guaranteed `Some`
                            // here for the same reason as that arm's own
                            // `None` case (`should_open_picker`'s
                            // precondition, checked once before this loop
                            // began).
                            None => {
                                let output_id = output_id.expect(
                                    "should_open_picker guarantees an output when no view is focused",
                                );
                                send_switch_tag_or_exit(
                                    &mut writer,
                                    &mut reader,
                                    output_id,
                                    tag_id,
                                );
                                break;
                            }
                        }
                    }
                    Some(wire::Response::Error { message }) => {
                        if message == checklist::REJECTION_MESSAGE {
                            pending_rejected_name = Some(name);
                        } else {
                            eprintln!("tag-picker: {message}");
                            break;
                        }
                    }
                    other => {
                        eprintln!("tag-picker: unexpected response to create-tag: {other:?}");
                        break;
                    }
                }
            }
        }
    }
}

/// Switch mode's single-shot flow (Story 2.4 Task 6.2): renders a plain,
/// un-checkboxed list of every registry tag, opens one `fuzzel`
/// invocation, and either sends `switch-tag` and returns, or returns with
/// no request sent at all — there is no reopen loop, unlike
/// [`run_assign_mode`]'s toggle-and-reopen shape (Technical notes "Switch
/// mode's single-shot shape").
fn run_switch_mode(
    mut writer: UnixStream,
    mut reader: BufReader<UnixStream>,
    tags: Vec<wire::TagDto>,
    output_id: u64,
    output_name: Option<&str>,
) {
    let known_ids: Vec<u8> = tags.iter().map(|t| t.id).collect();
    let input = checklist::render_switch_list(&tags);
    // No `initial_search` — switch mode never has a rejection row to
    // restore (that mechanism is assign mode's create-tag-cap concept
    // only).
    let (exit_success, stdout) = run_fuzzel(&input, None, output_name);

    match checklist::parse_switch_selection(exit_success, &stdout, &known_ids) {
        checklist::SwitchAction::Cancelled => {}
        checklist::SwitchAction::Selected(tag_id) => {
            send_switch_tag_or_exit(&mut writer, &mut reader, output_id, tag_id);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match mode::parse_args(&args) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("tag-picker: {message}");
            std::process::exit(1);
        }
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
