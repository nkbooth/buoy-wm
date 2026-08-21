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

//! Everything this WM launches, and the argv it launches it with.
//!
//! # Rationale
//!
//! The argv builders are separated from the `Command::spawn` calls that
//! use them so that the flags can be asserted: a bare `--output=` and a
//! transposed picker argument are both silently wrong at runtime and
//! neither could be tested while the two were one function (audit findings
//! J-04, J-09).
//!
//! Nothing here holds a Wayland proxy, so this is also where the
//! pinned-terminal spawn lives — the one launch whose failure has to be
//! reported back into `wm-core`, because the claim that makes it happen at
//! most once is committed before the process exists (audit finding D-01).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use buoy_common::log_err;
use buoy_wm::config::{self, Config};
use buoy_wm::ipc;
use buoy_wm::wm_core::ids::TagId;
use buoy_wm::wm_core::state::{WmCore, pinned_terminal_app_id};

use crate::compositor::child::{spawn_tracked, track_spawned_child};
use crate::compositor::report::{log_wm_core_err, notify_user};
use crate::compositor::window::ActiveOutput;

/// Code review follow-up (Story 2.2, finding #1): resolves the
/// `buoy-tag-picker` binary's path as a sibling of the WM's own running
/// executable, rather
/// than trusting `$PATH` — nothing in this repo installs the built
/// `buoy-tag-picker` binary onto `PATH`, and both binaries land in the same
/// Cargo workspace `target/{profile}/` directory, so `wm_exe`'s parent
/// directory is exactly where `buoy-tag-picker` lives too. Falls back to the
/// bare name if `wm_exe` unexpectedly has no parent (e.g. a bare filename
/// with no directory component) — `Command::spawn()` will then fail the
/// same `$PATH`-dependent way the old code always did, handled by the
/// existing error-logging call site rather than invented here.
pub(crate) fn tag_picker_path(wm_exe: &Path) -> PathBuf {
    match wm_exe.parent() {
        Some(dir) => dir.join("buoy-tag-picker"),
        None => PathBuf::from("buoy-tag-picker"),
    }
}

/// Appends the `fuzzel` flags that put a menu on the right layer and the
/// right monitor.
///
/// # Rationale
///
/// `--layer=overlay`, not the default `top`, renders above a fullscreen
/// window too (`fuzzel.ini(5)`) — kept as defense-in-depth even though the
/// pinned terminal no longer uses real protocol fullscreen — see
/// [`super::manager::WindowManager::recompute_pinned_terminal_geometry`].
///
/// `--output=<name>` names the connector this WM resolved as the active
/// output. Without it `fuzzel` falls back to "let the compositor choose",
/// which can land on a disabled output when docked (kanshi disables the
/// laptop panel): the menu maps with real keyboard focus and accepts input
/// while painting to a screen nothing shows on.
///
/// The flag is omitted entirely — never passed empty — when the name is
/// unknown *or* empty. A bare `--output=` is the malformed flag every call
/// site here was written to avoid, and only `buoy-tag-picker` had actually
/// guarded the empty case; the two arms in this file checked `None` alone
/// (audit finding J-09).
pub(crate) fn fuzzel_overlay_args(command: &mut std::process::Command, output_name: Option<&str>) {
    command.arg("--layer=overlay");
    if let Some(name) = output_name.filter(|name| !name.is_empty()) {
        command.arg(format!("--output={name}"));
    }
}

/// `buoy-tag-picker`'s leading argument in switch mode. Assign mode passes
/// no mode argument at all, which is what makes the rest of the argv
/// positionally identical between the two.
pub(crate) const TAG_PICKER_SWITCH_MODE: &str = "switch";

/// `buoy-tag-picker`'s argv: `[<mode>] [<output id> [<connector name>]]`.
///
/// This WM resolves "the active output" and hands the answer across the
/// process boundary; `buoy-tag-picker` never re-derives it. Each argument is
/// appended only once the one before it is known, so assign mode with no
/// registered output spawns with no arguments at all and an unknown
/// connector name leaves the one-argument shape untouched — both are
/// startup-race edge cases whose established behavior is "fewer arguments",
/// never a bogus one.
pub(crate) fn tag_picker_args(
    mode: Option<&str>,
    active_output: Option<ActiveOutput<'_>>,
) -> Vec<String> {
    let mut args: Vec<String> = mode.map(str::to_owned).into_iter().collect();
    if let Some(active) = active_output {
        args.push(active.id.0.to_string());
        if let Some(name) = active.name {
            args.push(name.to_owned());
        }
    }
    args
}

/// Spawns `buoy-tag-picker` with [`tag_picker_args`]' argv.
///
/// Fire-and-forget, and deliberately so: the picker resolves the focused
/// view itself over IPC and performs any tag switch through the same
/// socket, so this never touches [`WmCore`] and never produces a
/// pinned-terminal spawn signal — the switch and its pinned-terminal
/// follow-up happen later, once the user has picked something.
///
/// The binary is resolved as a sibling of this process' own executable
/// ([`tag_picker_path`]) rather than by bare name: nothing in this repo
/// installs it onto `$PATH`, so a bare name silently `ENOENT`s in a real
/// session. A `current_exe()` that fails is logged and skipped rather than
/// guessed at.
pub(crate) fn spawn_tag_picker(mode: Option<&str>, active_output: Option<ActiveOutput<'_>>) {
    let what = match mode {
        None => "buoy-tag-picker".to_string(),
        Some(mode) => format!("buoy-tag-picker in {mode} mode"),
    };
    match std::env::current_exe() {
        Ok(wm_exe) => {
            spawn_tracked(
                std::process::Command::new(tag_picker_path(&wm_exe))
                    .args(tag_picker_args(mode, active_output)),
                &what,
            );
        }
        Err(e) => log_err!("Failed to resolve wm's own executable path: {e}"),
    }
}

/// Opens the tag-switch picker for `active_output`.
///
/// The guard is here rather than inside [`spawn_tag_picker`] because the
/// asymmetry is real: assign mode has something to do with no output
/// registered (it resolves the focused view over IPC), switch mode has
/// nothing to switch.
pub(crate) fn spawn_switch_picker(active_output: Option<ActiveOutput<'_>>) {
    let Some(active) = active_output else {
        notify_user("The tag-switch keybind did nothing: no output is registered yet.");
        return;
    };
    spawn_tag_picker(Some(TAG_PICKER_SWITCH_MODE), Some(active));
}

/// Spawns `defaults.terminal`.
///
/// `WAYLAND_DEBUG` is stripped from every child by [`spawn_tracked`]: the
/// added noise makes debugging the window manager itself impractical.
pub(crate) fn spawn_terminal(config: &Config) {
    spawn_tracked(
        &mut std::process::Command::new(&config.defaults.terminal),
        &format!("terminal `{}`", config.defaults.terminal),
    );
}

/// Runs `command_line` through `sh -c`, so one binding can carry a whole
/// command line — arguments, pipes, `~` expansion — instead of just a bare
/// program name.
///
/// The string comes from the user's own config file, so shell
/// interpretation is the intent here and not an injection vector: anyone
/// who can edit that file can already run anything as this user.
pub(crate) fn spawn_exec(command_line: &str) {
    spawn_tracked(
        std::process::Command::new("sh").arg("-c").arg(command_line),
        &format!("`{command_line}`"),
    );
}

/// Spawns the user's configured launcher as an overlay on `active_output`.
///
/// Bare `fuzzel` with no `--dmenu` runs its own built-in desktop-entry
/// launcher, so there is nothing to wire to its stdin. This is the one
/// `fuzzel`-shaped spawn that honors `defaults.launcher`: the others drive
/// it as a dmenu-style pager with fuzzel-specific flags, which is a
/// different program role.
pub(crate) fn spawn_launcher(config: &Config, active_output: Option<ActiveOutput<'_>>) {
    let mut command = std::process::Command::new(&config.defaults.launcher);
    fuzzel_overlay_args(&mut command, active_output.and_then(|active| active.name));
    spawn_tracked(
        &mut command,
        &format!("launcher `{}`", config.defaults.launcher),
    );
}

/// Shows the generated hotkey cheat-sheet in a `fuzzel` pager.
///
/// # Rationale
///
/// `fuzzel` is deliberately not `config.defaults.launcher`: it is driven
/// here as a dmenu-style pager with fuzzel-specific flags, not as the
/// user's chosen launcher (audit finding J-09 records the cost — a user
/// without `fuzzel` gets a silent no-op).
///
/// stdin is written from its own thread. The cheat-sheet used to be a fixed
/// 11-entry constant, comfortably under the ~64KiB default pipe buffer,
/// which is what made a synchronous write safe; it is now generated from
/// the user's own bindings and has no bound at all, so a large enough
/// config could fill the pipe and block this — the WM's only thread — until
/// `fuzzel` drained it, freezing all window management.
pub(crate) fn spawn_hotkey_sheet(config: &Config, active_output: Option<ActiveOutput<'_>>) {
    let mut command = std::process::Command::new("fuzzel");
    command.arg("--dmenu");
    fuzzel_overlay_args(&mut command, active_output.and_then(|active| active.name));
    command.arg("--prompt").arg("Hotkeys: ");
    match command
        .stdin(std::process::Stdio::piped())
        .env_remove("WAYLAND_DEBUG")
        .spawn()
    {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                let help = config.hotkey_help().join("\n");
                std::thread::spawn(move || {
                    use std::io::Write;
                    if let Err(e) = writeln!(stdin, "{help}") {
                        log_err!("Failed to write hotkey list to fuzzel: {e}");
                    }
                });
            }
            track_spawned_child(child);
        }
        Err(e) => log_err!("Failed to spawn fuzzel for hotkey list: {e}"),
    }
}

/// Spawns a tag's pinned terminal as `<terminal> <argv...>`, where `argv`
/// is [`config::Defaults::pinned_terminal_argv`]'s already-substituted
/// result — by
/// default foot's `-a pinned-term-<tag id> zellij attach --create
/// <session>`.
///
/// Both the program and its argv are configurable because the flag that
/// sets a window's app-id is terminal-specific (code-review follow-up:
/// hardcoding foot's `-a` meant configuring `terminal` broke every pinned
/// terminal silently, and permanently — the spawn succeeds, the tag is
/// marked spawned, and the claim is idempotent so it never retries).
/// `Config::parse` requires the argv to carry `{app_id}`, since
/// [`pinned_terminal_app_id`] is how the rest of this WM recognizes the window
/// and recovers which tag it belongs to.
///
/// Arguments are passed individually to `Command`, never through a shell,
/// so an arbitrary tag name in the session carries no injection risk.
// Called from `ensure_pinned_terminal_spawned`, which gained its own
// production call site in `manage_seats` in Story 1.7.
fn spawn_pinned_terminal(terminal: &str, argv: &[String]) -> bool {
    spawn_tracked(
        std::process::Command::new(terminal).args(argv),
        &format!("pinned terminal `{terminal}`"),
    )
}

/// Spawns a tag's pinned terminal and, if the spawn does not happen,
/// releases the claim [`WmCore::claim_pinned_terminal_spawn`] already
/// committed for it.
///
/// The claim has to be committed before the spawn — it is what makes the
/// spawn happen at most once — so rolling it back is the only thing keeping
/// a missing terminal binary, a mid-upgrade replacement, or a transient
/// `EMFILE` from costing that tag its pinned terminal for the whole session
/// and mis-tagging every pinned terminal that maps after it (audit finding
/// D-01). Both spawn sites — the Wayland thread's keybind path and the IPC
/// thread's `switch-tag` path — go through here for that reason.
///
/// [`config::Defaults::pinned_terminals`] is honoured here rather than at
/// the two claim sites because only one of them can see the config: the
/// `switch-tag` claim happens inside `ipc::dispatch::handle_request`, which
/// is deliberately pure. So with the feature off the claim is released
/// exactly as a failed spawn releases it, leaving `wm-core` in the state it
/// had before the switch (audit finding C-10).
///
/// Must be called with `wm_core`'s mutex *not* held: [`ipc::lock_recovering`]
/// is not reentrant.
pub(crate) fn spawn_pinned_terminal_or_release_claim(
    wm_core: &Mutex<WmCore>,
    tag_id: TagId,
    defaults: &config::Defaults,
    session_name: &str,
) {
    if defaults.pinned_terminals
        && spawn_pinned_terminal(
            &defaults.terminal,
            &defaults.pinned_terminal_argv(&pinned_terminal_app_id(tag_id), session_name),
        )
    {
        return;
    }
    log_wm_core_err(
        ipc::lock_recovering(wm_core).release_pinned_terminal_claim(tag_id),
        "Failed to release the pinned-terminal claim because nothing was spawned",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use buoy_wm::wm_core::ids::OutputId;

    // Code review follow-up (Story 2.2, finding #1): `tag_picker_path` is
    // the pure path-resolution logic pulled out of
    // `Action::OpenAssignPicker`'s handler so it's testable without
    // actually calling `current_exe()`. The
    // `current_exe()`/`Command::spawn()` call site itself stays untested
    // I/O glue, same carve-out as the rest of this module.

    /// Audit finding J-09: the `--output=` guard had drifted three ways —
    /// `buoy-tag-picker` checked for an empty name, `Action::Launcher` and
    /// `Action::Hotkeys` only for `None`. An empty connector name produced
    /// a bare `--output=`, which is the malformed flag every one of those
    /// call sites was written to avoid.
    #[test]
    fn fuzzel_overlay_args_omit_output_for_an_empty_connector_name() {
        let mut command = std::process::Command::new("fuzzel");
        fuzzel_overlay_args(&mut command, Some(""));
        assert_eq!(args_of(&command), ["--layer=overlay"]);
    }

    #[test]
    fn fuzzel_overlay_args_omit_output_when_the_connector_name_is_unknown() {
        let mut command = std::process::Command::new("fuzzel");
        fuzzel_overlay_args(&mut command, None);
        assert_eq!(args_of(&command), ["--layer=overlay"]);
    }

    #[test]
    fn fuzzel_overlay_args_target_a_real_connector_name() {
        let mut command = std::process::Command::new("fuzzel");
        fuzzel_overlay_args(&mut command, Some("eDP-1"));
        assert_eq!(args_of(&command), ["--layer=overlay", "--output=eDP-1"]);
    }

    /// Audit finding J-04: the assign-picker and switch-picker arms built
    /// this argv twice, byte for byte apart from the leading `switch`. The
    /// asymmetry the extraction has to preserve is below: assign mode
    /// spawns with zero arguments when no output is registered, switch mode
    /// refuses to spawn at all.
    #[test]
    fn assign_mode_argv_is_empty_when_no_output_is_registered() {
        assert!(tag_picker_args(None, None).is_empty());
    }

    #[test]
    fn picker_argv_omits_the_connector_name_when_it_is_unknown() {
        let active = ActiveOutput {
            id: OutputId(3),
            name: None,
        };
        assert_eq!(tag_picker_args(None, Some(active)), ["3"]);
    }

    #[test]
    fn assign_mode_argv_is_the_output_id_then_its_connector_name() {
        let active = ActiveOutput {
            id: OutputId(3),
            name: Some("eDP-1"),
        };
        assert_eq!(tag_picker_args(None, Some(active)), ["3", "eDP-1"]);
    }

    #[test]
    fn switch_mode_argv_leads_with_the_mode_then_matches_assign_mode() {
        let active = ActiveOutput {
            id: OutputId(3),
            name: Some("eDP-1"),
        };
        assert_eq!(
            tag_picker_args(Some(TAG_PICKER_SWITCH_MODE), Some(active)),
            ["switch", "3", "eDP-1"]
        );
    }

    /// Audit finding D-01: `claim_pinned_terminal_spawn` commits the claim
    /// before any process exists, so a spawn that never happens would
    /// otherwise leave the tag marked spawned forever, with no retry.
    /// Exercises the real failure branch — the terminal path does not
    /// exist, so `Command::spawn` returns `ENOENT` and no process is
    /// created.
    #[test]
    fn a_failed_pinned_terminal_spawn_releases_the_claim() {
        let wm_core = Mutex::new(WmCore::default());
        let tag_id = ipc::lock_recovering(&wm_core).create_tag("web").unwrap();
        let session_name = ipc::lock_recovering(&wm_core)
            .claim_pinned_terminal_spawn(tag_id)
            .unwrap()
            .expect("a freshly created tag has not claimed its spawn yet");

        let defaults = config::Defaults {
            terminal: "/nonexistent/buoy-wm-test-no-such-terminal".to_string(),
            ..config::Defaults::default()
        };
        spawn_pinned_terminal_or_release_claim(&wm_core, tag_id, &defaults, &session_name);

        let mut core = ipc::lock_recovering(&wm_core);
        assert_eq!(
            core.claim_pinned_terminal_spawn(tag_id),
            Ok(Some("tag-web".to_string())),
            "a failed spawn left the tag marked spawned, so it can never retry"
        );
    }

    /// Audit finding C-10: with pinned terminals turned off, the claim
    /// `handle_request`/`ensure_pinned_terminal_spawned` already committed
    /// has to be released, or the tag stays marked spawned for the rest of
    /// the session and a user who turns the feature back on gets nothing
    /// until they restart.
    #[test]
    fn pinned_terminals_turned_off_releases_the_claim_instead_of_spawning() {
        let wm_core = Mutex::new(WmCore::default());
        let tag_id = ipc::lock_recovering(&wm_core).create_tag("web").unwrap();
        let session_name = ipc::lock_recovering(&wm_core)
            .claim_pinned_terminal_spawn(tag_id)
            .unwrap()
            .expect("a freshly created tag has not claimed its spawn yet");
        // `/bin/false` would exit non-zero but still *spawn*: the point of
        // this test is that no process is created at all, which is what
        // the released claim below proves — a successful spawn keeps it.
        let defaults = config::Defaults {
            pinned_terminals: false,
            ..config::Defaults::default()
        };

        spawn_pinned_terminal_or_release_claim(&wm_core, tag_id, &defaults, &session_name);

        assert_eq!(
            ipc::lock_recovering(&wm_core).claim_pinned_terminal_spawn(tag_id),
            Ok(Some("tag-web".to_string())),
            "the claim must be released when nothing is going to spawn"
        );
    }

    #[test]
    fn tag_picker_path_resolves_to_sibling_of_debug_wm_exe() {
        assert_eq!(
            tag_picker_path(Path::new("/workspaces/buoy-wm/target/debug/wm")),
            PathBuf::from("/workspaces/buoy-wm/target/debug/buoy-tag-picker")
        );
    }

    #[test]
    fn tag_picker_path_resolves_to_sibling_of_release_wm_exe() {
        assert_eq!(
            tag_picker_path(Path::new("/workspaces/buoy-wm/target/release/wm")),
            PathBuf::from("/workspaces/buoy-wm/target/release/buoy-tag-picker")
        );
    }

    #[test]
    fn tag_picker_path_falls_back_to_bare_name_when_wm_exe_has_no_parent() {
        // `Path::parent()` returns `None` only when the path terminates in a
        // root or prefix (or is empty) — `/` is the concrete case that hits
        // the fallback branch, not just a single-component relative path
        // (whose `.parent()` is `Some("")`, which `join` already reduces to
        // the bare name anyway).
        assert_eq!(
            tag_picker_path(Path::new("/")),
            PathBuf::from("buoy-tag-picker")
        );
    }

    fn args_of(command: &std::process::Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }
}
