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

//! Mirrors `wm/src/ipc/server.rs`'s `resolve_socket_path`/
//! `default_socket_path` byte-for-byte, same as `buoy-tag-picker/src/
//! socket_path.rs`'s own copy. This is the third accepted occurrence of
//! this duplication (Story 2.5 Task 1.3/`wire.rs`'s module doc comment
//! records the full three-strike-DRY rationale for why no shared crate is
//! extracted here) — keep all three copies in sync if the resolution rule
//! ever changes.

use std::path::PathBuf;

/// Resolves the Unix domain socket's filesystem path from explicit,
/// injectable parameters, mirroring `wm`'s own `resolve_socket_path`
/// exactly: `$XDG_RUNTIME_DIR/buoy-wm.sock` in the real-deployment case,
/// `/tmp/buoy-wm-<user>.sock` (using `user` then `logname` then the
/// literal `"unknown"`) as the devcontainer/sandbox-convenience fallback.
pub fn resolve_socket_path(
    xdg_runtime_dir: Option<&str>,
    user: Option<&str>,
    logname: Option<&str>,
) -> PathBuf {
    match xdg_runtime_dir.filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("buoy-wm.sock"),
        None => PathBuf::from("/tmp").join(format!(
            "buoy-wm-{}.sock",
            user.or(logname).unwrap_or("unknown")
        )),
    }
}

/// Thin, untested (I/O-reading, not logic) wrapper around
/// [`resolve_socket_path`] that reads the real environment variables.
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
            std::path::PathBuf::from("/run/user/1000/buoy-wm.sock")
        );
    }

    #[test]
    fn resolve_socket_path_falls_back_to_tmp_user_when_xdg_runtime_dir_unset() {
        assert_eq!(
            resolve_socket_path(None, Some("nick"), None),
            std::path::PathBuf::from("/tmp/buoy-wm-nick.sock")
        );
    }

    #[test]
    fn resolve_socket_path_falls_back_to_logname_when_user_also_unset() {
        assert_eq!(
            resolve_socket_path(None, None, Some("nick")),
            std::path::PathBuf::from("/tmp/buoy-wm-nick.sock")
        );
    }

    #[test]
    fn resolve_socket_path_falls_back_to_literal_unknown_when_nothing_is_set() {
        assert_eq!(
            resolve_socket_path(None, None, None),
            std::path::PathBuf::from("/tmp/buoy-wm-unknown.sock")
        );
    }
}
