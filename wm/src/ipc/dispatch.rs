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

//! The pure `Request` → `wm-core`-call → `Response` mapping. Deliberately
//! factored out of `server.rs` so it can be reviewed and tested with zero
//! socket/thread machinery in the loop — and, per this note, zero real
//! process spawning either — and so it is trivially diffable against Story
//! 1.7's keybind call sites for the "same functions, no duplicate logic
//! path" AC.
//!
//! Code-review follow-up (Story 2.4): `Request::SwitchTag`'s arm claims
//! that tag's pinned-terminal spawn slot (`WmCore::claim_pinned_terminal_
//! spawn`, a pure state mutation, fully unit-tested here) but must NOT
//! itself call `crate::spawn_pinned_terminal` — doing so made
//! [`handle_request`] launch a real `foot`/`zellij` `Command::spawn()` as
//! a side effect of `cargo test --workspace`, contradicting this module's
//! own "zero socket/thread machinery" claim. Instead, [`handle_request`]
//! returns the pending session name as data (the second element of its
//! return tuple) for its caller to act on. `server.rs`'s
//! `handle_connection_inner` is the one real caller and performs the
//! actual `crate::spawn_pinned_terminal` call, after the response has
//! already been written back to the client and the `wm-core` mutex
//! released — untested I/O glue, the same carve-out class as
//! `Action::SpawnFoot`, `Action::OpenTagPicker`, and
//! `ensure_pinned_terminal_spawned` itself.

use crate::ipc::protocol::{OutputDto, Request, Response, TagDto, ViewDto};
use crate::wm_core::MAX_TAGS;
use crate::wm_core::ids::{OutputId, TagId, ViewId};
use crate::wm_core::state::{WmCore, WmCoreError, WmCoreSnapshot};

impl From<WmCoreSnapshot> for Response {
    fn from(snapshot: WmCoreSnapshot) -> Self {
        Response::State {
            tags: snapshot
                .tags
                .into_iter()
                .map(|t| TagDto {
                    id: t.id.0,
                    name: t.name,
                })
                .collect(),
            views: snapshot
                .views
                .into_iter()
                .map(|v| ViewDto {
                    id: v.id.0,
                    app_id: v.app_id,
                    tags: v.tags.into_iter().map(|t| t.0).collect(),
                })
                .collect(),
            outputs: snapshot
                .outputs
                .into_iter()
                .map(|o| OutputDto {
                    id: o.id.0,
                    current_tag: o.current_tag.map(|t| t.0),
                })
                .collect(),
            focused_view: snapshot.focused_view.map(|v| v.0),
        }
    }
}

/// Maps a `WmCoreError` to the exact literal message string sent over the
/// wire. `create-tag`'s registry-cap message ("tag limit reached (64)") is
/// the single place that string is defined — Story 2.3's picker renders it
/// verbatim, not a duplicated literal.
///
/// The cap in that message is interpolated from
/// [`MAX_TAGS`](crate::wm_core::MAX_TAGS) rather than typed, so the number
/// the user is shown cannot disagree with the number actually enforced
/// (audit finding J-06). `buoy-tag-picker` cannot import from `wm` and so
/// keeps its own literal copy, which
/// `tag_limit_message_matches_the_pickers_hardcoded_copy` pins.
fn describe_wm_core_error(e: WmCoreError) -> String {
    match e {
        WmCoreError::UnknownView => "unknown view".to_string(),
        WmCoreError::UnknownTag => "unknown tag".to_string(),
        WmCoreError::UnknownOutput => "unknown output".to_string(),
        WmCoreError::TagLimitReached => format!("tag limit reached ({MAX_TAGS})"),
    }
}

/// A pinned-terminal spawn that [`handle_request`] has claimed but
/// deliberately not performed: the tag whose claim it is, and the zellij
/// session name to spawn with.
///
/// Carries the [`TagId`] as well as the name because the caller has to be
/// able to release the claim if the spawn fails, and by then the request
/// that identified the tag is gone (audit finding D-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPinnedSpawn {
    pub tag_id: TagId,
    pub session_name: String,
}

/// The single, pure, unit-tested mapping every socket connection calls
/// into: dispatches a parsed [`Request`] to the matching `wm-core` call
/// (the same functions Story 1.7's raw keybinds already exercise) and
/// returns the [`Response`] to send back, plus an optional "pending pinned-
/// terminal spawn" session name. Never panics (NFR2) — every `wm-core`-
/// level error becomes `Response::Error`, not a propagated panic.
///
/// `Request::SwitchTag`'s arm additionally claims that tag's pinned-
/// terminal spawn slot (Story 2.4 Task 2). The claim itself is a pure
/// `wm-core` state mutation and happens right here, but this function
/// deliberately does NOT call `crate::spawn_pinned_terminal` itself — the
/// second tuple element carries the [`PendingPinnedSpawn`] that needs
/// spawning back to the caller instead, so this function stays free of real process
/// spawns and safe to exercise from unit tests with no side effects beyond
/// `wm_core`'s own state (see this module's doc comment for the full
/// rationale). `None` means no spawn is needed: either the request wasn't
/// `SwitchTag`, the switch itself failed, or this tag's pinned terminal was
/// already spawned earlier this session.
pub fn handle_request(
    wm_core: &mut WmCore,
    request: Request,
) -> (Response, Option<PendingPinnedSpawn>) {
    match request {
        Request::GetState => (Response::from(wm_core.snapshot()), None),
        Request::ToggleTag { view_id, tag_id } => {
            match wm_core.toggle_view_tag(ViewId(view_id), TagId(tag_id)) {
                Ok(()) => (Response::Ok, None),
                Err(e) => (
                    Response::Error {
                        message: describe_wm_core_error(e),
                    },
                    None,
                ),
            }
        }
        Request::CreateTag { name } => match wm_core.create_tag(name) {
            Ok(id) => (Response::TagCreated { tag_id: id.0 }, None),
            Err(e) => (
                Response::Error {
                    message: describe_wm_core_error(e),
                },
                None,
            ),
        },
        Request::SwitchTag { output_id, tag_id } => {
            match wm_core.switch_tag(OutputId(output_id), TagId(tag_id)) {
                Ok(()) => {
                    // Story 2.4 Task 2: `switch-tag` is now reachable by a
                    // real user action (the picker's switch mode) for the
                    // first time, so this arm must guarantee the same
                    // pinned-terminal lazy-spawn `Action::TagCycle`'s
                    // keybind path already guarantees via `manage_seats` →
                    // `ensure_pinned_terminal_spawned`. `tag_id` was just
                    // proven valid by the successful `switch_tag` call
                    // above, so a `claim_pinned_terminal_spawn` `Err` here
                    // is unreachable in practice; if it somehow occurred,
                    // log and continue rather than turning an already-
                    // successful switch into a lie of a failure response.
                    let pending_spawn = match wm_core.claim_pinned_terminal_spawn(TagId(tag_id)) {
                        Ok(Some(session_name)) => Some(PendingPinnedSpawn {
                            tag_id: TagId(tag_id),
                            session_name,
                        }),
                        Ok(None) => None,
                        Err(e) => {
                            eprintln!(
                                "Failed to check pinned-terminal spawn state for tag {tag_id}: {e:?}"
                            );
                            None
                        }
                    };
                    (Response::Ok, pending_spawn)
                }
                Err(e) => (
                    Response::Error {
                        message: describe_wm_core_error(e),
                    },
                    None,
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::{Request, Response};
    use crate::wm_core::ids::{OutputId, TagId, ViewId};
    use crate::wm_core::state::WmCore;

    #[test]
    fn get_state_returns_state_response_matching_snapshot() {
        let mut core = WmCore::new();
        let view_id = core.register_view("foot");
        let tag_id = core.create_tag("web").unwrap();
        core.toggle_view_tag(view_id, tag_id).unwrap();
        let output_id = core.register_output();
        core.switch_tag(output_id, tag_id).unwrap();
        core.set_focus(view_id).unwrap();

        let snapshot = core.snapshot();
        let (response, pending_spawn) = handle_request(&mut core, Request::GetState);
        assert_eq!(pending_spawn, None);

        match response {
            Response::State {
                tags,
                views,
                outputs,
                focused_view,
            } => {
                assert_eq!(tags.len(), snapshot.tags.len());
                for (dto, snap) in tags.iter().zip(snapshot.tags.iter()) {
                    assert_eq!(dto.id, snap.id.0);
                    assert_eq!(dto.name, snap.name);
                }
                assert_eq!(views.len(), snapshot.views.len());
                for (dto, snap) in views.iter().zip(snapshot.views.iter()) {
                    assert_eq!(dto.id, snap.id.0);
                    assert_eq!(dto.app_id, snap.app_id);
                    assert_eq!(dto.tags, snap.tags.iter().map(|t| t.0).collect::<Vec<_>>());
                }
                assert_eq!(outputs.len(), snapshot.outputs.len());
                for (dto, snap) in outputs.iter().zip(snapshot.outputs.iter()) {
                    assert_eq!(dto.id, snap.id.0);
                    assert_eq!(dto.current_tag, snap.current_tag.map(|t| t.0));
                }
                assert_eq!(focused_view, snapshot.focused_view.map(|v| v.0));
            }
            other => panic!("expected Response::State, got {other:?}"),
        }
    }

    #[test]
    fn toggle_tag_calls_wm_core_toggle_view_tag_and_returns_ok() {
        let mut core = WmCore::new();
        let view_id = core.register_view("foot");
        let tag_id = core.create_tag("web").unwrap();
        let (response, pending_spawn) = handle_request(
            &mut core,
            Request::ToggleTag {
                view_id: view_id.0,
                tag_id: tag_id.0,
            },
        );
        assert_eq!(response, Response::Ok);
        assert_eq!(pending_spawn, None);
        assert_eq!(core.snapshot().views[0].tags, vec![tag_id]);
    }

    #[test]
    fn toggle_tag_unknown_view_returns_error_not_panic() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("web").unwrap();
        let (response, pending_spawn) = handle_request(
            &mut core,
            Request::ToggleTag {
                view_id: 9999,
                tag_id: tag_id.0,
            },
        );
        assert_eq!(
            response,
            Response::Error {
                message: "unknown view".into()
            }
        );
        assert_eq!(pending_spawn, None);
    }

    #[test]
    fn toggle_tag_unknown_tag_returns_error() {
        let mut core = WmCore::new();
        let view_id = core.register_view("foot");
        let (response, pending_spawn) = handle_request(
            &mut core,
            Request::ToggleTag {
                view_id: view_id.0,
                tag_id: 63,
            },
        );
        assert_eq!(
            response,
            Response::Error {
                message: "unknown tag".into()
            }
        );
        assert_eq!(pending_spawn, None);
    }

    #[test]
    fn create_tag_calls_wm_core_create_tag_and_returns_tag_created_with_real_id() {
        let mut core = WmCore::new();
        let (response, pending_spawn) =
            handle_request(&mut core, Request::CreateTag { name: "web".into() });
        assert_eq!(pending_spawn, None);
        match response {
            Response::TagCreated { tag_id } => {
                assert!(
                    core.snapshot()
                        .tags
                        .iter()
                        .any(|t| t.id == TagId(tag_id) && t.name == "web")
                );
            }
            other => panic!("expected Response::TagCreated, got {other:?}"),
        }
    }

    #[test]
    fn create_tag_at_registry_cap_returns_the_picker_facing_error_message() {
        let mut core = WmCore::new();
        for i in 0..64 {
            core.create_tag(format!("tag{i}")).unwrap();
        }
        let (response, pending_spawn) = handle_request(
            &mut core,
            Request::CreateTag {
                name: "one-too-many".into(),
            },
        );
        assert_eq!(
            response,
            Response::Error {
                message: "tag limit reached (64)".into()
            }
        );
        assert_eq!(pending_spawn, None);
    }

    #[test]
    fn switch_tag_calls_wm_core_switch_tag_and_returns_ok() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_id = core.create_tag("web").unwrap();
        // Ignoring the pending-spawn data here deliberately: this test is
        // about the tag-switch mapping itself, not the pinned-terminal
        // side channel (covered by
        // `switch_tag_success_claims_pinned_terminal_spawn_for_the_target_tag`
        // below). Note that `handle_request` never calls
        // `crate::spawn_pinned_terminal` itself, so simply discarding this
        // value — as this test does — is sufficient to guarantee no real
        // process is spawned.
        let (response, _pending_spawn) = handle_request(
            &mut core,
            Request::SwitchTag {
                output_id: output_id.0,
                tag_id: tag_id.0,
            },
        );
        assert_eq!(response, Response::Ok);
        assert_eq!(core.snapshot().outputs[0].current_tag, Some(tag_id));
    }

    #[test]
    fn switch_tag_unknown_output_returns_error() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("web").unwrap();
        let (response, pending_spawn) = handle_request(
            &mut core,
            Request::SwitchTag {
                output_id: 9999,
                tag_id: tag_id.0,
            },
        );
        assert_eq!(
            response,
            Response::Error {
                message: "unknown output".into()
            }
        );
        assert_eq!(pending_spawn, None);
    }

    #[test]
    fn switch_tag_unknown_tag_returns_error() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let (response, pending_spawn) = handle_request(
            &mut core,
            Request::SwitchTag {
                output_id: output_id.0,
                tag_id: 63,
            },
        );
        assert_eq!(
            response,
            Response::Error {
                message: "unknown tag".into()
            }
        );
        assert_eq!(pending_spawn, None);
    }

    /// Story 2.4 Task 2 (code-review follow-up): `switch_tag`'s IPC path is
    /// the second real caller of `claim_pinned_terminal_spawn` (after
    /// `Action::TagCycle`'s keybind path) — this proves `handle_request`
    /// itself consumes the claim as a side effect of a successful switch
    /// AND surfaces the resulting session name as returned data, never by
    /// spawning a real process itself. Two independent checks: (1) the
    /// returned pending-spawn value is `Some("tag-web")`, the actual data
    /// contract `server.rs` acts on; (2) the tri-state's second call
    /// against `wm_core` directly returns `Ok(None)` once claimed
    /// (mirroring
    /// `claim_pinned_terminal_spawn_is_idempotent_returns_none_after_first_claim`'s
    /// own precedent in `wm_core::state`), proving the claim was genuinely
    /// consumed rather than merely computed and discarded.
    #[test]
    fn switch_tag_success_claims_pinned_terminal_spawn_for_the_target_tag() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_id = core.create_tag("web").unwrap();

        let (response, pending_spawn) = handle_request(
            &mut core,
            Request::SwitchTag {
                output_id: output_id.0,
                tag_id: tag_id.0,
            },
        );

        assert_eq!(response, Response::Ok);
        assert_eq!(
            pending_spawn,
            Some(PendingPinnedSpawn {
                tag_id,
                session_name: "tag-web".to_string(),
            })
        );
        assert_eq!(core.claim_pinned_terminal_spawn(tag_id), Ok(None));
    }

    /// Story 2.4 Task 2: a failed `switch_tag` (bogus `tag_id`) must never
    /// claim that tag's pinned-terminal spawn slot — claiming for a tag the
    /// switch never actually touched would be a spurious side effect on an
    /// error path, and must never produce a pending spawn either. Regression
    /// guard: the same tag's terminal remains claimable exactly once
    /// afterward.
    #[test]
    fn switch_tag_error_does_not_claim_pinned_terminal_spawn() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let bogus_tag_id = TagId(63);

        let (response, pending_spawn) = handle_request(
            &mut core,
            Request::SwitchTag {
                output_id: output_id.0,
                tag_id: bogus_tag_id.0,
            },
        );

        assert_eq!(
            response,
            Response::Error {
                message: "unknown tag".into()
            }
        );
        assert_eq!(pending_spawn, None);
        assert_eq!(
            core.claim_pinned_terminal_spawn(bogus_tag_id),
            Err(WmCoreError::UnknownTag)
        );

        // Regression guard: a real, registered tag untouched by the failed
        // switch above must still be claimable exactly once — proving the
        // error path above never claimed anything for any tag.
        let real_tag_id = core.create_tag("chat").unwrap();
        assert_eq!(
            core.claim_pinned_terminal_spawn(real_tag_id),
            Ok(Some("tag-chat".to_string()))
        );
    }

    /// Regression guard for this story's NFR2 emphasis: sweep every
    /// (valid, bogus) id combination for all three mutation variants and
    /// assert `handle_request` returns (never panics) in every case. The
    /// `(valid_output, valid_tag)` `SwitchTag` combination below is a
    /// first-time successful switch, so `handle_request` does return
    /// `Some(_)` as its pending-spawn value here — but since this test, like
    /// every other test in this module, never acts on that value (never
    /// calls `crate::spawn_pinned_terminal`), no real process is spawned by
    /// running this sweep, which is exactly the property this code-review
    /// follow-up fix restores.
    #[test]
    fn handle_request_never_panics_regardless_of_which_ids_are_bogus() {
        let mut core = WmCore::new();
        let valid_view = core.register_view("foot");
        let valid_tag = core.create_tag("web").unwrap();
        let valid_output = core.register_output();
        let bogus_view = ViewId(9999);
        let bogus_tag = TagId(63);
        let bogus_output = OutputId(9999);

        for view_id in [valid_view, bogus_view] {
            for tag_id in [valid_tag, bogus_tag] {
                let _ = handle_request(
                    &mut core,
                    Request::ToggleTag {
                        view_id: view_id.0,
                        tag_id: tag_id.0,
                    },
                );
            }
        }
        for output_id in [valid_output, bogus_output] {
            for tag_id in [valid_tag, bogus_tag] {
                let _ = handle_request(
                    &mut core,
                    Request::SwitchTag {
                        output_id: output_id.0,
                        tag_id: tag_id.0,
                    },
                );
            }
        }
        // CreateTag has no id arguments to sweep, but is included for
        // completeness of "every mutation variant never panics."
        let _ = handle_request(
            &mut core,
            Request::CreateTag {
                name: "sweep".into(),
            },
        );
    }

    /// Audit finding E-06: `buoy-tag-picker` uses this exact string as
    /// *control flow* — `buoy-tag-picker/src/main.rs` compares a
    /// `create-tag` error response against its own hand-copied
    /// `checklist::REJECTION_MESSAGE` to decide whether to reopen the
    /// picker with the rejected name restored, or to print an opaque line
    /// and give up. The two literals live in crates whose test suites never
    /// meet, so nothing else notices a reword.
    #[test]
    fn tag_limit_message_matches_the_pickers_hardcoded_copy() {
        assert_eq!(
            describe_wm_core_error(WmCoreError::TagLimitReached),
            "tag limit reached (64)",
            "buoy-tag-picker/src/picker.rs's REJECTION_MESSAGE is a \
             hand-copied duplicate of this string and compares against it by \
             equality as control flow — update it in lockstep, or the tag-cap \
             flow silently degrades from \"reopen with your name preserved\" \
             to \"generic error, start over\""
        );
    }
}
