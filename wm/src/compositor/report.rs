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

//! How this window manager says that something went wrong.
//!
//! # Rationale
//!
//! Three audiences, deliberately distinguished. A developer tripwire the
//! user cannot act on goes to the journal
//! ([`log_wm_core_err`]); a failure they *can* act on also goes to their
//! screen ([`notify_user`]); and a panic gets both plus a greppable
//! sentence naming the source location, because `[profile.release] strip =
//! true` leaves the backtrace symbol-poor (audit findings F-01, G-03).

use std::fmt::Debug;

use buoy_common::log_err;
use buoy_wm::ipc;
use buoy_wm::wm_core::state::WmCoreError;

use crate::compositor::child::spawn_tracked;

/// Logs `result`'s error (if any) as `"{context}: {e}"`, otherwise no-ops.
/// `wm_core` mutators only fail on invalid/unknown ids that call sites here
/// already guard against structurally (NFR2) — this exists purely so a
/// future regression is visible instead of silently discarded.
pub(crate) fn log_wm_core_err(result: Result<(), WmCoreError>, context: &str) {
    if let Err(e) = result {
        log_err!("{context}: {e}");
    }
}

/// Formats a protocol enum for a log line without `WEnum`'s wrapper.
///
/// `{:?}` on a `WEnum` prints `Value(Disabled)`, leaking a detail of how
/// wayland-rs models "this could be a value the client's copy of the
/// protocol has never heard of" into output a person reads. An unknown
/// value still has to say so — it means river and this binary disagree
/// about the protocol — but it says it in words rather than a wrapper.
pub(crate) fn wenum_label<T: Debug>(value: wayland_client::WEnum<T>) -> String {
    // Matched on the variants rather than via `into_result`, whose `Err`
    // carries a pre-formatted "Unknown numeric value N for enum ..." string
    // — the raw number is the useful half, and the type name is already
    // implied by the log line it lands in.
    match value {
        wayland_client::WEnum::Value(known) => format!("{known:?}"),
        wayland_client::WEnum::Unknown(raw) => format!("unknown ({raw})"),
    }
}

/// The one line a panic leaves behind, whichever thread raised it.
///
/// `[profile.release] strip = true` leaves backtraces symbol-poor, so a
/// stable, greppable sentence naming the source location is the only thing
/// that will identify the next unforeseen panic in a journal from a
/// session that has already ended (audit finding F-01).
fn panic_report(location: &str, message: &str) -> String {
    format!("buoy-wm panicked at {location}: {message}")
}

/// Makes a panic visible to the user before the process dies, then defers
/// to the default hook so stderr and the exit path are unchanged.
///
/// Deliberately a hook rather than a `catch_unwind` around the dispatch
/// loop. A panic part-way through a manage sequence leaves `WmCore`,
/// `WindowManager::windows` and river's own in-flight transaction
/// half-updated with no way to tell which; resuming the loop over that
/// state would trade a visible crash for silently wrong window management,
/// which is the worse of the two outcomes for a session leader. Reporting
/// and dying is honest — the hook only makes sure the user finds out why
/// their desktop vanished (audit finding F-01).
pub(crate) fn install_panic_reporter() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|location| location.to_string())
            .unwrap_or_else(|| "an unknown location".to_string());
        notify_user(&panic_report(&location, ipc::panic_message(info.payload())));
        default_hook(info);
    }));
}

/// Puts `message` in front of the user — as a desktop notification as well
/// as in the journal.
///
/// This WM is `exec`'d by river with no attached TTY, so whether a log line
/// reaches the journal, `~/.xsession-errors` or `/dev/null` depends on the
/// display manager: from the user's seat every one of them is
/// conditionally invisible (audit finding G-03). Reserved for failures the
/// user can actually act on — a dead keybind, a rejected config, an IPC
/// server that never started. Structurally-unreachable developer
/// tripwires, such as the `log_wm_core_err` sites, stay on plain logging;
/// a notification the user cannot act on trains them to dismiss the ones
/// they can.
///
/// Routed through [`spawn_tracked`] like every other child: this process
/// lives for the whole session and would otherwise accumulate one zombie
/// per notification. There is no recursion risk in doing so —
/// `spawn_tracked`'s own failure path only logs.
pub(crate) fn notify_user(message: &str) {
    spawn_tracked(
        &mut buoy_common::notify::notify_send_command(message),
        "notify-send",
    );
    log_err!("{message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_report_names_where_it_happened_and_what_it_said() {
        let report = panic_report("wm/src/main.rs:42:9", "Seat not found");
        assert!(report.contains("wm/src/main.rs:42:9"), "{report}");
        assert!(report.contains("Seat not found"), "{report}");
    }

    #[test]
    fn wenum_label_prints_a_known_value_without_the_wrapper() {
        use crate::compositor::river::river_libinput_device_v1::TapState;
        assert_eq!(
            wenum_label(wayland_client::WEnum::Value(TapState::Disabled)),
            "Disabled"
        );
    }

    /// An unrecognized value means river and this binary disagree about the
    /// protocol, which has to stay visible rather than being smoothed into
    /// something that looks like a real setting.
    #[test]
    fn wenum_label_names_an_unknown_value_and_keeps_the_raw_number() {
        use crate::compositor::river::river_libinput_device_v1::TapState;
        assert_eq!(
            wenum_label::<TapState>(wayland_client::WEnum::Unknown(7)),
            "unknown (7)"
        );
    }
}
