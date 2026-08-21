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

//! Where the IPC socket lives, and what has to be true of the directory it
//! lives in.
//!
//! All three binaries have to resolve the *same* path or they cannot find
//! each other, which is why this rule is defined once here rather than
//! restated per crate. It was restated per crate until now — three
//! byte-identical copies, thirteen duplicated tests asserting five facts,
//! and drift had already started.
//!
//! There used to be a second, `/tmp/buoy-wm-<$USER>.sock` fallback for
//! environments with no `XDG_RUNTIME_DIR`, which shipped ungated in
//! release builds (audit finding C-02). It is gone. A predictable path in
//! a world-writable directory, keyed on an environment variable rather
//! than a uid, is squattable — pre-create the file and `/tmp`'s sticky bit
//! makes the WM's own removal fail, so the WM runs the whole session with
//! no IPC while the picker and status bar talk to the squatter's listener
//! instead. Nothing in the devcontainer ever resolved this path anyway
//! (the tests all inject one), so the fallback existed for no live
//! consumer.

use std::path::{Path, PathBuf};

/// The socket's filename inside the runtime directory.
const SOCKET_FILE_NAME: &str = "buoy-wm.sock";

/// Resolves the Unix domain socket's filesystem path from an explicit,
/// injectable `XDG_RUNTIME_DIR` value — not read from `std::env` inside
/// this function, so it stays a pure, testable decision.
///
/// `None` — meaning "there is nowhere safe to put the socket, so there is
/// no IPC" — for a value that is unset, empty (a real systemd and
/// container pattern), or relative. The XDG Base Directory specification
/// requires a relative value be ignored, and an empty one joined onto a
/// filename would yield a path resolved against the process's working
/// directory.
///
/// ```
/// use buoy_common::socket_path::resolve_socket_path;
/// use std::path::PathBuf;
///
/// assert_eq!(
///     resolve_socket_path(Some("/run/user/1000")),
///     Some(PathBuf::from("/run/user/1000/buoy-wm.sock")),
/// );
/// // Unset, empty and relative all mean "there is no socket path", not
/// // "resolve it against wherever the session happened to start".
/// assert_eq!(resolve_socket_path(None), None);
/// assert_eq!(resolve_socket_path(Some("")), None);
/// assert_eq!(resolve_socket_path(Some("run/user/1000")), None);
/// ```
pub fn resolve_socket_path(xdg_runtime_dir: Option<&str>) -> Option<PathBuf> {
    let dir = Path::new(xdg_runtime_dir?);
    if !dir.is_absolute() {
        return None;
    }
    Some(dir.join(SOCKET_FILE_NAME))
}

/// Thin, untested (I/O-reading, not logic) wrapper around
/// [`resolve_socket_path`] that reads the real environment variable —
/// what every binary calls to get the real socket path.
pub fn default_socket_path() -> Option<PathBuf> {
    resolve_socket_path(std::env::var("XDG_RUNTIME_DIR").ok().as_deref())
}

/// The message every binary prints when [`default_socket_path`] returns
/// `None`. Shared so the three binaries describe the same condition the
/// same way, and so it is greppable in a journal.
pub const NO_RUNTIME_DIR_MESSAGE: &str =
    "XDG_RUNTIME_DIR is unset, empty or relative, so there is no IPC socket path";

/// Verifies that `dir` is a directory this user owns and only this user
/// can reach, before anything binds a socket inside it.
///
/// This is what stops the socket's own mode from being the sole access
/// control (audit finding C-03): `bind(2)` creates the inode at
/// `0777 & ~umask` — `0755` under umask 022 — and Linux checks `AF_UNIX`
/// permissions at `connect(2)`, so a connection won in the window before
/// the follow-up `chmod` is *not* revoked by it. A private parent
/// directory closes that window structurally rather than narrowing it,
/// because an unreachable directory means an unreachable socket whatever
/// mode the socket carries.
///
/// Fails closed on anything unexpected, including a symlink: the check is
/// on `symlink_metadata`, so a link pointing at a private directory is
/// still refused.
pub fn verify_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(dir)?;
    if !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            format!("{} is not a directory", dir.display()),
        ));
    }
    let owner = metadata.uid();
    if owner != crate::peer::own_uid() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "{} is owned by uid {owner}, not by this user",
                dir.display()
            ),
        ));
    }
    // Group and other bits, all three of them: an executable-only
    // directory is still traversable by name, which is all an attacker
    // needs to reach a socket whose own name is predictable.
    let mode = metadata.mode() & 0o077;
    if mode != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "{} is reachable by other users (mode {:04o})",
                dir.display(),
                metadata.mode() & 0o7777
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_socket_path_uses_an_absolute_xdg_runtime_dir() {
        assert_eq!(
            resolve_socket_path(Some("/run/user/1000")),
            Some(PathBuf::from("/run/user/1000/buoy-wm.sock"))
        );
    }

    #[test]
    fn resolve_socket_path_rejects_an_unset_xdg_runtime_dir() {
        assert_eq!(resolve_socket_path(None), None);
    }

    /// A set-but-empty value is the case that actually fires in practice,
    /// and the one that would otherwise produce a relative path resolved
    /// against the WM's working directory.
    #[test]
    fn resolve_socket_path_rejects_an_empty_xdg_runtime_dir() {
        assert_eq!(resolve_socket_path(Some("")), None);
    }

    #[test]
    fn resolve_socket_path_rejects_a_relative_xdg_runtime_dir() {
        assert_eq!(resolve_socket_path(Some("run/user/1000")), None);
        assert_eq!(resolve_socket_path(Some("../run")), None);
    }

    /// A temporary directory created with an explicit mode, removed when
    /// the guard drops so a failing assertion does not litter `/tmp`.
    struct TempDir(PathBuf);

    impl TempDir {
        fn with_mode(label: &str, mode: u32) -> Self {
            use std::os::unix::fs::DirBuilderExt;
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("buoy-common-{label}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::DirBuilder::new()
                .mode(mode)
                .create(&path)
                .expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn verify_private_dir_accepts_a_0700_directory_we_own() {
        let dir = TempDir::with_mode("private", 0o700);
        assert!(verify_private_dir(dir.path()).is_ok());
    }

    /// The whole point of the check: a runtime directory anyone can
    /// traverse makes the socket's own mode load-bearing again.
    #[test]
    fn verify_private_dir_rejects_a_group_or_world_reachable_directory() {
        for mode in [0o750, 0o705, 0o777, 0o701] {
            let dir = TempDir::with_mode("shared", mode);
            let error = verify_private_dir(dir.path())
                .expect_err(&format!("mode {mode:o} must be refused"));
            assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        }
    }

    #[test]
    fn verify_private_dir_rejects_a_path_that_is_not_a_directory() {
        let dir = TempDir::with_mode("not-a-dir", 0o700);
        let file = dir.path().join("plain");
        std::fs::write(&file, b"").expect("write test file");
        assert!(verify_private_dir(&file).is_err());
    }

    /// `symlink_metadata`, not `metadata`: a link to a private directory is
    /// still not the private directory, and following it would be exactly
    /// the redirection the check exists to refuse.
    #[test]
    fn verify_private_dir_rejects_a_symlink_even_to_a_private_directory() {
        let target = TempDir::with_mode("symlink-target", 0o700);
        let holder = TempDir::with_mode("symlink-holder", 0o700);
        let link = holder.path().join("link");
        std::os::unix::fs::symlink(target.path(), &link).expect("create symlink");
        assert!(verify_private_dir(&link).is_err());
    }

    #[test]
    fn verify_private_dir_rejects_a_missing_directory() {
        let dir = TempDir::with_mode("missing-parent", 0o700);
        let error = verify_private_dir(&dir.path().join("absent"))
            .expect_err("a missing directory cannot be verified");
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }

    /// The deleted `/tmp/buoy-wm-<user>.sock` fallback, asserted absent:
    /// no combination of a missing runtime directory resolves to a shared
    /// directory any more.
    #[test]
    fn resolve_socket_path_never_falls_back_to_a_shared_directory() {
        for value in [None, Some(""), Some("relative")] {
            assert_eq!(resolve_socket_path(value), None, "{value:?}");
        }
    }
}
