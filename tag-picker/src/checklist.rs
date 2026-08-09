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
            format!("{glyph} {}\t{}\n", sanitize_name(&e.name), e.tag_id)
        })
        .collect()
}

/// Replaces embedded tabs/newlines in a tag name with a plain space, so a
/// name containing either can never corrupt the tab-delimited
/// `--with-nth`/`--accept-nth` row format both [`render_fuzzel_input`] and
/// [`render_switch_list`] emit (`wm_core::create_tag` accepts any string
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
/// row's dismissal mechanism for free (Story 2.3): its `--accept-nth`
/// column is deliberately empty, so selecting it or pressing Escape both
/// land on this same path.
///
/// Beyond that, per Story 2.3's disambiguation rule (`fuzzel(1)`'s
/// documented verbatim-echo-on-no-match behavior — Technical notes "Free-
/// text disambiguation evidence"): stdout that parses as a `u8` **and** is
/// a member of `known_tag_ids` (i.e. it came from a real, matched,
/// `--accept-nth`-transformed row) is `Toggled`; anything else non-empty —
/// including a `u8`-shaped string that just isn't a currently-known id, and
/// ordinary non-numeric free text — is `CreateTag` with the trimmed stdout
/// as the literal new tag name. A free-typed name that happens to be a
/// bare numeral equal to a *currently rendered* id is an accepted, v1
/// residual ambiguity (Task 1.3) and resolves to `Toggled`, not
/// `CreateTag`.
pub fn parse_fuzzel_output(exit_success: bool, stdout: &str, known_tag_ids: &[u8]) -> PickerAction {
    if !exit_success {
        return PickerAction::Cancelled;
    }
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return PickerAction::Cancelled;
    }
    match trimmed.parse::<u8>() {
        Ok(id) if known_tag_ids.contains(&id) => PickerAction::Toggled(id),
        _ => PickerAction::CreateTag(trimmed.to_string()),
    }
}

/// Renders the 64-tag-cap rejection's synthetic checklist row: the literal
/// [`REJECTION_MESSAGE`] text in column 1, an empty column 2 (the
/// `--accept-nth` column `fuzzel` returns on selection) — an empty second
/// column is what makes selecting this row equivalent to Escape, reusing
/// [`parse_fuzzel_output`]'s empty-stdout-is-cancelled rule at zero extra
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
/// same shape, and same empty-`--accept-nth`-column dismissal mechanism
/// (reusing [`parse_fuzzel_output`]'s empty-stdout-is-cancelled rule), as
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
/// [`parse_fuzzel_output`] minus the `CreateTag` catch-all arm: anything
/// that isn't a real matched row (non-success exit, empty stdout,
/// non-numeric text, or an in-range-but-unregistered numeral) is simply
/// `Cancelled`, never treated as a create attempt.
pub fn parse_switch_selection(
    exit_success: bool,
    stdout: &str,
    known_tag_ids: &[u8],
) -> SwitchAction {
    if !exit_success {
        return SwitchAction::Cancelled;
    }
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return SwitchAction::Cancelled;
    }
    match trimmed.parse::<u8>() {
        Ok(id) if known_tag_ids.contains(&id) => SwitchAction::Selected(id),
        _ => SwitchAction::Cancelled,
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
        assert_eq!(
            parse_fuzzel_output(true, "3\n", &[3]),
            PickerAction::Toggled(3)
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_nonzero_exit() {
        assert_eq!(
            parse_fuzzel_output(false, "3\n", &[3]),
            PickerAction::Cancelled
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_cancelled_for_empty_stdout() {
        assert_eq!(parse_fuzzel_output(true, "", &[]), PickerAction::Cancelled);
    }

    #[test]
    fn parse_fuzzel_output_returns_create_tag_for_non_numeric_freeform_stdout() {
        // Story 2.3 (Task 1.4-class repurposing, applied here too): under
        // the old two-variant `PickerAction`, any stdout that didn't parse
        // as an in-range `u8` — numeric-but-out-of-range *or* plain
        // garbage/free text — was defensively `Cancelled`, since no
        // legitimate reason for either shape to come back from `fuzzel`
        // was known before `CreateTag` existed. Now that free text is a
        // real, first-class outcome (AC1), non-numeric stdout that matches
        // no known tag id is exactly the `CreateTag` case — this is in
        // fact the *primary* new behavior, not an edge case; "not-a-number"
        // is just as valid a typed tag name as "deploy-watch".
        assert_eq!(
            parse_fuzzel_output(true, "not-a-number", &[3]),
            PickerAction::CreateTag("not-a-number".into())
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_create_tag_for_out_of_range_numeral_via_parse_failure_path() {
        // Story 2.3 Task 1.4: repurposes the old
        // `..._returns_cancelled_for_tag_id_out_of_u8_range` test. `"999"`
        // not matching any id `tag-picker` actually rendered this
        // invocation is exactly the `CreateTag("999")` case — an odd tag
        // name, but ADR-006 doesn't forbid numeral-shaped tag names, and
        // there is no real row `--accept-nth` could have produced `"999"`
        // from when no id `999` is currently registered.
        //
        // Code review follow-up (finding 4): renamed from
        // `..._returns_create_tag_for_numeral_not_among_known_ids`. `"999"`
        // exceeds `u8::MAX` (255), so `.parse::<u8>()` fails outright and
        // this test actually exercises the `_ =>` catch-all arm via a parse
        // *failure*, not the `Ok(id) if !known_tag_ids.contains(&id)`
        // guard-false branch its old name implied. That branch is exercised
        // separately below by
        // `..._returns_create_tag_for_in_range_numeral_not_among_known_ids`.
        assert_eq!(
            parse_fuzzel_output(true, "999\n", &[0, 1, 2]),
            PickerAction::CreateTag("999".into())
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_create_tag_for_in_range_numeral_not_among_known_ids() {
        // Code review follow-up (finding 4): the missing coverage case —
        // a genuinely valid, in-range `u8` numeral ("5") that parses
        // successfully but isn't a member of `known_tag_ids`, exercising
        // the `Ok(id) if known_tag_ids.contains(&id)` guard's *false*
        // branch specifically, distinct from the parse-failure path above.
        assert_eq!(
            parse_fuzzel_output(true, "5\n", &[0, 1, 2]),
            PickerAction::CreateTag("5".into())
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_toggled_when_stdout_matches_a_known_tag_id() {
        assert_eq!(
            parse_fuzzel_output(true, "1\n", &[0, 1, 2]),
            PickerAction::Toggled(1)
        );
    }

    #[test]
    fn parse_fuzzel_output_returns_create_tag_when_stdout_does_not_match_any_known_tag_id() {
        assert_eq!(
            parse_fuzzel_output(true, "deploy-watch\n", &[0, 1, 2]),
            PickerAction::CreateTag("deploy-watch".into())
        );
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
    fn parse_fuzzel_output_treats_known_id_with_leading_zero_or_whitespace_consistently_with_toggle()
     {
        assert_eq!(
            parse_fuzzel_output(true, " 1 \n", &[0, 1, 2]),
            PickerAction::Toggled(1)
        );
    }

    #[test]
    fn render_rejection_row_is_the_literal_cap_message_with_an_empty_accept_column() {
        // Column 1 is the exact literal `wm/src/ipc/dispatch.rs`'s
        // `describe_wm_core_error` returns for `TagLimitReached`; column 2
        // (the `--accept-nth` column) is deliberately empty, which is what
        // makes selecting this row land on the same empty-stdout-is-
        // cancelled path as pressing Escape (Task 1.2).
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
        // CREATE_APPLY_FAILED_MESSAGE, column 2 (the `--accept-nth` column)
        // is deliberately empty, same dismissal mechanism as
        // render_rejection_row.
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
    fn parse_switch_selection_returns_selected_for_a_known_tag_id() {
        assert_eq!(
            parse_switch_selection(true, "1\n", &[0, 1, 2]),
            SwitchAction::Selected(1)
        );
    }

    #[test]
    fn parse_switch_selection_returns_cancelled_for_nonzero_exit() {
        assert_eq!(
            parse_switch_selection(false, "1\n", &[0, 1, 2]),
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
    fn parse_switch_selection_returns_cancelled_for_freeform_text_not_a_known_id() {
        // The deliberate, load-bearing difference from assign mode's
        // `parse_fuzzel_output`: switch mode has no create-tag branch at
        // all, so anything that isn't a real matched row is simply
        // cancelled, never treated as a create attempt.
        assert_eq!(
            parse_switch_selection(true, "deploy-watch\n", &[0, 1, 2]),
            SwitchAction::Cancelled
        );
    }

    #[test]
    fn parse_switch_selection_returns_cancelled_for_a_numeral_not_among_known_ids() {
        // An in-range but unregistered numeral is not a valid selection in
        // switch mode, unlike assign mode where it would become a
        // `CreateTag` attempt.
        assert_eq!(
            parse_switch_selection(true, "99\n", &[0, 1, 2]),
            SwitchAction::Cancelled
        );
    }

    #[test]
    fn parse_switch_selection_trims_whitespace_consistently_with_toggle_parsing() {
        assert_eq!(
            parse_switch_selection(true, " 1 \n", &[0, 1, 2]),
            SwitchAction::Selected(1)
        );
    }
}
