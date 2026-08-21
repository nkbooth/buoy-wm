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

//! Who is allowed to speak on the IPC socket.
//!
//! The socket's `0600` mode was the *sole* access control on the whole IPC
//! surface (audit finding C-06), which meant anything that won a race on
//! the path or the mode got the full request set — including the
//! `switch-tag` that makes the WM spawn a process. `SO_PEERCRED` is a
//! second, independent control that a filesystem race cannot bypass: the
//! kernel stamps the peer's credentials at `connect(2)` time and no
//! userspace process can forge them.
//!
//! Both directions matter. The server rejects a client running as another
//! user; each client rejects a *server* that is not running as the user,
//! which is what makes a squatted socket path a failed connection rather
//! than a silent state-and-intent capture. Deliberately no token or
//! shared-secret scheme: the secret would have to live where the same
//! attacker can already read it.

use std::fmt;
use std::os::unix::net::UnixStream;

/// The identity the kernel reports for the process on the other end of a
/// Unix domain socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerIdentity {
    /// The peer's effective uid at `connect(2)`/`socketpair(2)` time.
    pub uid: u32,
    /// The peer's pid. Logged so a refusal can name the offending process
    /// rather than just the refusal.
    pub pid: i32,
}

/// Why a peer was not accepted.
#[derive(Debug)]
pub enum PeerRejection {
    /// The kernel would not report credentials for this socket. Treated as
    /// a rejection, never as a pass: an unreadable identity is not a
    /// verified one.
    Unreadable(std::io::Error),
    /// The peer is a different user.
    ForeignUid { peer: PeerIdentity, expected: u32 },
}

impl fmt::Display for PeerRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(e) => write!(f, "peer credentials are unreadable: {e}"),
            Self::ForeignUid { peer, expected } => write!(
                f,
                "peer runs as uid {} (pid {}), not uid {expected}",
                peer.uid, peer.pid
            ),
        }
    }
}

impl std::error::Error for PeerRejection {}

/// This process's effective uid — the identity every peer must match.
pub fn own_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// Accepts `peer` only if it is the same user as this process.
///
/// Split from [`authenticate_peer`] so the decision is testable without a
/// second user account to connect from.
pub fn accept_peer(peer: PeerIdentity, expected_uid: u32) -> Result<PeerIdentity, PeerRejection> {
    if peer.uid == expected_uid {
        Ok(peer)
    } else {
        Err(PeerRejection::ForeignUid {
            peer,
            expected: expected_uid,
        })
    }
}

/// Reads `stream`'s peer credentials and accepts the peer only if it runs
/// as this process's own effective uid.
pub fn authenticate_peer(stream: &UnixStream) -> Result<PeerIdentity, PeerRejection> {
    let credentials = rustix::net::sockopt::socket_peercred(stream)
        .map_err(|e| PeerRejection::Unreadable(e.into()))?;
    accept_peer(
        PeerIdentity {
            uid: credentials.uid.as_raw(),
            pid: credentials.pid.as_raw_nonzero().get(),
        },
        own_uid(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peer_with_the_expected_uid_is_accepted() {
        let peer = PeerIdentity { uid: 1000, pid: 42 };
        assert_eq!(accept_peer(peer, 1000).expect("same uid"), peer);
    }

    #[test]
    fn a_peer_with_another_uid_is_rejected() {
        let peer = PeerIdentity { uid: 0, pid: 42 };
        let rejection = accept_peer(peer, 1000).expect_err("root is not this user");
        assert!(matches!(rejection, PeerRejection::ForeignUid { .. }));
    }

    /// The rejection has to name the offender: a log line saying only
    /// "rejected" cannot be acted on.
    #[test]
    fn a_rejection_names_both_uids_and_the_pid() {
        let peer = PeerIdentity { uid: 0, pid: 42 };
        let message = accept_peer(peer, 1000)
            .expect_err("root is not this user")
            .to_string();
        assert!(message.contains("uid 0"), "{message}");
        assert!(message.contains("42"), "{message}");
        assert!(message.contains("1000"), "{message}");
    }

    /// Exercises the real `SO_PEERCRED` path end to end: both ends of a
    /// socketpair are this process, so authentication must succeed and
    /// report this process's own uid.
    #[test]
    fn both_ends_of_our_own_socketpair_authenticate_as_us() {
        let (a, b) = UnixStream::pair().expect("socketpair");
        assert_eq!(
            authenticate_peer(&a).expect("our own end is us").uid,
            own_uid()
        );
        assert_eq!(
            authenticate_peer(&b).expect("our own end is us").uid,
            own_uid()
        );
    }
}
