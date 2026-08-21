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

//! The children this window manager starts, and the one list that reaps
//! them.

use std::sync::Mutex;

use buoy_common::log_err;

/// Every child this WM spawns is fire-and-forget — nothing ever reads an
/// exit status. Without a `wait` each finished child lingers as a zombie
/// for the lifetime of the session, and this process is a long-lived
/// session daemon, so they accumulate (code-review follow-up). Rather than
/// tracking children per call site, keep one list and opportunistically
/// reap whatever has finished each time a new child is spawned.
static SPAWNED_CHILDREN: Mutex<Vec<std::process::Child>> = Mutex::new(Vec::new());

/// Records `child` for reaping and clears out any that have already
/// exited. Recovers from a poisoned lock the same way
/// [`buoy_wm::ipc::lock_recovering`] does — losing track of a child leaks
/// a zombie, which is never worth taking down the session for (NFR2).
pub(crate) fn track_child(child: std::process::Child) {
    let mut children = SPAWNED_CHILDREN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    children.retain_mut(child_is_still_running);
    children.push(child);
}

/// Reaps whatever has exited since the last call.
///
/// Called once per manage sequence as well as on every spawn, because
/// reaping only on spawn meant a session that launched thirty pickers and
/// then idled held thirty zombies until the next keypress (audit finding
/// F-05). Recovers from a poisoned lock for the same reason
/// [`track_child`] does.
pub(crate) fn reap_finished_children() {
    let mut children = SPAWNED_CHILDREN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    children.retain_mut(child_is_still_running);
}

/// Whether `tracked` is still running, reporting anything its exit had to
/// say on the way past.
///
/// The status used to be matched and thrown away. `Command::spawn` succeeds
/// for anything on `$PATH`, so a misconfigured `defaults.terminal` that
/// starts and immediately fails was indistinguishable from one that worked,
/// and nothing said so (audit finding F-05). The reaping design is right;
/// the observation is free.
fn child_is_still_running(tracked: &mut std::process::Child) -> bool {
    match tracked.try_wait() {
        Ok(Some(status)) => {
            if let Some(complaint) = child_exit_complaint(status) {
                log_err!("A child of this WM {complaint}");
            }
            false
        }
        Ok(None) => true,
        Err(e) => {
            // Keep it in the list: an unreadable status is not evidence the
            // process is gone, and dropping the handle would leak the zombie
            // permanently instead of retrying next pass.
            log_err!("Could not check on child process {}: {e}", tracked.id());
            true
        }
    }
}

/// What a finished child's exit status is worth saying, or `None` when it
/// exited cleanly.
fn child_exit_complaint(status: std::process::ExitStatus) -> Option<String> {
    (!status.success()).then(|| format!("exited unsuccessfully ({status})"))
}

/// Spawns `command` fire-and-forget, logging a failure as `"Failed to spawn
/// {what}: {e}"`. `WAYLAND_DEBUG` is removed from every child's environment
/// — the added noise makes debugging the window manager itself impractical.
///
/// Returns whether the child was actually created. Most callers spawn
/// something whose failure costs the user one keypress and ignore this; the
/// pinned terminal is the exception, because its spawn has already been
/// recorded as having happened (audit finding D-01).
pub(crate) fn spawn_tracked(command: &mut std::process::Command, what: &str) -> bool {
    match command.env_remove("WAYLAND_DEBUG").spawn() {
        Ok(child) => {
            track_child(child);
            true
        }
        Err(e) => {
            log_err!("Failed to spawn {what}: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Audit finding F-05: `Command::spawn` succeeds for anything on
    /// `$PATH`, so a misconfigured `defaults.terminal` that starts and
    /// immediately fails was indistinguishable from one that worked — the
    /// exit status was matched and discarded.
    #[test]
    fn a_child_that_exited_badly_has_something_to_report() {
        let failed = std::process::Command::new("/bin/false")
            .status()
            .expect("/bin/false ran");
        assert!(child_exit_complaint(failed).is_some());
    }

    #[test]
    fn a_child_that_exited_cleanly_has_nothing_to_report() {
        let succeeded = std::process::Command::new("/bin/true")
            .status()
            .expect("/bin/true ran");
        assert_eq!(child_exit_complaint(succeeded), None);
    }
}
