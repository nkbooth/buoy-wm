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

//! Where the IPC socket lives.
//!
//! All three binaries have to resolve the *same* path or they cannot find
//! each other, which is why this rule is defined once here rather than
//! restated per crate. It was restated per crate until now — three
//! byte-identical copies, twelve duplicated tests asserting four facts,
//! and drift had already started (only the server's copy tested the
//! set-but-empty `XDG_RUNTIME_DIR` case).

use std::path::PathBuf;

/// Resolves the Unix domain socket's filesystem path from explicit,
/// injectable parameters — not read from `std::env` inside this function,
/// so it stays a pure, testable decision.
///
/// Real-deployment case: `$XDG_RUNTIME_DIR/buoy-wm.sock` (the WM runs
/// under a real `river` login session, which always sets
/// `XDG_RUNTIME_DIR`). Devcontainer/sandbox-convenience fallback:
/// `/tmp/buoy-wm-<user>.sock`, using `user` then `logname` then the
/// literal `"unknown"` as the disambiguating suffix — `XDG_RUNTIME_DIR` is
/// typically unset in that environment.
pub fn resolve_socket_path(
    xdg_runtime_dir: Option<&str>,
    user: Option<&str>,
    logname: Option<&str>,
) -> PathBuf {
    // A set-but-empty `XDG_RUNTIME_DIR` (a real systemd/container pattern)
    // must fall through to the /tmp case too, not join onto an empty base
    // and produce a relative `buoy-wm.sock` path.
    match xdg_runtime_dir.filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("buoy-wm.sock"),
        None => PathBuf::from("/tmp").join(format!(
            "buoy-wm-{}.sock",
            user.or(logname).unwrap_or("unknown")
        )),
    }
}

/// Thin, untested (I/O-reading, not logic) wrapper around
/// [`resolve_socket_path`] that reads the real environment variables —
/// what every binary calls to get the real socket path.
pub fn default_socket_path() -> PathBuf {
    resolve_socket_path(
        std::env::var("XDG_RUNTIME_DIR").ok().as_deref(),
        std::env::var("USER").ok().as_deref(),
        std::env::var("LOGNAME").ok().as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_socket_path_uses_xdg_runtime_dir_when_set() {
        assert_eq!(
            resolve_socket_path(Some("/run/user/1000"), None, None),
            PathBuf::from("/run/user/1000/buoy-wm.sock")
        );
    }

    #[test]
    fn resolve_socket_path_treats_empty_xdg_runtime_dir_as_unset() {
        assert_eq!(
            resolve_socket_path(Some(""), Some("nick"), None),
            PathBuf::from("/tmp/buoy-wm-nick.sock")
        );
    }

    #[test]
    fn resolve_socket_path_falls_back_to_tmp_user_when_xdg_runtime_dir_unset() {
        assert_eq!(
            resolve_socket_path(None, Some("nick"), None),
            PathBuf::from("/tmp/buoy-wm-nick.sock")
        );
    }

    #[test]
    fn resolve_socket_path_falls_back_to_logname_when_user_also_unset() {
        assert_eq!(
            resolve_socket_path(None, None, Some("nick")),
            PathBuf::from("/tmp/buoy-wm-nick.sock")
        );
    }

    #[test]
    fn resolve_socket_path_falls_back_to_literal_unknown_when_nothing_is_set() {
        assert_eq!(
            resolve_socket_path(None, None, None),
            PathBuf::from("/tmp/buoy-wm-unknown.sock")
        );
    }
}
