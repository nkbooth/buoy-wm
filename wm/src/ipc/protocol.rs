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

//! Wire types and JSON (de)serialization for the IPC protocol (ADR-007).
//! `wm-core` stays protocol-agnostic — every `serde`/JSON-facing type lives
//! here, not in `wm_core`. Requests are newline-delimited JSON, one object
//! per line, in both directions; see the story's Technical notes "Wire
//! protocol" for the exact schema this module implements.

use serde::{Deserialize, Serialize};

/// A parsed client request. Deliberately **not**
/// `#[serde(deny_unknown_fields)]` — tolerating unknown extra fields is a
/// forward-compatibility no-op today and costs nothing; rejecting them
/// would just be one more way to fail closed for no behavioral benefit
/// this story needs.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Request {
    GetState,
    ToggleTag { view_id: u64, tag_id: u8 },
    CreateTag { name: String },
    SwitchTag { output_id: u64, tag_id: u8 },
}

/// A tag as it appears on the wire: `id` and `name`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TagDto {
    pub id: u8,
    pub name: String,
}

/// A view as it appears on the wire: `id`, `app_id`, and its tag
/// membership (`tags`, a list of tag ids).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ViewDto {
    pub id: u64,
    pub app_id: String,
    pub tags: Vec<u8>,
}

/// An output as it appears on the wire: `id` and its currently-displayed
/// tag, if any.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutputDto {
    pub id: u64,
    pub current_tag: Option<u8>,
}

/// A response sent back to the client, one JSON line per response.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Response {
    State {
        tags: Vec<TagDto>,
        views: Vec<ViewDto>,
        outputs: Vec<OutputDto>,
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

/// Errors returned by [`parse_request`]. Never a panic — every failure
/// mode a malformed/malicious client can trigger (invalid UTF-8, invalid
/// JSON, wrong shape, unknown `type`, missing/wrong-typed fields,
/// out-of-range values) is represented here.
///
/// Deliberately implements neither `Display` nor `std::error::Error`, and
/// this is the one exception to audit finding F-04's "every error type gets
/// a `Display`". A `Display` is an invitation to write `{e}`, and `{e}` on
/// [`ParseError::InvalidJson`] is a log-injection primitive — see that
/// variant. Rendering goes through `server::describe_parse_error`, which
/// escapes and length-caps, and which cannot be bypassed by a call site
/// that reaches for the obvious formatter.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    InvalidUtf8,
    /// Carries `serde_json`'s own message, which **quotes the offending
    /// input verbatim** (`unknown variant \`delete-everything\`, expected
    /// one of ...`). It is therefore peer-controlled text: log it with
    /// `{:?}` and a length cap, never with `{}`. `Display` would let
    /// anything that can reach the socket write newlines and forged log
    /// prefixes straight into the journal — see
    /// `server::describe_parse_error`, which is the only place in this
    /// crate that renders it.
    InvalidJson(String),
}

/// Parses one client request line (already newline-stripped) into a
/// [`Request`]. Validates UTF-8 before handing off to `serde_json`, so an
/// invalid-UTF-8 byte sequence is rejected here rather than reaching the
/// JSON parser at all. Never panics (NFR2) — every failure path returns
/// `Err`.
pub fn parse_request(bytes: &[u8]) -> Result<Request, ParseError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ParseError::InvalidUtf8)?;
    serde_json::from_str(text).map_err(|e| ParseError::InvalidJson(e.to_string()))
}

/// Serializes a [`Response`] to its one-line JSON wire form. `Response` is
/// a fully-controlled, always-serializable type — no NaN floats, no
/// non-string map keys — so the one `.expect()` here is safe-by-
/// construction, not caller-facing (same precedent as `cycle_focus`'s
/// internal unwraps, Story 1.5/1.7).
pub fn serialize_response(response: &Response) -> String {
    serde_json::to_string(response)
        .expect("Response is a fully-controlled, always-serializable type")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_get_state() {
        assert_eq!(
            parse_request(br#"{"type":"get-state"}"#),
            Ok(Request::GetState)
        );
    }

    #[test]
    fn parses_toggle_tag() {
        assert_eq!(
            parse_request(br#"{"type":"toggle-tag","view_id":3,"tag_id":2}"#),
            Ok(Request::ToggleTag {
                view_id: 3,
                tag_id: 2
            })
        );
    }

    #[test]
    fn parses_create_tag() {
        assert_eq!(
            parse_request(br#"{"type":"create-tag","name":"web"}"#),
            Ok(Request::CreateTag { name: "web".into() })
        );
    }

    #[test]
    fn parses_switch_tag() {
        assert_eq!(
            parse_request(br#"{"type":"switch-tag","output_id":0,"tag_id":2}"#),
            Ok(Request::SwitchTag {
                output_id: 0,
                tag_id: 2
            })
        );
    }

    #[test]
    fn parse_request_rejects_empty_string() {
        assert!(parse_request(b"").is_err());
    }

    #[test]
    fn parse_request_rejects_non_json_garbage() {
        // The *variant*, not just `is_err()`: this test and the invalid-utf8
        // one below would otherwise both keep passing if utf-8 rejection
        // started reporting `InvalidJson`, collapsing a distinction this
        // module documents deliberately and that `describe_parse_error`
        // renders differently (audit finding T-05).
        assert!(matches!(
            parse_request(b"not json at all"),
            Err(ParseError::InvalidJson(_))
        ));
    }

    #[test]
    fn parse_request_rejects_valid_json_that_is_not_an_object() {
        assert!(parse_request(b"[1,2,3]").is_err());
        assert!(parse_request(b"42").is_err());
        assert!(parse_request(b"\"a string\"").is_err());
    }

    #[test]
    fn parse_request_rejects_object_with_unknown_type() {
        assert!(parse_request(br#"{"type":"delete-everything"}"#).is_err());
    }

    #[test]
    fn parse_request_rejects_object_missing_type_field() {
        assert!(parse_request(b"{}").is_err());
    }

    #[test]
    fn parse_request_rejects_toggle_tag_missing_required_fields() {
        assert!(parse_request(br#"{"type":"toggle-tag"}"#).is_err());
        assert!(parse_request(br#"{"type":"toggle-tag","view_id":3}"#).is_err());
    }

    #[test]
    fn parse_request_rejects_wrong_field_types() {
        assert!(
            parse_request(br#"{"type":"toggle-tag","view_id":"not-a-number","tag_id":2}"#).is_err()
        );
    }

    #[test]
    fn parse_request_rejects_tag_id_out_of_u8_range() {
        assert!(parse_request(br#"{"type":"switch-tag","output_id":0,"tag_id":999}"#).is_err());
    }

    /// A deeply-nested, irrelevant extra field must never panic the parser.
    /// Since `Request` deliberately does not use `deny_unknown_fields`
    /// (see 3.3's rationale), an unrecognized `extra` field is tolerated,
    /// not rejected — this proves that tolerance doesn't come at the cost
    /// of blowing up on unusually-shaped extra JSON.
    #[test]
    fn parse_request_tolerates_deeply_nested_unknown_fields() {
        // Unlike malformed-shape/unknown-type inputs, an unrecognized
        // "extra" field (regardless of how deeply nested) is not itself
        // malformed — serde ignores unknown fields by default, so this
        // must still parse successfully rather than being rejected.
        let result = parse_request(br#"{"type":"get-state","extra":[[[[[1]]]]]}"#);
        assert_eq!(result, Ok(Request::GetState));
    }

    #[test]
    fn parse_request_rejects_invalid_utf8_bytes() {
        let bytes: &[u8] = &[0xFF, 0xFE];
        assert_eq!(parse_request(bytes), Err(ParseError::InvalidUtf8));
    }

    #[test]
    fn serializes_state_response_shape() {
        let response = Response::State {
            tags: vec![TagDto {
                id: 0,
                name: "web".into(),
            }],
            views: vec![ViewDto {
                id: 3,
                app_id: "foot".into(),
                tags: vec![0, 2],
            }],
            outputs: vec![OutputDto {
                id: 0,
                current_tag: Some(0),
            }],
            focused_view: Some(3),
        };
        assert_eq!(
            serialize_response(&response),
            r#"{"type":"state","tags":[{"id":0,"name":"web"}],"views":[{"id":3,"app_id":"foot","tags":[0,2]}],"outputs":[{"id":0,"current_tag":0}],"focused_view":3}"#
        );
    }

    #[test]
    fn serializes_ok_response() {
        assert_eq!(serialize_response(&Response::Ok), r#"{"type":"ok"}"#);
    }

    #[test]
    fn serializes_tag_created_response() {
        assert_eq!(
            serialize_response(&Response::TagCreated { tag_id: 5 }),
            r#"{"type":"tag-created","tag_id":5}"#
        );
    }

    #[test]
    fn serializes_error_response() {
        assert_eq!(
            serialize_response(&Response::Error {
                message: "unknown tag".into()
            }),
            r#"{"type":"error","message":"unknown tag"}"#
        );
    }
}
