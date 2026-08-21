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

//! Severity and prefix discipline for the workspace's log lines.
//!
//! There were 67 bare `eprintln!` calls and no severity anywhere except in
//! the wording, so `journalctl -p err` could not work and a fatal line was
//! indistinguishable to a filter from an informational one (audit finding
//! G-02). Prefixes were ad hoc too: `ipc: ` in one module, nothing in its
//! neighbour, bare sentences elsewhere.
//!
//! Deliberately not a `log`/`tracing` stack. Dependency minimalism is a
//! documented value here, and journald parses a leading `<N>` on a stream
//! it captures natively, so the whole framework this project needs is a
//! prefix. The subsystem comes from `module_path!()` rather than a
//! hand-written string, which is the only version of prefix discipline
//! that cannot drift or be forgotten at a new call site.
//!
//! A `<3>` visible in a terminal is the cost: these binaries log to
//! journald in the deployment that matters, and a run by hand is the
//! exception.

/// How much a log line matters. Two levels, not five: the only consumers
/// are `journalctl -p err` and a human reading a journal, and every line
/// in this workspace is either something the user may need to act on or a
/// note about what the WM did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Something failed, was refused, or was skipped. `journalctl -p err`
    /// shows exactly these.
    Error,
    /// Something happened that is worth a record but needs no action.
    Info,
}

impl Severity {
    /// The `<N>` prefix journald reads as this severity, matching
    /// `syslog(3)`'s numbering: 3 is `err`, 6 is `info`.
    pub const fn journald_prefix(self) -> &'static str {
        match self {
            Self::Error => "<3>",
            Self::Info => "<6>",
        }
    }
}

/// Formats one log line: severity prefix, the invoking module's path, then
/// the message. Exposed because it is the testable half — the macros that
/// wrap it write to stderr, which a unit test cannot see.
#[macro_export]
macro_rules! journal_line {
    ($severity:expr, $($arg:tt)*) => {
        format!(
            "{}{}: {}",
            $crate::log::Severity::journald_prefix($severity),
            module_path!(),
            format_args!($($arg)*)
        )
    };
}

/// Logs a failure, a refusal or a skipped action at journald's `err`
/// severity.
#[macro_export]
macro_rules! log_err {
    ($($arg:tt)*) => {
        eprintln!(
            "{}",
            $crate::journal_line!($crate::log::Severity::Error, $($arg)*)
        )
    };
}

/// Logs a note about what happened at journald's `info` severity.
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        eprintln!(
            "{}",
            $crate::journal_line!($crate::log::Severity::Info, $($arg)*)
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_maps_to_journalds_own_numbering() {
        assert_eq!(Severity::Error.journald_prefix(), "<3>");
        assert_eq!(Severity::Info.journald_prefix(), "<6>");
    }

    /// The prefix has to name the module that *logged*, not the module the
    /// macro was defined in — which is the whole reason `module_path!()`
    /// can replace hand-written prefixes.
    #[test]
    fn a_line_names_the_module_it_was_logged_from() {
        let line = journal_line!(Severity::Error, "tag {} rejected", 7);
        assert_eq!(line, "<3>buoy_common::log::tests: tag 7 rejected");
    }

    #[test]
    fn an_info_line_carries_the_info_prefix() {
        assert!(journal_line!(Severity::Info, "hello").starts_with("<6>"));
    }
}
