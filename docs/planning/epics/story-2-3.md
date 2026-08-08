---
baseline_commit: b1af4ab
---

# Story 2.3: Tag-Manager Picker — Create New Tag
Epic: 2 | Priority: H | Status: review

## Description
Extends `tag-picker`'s existing assign-mode loop (Story 2.2, `Mod4+A`) so
that typing a name not already in the registry and confirming creates that
tag over IPC and immediately applies it to the focused window, in the same
action. No new hotkey, no new WM-side wiring, and no changes to
`wm/src/ipc/protocol.rs`/`dispatch.rs` — Story 2.1 already implemented
`create-tag` completely (`Request::CreateTag { name }` →
`Response::TagCreated { tag_id }` or `Response::Error { message: "tag limit
reached (64)" }`), and this story's own work is entirely inside the
`tag-picker` binary: reusing the already-open `fuzzel` loop's own text input
box as the "text-input row" (there is no separate rendered row to build),
extending the toggle-and-reopen loop's decision logic to recognize a
free-text confirmation as a create attempt rather than only a cancel, and
rendering the 64-tag-cap rejection as a literal row on the next reopen.

**Four gaps neither the PRD, `EXPERIENCE.md`/`DESIGN.md`, the mockup, nor
Story 2.2 fully resolve, closed here rather than silently invented or
deferred** (see Technical notes for full rationale on each):

1. **How `tag-picker` tells "the user selected an existing tag" apart from
   "the user typed a new name" out of one bare stdout string.** Neither the
   PRD nor `EXPERIENCE.md` says how a single-select `fuzzel --dmenu`
   invocation (Story 2.2's spike finding — no native multi-widget input
   exists) is supposed to also accept free text for tag creation.
   **Resolution:** confirmed against `fuzzel(1)`'s real, current man page
   (no live spike needed — see Technical notes "Free-text disambiguation
   evidence"): in `--dmenu` mode, `--accept-nth`'s column transform applies
   only when an entry is actually selected/matched; unmatched input is
   printed back verbatim, untransformed. `tag-picker` therefore
   disambiguates by checking whether the returned string is a `u8` that is
   also one of the tag ids actually rendered in *this* invocation — if so,
   it's a toggle; otherwise, it's a create-tag attempt using the returned
   string as the literal name. This is a real, if narrow, behavioral
   change from Story 2.2's own defensive "any unparseable-as-in-range-`u8`
   stdout means cancelled" rule, and repurposes one of that story's
   existing tests accordingly (Technical notes explains why this is a
   deliberate widening, not a regression).
2. **How the 64-tag-cap rejection becomes a "literal message row" inside a
   sequential toggle-and-reopen loop that has no persistent UI state
   between invocations.** **Resolution:** on a `TagLimitReached` error,
   `tag-picker` remembers the rejected name locally and, on the next
   `fuzzel` reopen, prepends a synthetic row rendering the literal string
   `"tag limit reached (64)"` (dispatch.rs's own error string, quoted
   verbatim — see Technical notes) to the stdin list, and passes
   `--search=<rejected name>` so the attempted text is still visible in the
   input box, matching `mockups/key-picker.html`'s cap-out state. The
   synthetic row's `--accept-nth` column is deliberately left empty, which
   reuses the already-existing "empty stdout is always cancelled" rule
   (gap #1's Technical notes) as its dismissal mechanism at zero extra
   code.
3. **Whether creating a tag needs a second, separate confirm step to apply
   it to the focused window.** `EXPERIENCE.md`'s Component Patterns is
   explicit: "Typing + Enter creates a new tag and checks it for the
   focused window in the same action (FR8)." **Resolution:** `tag-picker`
   chains `create-tag` immediately followed by `toggle-tag` (adding the new
   tag) on the same connection, mirroring how Story 2.1's own dispatch
   layer already exposes both as the two primitives Story 1.7's raw
   keybinds independently exercise — no new WM-side "create-and-assign"
   request type is needed or added.
4. **Whether this story deprecates/removes `Mod4+T`
   (`Action::TagCreate`/`wm_core::create_tag_with_generated_name`), now
   that real, named tag creation exists via the picker.** FR13's own
   framing (Story 1.7) called the raw keybind "sufficient to validate
   multi-tag behavior end-to-end before the fuzzel-based picker exists" —
   phrasing that reads as scoping it as a stopgap, but no PRD line, ADR, or
   this story's own AC actually asks for its removal, and no other story in
   Epic 2 claims it either. **Resolution: kept, coexisting, out of scope.**
   `Mod4+T` still generates a placeholder `tag<N>` tag with no text-input
   needed at all, which remains useful as a fast way to manufacture
   multiple tags for testing/demoing the picker itself; removing it is a
   candidate for a future cleanup story once a human consciously decides
   the stopgap has outlived its purpose, not an implicit side effect of
   this one. No `wm/src/main.rs` changes of any kind are needed for this
   story as a result.

## Acceptance criteria
**Given** the tag-manager picker is open in assign mode (`Mod4+A`, Story
2.2) and the registry is below its 64-tag cap
**When** I type a name not already in the registry into fuzzel's own input
box and press Enter (no row matched — fuzzel's real `--dmenu` behavior
prints the typed text back verbatim rather than an `--accept-nth` column,
per Technical notes)
**Then** `tag-picker` sends `{"type":"create-tag","name":"<typed text>"}`
over the already-open IPC connection; on `{"type":"tag-created","tag_id":
<id>}` it immediately sends `{"type":"toggle-tag","view_id":<id>,"tag_id":
<id>}` on the same connection to apply the new tag to the focused window
(ADR-006 rules apply: the registry assigns the next sequential id,
idempotent by name); on `{"type":"ok"}` the loop's local tag list/
membership are updated and `fuzzel` reopens showing the new tag checked,
with no separate confirm step

**Given** the same setup, but the typed text exactly matches an existing
registry tag's rendered id (a real `u8` that is also one of the ids
`tag-picker` rendered in this invocation)
**Then** this is a toggle of that existing tag, not a tag-creation attempt
— Story 2.2's existing behavior is unchanged and takes precedence

**Given** the registry is at its 64-tag cap
**When** I type a new name and confirm
**Then** `tag-picker` sends `create-tag`, receives
`{"type":"error","message":"tag limit reached (64)"}`, and — rather than
silently failing or exiting — reopens `fuzzel` with a literal message row
reading `"tag limit reached (64)"` prepended to the checklist and the
rejected name pre-filled in the input box (`--search`); the tag is not
created and no `toggle-tag` is ever sent for it
**And** dismissing that state (Escape, or selecting the message row itself)
ends the loop the same way any other cancel does — no tag is created

**Given** any other `create-tag` error `wm` might return (not the
tag-limit message)
**Then** `tag-picker` prints it to stderr and ends the loop, the same
generic-error handling Story 2.2 already applies to `toggle-tag` errors —
no rejection-row state is entered for errors other than the exact
tag-limit-reached string

- [x] Tests pass (unit + integration where applicable)
- [ ] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: Resolve the free-text-vs-selection disambiguation and the rejection-row-dismissal mechanism (gaps #1-#2; no RED/GREEN — a documented design decision gating every later task, same class as Story 2.2's Task 1 spike)**
  - [x] 1.1 Confirm (again) that no live `fuzzel` spike is possible: `which fuzzel` and `man fuzzel` both still fail on the host and inside the running `bold_vaughan` devcontainer — same sandbox boundary Story 2.2 hit. Resolve via `fuzzel(1)`'s current, authoritative man page instead (Arch Linux package build, version 1.14.1) — already fetched for this story; see Technical notes "Free-text disambiguation evidence" for the exact quoted passages, not just a paraphrase.
  - [x] 1.2 Record the disambiguation rule: `checklist::parse_fuzzel_output` gains a `known_tag_ids: &[u8]` parameter (the ids actually rendered in *this* invocation). Non-empty, exit-success stdout that parses as `u8` **and** is a member of `known_tag_ids` is `Toggled`; anything else non-empty is `CreateTag(<the trimmed stdout, verbatim>)`. Exit failure or empty (trimmed) stdout is unconditionally `Cancelled`, checked before the create/toggle split — unchanged from Story 2.2's own defensive rule, and this is what makes the rejection row's dismissal "free" (its `--accept-nth` column is deliberately left empty; selecting it or pressing Escape both land on the same empty/failure-exit `Cancelled` path).
  - [x] 1.3 Record the accepted residual ambiguity, not hidden: a free-typed name that is itself a bare numeral equal to a tag id already rendered in this invocation (e.g. typing literal text `"3"` when tag id 3 already exists) is indistinguishable from selecting that row, and resolves to `Toggled(3)`, not tag creation. Flagged as an accepted v1 edge case — single-user hobby stakes (`non-functional-requirements.md`), the outcome is reversible (toggling an existing tag, not data loss), and no realistic tag name in this project's own usage (`web`, `chat`, `deploy-watch`, …) is a bare numeral.
  - [x] 1.4 Record that this repurposes, not silently breaks, one of Story 2.2's existing `checklist.rs` tests: `parse_fuzzel_output_returns_cancelled_for_tag_id_out_of_u8_range` (`"999"` → `Cancelled`) encoded a defensive rule from *before* `CreateTag` existed, where no legitimate reason for an out-of-registry numeral to come back from `fuzzel` was known. Now that free text is a real, first-class outcome, `"999"` not matching any known id is exactly the `CreateTag("999")` case (an odd tag name, but ADR-006 doesn't forbid numeral-shaped tag names) — Task 3 renames/repurposes this test rather than deleting its coverage outright.
  - [x] **No RED/GREEN** — documented decision only, sourced from real, current, dated evidence (`fuzzel(1)` v1.14.1, Arch Linux, quoted in Technical notes), not a live interactive test (same class of gap Story 2.2's Task 1 already carried, not a new one this story introduces).

- [x] **Task 2: `tag-picker/src/wire.rs` — add `CreateTag`/`TagCreated` to the client's protocol mirror (AC: "`tag-picker` sends `create-tag`... on `tag-created`...")**
  - [x] 2.1 RED — In `tag-picker/src/wire.rs`'s existing `#[cfg(test)] mod tests`: `serializes_create_tag_request` — `serialize_request(&Request::CreateTag{name:"web".into()})` equals `r#"{"type":"create-tag","name":"web"}"#`. `parses_tag_created_response` — `parse_response(br#"{"type":"tag-created","tag_id":5}"#)` equals `Ok(Response::TagCreated{tag_id:5})`. Leave `parse_response_rejects_unknown_type`'s existing `"tag-created"`-based test body alone but update its doc comment — that literal is no longer "a real `wm` response shape this client just doesn't model" once this task lands; swap its unknown-type fixture to a genuinely still-unmodeled type (`"switch-tag"`-triggered `"ok"` is already covered elsewhere, so use a clearly bogus type like `"delete-everything"`, matching `wm`'s own `protocol.rs` test precedent) so the test keeps testing what it claims to test. Confirm all new/changed tests fail to compile or fail as expected.
  - [x] 2.2 GREEN — Add `CreateTag { name: String }` to the client's `Request` enum (alongside the existing `GetState`/`ToggleTag`) and `TagCreated { tag_id: u8 }` to the client's `Response` enum (alongside `State`/`Ok`/`Error`) — both mirroring `wm/src/ipc/protocol.rs`'s shapes exactly, still no `SwitchTag`/`OutputDto`/`app_id` (Story 2.4's job, still out of scope here). Update this module's own doc comment, which currently states outright "no `CreateTag`/`SwitchTag` requests, no `TagCreated` response" — that sentence is now half-wrong and must be corrected to describe a client that sends `get-state`/`toggle-tag`/`create-tag` and understands `state`/`ok`/`tag-created`/`error`, while still explicitly noting `switch-tag` remains unmodeled (Story 2.4). Confirm all tests pass; confirm no regression in `wm`'s own, separate `protocol.rs` (untouched by this task — Story 2.1 already implemented the wire shape this mirrors).

- [x] **Task 3: `tag-picker/src/checklist.rs` — extend the toggle-and-reopen loop's decision logic for create-vs-toggle and the rejection row (AC: disambiguation, cap-rejection row, dismissal)**
  - [x] 3.1 RED — In `checklist.rs`'s existing `#[cfg(test)] mod tests`: extend `PickerAction` usage across new tests for a `CreateTag(String)` variant. `parse_fuzzel_output_returns_toggled_when_stdout_matches_a_known_tag_id` — `parse_fuzzel_output(true, "1\n", &[0,1,2])` equals `PickerAction::Toggled(1)`. `parse_fuzzel_output_returns_create_tag_when_stdout_does_not_match_any_known_tag_id` — `parse_fuzzel_output(true, "deploy-watch\n", &[0,1,2])` equals `PickerAction::CreateTag("deploy-watch".into())`. `parse_fuzzel_output_returns_create_tag_for_numeral_not_among_known_ids` — the Task 1.4-repurposed test: `parse_fuzzel_output(true, "999\n", &[0,1,2])` equals `PickerAction::CreateTag("999".into())` (supersedes the old "cancelled for out-of-range" expectation; doc comment explains why, cross-referencing Task 1.4). `parse_fuzzel_output_returns_cancelled_for_empty_stdout_regardless_of_known_ids` — `parse_fuzzel_output(true, "", &[0,1,2])` equals `Cancelled` (the rejection-row-dismissal mechanism's actual proof). `parse_fuzzel_output_returns_cancelled_for_nonzero_exit_even_with_freeform_stdout` — `parse_fuzzel_output(false, "deploy-watch\n", &[0,1,2])` equals `Cancelled` (exit status still wins over content, unchanged principle from Story 2.2). `parse_fuzzel_output_treats_known_id_with_leading_zero_or_whitespace_consistently_with_toggle` — `parse_fuzzel_output(true, " 1 \n", &[0,1,2])` equals `Toggled(1)` (trim behavior, already relied on, now exercised alongside the new branch). `render_rejection_row_is_the_literal_cap_message_with_an_empty_accept_column` — `render_rejection_row()` equals `"tag limit reached (64)\t\n"` (column 1 the exact literal string dispatch.rs's `describe_wm_core_error` returns for `TagLimitReached`, column 2 empty — the dismissal mechanism from Task 1.2). Confirm all new/changed tests fail to compile.
  - [x] 3.2 GREEN — Change `PickerAction` to `pub enum PickerAction { Toggled(u8), CreateTag(String), Cancelled }`. Change `parse_fuzzel_output`'s signature to `pub fn parse_fuzzel_output(exit_success: bool, stdout: &str, known_tag_ids: &[u8]) -> PickerAction`: `if !exit_success { return Cancelled }`; `let trimmed = stdout.trim(); if trimmed.is_empty() { return Cancelled }`; `match trimmed.parse::<u8>() { Ok(id) if known_tag_ids.contains(&id) => Toggled(id), _ => CreateTag(trimmed.to_string()) }`. Add `pub const REJECTION_MESSAGE: &str = "tag limit reached (64)";` with a doc comment cross-referencing `wm/src/ipc/dispatch.rs`'s `describe_wm_core_error`/`TagLimitReached` arm as the single source of truth for the literal string (a deliberate, documented cross-crate literal-string duplication — same gap-flagging convention as `socket_path.rs`'s existing "keep these in sync" note, not a logic duplication) — `tag-picker` never re-derives this string, it only recognizes it when `wm` sends it back. Add `pub fn render_rejection_row() -> String { format!("{REJECTION_MESSAGE}\t\n") }`. Update every doc comment referencing the old two-variant `PickerAction`/three-argument-free `parse_fuzzel_output` (module-level and function-level). Confirm all new tests pass; confirm every pre-existing `checklist.rs` test that called the old two-argument `parse_fuzzel_output` signature is updated to pass a `known_tag_ids` slice matching that test's own scenario (e.g. Story 2.2's `parse_fuzzel_output_returns_toggled_for_successful_exit_and_valid_tag_id` needs `&[3]` added) rather than left broken.

- [x] **Task 4: `tag-picker/src/main.rs` — orchestrate the create-then-assign chain and the rejection-row reopen (AC: all; process-spawn/live-socket glue, same carve-out class as Story 2.2's Task 6)**
  - [x] 4.1 Extend `run_fuzzel`'s signature to `fn run_fuzzel(input: &str, initial_search: Option<&str>) -> (bool, String)`: when `initial_search` is `Some(text)`, add `.arg(format!("--search={text}"))` to the spawned command (real, documented flag — "pre-fills the input box with the specified string"), used to restore the rejected name into view on a rejection-row reopen. Also add a static `.arg("--placeholder=type to filter, or a new name to create")` unconditionally (an implementation-decision gap-fill, same class as Story 2.2's `Mod4+A` hotkey-letter choice — no document specifies exact placeholder copy, `DESIGN.md` only names the `picker-newrow-placeholder` token's color, not its text).
  - [x] 4.2 In the loop, maintain `pending_rejected_name: Option<String>` (starts `None`) alongside the existing `current_tags`/`tags`. Each iteration: compute `known_ids: Vec<u8> = tags.iter().map(|t| t.id).collect()`; build `input` as `checklist::render_rejection_row()` prepended (only when `pending_rejected_name.is_some()`) to `checklist::render_fuzzel_input(&checklist::build_checklist_entries(&tags, &current_tags))`; call `run_fuzzel(&input, pending_rejected_name.as_deref())`; call `checklist::parse_fuzzel_output(exit_success, &stdout, &known_ids)`.
  - [x] 4.3 On `PickerAction::Toggled(tag_id)`: unchanged from Story 2.2 (send `ToggleTag`, apply `toggle_local_membership` on `Ok`, `break` on `Error`/unexpected), plus set `pending_rejected_name = None` (selecting a real row abandons any in-progress rejected creation attempt).
  - [x] 4.4 On `PickerAction::CreateTag(name)`: send `wire::Request::CreateTag { name: name.clone() }` on the same connection. On `Response::TagCreated { tag_id }`: push `wire::TagDto { id: tag_id, name: name.clone() }` onto the local `tags` list (so it renders as an ordinary checked row on the very next reopen, per `EXPERIENCE.md`'s "in the same action"), then immediately send `wire::Request::ToggleTag { view_id, tag_id }` on the same connection; on that `Response::Ok`, call `checklist::toggle_local_membership(&mut current_tags, tag_id)`, set `pending_rejected_name = None`, and loop again; on that leg's `Response::Error { message }` or an unexpected response, `eprintln!` and `break` (the newly-created tag still exists registry-side even if this second call fails — not rolled back, same "no compensating transaction" precedent as every other multi-step IPC sequence in this codebase). On the `CreateTag` request's own `Response::Error { message }`: if `message == checklist::REJECTION_MESSAGE`, set `pending_rejected_name = Some(name)` and loop again (reopen with the rejection row); for any other message, `eprintln!` and `break` (generic-error precedent, unchanged from Story 2.2's `toggle-tag` handling).
  - [x] 4.5 On `PickerAction::Cancelled`: unchanged, `break` — this is also the path a selected-rejection-row or an Escape-while-rejected both take (Task 1.2/3.1's empty-stdout rule), so no separate "dismiss" branch is needed.
  - [ ] **No RED/GREEN** — process-spawn and live-socket I/O glue, same carve-out class as Story 2.2's Task 6/Story 2.1's Task 7: every decision this composes (`parse_fuzzel_output`, `render_rejection_row`, `build_checklist_entries`, `render_fuzzel_input`, `toggle_local_membership`) is unit-tested in Tasks 2-3. Verified via `cargo build --workspace`/`cargo clippy --workspace --all-targets -- -D warnings` and structural code review against 4.1-4.5's description above.

- [x] **Task 5: Full in-container verification gate, across the workspace (AC: all)**
  - [x] 5.1 Run, inside the devcontainer — `devpod ssh buoy-wm -- <cmd>` first; fall back to `podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>` against the already-running container if `devpod ssh`'s transport is unreliable again (it was for both Story 2.1 and Story 2.2's own sessions) — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, all from the repo root. Confirm the full pre-existing `wm` + `tag-picker` suite (147 `wm` + 31 `tag-picker` as of Story 2.2's own final count) stays green apart from the deliberate Task 3 signature/behavior changes to `parse_fuzzel_output`'s call sites, plus this story's new tests, all passing. `devpod ssh buoy-wm -- echo ok` was confirmed unreliable again this session (`Error tunneling to container: wait: remote command exited without exit status or exit signal`); `podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>` used throughout instead. Confirmed: `cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace` green — 147 `wm` tests (unchanged) + 39 `tag-picker` tests (31 baseline + 2 new `wire.rs` + 6 net new/repurposed `checklist.rs`), 0 failures.
  - [x] 5.2 Run `pre-commit run --all-files` as the `vscode` user inside the container — confirm both the workspace-wide `cargo fmt --check` and `cargo clippy` hooks (already widened to cover both members since Story 2.2) still pass. Confirmed: both hooks passed.
  - [x] 5.3 Manual smoke-test note (same explicit, not-downplayed gap class as Story 2.2's Task 8.3): no live `fuzzel`/Wayland session exists in this sandbox, so the real create-then-toggle IPC sequence, the real `--search`-prefilled reopen, and the real appearance of the rejection row have not been exercised end-to-end. Every *decision* is unit-tested (Tasks 2-3); the live gap is recorded here and carried forward, same as Story 2.2's own carried-forward gap into this story.

## Technical notes

**Free-text disambiguation evidence (Task 1) — quoted, not paraphrased,
from `fuzzel(1)`'s current man page** (Arch Linux package build, version
1.14.1; fetched fresh for this story since neither the host nor the
`bold_vaughan` devcontainer has `fuzzel` installed or a man page for it,
same sandbox boundary as Story 2.2's own spike):

> **-d, --dmenu** — dmenu compatibility mode. In this mode, the list
> entries are read from stdin (newline separated). The selected entry is
> printed to stdout. **If the input string does not match any of the
> entries, the input string is printed as is on stdout.**

> **--accept-nth**=_N|FMT_ — Output the N:th column... of each input line
> to stdout. dmenu mode only... Example: `printf "1\tFirst\n2\tSecond" |
> fuzzel -d --accept-nth=2` — this will display the entries **1 First** and
> **2 Second**. **Depending on which one is selected**, First or Second
> will be printed to stdout.

Together these confirm exactly what Story 2.2's own `--with-nth=1
--accept-nth=2` setup already relies on: the column-2 transform is applied
only to a genuinely *selected* (matched) row; free-typed text that matches
nothing is echoed back raw, untransformed, exactly as the user typed it.
There is no ambiguity in fuzzel's own behavior — the ambiguity this story
resolves is purely on `tag-picker`'s receiving end, where both outcomes
arrive as one bare string with no tag saying which path produced it. The
resolution (Task 1.2): treat a `u8`-parseable string as a toggle **only**
if it's also among the ids `tag-picker` actually rendered this invocation;
anything else is a create-tag attempt. This is strictly a receiving-side
disambiguation rule, not a new fuzzel flag or capability — no live
`fuzzel` spike is needed to verify it, unlike Story 2.2's own multi-select
question, because the relevant behavior (verbatim echo on no-match) is
explicitly documented, current, and unambiguous in the man page text
above.

**Why this isn't just "parse as u8, else create" (the id-membership
check).** An earlier, simpler design considered treating *any* successful
`u8` parse as a toggle (Story 2.2's original rule, minus the range check).
That's wrong once `create-tag` exists: a user could legitimately want to
create a tag literally named `"7"`, and if `7` isn't currently a
registered tag id, there is no real row `--accept-nth` could have produced
`"7"` from — so it must be free text. Checking membership against
`known_tag_ids` (the ids actually in `tags` at render time, threaded
through from `main.rs`'s loop state) is what correctly separates "this
came from a matched row" from "this is coincidentally numeral-shaped free
text," in the one direction that matters (a real numeral id always toggles
correctly; only the reverse — free text that collides with a *currently
live* id — remains ambiguous, and is accepted as a documented v1 edge case
per Task 1.3).

**Cap-rejection row mechanism (Task 2) — reusing the empty-stdout-is-
cancelled rule, not inventing a sentinel.** `EXPERIENCE.md`'s "Picker —
cap-out rejection" state and `mockups/key-picker.html`'s second panel both
show the rejected name still visible in the input box alongside a literal
message row beneath it. Given the toggle-and-reopen loop's architecture
(no persistent UI state between separate `fuzzel` invocations — Story
2.2's own established shape), the only way to reproduce this is for
`tag-picker` to remember the rejected name across one loop iteration and
re-render it on the next `fuzzel` invocation: a synthetic row carrying the
literal `"tag limit reached (64)"` text (dispatch.rs's own error string,
verbatim — see its doc comment: *"the single place that string is
defined — Story 2.3's picker renders it verbatim, not a duplicated
literal"*, which this story fulfills by defining `checklist::
REJECTION_MESSAGE` as that exact literal, not by re-typing it a second
place with its own meaning), plus `--search=<rejected name>` to restore
the typed text. Selecting that synthetic row, or pressing Escape while
it's showing, both need to *not* create a tag and *not* re-attempt with
garbage input — solved for free by giving that row's `--accept-nth` column
an empty second field: empty (trimmed) stdout is already, and remains,
unconditionally `Cancelled` (Task 1.2), so no separate sentinel value or
dismissal branch is needed anywhere in `main.rs`'s orchestration.
**Visual-fidelity consequence, flagged not hidden (same class as Story
2.2's checkbox-color finding):** `DESIGN.md`'s `picker-rejection-row`
component specifies an error-wash background and `accent-error` text color
that `fuzzel --dmenu` cannot render per-row (Story 2.2's spike already
established fuzzel has one global text/selection color for the whole
list) — the rejection row satisfies the Accessibility Floor ("color is
never the sole carrier of state," since the literal text itself carries
the meaning) but is visually plainer than `mockups/key-picker.html`'s
colored mock.

**Create-then-assign chaining (Task 3) — no new WM-side request type.**
`EXPERIENCE.md` requires creation and assignment to happen "in the same
action," but `wm/src/ipc/dispatch.rs` (Story 2.1) only exposes `create-tag`
and `toggle-tag` as separate primitives — the same two primitives Story
1.7's raw keybinds independently exercise, and Story 2.1's own AC requires
IPC mutations go through exactly those existing `wm-core` functions, not a
new combined one invented for this story. `tag-picker` composes them
client-side: `create-tag` → (on success) `toggle-tag` for the same tag id
against the focused `view_id`, both on the one already-open connection.
This mirrors how Story 2.2's loop already chains "read state once, then
many sequential single-purpose requests" rather than needing a
multi-step-aware WM-side protocol.

**`Mod4+T` disposition (gap #4) — kept, not touched.** No document in this
repo (PRD, ADRs, `epics.md`, prior story files) states that the raw
tag-create keybind is removed once the picker's free-text row exists;
FR13's "sufficient... before the fuzzel-based picker exists" phrasing
describes *why* it was built, not a sunset condition tied to this specific
story. Removing a still-functioning, still-tested keybind with no AC
asking for it would be scope creep in the opposite direction (silently
deleting behavior nothing here requires deleting) — same discipline this
project applies to not silently adding scope. `wm/src/main.rs` is
untouched by this story.

No new ADR. Every gap above is an implementation-decision gap-fill within
this story's own existing scope (matching Story 2.1's four and Story 2.2's
five precedents), not a decision among architecturally significant
alternatives `adrs.md` needs to record.

Build/test/lint only ever run **inside the devcontainer**, `devpod ssh
buoy-wm` first, falling back to `podman exec -u vscode -w
/workspaces/buoy-wm bold_vaughan <cmd>` if that transport is unreliable —
confirmed unreliable again as recently as Story 2.2's own session, and the
`bold_vaughan` container is confirmed still running and still `fuzzel`-less
as of this story's own research pass.

## Test plan
This story's tests split into four tiers:

1. **Design resolution** (Task 1) — no automated tests; the deliverable is
   a documented decision sourced from `fuzzel(1)`'s current, authoritative
   man page (quoted verbatim in Technical notes), gating Tasks 2-4's
   design. No live spike is needed or attempted, unlike Story 2.2's Task
   1 — the relevant fuzzel behavior (verbatim echo on no-match) is
   explicitly, unambiguously documented.
2. **`tag-picker` wire protocol** (Task 2) — `tag-picker::wire`, two new
   unit tests (`serializes_create_tag_request`,
   `parses_tag_created_response`) plus one existing test's fixture/doc
   comment corrected (`parse_response_rejects_unknown_type`, whose
   `"tag-created"` fixture stops being a valid "unmodeled type" example
   once this task lands), run via `cargo test --workspace`, no sockets, no
   `fuzzel` process.
3. **`tag-picker` decision logic** (Task 3) — `tag-picker::checklist`, new
   and changed unit tests covering: the toggle/create split by id
   membership (`parse_fuzzel_output_returns_toggled_when_stdout_matches_a_
   known_tag_id`, `..._returns_create_tag_when_stdout_does_not_match_any_
   known_tag_id`), the repurposed out-of-range case
   (`..._returns_create_tag_for_numeral_not_among_known_ids`, replacing
   Story 2.2's now-superseded "cancelled for out-of-range" expectation,
   with its doc comment explaining why), the empty-stdout-is-always-
   cancelled rule doubling as the rejection-row dismissal mechanism
   (`..._returns_cancelled_for_empty_stdout_regardless_of_known_ids`),
   exit-status-wins-over-content (`..._returns_cancelled_for_nonzero_exit_
   even_with_freeform_stdout`), trim behavior on the toggle path
   (`..._treats_known_id_with_leading_zero_or_whitespace_consistently_
   with_toggle`), and the rejection row's exact literal format
   (`render_rejection_row_is_the_literal_cap_message_with_an_empty_accept_
   column`). Every pre-existing `checklist.rs` test that calls
   `parse_fuzzel_output` with the old two-argument signature is updated to
   pass an explicit `known_tag_ids` slice matching its own scenario, not
   left broken by the signature change.
4. **`tag-picker` orchestration** (Task 4) — **not** covered by automated
   tests, same boundary class as every prior story's process-spawn/socket-
   I/O glue: the real `Command::new("fuzzel")` invocation (now with a
   conditional `--search` argument and a static `--placeholder`), the real
   create-then-toggle two-request sequence over a live socket, and the
   real appearance of the rejection row are unverified beyond Task 1's
   man-page research and this task's own structural review against
   4.1-4.5's explicit description. Verified via `cargo build --workspace`/
   `cargo clippy --workspace --all-targets -- -D warnings` succeeding and
   the full `wm`+`tag-picker` suite staying green.
5. **Tooling gate** (Task 5) — `cargo fmt --all -- --check`, `cargo clippy
   --workspace --all-targets -- -D warnings`, `cargo test --workspace`, all
   in-container; `pre-commit run --all-files` against the already-widened
   workspace-wide hook definitions (no further hook changes needed — this
   story adds no new Cargo targets, unlike Story 2.2).

**Remaining, explicitly-flagged coverage gap, carried forward from Story
2.2 and widened by this one:** the real `fuzzel` process is still not
exercised anywhere in this sandbox, live or otherwise. This story adds two
new live-only behaviors on top of Story 2.2's own unverified surface: the
real `--search`-prefill-on-reopen interaction, and the real free-text-echo
behavior the disambiguation rule depends on (verified only against the man
page, Task 1, never against a running `fuzzel`). Both should be covered by
a live smoke test — `river` running, a real focused window, `Mod4+A`, a
real `fuzzel` on screen, typing a genuinely new name — before Story 2.4
builds a third interaction mode (switch mode) on top of this same binary.

## FR coverage
FR8, NFR1, NFR2

## Dev Agent Record

### Debug Log

- `devpod ssh buoy-wm -- echo ok` was confirmed unreliable again this
  session (`Error tunneling to container: wait: remote command exited
  without exit status or exit signal`), consistent with Stories 2.1/2.2's
  own notes. All build/test/lint/pre-commit commands ran via `podman exec
  -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>` against the
  already-running container instead.
- Baseline confirmed before any changes: `cargo test --workspace` = 147
  `wm` tests + 31 `tag-picker` tests, all green (matches Story 2.2's own
  final count exactly).
- Implementing Task 3.2's exact `match trimmed.parse::<u8>() { Ok(id) if
  known_tag_ids.contains(&id) => Toggled(id), _ => CreateTag(...) }`
  logic surfaced one pre-existing test whose expectation the story's task
  list didn't explicitly call out for repurposing:
  `parse_fuzzel_output_returns_cancelled_for_unparseable_stdout`
  (`"not-a-number"` → `Cancelled`). Only
  `..._returns_cancelled_for_tag_id_out_of_u8_range` (`"999"`) was named in
  Task 1.4/3.1 as the repurposed test, but the `_ => CreateTag(...)` catch-
  all arm applies identically to any non-numeric, non-matching stdout —
  `"not-a-number"` is exactly as much a "typed free text" case as
  `"deploy-watch"`. Renamed/repurposed this test too
  (`parse_fuzzel_output_returns_create_tag_for_non_numeric_freeform_stdout`,
  expectation now `CreateTag("not-a-number".into())`), same repurposing
  spirit as the explicitly-named test, with a doc comment cross-referencing
  the rationale. Not a scope change — a direct, necessary consequence of
  implementing the match arms exactly as Task 3.2 specifies them.
- `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets
  -- -D warnings` both passed clean on the first attempt after each GREEN
  step — no reformatting or lint fixups needed this story.

### Completion Notes

- Task 1 (design resolution): confirmed and recorded, not re-derived — the
  story's own drafting already resolved the free-text-vs-selection
  disambiguation (via `fuzzel(1)` v1.14.1's documented verbatim-echo-on-no-
  match behavior) and the rejection-row-dismissal mechanism (reusing the
  empty-stdout-is-cancelled rule). No RED/GREEN, per the story's own
  carve-out.
- Task 2: `tag-picker/src/wire.rs` gains `Request::CreateTag { name: String
  }` and `Response::TagCreated { tag_id: u8 }`, mirroring
  `wm/src/ipc/protocol.rs`'s shapes exactly (`SwitchTag`/`OutputDto`/
  `app_id` remain unmodeled — Story 2.4's job). Module doc comment
  corrected to describe the now-wider mirror. Two new tests
  (`serializes_create_tag_request`, `parses_tag_created_response`), RED-
  confirmed (compile failure against not-yet-defined variants) before
  GREEN. `parse_response_rejects_unknown_type`'s fixture swapped from
  `"tag-created"` (no longer unmodeled) to `"delete-everything"`, per the
  story's own instruction, with an updated doc comment.
- Task 3: `tag-picker/src/checklist.rs`'s `PickerAction` gains
  `CreateTag(String)`; `parse_fuzzel_output` gains a `known_tag_ids: &[u8]`
  parameter and the toggle/create disambiguation logic exactly as Task 3.2
  specifies. Added `pub const REJECTION_MESSAGE` and `render_rejection_row`.
  Every pre-existing call site updated to the new 3-argument signature (see
  Debug Log for the one test whose *expectation*, not just signature, also
  needed updating beyond what Task 1.4 explicitly named). All new/changed
  tests RED-confirmed (compile failures: wrong arity, missing variant,
  missing function) before GREEN.
- Task 4: `tag-picker/src/main.rs`'s `run_fuzzel` gains an
  `initial_search: Option<&str>` parameter (`--search=<text>` when `Some`)
  and an unconditional `--placeholder=type to filter, or a new name to
  create` arg. The loop now maintains `pending_rejected_name: Option
  <String>`, computes `known_ids` from the live `tags` list each iteration,
  prepends `checklist::render_rejection_row()` to the rendered input when a
  rejection is pending, and branches on `PickerAction::CreateTag(name)`:
  sends `create-tag`, on `TagCreated` pushes the new `TagDto` onto the
  local `tags` list and immediately chains `toggle-tag` for the same id
  against the focused view (client-side chaining on the same connection,
  no new WM-side request type), on the `create-tag` leg's own
  `TagLimitReached` error (`message == checklist::REJECTION_MESSAGE`) sets
  `pending_rejected_name` and loops again to show the rejection row; any
  other error path prints to stderr and breaks, matching Story 2.2's
  generic-error precedent. No RED/GREEN, per the story's own carve-out —
  every decision this composes is unit-tested in Tasks 2-3.
- Task 5: full workspace verification, all in-container via `podman exec`
  (`devpod ssh` unreliable, per the Debug Log): `cargo fmt --all --
  --check` clean, `cargo clippy --workspace --all-targets -- -D warnings`
  clean, `cargo build --workspace` clean, `cargo test --workspace` green
  (147 `wm` + 39 `tag-picker` = 186 tests, zero regressions — 8 net new
  `tag-picker` tests: 2 in `wire.rs`, 6 net new/repurposed in
  `checklist.rs`), `pre-commit run --all-files` clean (both hooks pass).

**Explicit coverage-gap statement (Task 5.3), carried forward from Story
2.2 and widened by this one:** the real `fuzzel` process is still not
exercised anywhere in this sandbox. This story adds two new live-only
behaviors on top of Story 2.2's own unverified surface: the real
`--search`-prefill-on-reopen interaction, and the real free-text-echo
behavior the disambiguation rule depends on (verified only against the
man page, Task 1, never against a running `fuzzel`). Every *decision*
around these has full unit-test coverage; the real subprocess's
stdin/stdout framing and the real appearance of the rejection row remain
unverified beyond structural review. This is the single largest residual
risk carried into Story 2.4, which builds a third interaction mode (switch
mode) on top of this same binary's assumptions.

### File List

- `tag-picker/src/wire.rs` (modified) — `Request::CreateTag`,
  `Response::TagCreated`, module doc comment, `parse_response_rejects_
  unknown_type`'s fixture swap, 2 new tests
- `tag-picker/src/checklist.rs` (modified) — `PickerAction::CreateTag`,
  `parse_fuzzel_output`'s 3-argument signature and new disambiguation
  logic, `REJECTION_MESSAGE`, `render_rejection_row`, module/function doc
  comments, updated pre-existing tests' call sites, new tests
- `tag-picker/src/main.rs` (modified) — `run_fuzzel`'s `initial_search`
  parameter and `--placeholder` arg, the loop's `pending_rejected_name`
  state and `PickerAction::CreateTag` branch (create-then-toggle chaining,
  rejection-row reopen)
- `docs/planning/epics/story-2-3.md` (modified) — task checkboxes, Dev
  Agent Record, File List, Change Log, Status, `baseline_commit`
  frontmatter

### Change Log

- 2026-08-08: Implemented Story 2.3 end-to-end (Tasks 1-5) via
  `bmad-dev-story`: recorded the free-text-vs-selection disambiguation and
  rejection-row-dismissal design decisions (Task 1); added `CreateTag`/
  `TagCreated` to `tag-picker`'s wire mirror (Task 2); extended
  `checklist::parse_fuzzel_output` with the `known_tag_ids`-based
  toggle/create split and added `render_rejection_row` (Task 3, TDD
  RED/GREEN, repurposing one Story 2.2 test as the story explicitly
  anticipated plus one additional test whose expectation the same logic
  change required); wired the create-then-toggle chain and the cap-
  rejection reopen into `main.rs`'s loop (Task 4). Full workspace
  verification (fmt/clippy/build/test/pre-commit) green, 186 total tests
  (147 `wm` + 39 `tag-picker`), zero regressions. Status moved to
  `review`.
