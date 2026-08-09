// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! Pure decision logic driving the toggle-and-reopen loop (Story 2.2's
//! spike finding — see `docs/planning/epics/story-2-2.md`'s Technical
//! notes "Spike finding"): building the checkbox-glyph-prefixed row list
//! from a `wm`-reported tag registry and a focused view's current tags,
//! rendering that list into `fuzzel --dmenu`'s tab-delimited stdin format,
//! parsing `fuzzel`'s exit status/stdout back into a toggle-or-create-or-
//! cancel decision (Story 2.3 adds the create-tag branch and the 64-tag-
//! cap rejection row — see `docs/planning/epics/story-2-3.md`'s Technical
//! notes "Free-text disambiguation evidence"), and applying a toggle to
//! the loop's local tag-membership copy. None of this touches a socket or
//! spawns a process — that's `main.rs`'s job, kept separate so every
//! decision here stays unit-testable without a live `fuzzel` binary, which
//! this sandbox does not have.

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
/// [`parse_fuzzel_output`] extracts from the full accepted line on
/// selection (Technical notes' "Spike finding"), so `tag-picker` never has
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

/// One outcome of a single `fuzzel` invocation: an existing tag was
/// selected (toggle it), free text was typed and confirmed (create a tag
/// with that name), or the invocation is treated as cancelled.
#[derive(Debug, Clone, PartialEq)]
pub enum PickerAction {
    Toggled(u8),
    CreateTag(String),
    Cancelled,
}

/// The literal cap-rejection message row text (Story 2.3). This is
/// `wm/src/ipc/dispatch.rs`'s own `describe_wm_core_error` string for
/// `WmCoreError::TagLimitReached`, quoted verbatim here — `dispatch.rs`'s
/// doc comment names itself the single source of truth for this string;
/// `tag-picker` never re-derives it, only recognizes it when `wm` sends it
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
/// verifiable live in this sandbox) — *any* non-success exit, or empty
/// (trimmed) stdout on a success exit, is unconditionally `Cancelled`,
/// checked before the create/toggle split. This is also the rejection
/// row's dismissal mechanism for free (Story 2.3): its second column is
/// deliberately empty, so selecting it or pressing Escape both land on
/// this same path.
///
/// Code review follow-up: `stdout` is now the **full raw line** `fuzzel`
/// returns (no `--accept-nth`, see `main.rs`'s `run_fuzzel` doc comment
/// for why that flag is gone — it corrupted every created tag's name).
/// Splitting the trimmed line on the *last* tab distinguishes the two real
/// cases: a real, rendered row (`"[ ] name\tid\n"`, always exactly one
/// tab) comes back as the full line including that tab, so a `Some` split
/// with an `id` part that parses as `u8` *and* is a member of
/// `known_tag_ids` is `Toggled`; a typed, non-matching custom entry has no
/// tab at all (`fuzzel(1)`'s documented "input string does not match any
/// entry, printed as-is" behavior), so a `None` split is `CreateTag` with
/// the raw trimmed text as the literal new tag name. A split that finds a
/// tab but whose id part is empty (the rejection/apply-failed rows) or
/// doesn't match a known id (should not occur — every real row's id is
/// always current) safely falls back to `Cancelled` rather than creating
/// a tag out of row furniture.
pub fn parse_fuzzel_output(exit_success: bool, stdout: &str, known_tag_ids: &[u8]) -> PickerAction {
    if !exit_success {
        return PickerAction::Cancelled;
    }
    // Only the trailing newline is stripped, *not* a blanket `.trim()` -
    // tab is ASCII whitespace, and a blanket trim would eat the
    // rejection/apply-failed rows' significant trailing tab (their whole
    // dismissal mechanism depends on a tab being present with nothing
    // after it).
    let trimmed = stdout.trim_end_matches('\n');
    if trimmed.is_empty() {
        return PickerAction::Cancelled;
    }
    match trimmed.rsplit_once('\t') {
        Some((_, id)) => match id.parse::<u8>() {
            Ok(id) if known_tag_ids.contains(&id) => PickerAction::Toggled(id),
            _ => PickerAction::Cancelled,
        },
        None => PickerAction::CreateTag(trimmed.to_string()),
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

/// Decides whether a chained `toggle-tag` request should follow a
/// successful `create-tag` response resolving to `tag_id` (Code review
/// follow-up, finding 1). `wm_core::TagRegistry::create_tag` is idempotent
/// by exact name: a `create-tag` call can resolve to an *existing* tag's
/// id instead of a genuinely new one. Blindly toggling that resolved id
/// would, if it's already applied to the focused view, *remove* it
/// (`toggle_view_tag`'s add-if-absent/remove-if-present semantics) — the
/// opposite of what typing its name was supposed to do, with both IPC
/// calls still reporting success and nothing signalling the mistake.
///
/// The correct rule is "ensure applied," not "blindly toggle": skip the
/// chained toggle only when `tag_id` is already a member of `current_tags`
/// (the focused view's current tag membership, as already tracked locally
/// from `get-state`); otherwise send it — this covers both a genuinely new
/// tag (never already a member, so the toggle-to-apply always fires) and a
/// name-collision with an existing-but-not-yet-applied tag (the toggle
/// still needs to fire to apply it).
pub fn should_toggle_after_create(tag_id: u8, current_tags: &[u8]) -> bool {
    !current_tags.contains(&tag_id)
}

/// Decides whether `tag_id` should be appended to the picker's local
/// `tags` mirror after a `create-tag` response (Code review follow-up,
/// finding 3 — a direct consequence of finding 1's idempotent-collision
/// case): only push when `tag_id` isn't already present in `tags`, so a
/// name-collision with an already-registered tag doesn't duplicate that
/// tag's row on every subsequent `fuzzel` reopen for the rest of the
/// session.
pub fn should_add_to_tag_mirror(tag_id: u8, tags: &[TagDto]) -> bool {
    !tags.iter().any(|t| t.id == tag_id)
}

/// The literal message row shown when a `create-tag` request succeeds but
/// the immediately-chained `toggle-tag` request fails (Code review
/// follow-up, finding 2). `wm_core` has no delete-tag API (ADR-006/v1
/// scope, deliberate) — a tag once created is permanent for the session,
/// so a failed chained toggle leaves a brand-new tag registered but never
/// applied. `tag-picker` is spawned by a WM keybind with no attached
/// terminal, so a bare `eprintln!` is invisible to the user; this row
/// surfaces the failure in the only UI surface the user can actually see.
/// Deliberately distinct wording from [`REJECTION_MESSAGE`] so the two
/// synthetic notices are never confused with each other.
pub const CREATE_APPLY_FAILED_MESSAGE: &str = "tag created but not applied — toggle it manually";

/// Renders [`CREATE_APPLY_FAILED_MESSAGE`] as a synthetic checklist row:
/// same shape, and same empty-second-column dismissal mechanism, as
/// [`render_rejection_row`].
pub fn render_create_apply_failed_row() -> String {
    format!("{CREATE_APPLY_FAILED_MESSAGE}\t\n")
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

/// Code review follow-up (Story 2.10): whether assign mode's picker has
/// *any* useful destination for a pick — either a focused view to toggle
/// membership on, or a known output to switch to (the no-focus fallback,
/// Story 2.10 Task 5). Restores the guard `should_open_picker` used to
/// provide (deleted when the old "must have a focused view" precondition
/// was relaxed) with the same shape, generalized to the new either/or
/// requirement. Called *before* any wire traffic in `run_assign_mode` —
/// without it, a `CreateTag` request could succeed and permanently
/// register a tag (no delete-tag API exists) in the rare case where
/// neither a view nor an output is known, only to then have nowhere to
/// apply or switch to it.
pub fn should_open_picker(view_id: Option<u64>, output_id: Option<u64>) -> bool {
    view_id.is_some() || output_id.is_some()
}

/// One outcome of a single switch-mode `fuzzel` invocation (Story 2.4):
/// an existing tag was selected (switch to it), or the invocation is
/// treated as cancelled. Deliberately no `CreateTag`-shaped variant —
/// switching only ever operates on existing tags (EXPERIENCE.md), unlike
/// assign mode's [`PickerAction`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SwitchAction {
    Selected(u8),
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
/// [`parse_fuzzel_output`] (see its doc comment for why `stdout` is now
/// the full raw line, not an `--accept-nth`-extracted id) minus the
/// `CreateTag` catch-all arm: anything that isn't a real matched row
/// (non-success exit, empty stdout, no tab at all, or a tab with an
/// in-range-but-unregistered numeral) is simply `Cancelled`, never treated
/// as a create attempt — switching only ever operates on existing tags
/// (`EXPERIENCE.md`).
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
    if trimmed.is_empty() {
        return SwitchAction::Cancelled;
    }
    match trimmed.rsplit_once('\t') {
        Some((_, id)) => match id.parse::<u8>() {
            Ok(id) if known_tag_ids.contains(&id) => SwitchAction::Selected(id),
            _ => SwitchAction::Cancelled,
        },
        None => SwitchAction::Cancelled,
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
    fn parse_fuzzel_output_returns_create_tag_for_freeform_text_with_no_tab() {
        // A typed, non-matching custom entry has no tab at all —
        // `fuzzel(1)`'s documented "input string does not match any of the
        // entries, printed as is" behavior — so this is exactly the
        // `CreateTag` case, the *primary* real-world path for creating a
        // new tag, not an edge case.
        assert_eq!(
            parse_fuzzel_output(true, "deploy-watch\n", &[0, 1, 2]),
            PickerAction::CreateTag("deploy-watch".into())
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_create_tag_for_a_bare_numeral_with_no_tab() {
        // A typed tag name that happens to look like a number is still a
        // create, not a toggle attempt, as long as it has no tab — there is
        // no real row this could be confused with. ADR-006 doesn't forbid
        // numeral-shaped tag names.
        assert_eq!(
            parse_fuzzel_output(true, "5\n", &[0, 1, 2]),
            PickerAction::CreateTag("5".into())
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_the_rejection_row_selection() {
        // Direct round-trip test of `render_rejection_row`'s own dismissal
        // mechanism: selecting it returns the full line with an empty id
        // part after the tab, which fails to parse and falls to
        // `Cancelled` — not `CreateTag` with the message text as a name.
        assert_eq!(
            parse_fuzzel_output(true, &render_rejection_row(), &[0, 1, 2]),
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
        // see `parse_fuzzel_output_returns_cancelled_for_the_rejection_row_selection`
        // for the actual round-trip through `parse_fuzzel_output`).
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
    fn should_toggle_after_create_true_for_genuinely_new_tag() {
        // Code review follow-up, finding 1(a): a genuinely new tag can
        // never already be a member of `current_tags`, so the
        // toggle-to-apply must still fire — existing behavior preserved.
        assert!(should_toggle_after_create(5, &[0, 1]));
    }

    #[test]
    fn should_toggle_after_create_false_when_resolved_tag_already_applied() {
        // Code review follow-up, finding 1(b): `create-tag` resolved to an
        // existing tag id (name-collision, idempotent by name) that is
        // already applied to the focused view — skip the toggle, since
        // sending it would remove the tag instead of leaving it applied.
        assert!(!should_toggle_after_create(1, &[0, 1]));
    }

    #[test]
    fn should_toggle_after_create_true_when_resolved_tag_exists_but_not_applied() {
        // Code review follow-up, finding 1(c): `create-tag` resolved to an
        // existing tag id (name-collision) that is *not* currently applied
        // to the focused view — the toggle must still fire to apply it,
        // this is "ensure applied," not "skip all toggles on collision."
        assert!(should_toggle_after_create(2, &[0, 1]));
    }

    #[test]
    fn should_add_to_tag_mirror_true_when_tag_id_absent() {
        assert!(should_add_to_tag_mirror(
            5,
            &[tag(0, "web"), tag(1, "chat")]
        ));
    }

    #[test]
    fn should_add_to_tag_mirror_false_when_tag_id_already_present() {
        // Code review follow-up, finding 3: a name-collision resolves to an
        // already-known tag id — pushing it again would duplicate that
        // tag's row in every subsequent picker reopen this session.
        assert!(!should_add_to_tag_mirror(
            1,
            &[tag(0, "web"), tag(1, "chat")]
        ));
    }

    #[test]
    fn render_create_apply_failed_row_is_the_literal_message_with_an_empty_accept_column() {
        // Code review follow-up, finding 2: column 1 is the literal
        // CREATE_APPLY_FAILED_MESSAGE, column 2 is deliberately empty,
        // same dismissal mechanism as render_rejection_row.
        assert_eq!(
            render_create_apply_failed_row(),
            "tag created but not applied — toggle it manually\t\n"
        );
    }

    #[test]
    fn create_apply_failed_message_is_textually_distinct_from_rejection_message() {
        // Code review follow-up, finding 2: the two synthetic notice rows
        // must never be visually/textually confusable with each other.
        assert_ne!(CREATE_APPLY_FAILED_MESSAGE, REJECTION_MESSAGE);
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
    fn parse_switch_selection_returns_cancelled_for_freeform_text_with_no_tab() {
        // The deliberate, load-bearing difference from assign mode's
        // `parse_fuzzel_output`: switch mode has no create-tag branch at
        // all, so a typed, non-matching entry (no tab at all) is simply
        // cancelled, never treated as a create attempt.
        assert_eq!(
            parse_switch_selection(true, "deploy-watch\n", &[0, 1, 2]),
            SwitchAction::Cancelled
        );
    }

    #[test]
    fn parse_switch_selection_returns_cancelled_for_a_bare_numeral_with_no_tab() {
        // Unlike assign mode, a numeral-shaped typed entry with no tab
        // never becomes a selection, since switch mode has no create path
        // to fall back to at all.
        assert_eq!(
            parse_switch_selection(true, "99\n", &[0, 1, 2]),
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
