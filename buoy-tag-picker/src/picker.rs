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

//! Pure decision logic for both picker modes.
//!
//! Named `picker`, not `checklist`: it started as assign mode's checklist
//! and grew switch mode's `SwitchAction`, `render_switch_list`,
//! `parse_switch_selection` and `render_rejection_row` too, at which point
//! the old name described a third of its contents and its module doc had to
//! disclaim the mismatch (audit finding J-09).
//!
//! Assign mode drives the toggle-and-reopen loop that ADR-004's
//! resolved spike settled on (`fuzzel` 1.14.1 offers no native checkbox
//! toggle and no `--multi`): building the checkbox-glyph-prefixed row list
//! from a `wm`-reported tag registry and a focused view's current tags,
//! rendering that list into `fuzzel --dmenu`'s tab-delimited stdin format,
//! parsing `fuzzel`'s exit status/stdout back into a toggle-or-cancel
//! decision, and reading a focused view's membership back out of a
//! freshly-requested state snapshot. The create-tag branch and its 64-tag-cap rejection row belong to
//! *switch* mode's
//! [`parse_switch_selection`] as of Story 2.13, not to assign mode: a new
//! tag is a place you go, not a label you attach. None of this touches a
//! socket or
//! spawns a process — that's `main.rs`'s job, kept separate so every
//! decision here stays unit-testable without a live `fuzzel` binary, which
//! this sandbox does not have.

use crate::wire::{TagDto, ViewDto};

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
/// [`parse_fuzzel_output`] extracts from the full accepted line on
/// selection (Technical notes' "Spike finding"), so `buoy-tag-picker` never has
/// to parse a display name back into an id.
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
            format!("{glyph} {}\t{}\n", sanitize_name(&e.name), e.tag_id)
        })
        .collect()
}

/// Replaces embedded tabs/newlines in a tag name with a plain space, so a
/// name containing either can never corrupt the tab-delimited row format
/// both [`render_fuzzel_input`] and [`render_switch_list`] emit — which
/// `parse_fuzzel_output`/`parse_switch_selection` now split on themselves
/// (`wm_core::create_tag` accepts any string
/// with no charset validation — ADR-006/YAGNI). Shared here since this is
/// the second real occurrence of the identical one-line operation, not
/// speculative abstraction (Technical notes "Consistency with the existing
/// three-strike DRY rule").
fn sanitize_name(name: &str) -> String {
    name.replace(['\t', '\n'], " ")
}

/// One outcome of a single assign-mode `fuzzel` invocation: an existing
/// tag was selected (toggle it), or the invocation is treated as cancelled.
///
/// Story 2.13 removed this enum's `CreateTag` variant. Creating a tag is
/// how you *start working somewhere new* — a switch, not an assignment —
/// so it now lives exclusively in [`SwitchAction`]. Do not restore it here:
/// two dialogs that both create means two cap-rejection paths and two
/// idempotent-collision rules to keep in sync, which is precisely what
/// Story 2.3's three code-review findings came out of.
#[derive(Debug, Clone, PartialEq)]
pub enum PickerAction {
    Toggled(u8),
    Cancelled,
}

/// The literal cap-rejection message row text (Story 2.3). This is
/// `wm/src/ipc/dispatch.rs`'s own `describe_wm_core_error` string for
/// `WmCoreError::TagLimitReached`, quoted verbatim here — `dispatch.rs`'s
/// doc comment names itself the single source of truth for this string;
/// `buoy-tag-picker` never re-derives it, only recognizes it when `wm` sends it
/// back in a `create-tag` error response. Same "keep these in sync"
/// cross-crate literal-duplication convention as `socket_path.rs`'s own
/// note, not a logic duplication.
pub const REJECTION_MESSAGE: &str = "tag limit reached (64)";

/// Decides the outcome of one `fuzzel` invocation from its exit status,
/// captured stdout, and the tag ids actually rendered this invocation
/// (`known_tag_ids`).
///
/// Defensive by construction for the cancel cases (Technical notes gap #5,
/// Story 2.2: fuzzel's exact cancel exit code/stdout behavior isn't
/// verifiable live in this sandbox) — *any* non-success exit is
/// unconditionally `Cancelled`, checked before anything is parsed at all.
///
/// Code review follow-up: `stdout` is the **full raw line** `fuzzel`
/// returns (no `--accept-nth`, see `main.rs`'s `run_fuzzel` doc comment
/// for why that flag is gone — it corrupted every created tag's name).
/// Splitting the trimmed line on the *last* tab is what identifies a real
/// pick: a rendered row (`"[ ] name\tid\n"`, always exactly one tab) comes
/// back as the full line including that tab, so a `Some` split with an `id`
/// part that parses as `u8` *and* is a member of `known_tag_ids` is
/// `Toggled`.
///
/// Every other shape is `Cancelled`, including a typed, non-matching custom
/// entry with no tab at all (`fuzzel(1)`'s documented "input string does
/// not match any entry, printed as-is" behavior). That line used to mean
/// `CreateTag` here; Story 2.13 moved creation to switch mode entirely, so
/// assign mode now has nothing to do with a name that isn't already a tag —
/// see [`parse_switch_selection`], which owns that arm now.
///
/// ```
/// use buoy_tag_picker::picker::{PickerAction, parse_fuzzel_output};
///
/// // `fuzzel` prints the whole accepted line; the tag id is the field
/// // after the tab.
/// let accepted = parse_fuzzel_output(true, "[x] web\t0\n", &[0, 1]);
/// assert_eq!(accepted, PickerAction::Toggled(0));
///
/// // Escape, an id that is not in the registry, and a typed name that
/// // matched no row are all the same non-action.
/// assert_eq!(parse_fuzzel_output(false, "", &[0, 1]), PickerAction::Cancelled);
/// assert_eq!(
///     parse_fuzzel_output(true, "[ ] gone\t9\n", &[0, 1]),
///     PickerAction::Cancelled,
/// );
/// assert_eq!(parse_fuzzel_output(true, "brand new\n", &[0, 1]), PickerAction::Cancelled);
/// ```
pub fn parse_fuzzel_output(exit_success: bool, stdout: &str, known_tag_ids: &[u8]) -> PickerAction {
    if !exit_success {
        return PickerAction::Cancelled;
    }
    // Only the trailing newline is stripped, *not* a blanket `.trim()` -
    // tab is ASCII whitespace, and a blanket trim would eat a row's
    // significant trailing tab.
    let trimmed = stdout.trim_end_matches('\n');
    match trimmed.rsplit_once('\t') {
        Some((_, id)) => match id.parse::<u8>() {
            Ok(id) if known_tag_ids.contains(&id) => PickerAction::Toggled(id),
            _ => PickerAction::Cancelled,
        },
        None => PickerAction::Cancelled,
    }
}

/// Renders the 64-tag-cap rejection's synthetic checklist row: the literal
/// [`REJECTION_MESSAGE`] text in column 1, an empty column 2 — selecting
/// it returns the full line "`REJECTION_MESSAGE`\t", whose id part (after
/// [`parse_fuzzel_output`]'s tab split) is empty and fails to parse,
/// landing on the same `Cancelled` path as pressing Escape at zero extra
/// dismissal code.
pub fn render_rejection_row() -> String {
    format!("{REJECTION_MESSAGE}\t\n")
}

/// `view_id`'s tag membership as `views` reports it: empty when no view is
/// focused, and empty when the view is no longer in the snapshot.
///
/// Replaced a local mirror that was snapshotted once and thereafter only
/// mutated (audit finding K-03). Because the wire operation is a *toggle*
/// rather than an absolute set, a mirror that had drifted from the server
/// — a keybind or a second client having changed membership meanwhile —
/// **inverted** the user's next pick instead of merely losing it. The fix
/// is to have no mirror: recompute from a freshly requested snapshot on
/// every pass of the loop.
pub fn view_tag_membership(views: &[ViewDto], view_id: Option<u64>) -> Vec<u8> {
    view_id
        .and_then(|view_id| views.iter().find(|view| view.id == view_id))
        .map(|view| view.tags.clone())
        .unwrap_or_default()
}

/// Code review follow-up (Story 2.10): whether assign mode's picker has
/// *any* useful destination for a pick — either a focused view to toggle
/// membership on, or a known output to switch to (the no-focus fallback,
/// Story 2.10 Task 5). Restores the guard `should_open_picker` used to
/// provide (deleted when the old "must have a focused view" precondition
/// was relaxed) with the same shape, generalized to the new either/or
/// requirement. Called *before* any wire traffic in `run_assign_mode`.
///
/// Story 2.13 removed assign mode's create path, so this guard's original
/// rationale — a `CreateTag` request permanently registering a tag (no
/// delete-tag API exists) that then has nowhere to be applied — no longer
/// applies. Its own claim still does, and is why it stays: a pick with
/// neither a view to toggle nor an output to switch has no destination at
/// all, so opening the picker could only waste the user's time.
pub fn should_open_picker(view_id: Option<u64>, output_id: Option<u64>) -> bool {
    view_id.is_some() || output_id.is_some()
}

/// One outcome of a single switch-mode `fuzzel` invocation (Story 2.4):
/// an existing tag was selected (switch to it), free text was typed and
/// confirmed (create a tag with that name, then switch to it — Story
/// 2.13), or the invocation is treated as cancelled.
///
/// `CreateTag` is the variant Story 2.13 moved here from assign mode's
/// [`PickerAction`]: creating a tag is how you start working somewhere
/// new, which is a switch, not an assignment. Not `Copy` as a result — the
/// typed name is an owned `String`.
#[derive(Debug, Clone, PartialEq)]
pub enum SwitchAction {
    Selected(u8),
    CreateTag(String),
    Cancelled,
}

/// Renders `tags` into `fuzzel --dmenu`'s tab-delimited stdin format for
/// switch mode: one plain row per tag, `"<name>\t<tag_id>\n"` — no
/// `[✓]`/`[ ]` checkbox glyph prefix, since membership isn't the concept
/// in switch mode at all (contrast with [`render_fuzzel_input`]'s
/// glyph-prefixed rows).
pub fn render_switch_list(tags: &[TagDto]) -> String {
    tags.iter()
        .map(|t| format!("{}\t{}\n", sanitize_name(&t.name), t.id))
        .collect()
}

/// Decides the outcome of one switch-mode `fuzzel` invocation from its
/// exit status, captured stdout, and the tag ids actually rendered this
/// invocation (`known_tag_ids`). Structurally the same shape as
/// [`parse_fuzzel_output`] (see its doc comment for why `stdout` is the
/// full raw line, not an `--accept-nth`-extracted id), plus the
/// `CreateTag` catch-all arm Story 2.13 moved here from that function:
/// a line with no tab at all is `fuzzel(1)`'s documented "input string
/// does not match any of the entries, printed as is" behavior, i.e. a name
/// the user typed that isn't a tag yet.
///
/// A whitespace-only (or empty) typed line is `Cancelled`, not a create.
/// `wm_core` has no delete-tag API by design (ADR-006), so a tag created
/// from a stray Enter on a blank-looking input box would be stuck in the
/// registry for the whole session as an unreadable, unselectable row. This
/// is deliberately *not* a trim — `"web "` still creates `"web "`,
/// unchanged from the behavior this arm had in assign mode; only the
/// all-whitespace case is refused.
///
/// Everything else is `Cancelled`: a non-success exit, or a tab whose id
/// part is empty (the cap-rejection row's dismissal mechanism) or doesn't
/// match a currently-rendered id.
pub fn parse_switch_selection(
    exit_success: bool,
    stdout: &str,
    known_tag_ids: &[u8],
) -> SwitchAction {
    if !exit_success {
        return SwitchAction::Cancelled;
    }
    // See `parse_fuzzel_output`'s doc comment: only the trailing newline
    // is stripped, not a blanket `.trim()`, since tab is ASCII whitespace.
    let trimmed = stdout.trim_end_matches('\n');
    match trimmed.rsplit_once('\t') {
        Some((_, id)) => match id.parse::<u8>() {
            Ok(id) if known_tag_ids.contains(&id) => SwitchAction::Selected(id),
            _ => SwitchAction::Cancelled,
        },
        None if trimmed.trim().is_empty() => SwitchAction::Cancelled,
        None => SwitchAction::CreateTag(trimmed.to_string()),
    }
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
        // row format `--with-nth`'s display and `parse_fuzzel_output`'s own
        // `rsplit_once('\t')` both rely on — e.g. an unescaped embedded tab
        // here would shift where the id column is found entirely. Both
        // characters are replaced with a plain space.
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
    fn parse_fuzzel_output_returns_toggled_for_a_full_row_with_a_known_tag_id() {
        // Code review follow-up: `stdout` is now the *full raw line*
        // fuzzel returns with no `--accept-nth` (see the function's own
        // doc comment for why) — a real selected row always has exactly
        // one tab, and its id part must be a known, currently-rendered id.
        assert_eq!(
            parse_fuzzel_output(true, "[ ] web\t3\n", &[3]),
            PickerAction::Toggled(3)
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_nonzero_exit() {
        assert_eq!(
            parse_fuzzel_output(false, "[ ] web\t3\n", &[3]),
            PickerAction::Cancelled
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_empty_stdout() {
        assert_eq!(parse_fuzzel_output(true, "", &[]), PickerAction::Cancelled);
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_empty_stdout_regardless_of_known_ids() {
        // The rejection row's actual dismissal mechanism (Task 1.2): empty
        // (trimmed) stdout is unconditionally `Cancelled`, checked before
        // the create/toggle split, regardless of what ids are known this
        // invocation.
        assert_eq!(
            parse_fuzzel_output(true, "", &[0, 1, 2]),
            PickerAction::Cancelled
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_a_row_with_an_unknown_tag_id() {
        // Should not occur in practice — every real, rendered row's id is
        // always current — but a tab is present here, so this exercises
        // the "found a tab, id doesn't match" branch specifically, distinct
        // from the no-tab `CreateTag` path below. Cancelling rather than
        // creating a tag out of row furniture is the safer fallback.
        assert_eq!(
            parse_fuzzel_output(true, "[ ] web\t99\n", &[0, 1, 2]),
            PickerAction::Cancelled
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_freeform_text_with_no_tab() {
        // Story 2.13: this assertion is inverted from Story 2.3's. A typed,
        // non-matching custom entry (no tab at all — `fuzzel(1)`'s
        // documented "input string does not match any of the entries,
        // printed as is" behavior) used to be assign mode's `CreateTag`
        // path. Creation now lives in the switcher only, so the same input
        // is simply cancelled here.
        assert_eq!(
            parse_fuzzel_output(true, "deploy-watch\n", &[0, 1, 2]),
            PickerAction::Cancelled
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_a_bare_numeral_with_no_tab() {
        // Same inversion as above for a numeral-shaped typed name — with no
        // create path in assign mode there is nothing for a no-tab line to
        // mean, whatever it looks like.
        assert_eq!(
            parse_fuzzel_output(true, "5\n", &[0, 1, 2]),
            PickerAction::Cancelled
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_nonzero_exit_even_with_freeform_stdout() {
        // Exit status still wins over content, unchanged principle from
        // Story 2.2 — even stdout that looks like a perfectly good new tag
        // name is discarded if fuzzel itself reports a non-success exit.
        assert_eq!(
            parse_fuzzel_output(false, "deploy-watch\n", &[0, 1, 2]),
            PickerAction::Cancelled
        );
    }

    #[test]
    fn parse_fuzzel_output_strips_only_the_trailing_newline_not_a_full_row() {
        // Code review follow-up: a blanket `.trim()` would eat the
        // rejection row's significant trailing tab (tab is ASCII
        // whitespace) — only the trailing newline is stripped, so a real
        // row's tab-delimited structure survives intact.
        assert_eq!(
            parse_fuzzel_output(true, "[ ] web\t3\n", &[3]),
            PickerAction::Toggled(3)
        );
    }

    #[test]
    fn render_rejection_row_is_the_literal_cap_message_with_an_empty_accept_column() {
        // Column 1 is the exact literal `wm/src/ipc/dispatch.rs`'s
        // `describe_wm_core_error` returns for `TagLimitReached`; column 2
        // is deliberately empty, which is what makes selecting this row
        // land on the same cancelled path as pressing Escape (Task 1.2;
        // see `parse_switch_selection_returns_cancelled_for_the_rejection_row_selection`
        // for the actual round-trip, which Story 2.13 moved to switch mode
        // along with the create path that is the only way to reach the cap).
        assert_eq!(render_rejection_row(), "tag limit reached (64)\t\n");
    }

    #[test]
    fn should_open_picker_true_when_view_focused() {
        assert!(should_open_picker(Some(3), None));
    }

    #[test]
    fn should_open_picker_true_when_output_known_even_without_view() {
        assert!(should_open_picker(None, Some(0)));
    }

    #[test]
    fn should_open_picker_true_when_both_view_and_output_known() {
        assert!(should_open_picker(Some(3), Some(0)));
    }

    #[test]
    fn should_open_picker_false_when_neither_view_nor_output_known() {
        assert!(!should_open_picker(None, None));
    }

    #[test]
    fn membership_comes_from_the_snapshot_not_a_local_mirror() {
        let views = vec![
            ViewDto {
                id: 7,
                tags: vec![1, 3],
            },
            ViewDto {
                id: 8,
                tags: vec![2],
            },
        ];
        assert_eq!(view_tag_membership(&views, Some(7)), vec![1, 3]);
    }

    #[test]
    fn membership_is_empty_when_no_view_is_focused() {
        let views = vec![ViewDto {
            id: 7,
            tags: vec![1],
        }];
        assert_eq!(view_tag_membership(&views, None), Vec::<u8>::new());
    }

    /// Audit finding K-03: the view the picker was spawned for can be gone
    /// by the time the loop asks again — a window closing mid-pick is
    /// ordinary. Every tag then reads unchecked, which is true, rather than
    /// the loop toggling against a membership list for a window that no
    /// longer exists.
    #[test]
    fn membership_is_empty_when_the_focused_view_has_gone_away() {
        let views = vec![ViewDto {
            id: 8,
            tags: vec![2],
        }];
        assert_eq!(view_tag_membership(&views, Some(7)), Vec::<u8>::new());
    }

    // --- Story 2.4: switch-mode rendering and selection parsing ---

    #[test]
    fn render_switch_list_formats_one_plain_row_per_tag() {
        assert_eq!(
            render_switch_list(&[tag(0, "web"), tag(1, "chat")]),
            "web\t0\nchat\t1\n"
        );
    }

    #[test]
    fn render_switch_list_of_empty_tags_is_empty_string() {
        assert_eq!(render_switch_list(&[]), "");
    }

    #[test]
    fn render_switch_list_replaces_embedded_tab_and_newline_in_tag_name() {
        // Same corruption-avoidance rationale as
        // `render_fuzzel_input_replaces_embedded_tab_and_newline_in_tag_name`
        // above — the second real occurrence of this sanitization rule.
        assert_eq!(
            render_switch_list(&[tag(2, "we\tb\nsite")]),
            "we b site\t2\n"
        );
    }

    #[test]
    fn parse_switch_selection_returns_selected_for_a_full_row_with_a_known_tag_id() {
        // Code review follow-up: `stdout` is now the full raw line (no
        // `--accept-nth`, see `parse_fuzzel_output`'s doc comment) — a real
        // selected row always has exactly one tab, and a known id after it.
        assert_eq!(
            parse_switch_selection(true, "web\t1\n", &[0, 1, 2]),
            SwitchAction::Selected(1)
        );
    }

    #[test]
    fn parse_switch_selection_returns_cancelled_for_nonzero_exit() {
        assert_eq!(
            parse_switch_selection(false, "web\t1\n", &[0, 1, 2]),
            SwitchAction::Cancelled
        );
    }

    #[test]
    fn parse_switch_selection_returns_cancelled_for_empty_stdout() {
        assert_eq!(
            parse_switch_selection(true, "", &[0, 1, 2]),
            SwitchAction::Cancelled
        );
    }

    #[test]
    fn parse_switch_selection_returns_create_tag_for_freeform_text_with_no_tab() {
        // Story 2.13: the deliberate, load-bearing difference from assign
        // mode's `parse_fuzzel_output` now points the other way — switch
        // mode is the *only* mode that creates. A typed, non-matching entry
        // (no tab at all) is the primary real-world path for creating a new
        // tag and switching to it in one action, not an edge case.
        assert_eq!(
            parse_switch_selection(true, "deploy-watch\n", &[0, 1, 2]),
            SwitchAction::CreateTag("deploy-watch".into())
        );
    }

    #[test]
    fn parse_switch_selection_returns_create_tag_for_a_bare_numeral_with_no_tab() {
        // A typed tag name that happens to look like a number is still a
        // create, not a selection — there is no real row a tab-less line
        // could be confused with, and ADR-006 doesn't forbid numeral-shaped
        // tag names.
        assert_eq!(
            parse_switch_selection(true, "99\n", &[0, 1, 2]),
            SwitchAction::CreateTag("99".into())
        );
    }

    #[test]
    fn parse_switch_selection_returns_cancelled_for_whitespace_only_input() {
        // Story 2.13: `wm_core` has no delete-tag API (ADR-006), so a tag
        // created from a stray Enter on a blank-looking input box would be
        // stuck in the registry for the session, rendering as an
        // unreadable, unselectable row. Empty input was already cancelled;
        // whitespace-only lands on the same path. Note this is *not* a
        // trim — `"web "` still creates `"web "` — only the all-whitespace
        // case is refused.
        assert_eq!(
            parse_switch_selection(true, "   \n", &[0, 1, 2]),
            SwitchAction::Cancelled
        );
    }

    #[test]
    fn parse_switch_selection_returns_cancelled_for_the_rejection_row_selection() {
        // Story 2.13: the cap-rejection row moved to switch mode along with
        // the create path that is the only way to reach it. Same dismissal
        // mechanism as it had in assign mode — its empty id column fails to
        // parse, landing on `Cancelled` rather than being treated as a
        // typed name of the message text.
        assert_eq!(
            parse_switch_selection(true, &render_rejection_row(), &[0, 1, 2]),
            SwitchAction::Cancelled
        );
    }

    #[test]
    fn parse_switch_selection_returns_cancelled_for_a_row_with_an_unknown_tag_id() {
        // A tab is present, but the id doesn't match any currently-known
        // tag — should not occur in practice (every real row's id is
        // always current), but exercises the "found a tab, id doesn't
        // match" branch distinctly from the no-tab case above.
        assert_eq!(
            parse_switch_selection(true, "web\t99\n", &[0, 1, 2]),
            SwitchAction::Cancelled
        );
    }

    #[test]
    fn parse_switch_selection_strips_only_the_trailing_newline() {
        // Same reasoning as `parse_fuzzel_output`'s equivalent test — a
        // blanket `.trim()` would eat a significant trailing tab.
        assert_eq!(
            parse_switch_selection(true, "web\t1\n", &[0, 1, 2]),
            SwitchAction::Selected(1)
        );
    }
}
