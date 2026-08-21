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

//! Opening one `fuzzel --dmenu` window and finding out what the user did
//! with it.
//!
//! Behind a trait, so [`session`](crate::session)'s two mode loops can be
//! driven by a scripted sequence of outcomes instead of a real launcher on a
//! real Wayland session. Both loops used to hold the spawn concretely, which
//! is half of why the file they lived in had no tests at all (audit finding
//! T-01).

use std::io::Write;
use std::process::{Command, Stdio};

use buoy_common::log_err;

/// What one launcher invocation should show and where.
///
/// A parameter object rather than four arguments, because it travels
/// through [`Launcher::run`] to [`run_dmenu`] unchanged and every call site
/// names all four (finding J-10's parameter ceiling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LauncherRequest<'a> {
    /// The tab-delimited rows to feed the launcher on stdin.
    pub input: &'a str,
    /// Text to pre-fill the input box with. `Some` only when restoring a
    /// name the WM rejected at the tag cap (Story 2.3).
    pub initial_search: Option<&'a str>,
    /// The Wayland connector name to render on, or `None` to leave the
    /// choice to the compositor (Story 2.9).
    pub output_name: Option<&'a str>,
    /// The greyed-out prompt text, which differs by mode (Story 2.13).
    pub placeholder: &'a str,
}

/// What one launcher invocation produced.
///
/// `accepted` distinguishes "the user picked something" from "the user
/// dismissed the window"; it is *not* the failure channel. A launcher that
/// could not be spawned at all is an `Err` from [`Launcher::run`], because
/// the two used to be the same value and `fuzzel` not being installed
/// therefore presented as `Super+A` doing nothing, with a zero exit code
/// (audit finding F-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherOutcome {
    pub accepted: bool,
    pub stdout: String,
}

/// One dmenu-style picker window.
pub trait Launcher {
    /// Shows `request` and blocks until the user accepts or dismisses it.
    /// `Err` means the launcher could not be run or waited for — never that
    /// the user said no.
    fn run(&mut self, request: &LauncherRequest<'_>) -> Result<LauncherOutcome, String>;
}

/// The launcher binary. Hardcoded rather than read from
/// `defaults.launcher`: `fuzzel` is driven here as a dmenu-style pager with
/// fuzzel-specific flags (`--with-nth`, `--nth-delimiter`, `--accept-nth`'s
/// absence), not as the user's chosen launcher, and this process has no
/// access to the WM's config anyway. `wm`'s own `Action::Hotkeys` arm
/// hardcodes it for the same reason. The consequence is worth stating: a
/// user without `fuzzel` installed gets a `Super+A`/`Super+S` that reports
/// a missing launcher rather than one that silently does nothing (audit
/// findings J-09, F-02).
pub const FUZZEL: &str = "fuzzel";

/// The production [`Launcher`]: a real `fuzzel` process.
///
/// Real flags cross-checked against `fuzzel(1)` (Technical notes' "Spike
/// finding"): `--with-nth=1` displays only the checkbox-glyph+name column
/// (`--nth-delimiter` is the tab this crate's rows use as the field
/// separator).
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
#[derive(Debug, Clone, Copy, Default)]
pub struct Fuzzel;

impl Launcher for Fuzzel {
    fn run(&mut self, request: &LauncherRequest<'_>) -> Result<LauncherOutcome, String> {
        run_dmenu(FUZZEL, request)
    }
}

/// [`Fuzzel::run`] with the program named explicitly, so the tests can
/// drive the spawn-failed, dismissed and accepted paths without a real
/// `fuzzel` or a Wayland session.
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
pub fn run_dmenu(program: &str, request: &LauncherRequest<'_>) -> Result<LauncherOutcome, String> {
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
        .arg(format!("--placeholder={}", request.placeholder));
    if let Some(text) = request.initial_search {
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
    if let Some(name) = request.output_name.filter(|name| !name.is_empty()) {
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
        let input = request.input.to_owned();
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
        .map(|output| LauncherOutcome {
            accepted: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A [`LauncherRequest`] with nothing in it, for the tests that care
    /// only about how the process itself ended.
    fn empty_request() -> LauncherRequest<'static> {
        LauncherRequest {
            input: "",
            initial_search: None,
            output_name: None,
            placeholder: "",
        }
    }

    /// Audit finding F-02: a launcher that could not be spawned and a user
    /// who pressed Escape were byte-for-byte identical, so `fuzzel` not
    /// being installed presented as "`Super+A` does nothing" — including a
    /// zero exit code, so even a scripted caller could not tell.
    #[test]
    fn a_launcher_that_cannot_be_spawned_is_an_error_not_a_cancellation() {
        let error = run_dmenu("buoy-no-such-launcher-binary", &empty_request())
            .expect_err("a missing launcher must not look like a dismissal");
        assert!(error.contains("buoy-no-such-launcher-binary"), "{error}");
    }

    #[test]
    fn a_launcher_the_user_dismissed_is_a_completed_invocation() {
        let outcome = run_dmenu("/bin/false", &empty_request()).expect("/bin/false ran");
        assert!(!outcome.accepted);
        assert!(outcome.stdout.is_empty());
    }

    #[test]
    fn a_launcher_that_exits_zero_is_an_accepted_invocation() {
        let outcome = run_dmenu("/bin/true", &empty_request()).expect("/bin/true ran");
        assert!(outcome.accepted);
    }
}
