// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! `tag-picker`: a companion binary spawned by `wm`'s `Mod4+A` keybind
//! (Story 2.2 Task 7). Connects to `wm`'s IPC socket, fetches the focused
//! view's current tags, and drives a sequential `fuzzel --dmenu`
//! toggle-and-reopen loop (see `docs/planning/epics/story-2-2.md`'s
//! Technical notes "Spike finding" for why this loop shape, not a single
//! native multi-select, is the only interaction fuzzel's real flag surface
//! supports). This module is process-spawn and live-socket I/O glue only
//! — every decision it makes (what to render, how to parse `fuzzel`'s
//! output, how to detect cancel) is unit-tested in `checklist`/`wire`;
//! nothing here has its own RED/GREEN tests (same carve-out class as
//! `wm/src/main.rs`'s Wayland-`Dispatch` glue).

mod checklist;
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

/// Spawns one `fuzzel --dmenu` invocation, writes `input` to its stdin,
/// and waits for it to exit. Real flags cross-checked against `fuzzel(1)`
/// (Technical notes' "Spike finding"): `--with-nth=1` displays only the
/// checkbox-glyph+name column, `--accept-nth=2` prints only the bare tag
/// id column on selection, `--nth-delimiter` is the tab this module's
/// stdin rows use. Returns `(exit_success, stdout_as_lossy_utf8)`; a
/// failure to spawn or wait is treated as a failed/cancelled invocation
/// rather than panicking (never exercised live in this sandbox — no
/// `fuzzel` binary and no Wayland session, see the story's Task 8.3 note).
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
fn run_fuzzel(input: &str) -> (bool, String) {
    let mut child = match Command::new("fuzzel")
        .arg("--dmenu")
        .arg("--with-nth=1")
        .arg("--accept-nth=2")
        .arg("--nth-delimiter=\t")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
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

fn main() {
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

    if !checklist::should_open_picker(focused_view) {
        eprintln!("tag-picker: no window focused");
        std::process::exit(0);
    }
    // should_open_picker just confirmed focused_view.is_some().
    let view_id = focused_view.expect("should_open_picker confirmed focused_view is Some");

    let mut current_tags: Vec<u8> = views
        .iter()
        .find(|v| v.id == view_id)
        .map(|v| v.tags.clone())
        .unwrap_or_default();

    loop {
        let entries = checklist::build_checklist_entries(&tags, &current_tags);
        let input = checklist::render_fuzzel_input(&entries);
        let (exit_success, stdout) = run_fuzzel(&input);

        match checklist::parse_fuzzel_output(exit_success, &stdout) {
            checklist::PickerAction::Cancelled => break,
            checklist::PickerAction::Toggled(tag_id) => {
                if !send_request(&mut writer, &wire::Request::ToggleTag { view_id, tag_id }) {
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
                        eprintln!("tag-picker: unexpected response to toggle-tag: {other:?}");
                        break;
                    }
                }
            }
        }
    }
}
