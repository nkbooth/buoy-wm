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

//! The one command-line flag every binary in the workspace shares.
//!
//! `--version` exists so a release can prove an installed package's
//! binaries load on their target without a compositor, an IPC socket or a
//! config file (NFR6): `release.yml`'s `verify` job runs each binary with
//! it in a clean container. Before it, `buoy-wm` never read its arguments
//! and would try to start the WM, and the two companions rejected any
//! argument they did not expect.
//!
//! Deliberately not an argument parser. Only the first argument is read,
//! because "accept `--version` as the first argument" is the whole
//! contract; each binary keeps parsing everything else exactly as before.

/// Returns the line a binary prints for `--version`, `"<name> <version>"`,
/// when `args` (the arguments after the program name) starts with exactly
/// `--version`, and `None` otherwise.
///
/// Anything after the flag is ignored; the flag anywhere but first, and
/// spellings such as `-V` or `version`, are not version requests.
pub fn version_line(args: &[String], name: &str, version: &str) -> Option<String> {
    match args.first() {
        Some(first) if first == "--version" => Some(format!("{name} {version}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn version_as_the_first_argument_gives_the_name_and_version() {
        assert_eq!(
            version_line(&args(&["--version"]), "buoy-wm", "1.0.0-rc.2"),
            Some("buoy-wm 1.0.0-rc.2".to_string())
        );
    }

    #[test]
    fn arguments_after_the_flag_are_ignored() {
        assert_eq!(
            version_line(
                &args(&["--version", "7", "--help"]),
                "buoy-status-bar",
                "1.2.3"
            ),
            Some("buoy-status-bar 1.2.3".to_string())
        );
    }

    #[test]
    fn no_arguments_is_not_a_version_request() {
        assert_eq!(version_line(&[], "buoy-wm", "1.2.3"), None);
    }

    #[test]
    fn the_flag_in_second_place_is_not_a_version_request() {
        assert_eq!(
            version_line(&args(&["switch", "--version"]), "buoy-tag-picker", "1.2.3"),
            None
        );
    }

    #[test]
    fn only_the_exact_long_flag_counts() {
        for other in ["-V", "--Version", "version", "--version=1", " --version"] {
            assert_eq!(
                version_line(&args(&[other]), "buoy-wm", "1.2.3"),
                None,
                "{other:?} was taken for --version"
            );
        }
    }
}
