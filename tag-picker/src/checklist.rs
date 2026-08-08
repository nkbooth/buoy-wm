// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! Pure decision logic driving the toggle-and-reopen loop (Story 2.2's
//! spike finding — see `docs/planning/epics/story-2-2.md`'s Technical
//! notes "Spike finding"): building the checkbox-glyph-prefixed row list
//! from a `wm`-reported tag registry and a focused view's current tags,
//! rendering that list into `fuzzel --dmenu`'s tab-delimited stdin format,
//! parsing `fuzzel`'s exit status/stdout back into a toggle-or-cancel
//! decision, and applying a toggle to the loop's local tag-membership
//! copy. None of this touches a socket or spawns a process — that's
//! `main.rs`'s job (Task 6), kept separate so every decision here stays
//! unit-testable without a live `fuzzel` binary, which this sandbox does
//! not have.

use crate::wire::TagDto;

/// One row of the checklist: a tag id, its display name, and whether the
/// focused view currently has it.
#[derive(Debug, Clone, PartialEq)]
pub struct ChecklistEntry {
    pub tag_id: u8,
    pub name: String,
    pub checked: bool,
}

/// Builds one [`ChecklistEntry`] per tag in `tags`, in the same order,
/// marking each `checked` iff its id appears in `focused_view_tags`.
pub fn build_checklist_entries(tags: &[TagDto], focused_view_tags: &[u8]) -> Vec<ChecklistEntry> {
    tags.iter()
        .map(|t| ChecklistEntry {
            tag_id: t.id,
            name: t.name.clone(),
            checked: focused_view_tags.contains(&t.id),
        })
        .collect()
}

/// Renders `entries` into `fuzzel --dmenu`'s tab-delimited stdin format:
/// one line per entry, `"[✓] <name>\t<tag_id>\n"` (checked) or
/// `"[ ] <name>\t<tag_id>\n"` (unchecked). Column 1 (before the tab) is
/// what `--with-nth=1` displays; column 2 is the bare tag id
/// `--accept-nth=2` returns on selection — real, existing `fuzzel` flags
/// (Technical notes' "Spike finding"), so `tag-picker` never has to parse
/// a display name back into an id.
///
/// Code review follow-up (finding #3): `wm_core::create_tag` accepts any
/// string with no charset validation (ADR-006/YAGNI), so a tag name could
/// contain an embedded tab or newline (creatable today via a raw
/// `create-tag` IPC call, even though no UI does this yet) that would
/// otherwise corrupt this tab-delimited row format. Embedded tabs/newlines
/// are replaced with a plain space — chosen over rejecting the tag
/// entirely, since rendering is display-only and every other consumer of
/// the tag name (wire ids, IPC) is unaffected by this substitution.
pub fn render_fuzzel_input(entries: &[ChecklistEntry]) -> String {
    entries
        .iter()
        .map(|e| {
            let glyph = if e.checked { "[✓]" } else { "[ ]" };
            let sanitized_name = e.name.replace(['\t', '\n'], " ");
            format!("{glyph} {sanitized_name}\t{}\n", e.tag_id)
        })
        .collect()
}

/// One outcome of a single `fuzzel` invocation: either a tag id was
/// selected, or the invocation is treated as cancelled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PickerAction {
    Toggled(u8),
    Cancelled,
}

/// Decides the outcome of one `fuzzel` invocation from its exit status and
/// captured stdout. Defensive by construction (Technical notes gap #5:
/// fuzzel's exact cancel exit code/stdout behavior isn't verifiable live
/// in this sandbox) — *any* non-success exit is `Cancelled` regardless of
/// stdout content, and *any* success-exit stdout that doesn't parse
/// cleanly as an in-range `u8` (empty, garbage, or out-of-range) is
/// `Cancelled` too, rather than assuming one specific failure shape is the
/// only one real `fuzzel` produces.
pub fn parse_fuzzel_output(exit_success: bool, stdout: &str) -> PickerAction {
    if !exit_success {
        return PickerAction::Cancelled;
    }
    match stdout.trim().parse::<u8>() {
        Ok(id) => PickerAction::Toggled(id),
        Err(_) => PickerAction::Cancelled,
    }
}

/// Toggles `tag_id`'s membership in `tags` in place: removes it if
/// present, appends it if absent. Same add-if-absent/remove-if-present
/// semantics as `wm_core`'s own `toggle_view_tag`/`TagSet`, kept
/// independent per the story's Technical notes gap #3 duplication
/// rationale rather than calling into `wm_core` from a different
/// binary/crate.
pub fn toggle_local_membership(tags: &mut Vec<u8>, tag_id: u8) {
    if let Some(pos) = tags.iter().position(|&t| t == tag_id) {
        tags.remove(pos);
    } else {
        tags.push(tag_id);
    }
}

/// Whether the picker should open a `fuzzel` invocation at all: only when
/// a view is focused.
pub fn should_open_picker(focused_view: Option<u64>) -> bool {
    focused_view.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::TagDto;

    fn tag(id: u8, name: &str) -> TagDto {
        TagDto {
            id,
            name: name.into(),
        }
    }

    #[test]
    fn build_checklist_entries_marks_focused_views_tags_as_checked() {
        assert_eq!(
            build_checklist_entries(&[tag(0, "web"), tag(1, "chat")], &[0]),
            vec![
                ChecklistEntry {
                    tag_id: 0,
                    name: "web".into(),
                    checked: true
                },
                ChecklistEntry {
                    tag_id: 1,
                    name: "chat".into(),
                    checked: false
                },
            ]
        );
    }

    #[test]
    fn build_checklist_entries_preserves_registry_order() {
        let entries = build_checklist_entries(&[tag(5, "z"), tag(1, "a"), tag(3, "m")], &[1, 3, 5]);
        assert_eq!(
            entries.iter().map(|e| e.tag_id).collect::<Vec<_>>(),
            vec![5, 1, 3]
        );
    }

    #[test]
    fn build_checklist_entries_with_no_focused_tags_are_all_unchecked() {
        let entries = build_checklist_entries(&[tag(0, "web"), tag(1, "chat")], &[]);
        assert!(entries.iter().all(|e| !e.checked));
    }

    #[test]
    fn render_fuzzel_input_formats_checked_row() {
        assert_eq!(
            render_fuzzel_input(&[ChecklistEntry {
                tag_id: 0,
                name: "web".into(),
                checked: true
            }]),
            "[✓] web\t0\n"
        );
    }

    #[test]
    fn render_fuzzel_input_formats_unchecked_row() {
        assert_eq!(
            render_fuzzel_input(&[ChecklistEntry {
                tag_id: 1,
                name: "chat".into(),
                checked: false
            }]),
            "[ ] chat\t1\n"
        );
    }

    #[test]
    fn render_fuzzel_input_joins_multiple_rows_in_order() {
        assert_eq!(
            render_fuzzel_input(&[
                ChecklistEntry {
                    tag_id: 0,
                    name: "web".into(),
                    checked: true
                },
                ChecklistEntry {
                    tag_id: 1,
                    name: "chat".into(),
                    checked: false
                },
            ]),
            "[✓] web\t0\n[ ] chat\t1\n"
        );
    }

    #[test]
    fn render_fuzzel_input_of_empty_entries_is_empty_string() {
        assert_eq!(render_fuzzel_input(&[]), "");
    }

    #[test]
    fn render_fuzzel_input_replaces_embedded_tab_and_newline_in_tag_name() {
        // Code review follow-up (finding #3): a tag name with an embedded
        // tab or newline (creatable today via a raw `create-tag` IPC call,
        // no UI does this yet) would otherwise corrupt the tab-delimited
        // row format fuzzel's `--dmenu`/`--with-nth`/`--accept-nth` parses —
        // e.g. an unescaped embedded tab here would shift column 2 (the
        // bare tag id `--accept-nth=2` returns) to the wrong text entirely.
        // Both characters are replaced with a plain space.
        assert_eq!(
            render_fuzzel_input(&[ChecklistEntry {
                tag_id: 2,
                name: "we\tb\nsite".into(),
                checked: false
            }]),
            "[ ] we b site\t2\n"
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_toggled_for_successful_exit_and_valid_tag_id() {
        assert_eq!(parse_fuzzel_output(true, "3\n"), PickerAction::Toggled(3));
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_nonzero_exit() {
        assert_eq!(parse_fuzzel_output(false, "3\n"), PickerAction::Cancelled);
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_empty_stdout() {
        assert_eq!(parse_fuzzel_output(true, ""), PickerAction::Cancelled);
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_unparseable_stdout() {
        assert_eq!(
            parse_fuzzel_output(true, "not-a-number"),
            PickerAction::Cancelled
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_tag_id_out_of_u8_range() {
        assert_eq!(parse_fuzzel_output(true, "999"), PickerAction::Cancelled);
    }

    #[test]
    fn toggle_local_membership_adds_tag_when_absent() {
        let mut tags = vec![0];
        toggle_local_membership(&mut tags, 1);
        assert_eq!(tags, vec![0, 1]);
    }

    #[test]
    fn toggle_local_membership_removes_tag_when_present() {
        let mut tags = vec![0, 1];
        toggle_local_membership(&mut tags, 1);
        assert_eq!(tags, vec![0]);
    }

    #[test]
    fn toggle_local_membership_is_idempotent_pairwise() {
        // Toggling an absent id twice must round-trip back to the exact
        // original vec, including order: add-if-absent appends at the end,
        // and remove-if-present removes that same trailing element, so the
        // net effect after two toggles of the same id is a no-op. (Toggling
        // an id that starts out present, and is not the last element,
        // would instead re-append it at the end rather than its original
        // position — membership-idempotent but not order-preserving; this
        // test exercises the round-trip case the loop's real usage
        // actually relies on, since each `fuzzel` reopen re-derives display
        // order fresh from the registry, not from this vec's order.)
        let mut tags = vec![0, 2];
        let original = tags.clone();
        toggle_local_membership(&mut tags, 1);
        toggle_local_membership(&mut tags, 1);
        assert_eq!(tags, original);
    }

    #[test]
    fn should_open_picker_true_when_focused_view_is_some() {
        assert!(should_open_picker(Some(3)));
    }

    #[test]
    fn should_open_picker_false_when_focused_view_is_none() {
        assert!(!should_open_picker(None));
    }
}
