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

//! The pure render-decision logic (Task 1.5's render-decision table, made
//! concrete). Every state `buoy-status-bar` can display for a given output, and
//! how each renders to waybar's `custom/tag` JSON line
//! (`{"text":...,"class":...}`, `return-type: "json"`):
//!
//! - [`BarLine::Normal`] — the output has a real current tag; class
//!   `normal`, text is the tag's name.
//! - [`BarLine::NoTagSelected`] — the output is registered but has never
//!   had `switch_tag` called for it (`current_tag` is `None`, the
//!   corrected "no startup default" case). Class `normal` (this is not an
//!   error), literal text `"no tag"`. A third state the UX pass never
//!   named — it specified only "normal" and "disconnected" — resolved this
//!   way rather than being dressed up as an error.
//! - [`BarLine::UnknownOutput`] — the requested output id does not appear
//!   in `get-state`'s `outputs` list at all. Folded into the disconnect
//!   family (class `disconnected`) rather than invented as a third
//!   error-styled state: the design reserved error treatment for exactly
//!   two named states, and this is not one of them.
//! - The connect/send/read-failure case (`None` passed to
//!   [`format_waybar_line`]) is *not* part of [`BarLine`] at all: it is
//!   purely a function of I/O outcome (Task 6.3's poll loop), never of any
//!   parsed `get-state` data, so it has no representation here.
//!
//! [`format_waybar_line`] is the one place a tag name (an arbitrary,
//! charset-unrestricted user-authored string, ADR-006/YAGNI) reaches JSON
//! output — it always goes through a `#[derive(Serialize)]` struct and
//! `serde_json::to_string`, never hand-formatted string interpolation, so
//! a name containing `"` or `\` can never corrupt the emitted line (Task
//! 1.6's JSON-injection-safety decision).

use crate::wire::{OutputDto, TagDto};
use serde::Serialize;

/// The four render-decision-table entries this module resolves. See the
/// module doc comment for what each renders to.
#[derive(Debug, Clone, PartialEq)]
pub enum BarLine {
    Normal(String),
    NoTagSelected,
    UnknownOutput,
}

/// Resolves the [`BarLine`] state for `output_id`, given a fresh
/// `get-state` snapshot's `outputs`/`tags` lists (Task 1.5's
/// render-decision table). `output_id` absent from `outputs` at all is
/// [`BarLine::UnknownOutput`]; present with `current_tag: None` is
/// [`BarLine::NoTagSelected`]; present with `current_tag: Some(tag_id)`
/// looks up `tag_id`'s name in `tags`, falling back to the raw numeral
/// (`"tag {tag_id}"`) if somehow absent — practically unreachable given
/// `wm-core`'s own tag-registry invariants (an output can never point at a
/// tag id that was never registered), but NFR2-style "never panic on any
/// input shape" discipline demands a defined answer rather than a panic or
/// silent data loss, same class as Story 2.4's "unreachable in practice"
/// `claim_pinned_terminal_spawn` error arm.
pub fn resolve_bar_line(output_id: u64, outputs: &[OutputDto], tags: &[TagDto]) -> BarLine {
    let Some(output) = outputs.iter().find(|o| o.id == output_id) else {
        return BarLine::UnknownOutput;
    };
    let Some(tag_id) = output.current_tag else {
        return BarLine::NoTagSelected;
    };
    match tags.iter().find(|t| t.id == tag_id) {
        Some(tag) => BarLine::Normal(tag.name.clone()),
        None => BarLine::Normal(format!("tag {tag_id}")),
    }
}

/// The CSS class waybar applies to a healthy line, and half of this
/// binary's public contract: users target `#custom-tag.normal` from their
/// own `style.css`, so it is a name that cannot be changed freely, not an
/// incidental literal (audit finding J-09).
pub const CLASS_NORMAL: &str = "normal";

/// The CSS class waybar applies when the WM could not be reached or
/// disowned the output. Same public-contract status as [`CLASS_NORMAL`] —
/// dimming and color for this state are the user's `style.css`'s job, which
/// is only possible because this string is stable.
pub const CLASS_DISCONNECTED: &str = "disconnected";

/// The waybar-facing wire shape for a `custom/tag` module line
/// (`return-type: "json"`): `{"text":...,"class":...}`. Private — only
/// [`format_waybar_line`] constructs one, always through `serde_json`
/// (Task 1.6), never hand-formatted.
#[derive(Serialize)]
struct WaybarLine<'a> {
    text: &'a str,
    class: &'static str,
}

/// Renders `line` to its one-line waybar-facing JSON string. `None` is the
/// connect/send/read-failure case (Task 6.3's poll loop) — not a
/// [`BarLine`] variant, since it is a function of I/O outcome, not of any
/// parsed `get-state` data — and renders as the disconnected state with a
/// diagnostic glyph in the text itself. Color and dimming are the user's
/// own `style.css`'s job against the `disconnected` class, not this
/// binary's. Always built via
/// [`WaybarLine`] through `serde_json::to_string`, so a tag name
/// containing `"`/`\` is escaped correctly rather than corrupting the
/// line (Task 1.6). `WaybarLine` is a fully-controlled,
/// always-serializable type, so the one `.expect()` here is
/// safe-by-construction, not caller-facing (same precedent as `wm`'s own
/// `serialize_response`).
///
/// ```
/// use buoy_status_bar::bar_line::{BarLine, format_waybar_line};
///
/// assert_eq!(
///     format_waybar_line(Some(BarLine::Normal("web".to_string()))),
///     r#"{"text":"web","class":"normal"}"#,
/// );
/// // A name that would corrupt a hand-interpolated line is escaped, not
/// // rejected: this goes through `serde_json`.
/// assert_eq!(
///     format_waybar_line(Some(BarLine::Normal(r#"say "hi""#.to_string()))),
///     r#"{"text":"say \"hi\"","class":"normal"}"#,
/// );
/// ```
pub fn format_waybar_line(line: Option<BarLine>) -> String {
    let waybar_line = match &line {
        None => WaybarLine {
            text: "⚠ disconnected",
            class: CLASS_DISCONNECTED,
        },
        Some(BarLine::Normal(name)) => WaybarLine {
            text: name,
            class: CLASS_NORMAL,
        },
        Some(BarLine::NoTagSelected) => WaybarLine {
            text: "no tag",
            class: CLASS_NORMAL,
        },
        Some(BarLine::UnknownOutput) => WaybarLine {
            text: "⚠ unknown output",
            class: CLASS_DISCONNECTED,
        },
    };
    serde_json::to_string(&waybar_line)
        .expect("WaybarLine is a fully-controlled, always-serializable type")
}

/// `true` when `next` differs from the last line actually printed
/// (`previous`), so `main`'s poll loop (Task 6.3) only writes a new stdout
/// line — and thus only needlessly re-triggers waybar's own re-render —
/// when the rendered state genuinely changed (AC: "no duplicate stdout
/// lines for an unchanged state").
pub fn changed(previous: Option<&str>, next: &str) -> bool {
    previous != Some(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{OutputDto, TagDto};

    #[test]
    fn resolve_bar_line_returns_normal_with_the_tags_name_when_output_has_a_current_tag() {
        assert_eq!(
            resolve_bar_line(
                0,
                &[OutputDto {
                    id: 0,
                    current_tag: Some(2)
                }],
                &[TagDto {
                    id: 2,
                    name: "web".into()
                }]
            ),
            BarLine::Normal("web".into())
        );
    }

    #[test]
    fn resolve_bar_line_returns_no_tag_selected_when_output_exists_with_none_current_tag() {
        assert_eq!(
            resolve_bar_line(
                0,
                &[OutputDto {
                    id: 0,
                    current_tag: None
                }],
                &[]
            ),
            BarLine::NoTagSelected
        );
    }

    #[test]
    fn resolve_bar_line_returns_unknown_output_when_output_id_is_absent_from_the_list() {
        assert_eq!(resolve_bar_line(0, &[], &[]), BarLine::UnknownOutput);
        assert_eq!(
            resolve_bar_line(
                5,
                &[OutputDto {
                    id: 0,
                    current_tag: None
                }],
                &[]
            ),
            BarLine::UnknownOutput
        );
    }

    #[test]
    fn resolve_bar_line_defensively_falls_back_when_current_tag_id_is_missing_from_the_tags_list() {
        assert_eq!(
            resolve_bar_line(
                0,
                &[OutputDto {
                    id: 0,
                    current_tag: Some(9)
                }],
                &[]
            ),
            BarLine::Normal("tag 9".into())
        );
    }

    #[test]
    fn format_waybar_line_renders_disconnected_json_for_none() {
        assert_eq!(
            format_waybar_line(None),
            r#"{"text":"⚠ disconnected","class":"disconnected"}"#
        );
    }

    #[test]
    fn format_waybar_line_renders_normal_tag_json() {
        assert_eq!(
            format_waybar_line(Some(BarLine::Normal("web".into()))),
            r#"{"text":"web","class":"normal"}"#
        );
    }

    #[test]
    fn format_waybar_line_renders_no_tag_selected_json() {
        assert_eq!(
            format_waybar_line(Some(BarLine::NoTagSelected)),
            r#"{"text":"no tag","class":"normal"}"#
        );
    }

    #[test]
    fn format_waybar_line_renders_unknown_output_with_the_disconnected_class_and_distinct_text() {
        assert_eq!(
            format_waybar_line(Some(BarLine::UnknownOutput)),
            r#"{"text":"⚠ unknown output","class":"disconnected"}"#
        );
    }

    #[test]
    fn format_waybar_line_escapes_a_tag_name_containing_a_double_quote_and_a_backslash() {
        let line = format_waybar_line(Some(BarLine::Normal(r#"he said "hi"\"#.into())));
        let parsed: serde_json::Value = serde_json::from_str(&line).expect("must be valid JSON");
        assert_eq!(parsed["text"], r#"he said "hi"\"#);
        assert_eq!(parsed["class"], "normal");
    }

    #[test]
    fn changed_is_true_when_previous_is_none() {
        assert!(changed(None, "anything"));
    }

    #[test]
    fn changed_is_true_when_previous_differs_from_next() {
        assert!(changed(Some("old"), "new"));
    }

    #[test]
    fn changed_is_false_when_previous_equals_next() {
        assert!(!changed(Some("same"), "same"));
    }
}
