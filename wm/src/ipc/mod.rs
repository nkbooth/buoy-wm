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

/// The human-readable half of a caught panic's payload.
///
/// `panic!("literal")` yields a `&'static str` and `panic!("{x}")` yields a
/// `String`, so a reporter that reads only one of them is blank for half
/// the panics this codebase can raise; `Debug for dyn Any` renders the
/// literal `Any { .. }`, so printing the payload directly is worse still
/// (audit finding G-05).
///
/// One copy, used by both of this binary's panic barriers: the
/// process-wide hook that reports before the session dies, and
/// [`server`]'s per-connection `catch_unwind`. They were deliberately two
/// copies while `ipc::server` still reached up into the crate root for its
/// process spawn — merging them then would have added a fourth reach-up
/// instead of removing three (audit finding J-02). It lives here, beside
/// [`lock_recovering`], because this module is already where the helpers
/// that keep one thread's failure from ending the session live, and
/// because nothing here depends on the crate root — which is what lets
/// `ipc` move to a library crate later (audit finding T-04).
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(literal) = payload.downcast_ref::<&'static str>() {
        literal
    } else if let Some(owned) = payload.downcast_ref::<String>() {
        owned
    } else {
        "a non-string panic payload"
    }
}

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

    use super::{lock_recovering, panic_message, should_report_poisoning};

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

    /// `panic!("literal")` and `panic!("{x}")` produce payloads of two
    /// different types, and a reporter that can only read one of them is
    /// blank for half the panics this codebase can actually raise.
    #[test]
    fn a_panic_message_is_read_from_either_payload_type() {
        assert_eq!(panic_message(&"a string literal"), "a string literal");
        assert_eq!(
            panic_message(&String::from("a formatted panic")),
            "a formatted panic"
        );
    }

    /// A payload that is neither still has to produce something a journal
    /// reader can act on, because this is the one place the code
    /// deliberately reaches for crash telemetry.
    #[test]
    fn an_unreadable_panic_payload_still_produces_a_message() {
        assert_eq!(panic_message(&7u32), "a non-string panic payload");
    }

    /// The shape `catch_unwind` hands back, rather than a bare payload
    /// reference: the per-connection barrier gets a `Box<dyn Any + Send>`,
    /// and it has to decode through it.
    #[test]
    fn a_boxed_payload_from_catch_unwind_decodes_the_same_way() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(format!("boom {}", 7));
        assert_eq!(panic_message(&*payload), "boom 7");
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
