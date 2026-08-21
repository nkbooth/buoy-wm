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

//! One test per message shape, joining the three hand-written declarations
//! of the JSON Lines IPC contract.
//!
//! The contract is declared independently in `wm/src/ipc/protocol.rs`,
//! `buoy-tag-picker/src/wire.rs` and `buoy-status-bar/src/wire.rs`, with
//! three deliberately different field sets — the satellites' narrower shapes
//! are a real property worth keeping (`buoy-status-bar` *cannot* send a
//! mutation, because its `Request` has one variant), which is why the audit
//! adjudicated against merging them into a shared crate. The cost of keeping
//! three declarations is that renaming a field on one side compiles cleanly
//! across the whole workspace, passes CI, and produces a picker that fails
//! `parse_response` on every `get-state` — `Super+A` dead, with no compile
//! error and no failing test, because both ends' unit tests use hand-written
//! JSON fixtures that drift together with the types (audit finding J-01).
//!
//! So the insurance is this: every shape, serialized by the side that sends
//! it and parsed by the side that receives it, with no JSON literal shared
//! between the two. One of roughly ten shapes had such a test before.
//!
//! Requests travel satellite → `wm`; responses travel `wm` → satellite.
//! Nothing here asserts a *byte string*: that would only pin serde's
//! formatting. What is pinned is that the two sides still agree about the
//! same value.

use buoy_wm::ipc::protocol;

use buoy_status_bar::wire as bar_wire;
use buoy_tag_picker::wire as picker_wire;

/// Round-trips one request through the picker's serializer and `wm`'s
/// parser.
fn picker_request_reaches_wm_as(sent: &picker_wire::Request, expected: protocol::Request) {
    let line = picker_wire::serialize_request(sent);
    assert_eq!(
        protocol::parse_request(line.as_bytes()),
        Ok(expected),
        "the picker sent {line} and the wm read something else"
    );
}

/// Round-trips one response through `wm`'s serializer and the picker's
/// parser.
fn wm_response_reaches_picker_as(sent: &protocol::Response, expected: picker_wire::Response) {
    let line = protocol::serialize_response(sent);
    assert_eq!(
        picker_wire::parse_response(line.as_bytes()),
        Ok(expected),
        "the wm sent {line} and the picker read something else"
    );
}

/// Round-trips one response through `wm`'s serializer and the status bar's
/// parser.
fn wm_response_reaches_bar_as(sent: &protocol::Response, expected: bar_wire::Response) {
    let line = protocol::serialize_response(sent);
    assert_eq!(
        bar_wire::parse_response(line.as_bytes()),
        Ok(expected),
        "the wm sent {line} and the status bar read something else"
    );
}

#[test]
fn get_state_from_the_picker_is_the_wms_get_state() {
    picker_request_reaches_wm_as(&picker_wire::Request::GetState, protocol::Request::GetState);
}

#[test]
fn get_state_from_the_status_bar_is_the_wms_get_state() {
    // The status bar's `Request` has exactly one variant, so this is its
    // entire outbound surface.
    let line = bar_wire::serialize_request(&bar_wire::Request::GetState);
    assert_eq!(
        protocol::parse_request(line.as_bytes()),
        Ok(protocol::Request::GetState)
    );
}

#[test]
fn toggle_tag_carries_the_view_and_tag_it_was_given() {
    picker_request_reaches_wm_as(
        &picker_wire::Request::ToggleTag {
            view_id: 41,
            tag_id: 7,
        },
        protocol::Request::ToggleTag {
            view_id: 41,
            tag_id: 7,
        },
    );
}

#[test]
fn create_tag_carries_the_name_it_was_given() {
    // A name with a quote and a backslash in it: the one field on any
    // request that is free-form user text, so it is the one that can break
    // framing if either side ever stops going through serde.
    picker_request_reaches_wm_as(
        &picker_wire::Request::CreateTag {
            name: r#"say "hi"\ok"#.to_string(),
        },
        protocol::Request::CreateTag {
            name: r#"say "hi"\ok"#.to_string(),
        },
    );
}

#[test]
fn switch_tag_carries_the_output_and_tag_it_was_given() {
    picker_request_reaches_wm_as(
        &picker_wire::Request::SwitchTag {
            output_id: 3,
            tag_id: 2,
        },
        protocol::Request::SwitchTag {
            output_id: 3,
            tag_id: 2,
        },
    );
}

/// The one response with structure in it, and the one whose field names the
/// satellites disagree about on purpose: the picker models `views` and
/// `focused_view` and not `outputs`; the status bar models `outputs` and
/// neither of the others.
fn a_populated_state() -> protocol::Response {
    protocol::Response::State {
        tags: vec![
            protocol::TagDto {
                id: 0,
                name: "web".to_string(),
            },
            protocol::TagDto {
                id: 1,
                name: "code".to_string(),
            },
        ],
        views: vec![protocol::ViewDto {
            id: 9,
            app_id: "foot".to_string(),
            tags: vec![0, 1],
        }],
        outputs: vec![protocol::OutputDto {
            id: 4,
            current_tag: Some(1),
        }],
        focused_view: Some(9),
    }
}

#[test]
fn state_reaches_the_picker_with_its_tags_views_and_focus() {
    wm_response_reaches_picker_as(
        &a_populated_state(),
        picker_wire::Response::State {
            tags: vec![
                picker_wire::TagDto {
                    id: 0,
                    name: "web".to_string(),
                },
                picker_wire::TagDto {
                    id: 1,
                    name: "code".to_string(),
                },
            ],
            views: vec![picker_wire::ViewDto {
                id: 9,
                tags: vec![0, 1],
            }],
            focused_view: Some(9),
        },
    );
}

#[test]
fn state_reaches_the_status_bar_with_its_tags_and_outputs() {
    wm_response_reaches_bar_as(
        &a_populated_state(),
        bar_wire::Response::State {
            tags: vec![
                bar_wire::TagDto {
                    id: 0,
                    name: "web".to_string(),
                },
                bar_wire::TagDto {
                    id: 1,
                    name: "code".to_string(),
                },
            ],
            outputs: vec![bar_wire::OutputDto {
                id: 4,
                current_tag: Some(1),
            }],
        },
    );
}

#[test]
fn an_empty_state_reaches_both_satellites_as_an_empty_state() {
    // `focused_view: None` is the shape at login before anything is mapped,
    // and `null` versus a missing key is exactly the kind of difference a
    // hand-written fixture on one side hides.
    let empty = protocol::Response::State {
        tags: vec![],
        views: vec![],
        outputs: vec![],
        focused_view: None,
    };
    wm_response_reaches_picker_as(
        &empty,
        picker_wire::Response::State {
            tags: vec![],
            views: vec![],
            focused_view: None,
        },
    );
    wm_response_reaches_bar_as(
        &empty,
        bar_wire::Response::State {
            tags: vec![],
            outputs: vec![],
        },
    );
}

#[test]
fn ok_reaches_both_satellites_as_ok() {
    wm_response_reaches_picker_as(&protocol::Response::Ok, picker_wire::Response::Ok);
    wm_response_reaches_bar_as(&protocol::Response::Ok, bar_wire::Response::Ok);
}

#[test]
fn tag_created_reaches_both_satellites_with_its_new_id() {
    wm_response_reaches_picker_as(
        &protocol::Response::TagCreated { tag_id: 63 },
        picker_wire::Response::TagCreated { tag_id: 63 },
    );
    wm_response_reaches_bar_as(
        &protocol::Response::TagCreated { tag_id: 63 },
        bar_wire::Response::TagCreated { tag_id: 63 },
    );
}

#[test]
fn error_reaches_both_satellites_with_its_message_intact() {
    // The picker compares this string by equality to decide whether to
    // reopen with the rejected name restored, so a change to it here is a
    // behaviour change there (audit finding E-06).
    let message = buoy_tag_picker::picker::REJECTION_MESSAGE.to_string();
    wm_response_reaches_picker_as(
        &protocol::Response::Error {
            message: message.clone(),
        },
        picker_wire::Response::Error {
            message: message.clone(),
        },
    );
    wm_response_reaches_bar_as(
        &protocol::Response::Error {
            message: message.clone(),
        },
        bar_wire::Response::Error { message },
    );
}
