// SPDX-FileCopyrightText: © 2026 Nick Booth
// Portions © 2026 Julian Andrews, originally distributed under 0BSD as
// part of tinyrwm <https://codeberg.org/river/tinyrwm>.
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

//! The `buoy-wm` binary's composition root.
//!
//! # Rationale
//!
//! What is left here is only what can be done once, at startup, in an
//! order: install the panic reporter, connect, load the config before the
//! first roundtrip binds a seat, check the two required globals, bind the
//! IPC socket, dispatch until the connection ends, unlink the socket. Every
//! decision either of those steps makes lives in [`compositor`] or in
//! `buoy_wm` below it.
//!
//! The two remaining rules this file enforces are both about being the
//! session leader. Nothing here aborts startup over a recoverable problem —
//! a rejected config and a socket that will not bind both degrade and say
//! so, because this process is the only thing that can put a screen in
//! front of the user to fix it with. And every failure the user could act
//! on goes through [`notify_user`], not `eprintln!`: river `exec`s this
//! binary with no TTY, so a log line's destination depends on the display
//! manager (audit findings B-03, G-03).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use buoy_common::log_err;
use wayland_client::Connection;

mod compositor;

use crate::compositor::launch::spawn_pinned_terminal_or_release_claim;
use crate::compositor::manager::AppData;
use crate::compositor::report::{install_panic_reporter, notify_user};

use buoy_wm::{config, ipc, wm_core};

use config::Config;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    install_panic_reporter();

    // Queue up a get_registry event.
    let conn = Connection::connect_to_env()?;
    let display = conn.display();
    let mut event_queue = conn.new_event_queue();
    let _registry = display.get_registry(&event_queue.handle(), ());

    // Initial state
    let mut app_data = AppData::default();

    // A broken config file is reported and then ignored in favour of the
    // built-in defaults, rather than aborting startup: this WM is the only
    // thing that can put a screen in front of the user to fix the typo
    // with, so refusing to start over a bad keybind would lock them out of
    // their own session. Loaded before the first roundtrip binds any seat,
    // since `init_new_seats` registers whatever this produces.
    // `load` only reports a problem for a file it actually found, so the
    // path is always resolvable here; the fallback label is
    // belt-and-braces rather than a reachable case.
    let config_path = config::config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "the config file".to_string());
    app_data.wm.config = match Config::load() {
        Ok(loaded) => {
            // Reported separately from the skipped entries, and first: the
            // file was used in full either way, so folding the two into one
            // notification would read as though the mode had cost the user
            // a binding (audit finding C-08).
            if let Some(problem) = loaded.trust_problem {
                notify_user(&format!("{config_path}: {problem}"));
            }
            if let Some(error) = config::ConfigError::from_many(loaded.skipped) {
                notify_user(&format!(
                    "{config_path}: {error}\nThose entries were skipped; \
                     everything else in the file is in effect."
                ));
            }
            loaded.config
        }
        Err(e) => {
            notify_user(&format!(
                "{config_path} was rejected ({e}).\nRunning with built-in \
                 default keybinds until it is fixed."
            ));
            Config::default()
        }
    };

    // Roundtrip to process the get_registry event and bind interfaces.
    event_queue.roundtrip(&mut app_data)?;
    if app_data.river_wm.is_none() {
        log_err!("river_window_manager_v1 global not found! Is river running?");
        std::process::exit(1);
    }
    if app_data.river_xkb.is_none() {
        log_err!("river_xkb_bindings_v1 global not found! Is river running with xkb support?");
        std::process::exit(1);
    }

    // A failed IPC-server bind (e.g. a permissions issue) is logged and
    // does not abort WM startup: the WM's core job — managing windows —
    // must not depend on the IPC server. Deliberate degrade-not-crash
    // choice, consistent with NFR2's priority ordering, and mirrors this
    // file's existing `log_wm_core_err`-style "log, don't abort"
    // convention rather than `std::process::exit`, which this file
    // otherwise reserves for unrecoverable Wayland-protocol-level
    // failures only.
    // A failed IPC server is not fatal — the WM's core job, managing
    // windows, must not depend on it, and refusing to start is the one
    // outcome `SECURITY.md` rules out for the session leader. But three
    // keybinds and every status bar are dead until the next restart, which
    // used to be explained by a single line on an invisible stderr (audit
    // finding B-03).
    let (ipc_socket, ipc_outage) =
        match start_ipc_server(&app_data.wm.wm_core, &app_data.wm.config.defaults) {
            Ok(socket_path) => (Some(socket_path), None),
            Err(reason) => {
                notify_user(&format!(
                    "buoy-wm started with no IPC server: {reason}.\nThe tag \
                     picker (Super+A), the tag switcher (Super+S) and every \
                     status bar will do nothing until buoy-wm is restarted."
                ));
                (None, Some(reason))
            }
        };

    let outcome = run_event_loop(&mut event_queue, &mut app_data, ipc_outage);

    // Audit finding B-04: nothing used to unlink the socket, so every
    // start found a stale inode and had to decide whether to clobber it.
    // Guarded on `Some`, which `start_ipc_server` only returns when *this*
    // process bound the socket — a second instance that was refused
    // (C-04) must never remove the running instance's endpoint on its way
    // out. Still skipped by `std::process::exit` and by signals, which
    // this binary handles nowhere; under `$XDG_RUNTIME_DIR` (tmpfs,
    // cleared at logout) a leftover inode is cosmetic.
    if let Some(socket_path) = ipc_socket {
        if let Err(e) = std::fs::remove_file(&socket_path) {
            log_err!("Failed to remove the IPC socket at {socket_path:?}: {e}");
        }
    }

    // Reported here rather than returned: `main`'s `Box<dyn Error>` return
    // makes Rust print `Error: ` followed by the *`Debug`* of a
    // `DispatchError`, which is how the single most likely way this process
    // ever ends used to describe itself (audit finding G-02).
    if let Err(e) = outcome {
        log_err!("The Wayland connection ended, so buoy-wm is exiting: {e}");
        std::process::exit(1);
    }
    Ok(())
}

/// Resolves, checks and binds the IPC socket, returning the path bound so
/// the caller can unlink it on the way out — `None` if IPC is not running,
/// for any reason.
///
/// Every failure here degrades rather than aborts: a WM with no IPC still
/// manages windows and still gives the user a shell to fix the problem
/// from, and refusing to start is the one outcome `SECURITY.md` rules out
/// for the session leader. Three keybinds and the status bar are dead
/// until the next restart, though, so each failure says which condition
/// caused it.
fn start_ipc_server(
    wm_core: &Arc<Mutex<wm_core::state::WmCore>>,
    defaults: &config::Defaults,
) -> Result<PathBuf, String> {
    let socket_path = buoy_common::socket_path::default_socket_path()
        .ok_or_else(|| buoy_common::socket_path::NO_RUNTIME_DIR_MESSAGE.to_string())?;
    // The socket's own `0600` mode is applied one syscall after `bind`,
    // and Linux checks AF_UNIX permissions at `connect(2)` — so the
    // directory, not the mode, is what actually has to be private (audit
    // finding C-03).
    let parent = socket_path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", socket_path.display()))?;
    buoy_common::socket_path::verify_private_dir(parent).map_err(|e| e.to_string())?;
    ipc::server::spawn(
        Arc::clone(wm_core),
        &socket_path,
        pinned_terminal_spawner(wm_core, defaults),
    )
    .map(|_accept_thread| socket_path.clone())
    .map_err(|e| format!("cannot listen on {}: {e}", socket_path.display()))
}

/// The pinned-terminal spawn the IPC server performs when a dispatched
/// `switch-tag` has claimed one, as a closure over the config and the
/// shared core.
///
/// This is the composition-root half of audit finding J-02: `ipc::server`
/// used to call `crate::spawn_pinned_terminal_or_release_claim` directly,
/// which made a leaf transport module depend on the module that owns the
/// whole WM. Building the closure here instead puts the knowledge of *what*
/// a spawn is where the config already lives, and leaves the server holding
/// only a `Fn`.
fn pinned_terminal_spawner(
    wm_core: &Arc<Mutex<wm_core::state::WmCore>>,
    defaults: &config::Defaults,
) -> ipc::server::SpawnPinnedTerminal {
    let wm_core = Arc::clone(wm_core);
    let defaults = defaults.clone();
    Arc::new(move |pending: &ipc::dispatch::PendingPinnedSpawn| {
        spawn_pinned_terminal_or_release_claim(
            &wm_core,
            pending.tag_id,
            &defaults,
            &pending.session_name,
        )
    })
}

/// How long between reminders that this session has no IPC server. Long
/// enough not to become the noise it is trying to be heard over, short
/// enough that a journal from any point in the session says so.
const IPC_OUTAGE_REMINDER: Duration = Duration::from_secs(600);

/// Dispatches Wayland events until the connection ends.
///
/// `ipc_outage`, when set, is the reason IPC never started; it is re-logged
/// on a slow timer rather than once at startup, so a journal read hours
/// later still explains why `Super+A` does nothing.
fn run_event_loop(
    event_queue: &mut wayland_client::EventQueue<AppData>,
    app_data: &mut AppData,
    ipc_outage: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reminded_at = std::time::Instant::now();
    loop {
        event_queue.blocking_dispatch(app_data)?;
        if let Some(reason) = &ipc_outage
            && reminded_at.elapsed() >= IPC_OUTAGE_REMINDER
        {
            log_err!("still running with no IPC server: {reason}");
            reminded_at = std::time::Instant::now();
        }
    }
}
