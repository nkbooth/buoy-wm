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

//! `buoy-tag-picker`'s own minimal mirror of the wire shapes it needs from
//! `wm/src/ipc/protocol.rs` (ADR-007). This is a deliberate, accepted
//! duplication, not an oversight: this project's own three-strike DRY rule
//! extracts shared code after ~3 independent occurrences, and right now
//! there are exactly two (`wm`'s and this one) — one below the threshold. A
//! shared crate is deferred to whichever story first gives `buoy-status-bar`
//! (Story 2.5) the same need, a genuine third consumer. As of Story 2.3,
//! `buoy-tag-picker` sends `get-state`/`toggle-tag`/`create-tag`/`switch-tag` and
//! understands the `state`/`ok`/`tag-created`/`error` responses those
//! produce (Story 2.4 adds `switch-tag`, the switch-mode picker's sole
//! mutation). `State`/`ViewDto` still omit `outputs`/`app_id` — fields this
//! client has no use for, tolerated as unknown fields by serde's default
//! leniency.

use serde::{Deserialize, Serialize};

/// A request this client can send. Mirrors `wm`'s own `Request`
/// (`wm/src/ipc/protocol.rs`) shape-for-shape for every variant this client
/// uses — `switch-tag` (Story 2.4) is the switch-mode picker's sole
/// mutation, sent once per single-shot invocation.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Request {
    GetState,
    ToggleTag { view_id: u64, tag_id: u8 },
    CreateTag { name: String },
    SwitchTag { output_id: u64, tag_id: u8 },
}

/// A tag as it appears on the wire: `id` and `name`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TagDto {
    pub id: u8,
    pub name: String,
}

/// A view as it appears on the wire, as much of it as this client needs:
/// `id` and its tag membership. Deliberately no `app_id` field — unused by
/// this client, and serde's default unknown-field tolerance means the
/// server can keep sending it without this client having to model it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ViewDto {
    pub id: u64,
    pub tags: Vec<u8>,
}

/// A response this client can receive. Narrower than `wm`'s own `Response`
/// enum — no `outputs` field on `State`, since `switch-tag`'s `output_id`
/// argument (Story 2.4) comes from `wm`'s own CLI-argument spawn, never
/// from parsing this client's own `get-state` snapshot (Task 1.2). No new
/// `Response` variant is needed for `switch-tag` either — it only ever
/// produces `Ok`/`Error`, both already modeled below.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Response {
    State {
        tags: Vec<TagDto>,
        views: Vec<ViewDto>,
        focused_view: Option<u64>,
    },
    Ok,
    TagCreated {
        tag_id: u8,
    },
    Error {
        message: String,
    },
}

/// Errors returned by [`parse_response`]. Never a panic — mirrors `wm`'s
/// own `ParseError` (`wm/src/ipc/protocol.rs`).
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    InvalidUtf8,
    InvalidJson(String),
}

/// Serializes a [`Request`] to its one-line JSON wire form. `Request` is a
/// fully-controlled, always-serializable type, so the one `.expect()` here
/// is safe-by-construction, not caller-facing (same precedent as `wm`'s
/// own `serialize_response`).
pub fn serialize_request(request: &Request) -> String {
    serde_json::to_string(request).expect("Request is a fully-controlled, always-serializable type")
}

/// Parses one response line (already newline-stripped) into a [`Response`].
/// Validates UTF-8 before handing off to `serde_json`, mirroring `wm`'s own
/// `parse_request` exactly. Never panics.
pub fn parse_response(bytes: &[u8]) -> Result<Response, ParseError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ParseError::InvalidUtf8)?;
    serde_json::from_str(text).map_err(|e| ParseError::InvalidJson(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_get_state_request() {
        assert_eq!(
            serialize_request(&Request::GetState),
            r#"{"type":"get-state"}"#
        );
    }

    #[test]
    fn serializes_toggle_tag_request() {
        assert_eq!(
            serialize_request(&Request::ToggleTag {
                view_id: 3,
                tag_id: 2
            }),
            r#"{"type":"toggle-tag","view_id":3,"tag_id":2}"#
        );
    }

    #[test]
    fn serializes_create_tag_request() {
        assert_eq!(
            serialize_request(&Request::CreateTag { name: "web".into() }),
            r#"{"type":"create-tag","name":"web"}"#
        );
    }

    #[test]
    fn serializes_switch_tag_request() {
        // Byte-for-byte matching `wm`'s own `protocol.rs`'s
        // `parses_switch_tag` fixture, so the two ends are provably
        // wire-compatible.
        assert_eq!(
            serialize_request(&Request::SwitchTag {
                output_id: 0,
                tag_id: 2
            }),
            r#"{"type":"switch-tag","output_id":0,"tag_id":2}"#
        );
    }

    #[test]
    fn parses_tag_created_response() {
        assert_eq!(
            parse_response(br#"{"type":"tag-created","tag_id":5}"#),
            Ok(Response::TagCreated { tag_id: 5 })
        );
    }

    #[test]
    fn parses_state_response_ignoring_unmodeled_fields() {
        let line = br#"{"type":"state","tags":[{"id":0,"name":"web"}],"views":[{"id":3,"app_id":"foot","tags":[0,2]}],"outputs":[{"id":0,"current_tag":0}],"focused_view":3}"#;
        assert_eq!(
            parse_response(line),
            Ok(Response::State {
                tags: vec![TagDto {
                    id: 0,
                    name: "web".into()
                }],
                views: vec![ViewDto {
                    id: 3,
                    tags: vec![0, 2]
                }],
                focused_view: Some(3),
            })
        );
    }

    #[test]
    fn parses_ok_response() {
        assert_eq!(parse_response(br#"{"type":"ok"}"#), Ok(Response::Ok));
    }

    #[test]
    fn parses_error_response() {
        assert_eq!(
            parse_response(br#"{"type":"error","message":"unknown view"}"#),
            Ok(Response::Error {
                message: "unknown view".into()
            })
        );
    }

    #[test]
    fn parse_response_rejects_non_json_garbage() {
        assert!(parse_response(b"not json at all").is_err());
    }

    #[test]
    fn parse_response_rejects_invalid_utf8_bytes() {
        let bytes: &[u8] = &[0xFF, 0xFE];
        assert!(parse_response(bytes).is_err());
    }

    #[test]
    fn parse_response_rejects_unknown_type() {
        // Story 2.3: `"tag-created"` used to be this test's "real `wm`
        // response shape this client just doesn't model" fixture. Once
        // `Response::TagCreated` is added below, that literal becomes a
        // genuinely modeled type, so this test would silently stop testing
        // what it claims to — swapped to a still-unmodeled, clearly bogus
        // type instead (same fixture-swap precedent as `wm`'s own
        // `protocol.rs` tests use for this exact kind of case).
        assert!(parse_response(br#"{"type":"delete-everything"}"#).is_err());
    }

    #[test]
    fn parse_response_rejects_object_missing_type_field() {
        assert!(parse_response(b"{}").is_err());
    }
}
