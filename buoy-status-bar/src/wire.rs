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

//! `buoy-status-bar`'s own minimal mirror of the wire shapes it needs from
//! `wm/src/ipc/protocol.rs` (ADR-007), following `buoy-tag-picker/src/wire.rs`'s
//! exact existing shape and doc-comment convention (Story 2.5 Task 1.3).
//!
//! **Why this is a third copy, not a shared crate (the three-strike-DRY
//! decision, recorded here in full so a future reader hits the same
//! reasoning at the same place as the story file).** `buoy-tag-picker/src/
//! wire.rs`'s own doc comment already named this exact moment: "A shared
//! crate is deferred to whichever story first gives `buoy-status-bar` (Story
//! 2.5) the same need, a genuine third consumer." `buoy-status-bar` is that
//! third independent occurrence (`wm/src/ipc/protocol.rs`,
//! `buoy-tag-picker/src/wire.rs`, and this module). The three-strike rule
//! exists to bound *future* maintenance drift across call sites that keep
//! evolving independently — but this is the last story in Epic 2 and in
//! the entire currently planned project, so there is no fourth consumer
//! this rule is protecting against, ever, under the current plan.
//! Extracting a shared `buoy-wm-protocol`/`buoy-wm-ipc-client` crate now,
//! purely to satisfy the letter of a rule whose entire purpose is
//! amortizing future drift, with no future call site left to drift, would
//! be premature-generality YAGNI — abstracting for an audience of zero
//! remaining callers. If a fourth consumer is ever added to this project
//! in the future, that is the correct point to finally extract a shared
//! crate; this story explicitly does not do so.
//!
//! This client only ever sends `get-state` — it never mutates, so
//! `Request` has exactly one variant (no `ToggleTag`/`CreateTag`/
//! `SwitchTag`). `Response::State` is narrower than `wm`'s own `Response`
//! too: `views`/`focused_view` are omitted, since this client has no use
//! for them (serde's default unknown-field tolerance means the server can
//! keep sending them without this client having to model them) — but,
//! unlike `buoy-tag-picker`, `outputs` **is** modeled, since `buoy-status-bar` is the
//! first client to need per-output current-tag data at all.
//! `Response::Ok`/`TagCreated` are kept only so `parse_response_rejects_*`
//! negative tests and any defensive "unexpected response" handling in
//! `main.rs` have real variants to pattern-match against — never sent by
//! this client, never expected as a real reply to `get-state`.

use serde::{Deserialize, Serialize};

/// A request this client can send. `GetState` is the only variant — this
/// client never mutates (see module doc comment).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Request {
    GetState,
}

/// A tag as it appears on the wire: `id` and `name`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TagDto {
    pub id: u8,
    pub name: String,
}

/// An output as it appears on the wire: `id` and its currently-displayed
/// tag, if any. The first client-side mirror to need this DTO at all
/// (`buoy-tag-picker` never needed per-output state).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OutputDto {
    pub id: u64,
    pub current_tag: Option<u8>,
}

/// A response this client can receive. Narrower than `wm`'s own `Response`
/// enum — no `views`/`focused_view` fields on `State` (module doc
/// comment), but `outputs` is present since this client's whole purpose is
/// reading per-output current-tag state.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Response {
    State {
        tags: Vec<TagDto>,
        outputs: Vec<OutputDto>,
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
    fn parses_state_response_with_tags_and_outputs_ignoring_unmodeled_fields() {
        let line = br#"{"type":"state","tags":[{"id":0,"name":"web"}],"views":[{"id":3,"app_id":"foot","tags":[0,2]}],"outputs":[{"id":0,"current_tag":0}],"focused_view":3}"#;
        assert_eq!(
            parse_response(line),
            Ok(Response::State {
                tags: vec![TagDto {
                    id: 0,
                    name: "web".into()
                }],
                outputs: vec![OutputDto {
                    id: 0,
                    current_tag: Some(0),
                }],
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
        assert!(parse_response(br#"{"type":"delete-everything"}"#).is_err());
    }

    #[test]
    fn parse_response_rejects_object_missing_type_field() {
        assert!(parse_response(b"{}").is_err());
    }
}
