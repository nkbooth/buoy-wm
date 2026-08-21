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

//! Telling the user something went wrong, somewhere they will actually see
//! it.
//!
//! `buoy-wm` is `exec`'d by river with no attached TTY and
//! `buoy-tag-picker` is spawned by a keybind with no terminal either, so
//! whether their stderr reaches the journal, `~/.xsession-errors` or
//! `/dev/null` depends entirely on the display manager. Every log line in
//! this workspace is therefore conditionally `/dev/null` from the user's
//! seat, and until now exactly one failure in the whole product was made
//! visible to them: the tag-cap row rendered into the picker (audit
//! finding G-03).
//!
//! A desktop notification rather than the picker's own `fuzzel` overlay,
//! which is what the audit suggested: the single most common cause of
//! "`Super+A` does nothing" is `fuzzel` not being installed, and a message
//! rendered through `fuzzel` cannot report that. `notify-send` is present
//! wherever a notification daemon is, which on this deployment is the same
//! place waybar is.
//!
//! Only the command is shared. Each binary spawns it under its own policy:
//! the WM is a session-long process that has to reap its children, and the
//! picker is about to exit.

use std::process::Command;

/// The `--app-name` every notification from this project carries, so a user
/// can tell one from their mail client's.
const APP_NAME: &str = "buoy-wm";

/// Builds the `notify-send` invocation that puts `message` in front of the
/// user.
///
/// `--urgency=critical` because every caller is reporting something the
/// user has to act on — a dead keybind, a rejected config, an IPC server
/// that never started — and because critical notifications are the ones
/// most notification daemons refuse to auto-dismiss.
///
/// `message` is passed as one argv element, never through a shell.
pub fn notify_send_command(message: &str) -> Command {
    let mut command = Command::new("notify-send");
    command
        .arg(format!("--app-name={APP_NAME}"))
        .arg("--urgency=critical")
        .arg(message);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notification_names_this_app_and_is_critical() {
        let command = notify_send_command("the config was rejected");
        assert_eq!(command.get_program(), "notify-send");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(
            args,
            vec![
                "--app-name=buoy-wm",
                "--urgency=critical",
                "the config was rejected"
            ]
        );
    }

    /// One argv element, so a message built from a path or an error string
    /// cannot become shell syntax.
    #[test]
    fn a_message_that_looks_like_shell_stays_one_argument() {
        let command = notify_send_command("; rm -rf ~ #");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args.len(), 3);
        assert_eq!(args[2], "; rm -rf ~ #");
    }
}
