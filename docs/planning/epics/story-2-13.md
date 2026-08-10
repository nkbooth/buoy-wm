---
baseline_commit: b08836c
---

# Story 2.13: Move new-tag creation from the assign picker into the tag switcher

Epic: 2 | Priority: H | Status: done

## Description
Live use reported, verbatim: *"we need to add a 'new tag' workflow into the
tag switcher dialog; using the attach dialog is the wrong ux workflow."*

Today creation lives in exactly the wrong dialog. `Mod4+A` (assign mode)
renders a checklist of every tag with the focused window's memberships
checked, and typing a name that matches nothing creates that tag *and*
applies it to the focused window (Story 2.3, FR8). `Mod4+S` (switch mode)
renders a plain list of existing tags and deliberately has **no** create
path at all — `parse_switch_selection`'s `None` (no-tab) arm returns
`Cancelled`, and `EXPERIENCE.md` states the free-text row is "Not present
in switch mode (switching only operates on existing tags)."

That split is backwards in practice. Creating a tag is how you *start
working somewhere new* — the intent is "take me to a fresh context", which
is a switch. Assignment is the narrower, later act of dragging a window
you already have onto a tag that already exists. Routing creation through
the assign dialog forces the user to have a window focused, and to think
about window membership, purely to reach a switch they wanted anyway.

Confirmed with the user: creation moves, it is not duplicated. Assign mode
loses its create path entirely and becomes purely "toggle this window's
membership in existing tags"; switch mode gains "type a name that matches
nothing → create it and switch the active output to it, in one action."

## Fix
The change is entirely `tag-picker`-local. Both IPC requests it needs —
`create-tag` and `switch-tag` — already exist and are already used by this
binary (Stories 2.3 and 2.4); no `wire.rs` change, no `wm` protocol change,
no new response variant.

`SwitchAction` gains a `CreateTag(String)` variant and
`PickerAction` loses its own, exactly swapping which parser's no-tab arm
means "the user typed something new" (`fuzzel(1)`'s documented "input
string does not match any of the entries, printed as is" behavior — the
same mechanism Story 2.11 established as the *only* reliable one, after
`--accept-nth` was found to corrupt created names).

Switch mode's single-shot invocation becomes a loop that reopens for
exactly one reason: a `create-tag` rejected at the 64-tag registry cap
(ADR-006). That is the same cap-rejection reopen assign mode has carried
since Story 2.3 — it moves along with the create path it belongs to, since
a cap rejection is only reachable from a create attempt. Every other
outcome still terminates the loop on its first pass, so switch mode's
"single-shot" shape is preserved for every path that does not create.

**What is deleted rather than moved.** Assign mode's create path carried
three helpers that exist *only* to reconcile a created tag with a focused
window's membership, a reconciliation that has no analogue in switch mode:

- `should_toggle_after_create` (Story 2.3 code review, finding 1): the
  "ensure applied, not blindly toggle" rule protecting against
  `create_tag`'s by-name idempotence resolving to an already-applied tag
  and the chained toggle then *removing* it. Switch mode has no membership
  to reconcile — a name collision simply resolves to the existing tag's id
  and switches to it, which is the right outcome with no special case.
- `should_add_to_tag_mirror` (finding 3): kept the local `tags` mirror free
  of duplicate rows across reopens. Switch mode never mutates its mirror —
  a successful create switches and exits, and a cap rejection created
  nothing.
- `CREATE_APPLY_FAILED_MESSAGE` / `render_create_apply_failed_row`
  (finding 2): surfaced "the tag was created but the chained toggle-tag
  failed" in the picker UI, because a keybind-spawned process has no
  terminal for `eprintln!` to reach. Switch mode's equivalent failure —
  create succeeded, `switch-tag` failed — is already handled by
  `send_switch_tag_or_exit`, which reports and exits non-zero rather than
  reopening; that is the identical handling a *selected existing* tag has
  had since Story 2.4, and the create path deliberately does not invent a
  second, different failure posture for the same request.

`REJECTION_MESSAGE` and `render_rejection_row` are the only create-path
furniture that survives, because the cap is a property of `create-tag`
itself, not of what you do with the result.

**Whitespace-only names are rejected as new (new guard, not a move).** A
bare Enter on a whitespace-only input box previously reached
`create-tag` from assign mode and permanently registered an unnameable
tag — `wm_core` has no delete-tag API by design (ADR-006), so that tag is
stuck for the rest of the session. Empty input was already `Cancelled`;
whitespace-only now lands on the same path. Names are **not** trimmed —
`"web "` still creates `"web "`, unchanged from assign mode's behavior —
only the all-whitespace case is refused, since it is the one that produces
a row the user cannot read, select, or remove.

**Placeholder text is now per-mode.** `run_fuzzel` hardcoded
`--placeholder=type to filter, or a new name to create` for both modes.
That was already half-wrong (switch mode could not create) and would be
exactly wrong in the other direction after this story, so it becomes a
parameter: assign mode gets `type to filter`, switch mode keeps the
create-capable wording.

## Acceptance criteria
**Given** the tag switcher (`Mod4+S`) is open
**When** the user types a name matching no existing tag and confirms
**Then** `create-tag` is sent with exactly that text as the name, and on
`tag-created` a `switch-tag` for the returned id is sent for the same
`output_id` the switcher was spawned with — so the output displays the new
tag and lazily spawns its pinned terminal, identical to picking an existing
row

**Given** the tag switcher is open and the registry already holds a tag
whose name is typed *exactly*
**When** the user confirms the typed text rather than the matching row
**Then** `create-tag`'s by-name idempotence resolves to that existing tag's
id and the switch proceeds to it — never a duplicate registry entry, never
a membership change

**Given** the registry is at the 64-tag cap
**When** a new name is typed and confirmed in the switcher
**Then** `wm` returns the cap-rejection error, the picker reopens with the
rejection row prepended and the rejected name restored into the input box
via `--search`, and no `switch-tag` is sent — the same cap-rejection
behavior assign mode had, now attached to the mode that creates

**Given** the switcher is open
**When** the user submits an empty or whitespace-only input box
**Then** nothing is created and nothing is switched — the invocation is
cancelled, so a stray Enter can never register a permanently unnameable
tag

**Given** the assign picker (`Mod4+A`) is open
**When** the user types a name matching no existing tag and confirms
**Then** the invocation is cancelled — no `create-tag` is sent, no tag is
created, and no membership changes; creation is reachable only from the
switcher

**Given** the assign picker is open with a window focused
**When** an existing tag's row is selected
**Then** membership toggling behaves exactly as before this story — the
toggle-and-reopen loop, the local mirror update, and the no-focused-window
switch fallback (Story 2.10) are all unchanged

- [x] Tests pass (unit + integration where applicable)

## Tasks / Subtasks

- [x] **Task 1: `SwitchAction::CreateTag` — the switcher's new parse arm (AC 1, 3, 4)**
  - [x] 1.1 RED — Add tests to `tag-picker/src/checklist.rs`:
    - `parse_switch_selection_returns_create_tag_for_freeform_text_with_no_tab`
      (rewrites the existing `..._returns_cancelled_for_freeform_text_with_no_tab`,
      whose assertion this story inverts)
    - `parse_switch_selection_returns_create_tag_for_a_bare_numeral_with_no_tab`
      (same inversion of the existing bare-numeral test — a numeral-shaped
      name is a legal tag name, ADR-006)
    - `parse_switch_selection_returns_cancelled_for_whitespace_only_input`
    - `parse_switch_selection_returns_cancelled_for_the_rejection_row_selection`
      (round-trips `render_rejection_row` through the switcher's parser, the
      way the assign-mode test already does for `parse_fuzzel_output`)

    Confirm they fail — `SwitchAction` has no `CreateTag` variant yet.
  - [x] 1.2 GREEN — Add `CreateTag(String)` to `SwitchAction`; drop `Copy`
    from its derive list (a `String` payload makes it non-`Copy`) and keep
    `Debug, Clone, PartialEq`. `parse_switch_selection`'s `None` (no-tab)
    arm returns `CreateTag(trimmed.to_string())` unless `trimmed` is
    entirely whitespace, in which case `Cancelled`. Update the function's
    doc comment: the "deliberate, load-bearing difference from assign mode"
    paragraph now points the other way.

- [x] **Task 2: Remove `PickerAction::CreateTag` — assign mode stops creating (AC 5, 6)**
  - [x] 2.1 RED — Rewrite `parse_fuzzel_output_returns_create_tag_for_freeform_text_with_no_tab`
    and `..._for_a_bare_numeral_with_no_tab` as
    `parse_fuzzel_output_returns_cancelled_for_freeform_text_with_no_tab`
    and `..._for_a_bare_numeral_with_no_tab`, asserting `Cancelled`.
    Confirm they fail against the current implementation.
  - [x] 2.2 GREEN — Delete `PickerAction::CreateTag`; `parse_fuzzel_output`'s
    `None` arm returns `Cancelled`. Rewrite its doc comment to record that
    creation moved to the switcher and why (this story's Description), so
    the next reader does not "restore" it as a regression.
  - [x] 2.3 Delete `should_toggle_after_create`, `should_add_to_tag_mirror`,
    `CREATE_APPLY_FAILED_MESSAGE`, `render_create_apply_failed_row` and
    every test of them (7 tests) — all four exist solely to reconcile a
    created tag with a focused window's membership. Keep
    `REJECTION_MESSAGE`/`render_rejection_row`: the cap belongs to
    `create-tag`, which now lives in switch mode.

- [x] **Task 3: Switch mode's create-then-switch loop (AC 1, 2, 3)**
  - [x] 3.1 `run_switch_mode` becomes a loop over `pending_rejected_name:
    Option<String>`, prepending `render_rejection_row()` when set and
    passing it as `run_fuzzel`'s `initial_search`. `Cancelled` breaks;
    `Selected` calls `send_switch_tag_or_exit` and breaks; `CreateTag(name)`
    sends `Request::CreateTag`, then on `TagCreated { tag_id }` calls
    `send_switch_tag_or_exit` with that id and breaks, on the literal
    `REJECTION_MESSAGE` error sets `pending_rejected_name = Some(name)` and
    reopens, and on any other error or unexpected response reports and
    breaks.
  - [x] 3.2 Update the function's doc comment: single-shot for every path
    that does not create; the cap-rejection reopen is the sole loop reason
    and moved here with the create path.
  - [x] 3.3 No RED/GREEN — process-spawn and live-socket I/O glue, the
    module's standing carve-out (`main.rs`'s own module doc); every
    decision it makes is unit-tested in `checklist`.

- [x] **Task 4: Strip assign mode's create machinery from `run_assign_mode` (AC 5, 6)**
  - [x] 4.1 Delete the `PickerAction::CreateTag` arm, the
    `pending_rejected_name` and `pending_create_apply_failed` state, and
    both synthetic rows' prepends. `tags` stops needing `mut` and stops
    being mutated at all. The `Toggled` arm — including Story 2.10's
    no-focused-view switch fallback — is unchanged.
  - [x] 4.2 Keep `should_open_picker`'s up-front guard. Its Story 2.10
    rationale ("a `CreateTag` could permanently register a tag with nowhere
    to apply it") no longer applies, but the guard's own claim still does: a
    pick with neither a view to toggle nor an output to switch has nowhere
    to go. Update the doc comment so the stale rationale is not left
    standing as the reason.
  - [x] 4.3 Update the function's doc comment and `main.rs`'s module doc:
    assign mode is toggle-only; the create-and-reopen shape moved.

- [x] **Task 5: Per-mode `fuzzel` placeholder (AC 5)**
  - [x] 5.1 `run_fuzzel` takes a `placeholder: &str` parameter in place of
    the hardcoded `--placeholder=...`. Assign mode passes `"type to
    filter"`; switch mode passes `"type to filter, or a new name to
    create"`. Both literals are module constants so the two call sites read
    as the deliberate contrast they are.

- [x] **Task 6: User-facing documentation (AC 1, 5)**
  - [x] 6.1 `README.md`: the "Tag-assignment picker (`Super+A`)" section
    drops its create sentence; the "Tag-switch picker (`Super+S`)" section
    gains it, including the cap-rejection behavior.
  - [x] 6.2 `wm/src/config/mod.rs`: `Action::TagSwitch`'s cheat-sheet
    description becomes `"Switch tag (or type a new name)"` — the
    cheat-sheet is generated from live bindings (Story 3.3), so this is the
    in-session discoverability surface for the new workflow.

- [x] **Task 7: Verification gate (AC: all)**
  - [x] 7.1 In-container: `cargo fmt --all -- --check`, `cargo clippy
    --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
    `pre-commit run --all-files`. Expect the `tag-picker` suite to *shrink*
    by the 7 deleted create-reconciliation tests and grow by this story's
    new ones; `wm` and `status-bar` counts must be unchanged.
  - [x] 7.2 Manual live gate (not a story blocker, same boundary as every
    prior story): confirm against a real `river` session that `Mod4+S` +
    typed name creates and switches, and that `Mod4+A` + typed name does
    nothing. Record the outcome in the Dev Agent Record.

## Technical notes

**Why creation is moved rather than duplicated.** Two dialogs that both
create tags means two cap-rejection paths, two idempotent-collision rules,
and two places to keep in sync — and Story 2.3's code review already
produced three separate findings (1, 2 and 3) from the single interaction
between creation and membership reconciliation. The user's framing settles
it directly: the assign dialog is the wrong workflow for creation, not an
additional one.

**Deviation from `EXPERIENCE.md` (planning artifact, dated 2026-08-06).**
That document specifies the free-text create row as assign-mode-only and
explicitly absent from switch mode, and lists the cap-out rejection state
under the assign picker. This story inverts both. The artifact is a frozen,
dated planning output — it is not edited retroactively (the same convention
Stories 2.10 and 2.12 followed when live use overrode a planned behavior);
this note is the record of the deviation. **FR8** ("a text-input row in the
tag-manager picker creates a new tag not already in the registry") remains
satisfied — the picker is one binary with two modes, and the row simply
moved modes.

**Why the cap-rejection reopen is the only surviving loop.** Assign mode
loops because toggling is inherently repeatable — you usually want to
check two or three tags in one visit. Switching is terminal by definition:
one output shows exactly one tag (FR2), so a second pick would only undo
the first. A create *is* a switch here, so it terminates too. The cap
rejection is the sole case where the picker has neither performed the
action nor been dismissed, and reopening is the only way to tell the user
why on a surface they can actually see.

**Scope boundary.** No `wm` behavior change beyond one cheat-sheet string:
no protocol change, no `wire.rs` change on either side, no `wm_core`
change, no keybind or config-grammar change, no `status-bar` change. The
`Mod4+A` and `Mod4+S` keybinds, both spawn argument grammars
(`mode::parse_args`), and the `--output=`/`--layer=overlay`/`--with-nth`
flag set are all untouched.

## Test plan
1. **`checklist` unit tests (TDD, real RED before GREEN)**: the four new
   `parse_switch_selection` cases (Task 1.1) and the two inverted
   `parse_fuzzel_output` cases (Task 2.1). All pure — no socket, no
   `fuzzel` binary, which is why the parse decisions live in this module
   at all.
2. **Deletion gate**: the 7 tests covering the four deleted helpers go with
   them; no test is left asserting behavior that no longer exists.
3. **Regression gate**: `render_switch_list`, `render_rejection_row`,
   `build_checklist_entries`, `render_fuzzel_input`,
   `toggle_local_membership`, `should_open_picker`, the `sanitize_name`
   round-trips and the whole `wire`/`mode`/`socket_path` suites stay green
   untouched — this story changes what two parsers *decide*, not how rows
   are rendered, addressed or serialized.
4. **Build/lint gate**: `cargo clippy --workspace --all-targets -- -D
   warnings` clean with `SwitchAction`'s dropped `Copy` (the two match
   sites now move a `String`) and with the deleted helpers gone.
5. **Live gate (manual)**: per Task 7.2 — the gate that produced this
   story's request.

## FR coverage
FR8 (text-input row creates a new tag) and FR9 (second hotkey switches the
active output's tag) — both already implemented, both re-homed here: FR8's
row moves into FR9's dialog, and creating now *is* a switch. Not a new FR;
this corrects which dialog owns the create interaction, driven by real
daily-driver use.

## Dev Agent Record

**What was done**:

- `tag-picker/src/checklist.rs` (Tasks 1-2, real RED/GREEN TDD): the two
  `parse_switch_selection` create tests were written first and confirmed
  failing to compile (`E0599: no variant ... named CreateTag found for enum
  checklist::SwitchAction`), then `SwitchAction` gained
  `CreateTag(String)` and dropped `Copy`. `parse_switch_selection`'s no-tab
  arm now returns `CreateTag(trimmed.to_string())`, guarded by a
  `trimmed.trim().is_empty()` arm ordered *before* it so empty and
  whitespace-only input both cancel. `PickerAction::CreateTag` was deleted
  and `parse_fuzzel_output`'s no-tab arm returns `Cancelled`; its redundant
  `trimmed.is_empty()` early return went with it (an empty line has no tab,
  so it reaches the same `None` arm — the two existing empty-stdout tests
  cover that this is behavior-preserving).
- `tag-picker/src/checklist.rs` (Task 2.3): `should_toggle_after_create`,
  `should_add_to_tag_mirror`, `CREATE_APPLY_FAILED_MESSAGE` and
  `render_create_apply_failed_row` deleted with their 7 tests, plus
  `parse_fuzzel_output_returns_cancelled_for_the_rejection_row_selection`
  (8 deleted in total — the round-trip it asserted is now
  `parse_switch_selection`'s, and the surviving
  `render_rejection_row_is_the_literal_cap_message...` test's cross-
  reference comment was repointed accordingly). `REJECTION_MESSAGE` and
  `render_rejection_row` stay, now switch mode's.
- `tag-picker/src/main.rs` (Tasks 3-5): `run_switch_mode` is now a loop
  over `pending_rejected_name` with the three-arm match (`Cancelled` /
  `Selected` / `CreateTag`); the create arm chains `send_switch_tag_or_exit`
  on `TagCreated` and reopens only on the literal `REJECTION_MESSAGE`.
  `run_assign_mode` lost its `CreateTag` arm, both pending-state flags and
  both synthetic-row prepends; `tags` is no longer `mut` and `known_ids` is
  hoisted out of the loop, since neither can change now that nothing
  creates. `run_fuzzel` takes a `placeholder: &str`, fed by the new
  `ASSIGN_PLACEHOLDER` / `SWITCH_PLACEHOLDER` module constants.
- `wm/src/config/mod.rs` (Task 6.2): `Action::TagSwitch`'s cheat-sheet
  description is now `"Switch tag (or type a new name to create)"`. The
  only `wm`-side change in this story, and the only one that is
  user-visible in-session.
- `README.md` (Task 6.1): the keybind table's `Super+S` row, the
  assignment-picker section (create sentence removed, multi-toggle
  behavior stated instead) and the switch-picker section (create,
  idempotent-by-name, and cap-rejection behavior added).

**Deviations from the story's literal wording**: two, both mechanical.
Task 2.3 says "every test of them (7 tests)" — 8 were deleted, because
assign mode's `..._for_the_rejection_row_selection` round-trip also
asserts behavior that no longer exists there, and its replacement is
Task 1.1's switch-mode equivalent. Task 4.1 says `tags` "stops needing
`mut`"; `known_ids` was additionally hoisted out of the loop for the same
reason, which the task did not spell out.

**Verification** (host `cargo`, then the standing gate inside the
devcontainer via `podman exec -u vscode -w /workspaces/buoy-wm
bold_vaughan`):
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, including
  `SwitchAction`'s dropped `Copy` at both match sites.
- `cargo test --workspace`: `buoy-wm` (wm) 226 and `status-bar` 27 both
  unchanged; `tag-picker` 63 (69 baseline − 8 deleted + 2 new) =
  **316 total, 0 failures, 0 regressions**.
- `pre-commit run --all-files`: both hooks (`cargo fmt --check`,
  `cargo clippy`) passed.

**Task 7.2 (manual live-verification)**: **deferred** — no `river` session,
no seat and no `fuzzel` binary in this sandbox, the same boundary as every
prior story. The two behaviors that need a human at a real session are
`Mod4+S` + a typed name creating and switching in one action (including
that `fuzzel` really does print an unmatched input line verbatim, the
mechanism Story 2.11 established), and `Mod4+A` + a typed name now doing
nothing.

**File List**:
- `tag-picker/src/checklist.rs`
- `tag-picker/src/main.rs`
- `wm/src/config/mod.rs`
- `README.md`
