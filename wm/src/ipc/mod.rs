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

//! `ipc`: the in-process Unix domain socket server exposing `wm-core` state
//! and mutation commands (Story 2.1). A sibling module to `wm_core`, inside
//! the same `wm` binary — no socket/JSON types leak into `wm_core` itself.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use buoy_common::log_err;

pub mod dispatch;
pub mod protocol;
pub mod server;

/// Locks `mutex`, recovering the last-known-good inner value instead of
/// panicking if the mutex is poisoned. A panic inside one client
/// connection's request handling (however unlikely, given `wm-core`'s own
/// `Result`-based, `.expect()`-light design) must not turn into a WM-wide
/// crash the next time the Wayland thread locks the same `wm_core` —
/// `wm-core`'s own mutators are already `Result`-based and never leave
/// partial writes on their own `Err` paths, so recovering a poisoned
/// guard's inner value is strictly better than propagating the panic
/// (NFR2). Used at every lock site that shares a `WmCore` mutex across the
/// IPC thread family and the pre-existing Wayland-dispatch main thread.
pub fn lock_recovering<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| {
        if should_report_poisoning(&POISONING_REPORTED) {
            log_err!(
                "recovered a poisoned wm_core lock: a thread panicked while holding it, \
                 so shared state may be inconsistent from here on"
            );
        }
        poisoned.into_inner()
    })
}

/// Set once the first poisoned lock has been reported.
static POISONING_REPORTED: AtomicBool = AtomicBool::new(false);

/// Whether this poisoning is the one worth a log line.
///
/// Recovery was previously entirely silent: after one thread panicked
/// holding the lock, every subsequent lock proceeded on possibly
/// inconsistent shared state forever with nothing written anywhere (audit
/// finding G-05). Reported exactly once, because every later locker sees
/// the same poisoned flag — one line per lock would be the flooding shape
/// the accept loop's backoff exists to prevent.
fn should_report_poisoning(reported: &AtomicBool) -> bool {
    !reported.swap(true, Ordering::AcqRel)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{lock_recovering, should_report_poisoning};

    /// The recovery is right, but silently proceeding on possibly
    /// inconsistent shared state forever is not — and one line per lock
    /// after the first panic would be the flooding shape the accept loop's
    /// backoff exists to prevent, so it is exactly once.
    #[test]
    fn poisoning_is_reported_once_and_then_never_again() {
        use std::sync::atomic::AtomicBool;

        let reported = AtomicBool::new(false);
        assert!(should_report_poisoning(&reported));
        assert!(!should_report_poisoning(&reported));
        assert!(!should_report_poisoning(&reported));
    }

    #[test]
    fn lock_recovering_returns_the_guard_normally_when_not_poisoned() {
        let mutex = Mutex::new(5);
        assert_eq!(*lock_recovering(&mutex), 5);
    }

    #[test]
    fn lock_recovering_recovers_the_inner_value_after_a_panic_while_locked() {
        let mutex = Arc::new(Mutex::new(5));
        let mutex2 = Arc::clone(&mutex);
        let join_result = std::thread::spawn(move || {
            let _g = mutex2.lock().unwrap();
            panic!("simulated");
        })
        .join();
        assert!(
            join_result.is_err(),
            "the spawned thread was expected to panic"
        );

        assert_eq!(*lock_recovering(&mutex), 5);
    }
}
