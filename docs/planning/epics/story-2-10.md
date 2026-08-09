---
baseline_commit: 56f3287
---

# Story 2.10: Default tag on login, and creating a tag with no window focused

Epic: 2 | Priority: H | Status: done

## Description
Live testing after the pinned-terminal-geometry and focus-stealing fixes
found two related bootstrapping gaps:

**Gap 1 — nothing happens on login.** A fresh `buoy-wm` session starts with
zero tags and zero windows. The only two ways to create a tag are `Mod4+A`
(assign mode, requires an already-focused window per `checklist::
should_open_picker`) and `Mod4+S` (switch mode, which the project's own UX
spec, `EXPERIENCE.md`, explicitly documents as selection-only — "switching
only operates on existing tags," deliberately no create path). On a
completely cold login there is no window to focus `Mod4+A` on, and no tag
to select via `Mod4+S` — the session sits there with a black/empty screen
and no discoverable way forward.

**Gap 2 — creating a *new* tag with its own terminal, with nothing already
focused, has no direct path.** The user confirmed `Mod4+S` staying
selection-only is intentional (matches `EXPERIENCE.md`) and should not
change. But that leaves a real workflow gap once you already have one tag
open and want to start a second one from scratch: `Mod4+A` requires a
focused window to attach the new tag *to* (coupling the new tag's creation
to whatever window happened to be focused, which may not be what the user
wants tagged), and `Mod4+S` can't create at all.

**The fix for both is the same underlying idea — extend `Mod4+A`
(assign mode) to also work with *no* focused window, not extend `Mod4+S`.**
With nothing focused, "toggle this tag's membership on the focused window"
has no target to operate on — but the picker can still usefully do
something with whatever tag you pick or type: switch the active output to
it (creating it first if it's a new name), exactly what switch mode does
for an *existing* tag, just also covering the create-a-new-one case. This
keeps `Mod4+S` byte-for-byte as documented (selection-only) while closing
the real gap — tag creation stays exclusively an assign-mode capability, as
designed, just no longer gated on having a window to attach to.

Together: **Gap 1** ensures a fresh login always has at least the
"default" tag and its pinned terminal already running (so there's always a
focused window for `Mod4+A`'s *existing* toggle-mode to work with
immediately). **Gap 2** ensures that even after that, spinning up further
tags from scratch — with their own dedicated terminal, not coupled to
whatever's currently focused — has a direct, single keybind path.

## Acceptance criteria
**Given** a fresh `buoy-wm` session with a completely empty tag registry
**When** the first output is registered
**Then** a tag named `"default"` is created, that output is switched to
it, and its pinned terminal is spawned — the same three effects
`Action::TagCycle`'s keybind path already produces for an existing tag,
just triggered once automatically at startup instead of by a keypress

**Given** a tag registry that already has at least one tag (this story's
bootstrap already ran, or the user created tags manually)
**When** any output (first or subsequent) is registered
**Then** nothing is auto-created — the bootstrap is strictly a one-time,
empty-registry-only action, never overwriting or duplicating

**Given** `Mod4+A` is pressed with no window currently focused
**When** the picker opens
**Then** it shows the full tag registry (all rows unchecked — there is no
view's membership to reflect) exactly like today's checkbox list, not a
different UI

**Given** the user selects an *existing* tag from that no-focus picker
**When** the selection is confirmed
**Then** the active output switches to that tag (lazily spawning its
pinned terminal if this is its first time being displayed, exactly like
`Mod4+S`'s existing switch behavior) — not a toggle, since there is no
view to toggle membership on

**Given** the user types a name not in the registry from that no-focus
picker
**When** the selection is confirmed
**Then** that tag is created (exactly like today's create-tag flow) *and*
the active output immediately switches to it, rather than being toggled
onto a focused view (there is none)

**Given** a window *is* focused when `Mod4+A` is pressed
**When** the picker is used (toggle an existing tag, or create a new one)
**Then** behavior is byte-for-byte unchanged from today — this story only
adds a new fallback path for the no-focus case, it does not alter the
existing focused-window path at all

**Given** `Mod4+S` (switch mode) in any state
**When** the user types a name not in the registry
**Then** behavior is unchanged (still `Cancelled`, per `EXPERIENCE.md` —
this story does not touch switch mode at all)

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: `WmCore::tag_count` — a small pure query (AC 1, 2)**
  - [x] 1.1 RED — Add tests to `wm/src/wm_core/state.rs`: `tag_count_is_zero_on_a_fresh_registry`, `tag_count_reflects_created_tags` (create two tags, assert `2`). Confirm both fail to compile — `tag_count` doesn't exist yet.
  - [x] 1.2 GREEN — Implement `pub fn tag_count(&self) -> usize { self.tags.count() }` on `WmCore`, thin delegation to the already-existing, already-tested `TagRegistry::count()`. `///` doc comment: exists so `main.rs` can detect "completely fresh, no tags at all yet" without a heavier `snapshot()` call. Confirm both tests pass.

- [x] **Task 2: Bootstrap a default tag on first output registration (AC 1, 2)**
  - [x] 2.1 In `wm/src/main.rs`'s `Dispatch<RiverWindowManagerV1, ()>`'s `Event::Output` handler: after `register_output()` (same lock), check `wm_core.tag_count() == 0`; if so, call `wm_core.create_tag("default")`, and on `Ok(tag_id)`, `wm_core.switch_tag(output_id, tag_id)` (log-and-continue on either call's `Err` — both are structurally near-impossible on a fresh registry, but handled per NFR2, not panicked). Drop the lock (same "lock, mutate, drop, fresh borrow" pattern `manage_seats`/`remove_outputs` already establish), then — only if the tag was actually created — call `self.ensure_pinned_terminal_spawned(tag_id)`, mirroring `Action::TagCycle`'s own post-switch call.
  - [x] 2.2 No RED/GREEN — Wayland registry glue, untestable without a live compositor, same carve-out as every prior `main.rs` registration-path story (1.7, 2.6, 2.8, 2.9). Verify via `cargo build`/`cargo clippy --all-targets -- -D warnings` and structural review: confirm the bootstrap only fires when `tag_count() == 0` (never on a second or later output, never once any tag exists), confirm the `wm_core` lock is dropped before `ensure_pinned_terminal_spawned` re-locks it (Story 2.1's deadlock hazard), confirm `Output::new`'s insertion into `self.outputs` still happens exactly once regardless of which branch runs.

- [x] **Task 3: Thread the active output's id into assign-mode's spawn (AC 3, 4, 5)**
  - [x] 3.1 In `wm/src/main.rs`'s `Action::OpenTagPicker` arm, add `.arg(output_id.to_string())` before the existing conditional `.arg(name)` (output name) whenever `active_output_id` is `Some` — mirrors `Action::TagSwitch`'s existing `<output_id> [<output_name>]` argument order. When `active_output_id` is `None` (no output registered at all — a startup-race edge case Task 2 makes rare but doesn't eliminate), spawn with zero args, same as today's behavior.
  - [x] 3.2 No RED/GREEN, same reasoning as 2.2.

- [x] **Task 4: Extend `tag-picker`'s assign-mode argv grammar (AC 3, 4, 5, 6)**
  - [x] 4.1 RED — In `tag-picker/src/mode.rs`, change `Mode::Assign` to `Assign { output_id: Option<u64>, output_name: Option<String> }`. Add tests: `parse_args_with_no_arguments_is_assign_mode_with_no_output_id_or_name` (existing zero-arg test, updated shape), `parse_args_with_one_argument_is_assign_mode_with_output_id_only`, `parse_args_with_two_arguments_is_assign_mode_with_output_id_and_name` (replaces the old single-arg-is-output-*name* test — Task 3.1 changes what `main.rs` actually sends, so the old one-arg-is-a-name-string shape is retired, not kept alongside the new one), plus updated rejection tests for the widened-by-one argument-count range. Confirm all fail to compile against the current `Assign { output_name: Option<String> }` shape.
  - [x] 4.2 GREEN — `[] => Assign { output_id: None, output_name: None }`; `[id] => id.parse::<u64>().map(|output_id| Assign { output_id: Some(output_id), output_name: None }).map_err(...)`; `[id, name] => ... Assign { output_id: Some(output_id), output_name: Some(name.clone()) }`; everything else still `Err`. Update the usage string and the module/`Mode` doc comments — assign mode's single positional argument now means "output id" (a `u64`), not "output name" (a `String`) as it did before this story; this is a controlled, self-consistent break of an internal-only CLI shape (`tag-picker` has exactly one real caller, `wm`, updated in lockstep by Task 3 — not a public/stable API). Confirm all Task 4.1 tests pass.

- [x] **Task 5: Fork assign mode's outcome handling on `view_id.is_none()` (AC 3, 4, 5, 6)**
  - [x] 5.1 In `tag-picker/src/main.rs`, remove `checklist::should_open_picker`'s enforcement in `run_assign_mode` (the `if !should_open_picker(focused_view) { eprintln!(...); std::process::exit(0); }` block) — the picker now always opens. `view_id: Option<u64> = focused_view` (no `.expect()` unwrap needed anymore).
  - [x] 5.2 `run_assign_mode` gains an `output_id: Option<u64>` parameter (threaded from `main`'s `Mode::Assign { output_id, output_name }` destructure). The existing `build_checklist_entries`/`render_fuzzel_input` calls are unchanged — reused as-is: pass `current_tags` (already empty-by-default when `view_id` is `None`, since there's no view to look up membership for) exactly as today's code already computes it, no new rendering path needed (an empty `current_tags` slice already renders every row unchecked).
  - [x] 5.3 Fork the two outcome arms on `view_id`:
    - `PickerAction::Toggled(tag_id)`: if `view_id.is_some()`, byte-for-byte unchanged existing toggle-and-reopen behavior. If `view_id.is_none()`, send `Request::SwitchTag { output_id, tag_id }` instead (using `output_id.unwrap_or(...)` — see 5.4 for the `None`-output-id case), handle `Ok`/`Error` the same shape `run_switch_mode`'s existing `Selected` arm already uses, then `break` the loop (switching is a one-shot terminal action, not a toggle-and-reopen — matches `EXPERIENCE.md`'s "the picker closes" for a switch).
    - `PickerAction::CreateTag(name)`: the existing `Request::CreateTag`/`Response::TagCreated` round-trip is unchanged. After a successful `TagCreated { tag_id }`: if `view_id.is_some()`, byte-for-byte unchanged existing chained-toggle-and-reopen behavior. If `view_id.is_none()`, chain `Request::SwitchTag { output_id, tag_id }` instead of `Request::ToggleTag`, handle its response the same way as the forked `Toggled` arm above, then `break` (one-shot, same reasoning). The cap-rejection (`REJECTION_MESSAGE`) path is unchanged either way — that's about the *create* step, not what happens after.
  - [x] 5.4 If a switch-style outcome is reached (either fork of 5.3) but `output_id` is `None` (the rare no-output-registered-yet race Task 2 doesn't fully eliminate — see Task 3.1), print `"tag-picker: no output to switch to"` and `std::process::exit(1)` rather than sending a request with a fabricated id — mirrors `Action::TagCycle`'s own `None`-output message style in `wm`.
  - [x] 5.5 No RED/GREEN for this task — `run_assign_mode`/`run_switch_mode` are real-socket-I/O and real-`fuzzel`-process-spawning glue, already explicitly documented as untestable in this sandbox (same carve-out `run_fuzzel`'s own doc comment already claims). Verify via `cargo build`/`cargo clippy --all-targets -- -D warnings` and structural review that the `view_id.is_some()` branches are textually unchanged (this story adds a new fallback path, it must not alter the existing one) and that both `break` points are reached (no accidental fallthrough back into the reopen loop after a switch).
  - [x] 5.6 Delete `checklist::should_open_picker` and its two now-obsolete tests (`should_open_picker_true_when_focused_view_is_some`, `should_open_picker_false_when_focused_view_is_none`) — dead code once 5.1 removes its only call site (no backwards-compatibility shim; nothing else in the codebase calls it).

- [x] **Task 6: Full in-container verification gate (AC: all)**
  - [x] 6.1 Inside the devcontainer (`devpod ssh buoy-wm`, or the `podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>` fallback): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pre-commit run --all-files`. Confirm the existing 266-test suite (with Task 1's 2 new tests, Task 4's grammar tests updated/replacing the retired single-arg-is-name tests, and Task 5.6's 2 deletions) all pass, net test count change documented in the Dev Agent Record.
  - [x] 6.2 Manual live-verification note (not automated, not a story blocker — same boundary as every prior story's live-compositor gap): once rebuilt, confirm a fresh login lands on an already-usable "default" tag with its terminal running, and that `Mod4+A` with nothing focused lets you create-and-switch to a brand new tag directly. Record in the Dev Agent Record whether this was actually confirmed live or deferred.

## Technical notes

**Why not extend `Mod4+S` instead.** The user explicitly confirmed
`Mod4+S`'s selection-only scope is intentional, documented in
`EXPERIENCE.md`, and should not change. Extending `Mod4+A` instead keeps
every tag-creation code path in exactly one place (assign mode), which is
also where it already lived — this story adds a *precondition relaxation*
(no window required) and a *fallback outcome* (switch instead of toggle
when there's nothing to toggle), not a second, parallel creation mechanism.

**Why the assign-mode CLI grammar's one-arg meaning is allowed to change.**
Story 2.9 defined `tag-picker`'s single positional argument as an output
*name* (a string, for `fuzzel --output=`). This story repurposes it as an
output *id* (a `u64`, for the `SwitchTag` IPC request) with the name
becoming a second, optional argument — an internal-only contract with
exactly one real caller (`wm`'s own spawn command), updated in the same
story, not a public/stable interface anything else depends on. Contrast
with `Request`/`Response` shapes in `wire.rs`, which *are* a stable-ish
contract between two independently-evolving binaries and get additive
changes, never redefinitions, for exactly that reason.

**Why the bootstrap tag is unconditional on `tag_count() == 0`, not tied
to "is this the very first output ever."** A multi-output login (laptop +
already-docked external monitor, both outputs arriving before the first
manage sequence completes) would otherwise risk a race on "which output
counts as first." Gating on the *tag registry* being empty instead means
the very first `Event::Output` handled — whichever one that is — creates
"default" and claims it; every other output registered afterward
correctly sees `tag_count() > 0` and does nothing, left tagless until the
user manually switches it (same, already-existing, unremarkable state a
manually-created second output has always been in).

**Scope boundary.** This story does not let a *second* completely-empty
registry situation re-trigger (there's no "delete all tags" operation in
this project, ADR-006-adjacent, so `tag_count()` can never return to zero
once non-zero — a one-shot bootstrap is sufficient, no reset logic
needed). It does not change the tag *name* "default" to be
user-configurable (YAGNI — a fixed, obvious name is enough to be useful
and edit later via the same rename... which doesn't exist yet either;
out of scope). It does not touch `status-bar`, `kanshi`, or any output-
geometry code at all.

## Test plan
1. **`wm_core` unit test (TDD, RED before GREEN)**: `tag_count`'s two cases (Task 1.1). Fully testable — no Wayland types involved.
2. **`tag-picker` unit tests (TDD, RED before GREEN)**: `mode::parse_args`'s revised assign-mode grammar (Task 4.1), and the removal of `should_open_picker`'s two now-obsolete tests (Task 5.6).
3. **Build/lint gate**: `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` clean with every signature change (Task 2's bootstrap, Task 3's new spawn arg, Task 5's `run_assign_mode` fork) compiled in.
4. **Regression gate**: existing test suite unaffected beyond Tasks 1/4/5.6's intentional additions/removals — particularly, every `checklist`/`wire` test *not* touched by this story stays green untouched, confirming the focused-window path is byte-for-byte preserved.
5. **Live gate (manual, not automated)**: fresh login lands on a usable default tag; `Mod4+A` with nothing focused creates-and-switches to a new tag directly, per Task 6.2.

## FR coverage
Tag-manager popup and pinned-terminal lifecycle (existing FRs) made usable
from a cold start, which no prior story addressed — Epic 1/2's live
testing to date always began from an already-populated tag registry
(created manually mid-session). Not a new FR; closes a bootstrapping gap
the PRD's acceptance criteria implicitly assumed away.

## Dev Agent Record

**What was done**:

- `wm/src/wm_core/state.rs` (Task 1, real RED/GREEN TDD): added
  `tag_count_is_zero_on_a_fresh_registry` and
  `tag_count_reflects_created_tags`, confirmed both failed to compile
  (`E0599: no method named tag_count`) against `WmCore` in-container, then
  implemented `pub fn tag_count(&self) -> usize { self.tags.count() }`
  (thin delegation to `TagRegistry::count()`) and confirmed both GREEN.
- `wm/src/main.rs` (Task 2): `Dispatch<RiverWindowManagerV1, ()>`'s
  `Event::Output` arm now holds one `wm_core` guard across
  `register_output()`, the `tag_count() == 0` check, and (when empty)
  `create_tag("default")` + `switch_tag(output_id, tag_id)` — both
  log-and-continue on `Err`, never panic. The guard is explicitly `drop`ped
  before `state.wm.outputs.insert(...)` and, only if a tag was actually
  bootstrapped, `state.wm.ensure_pinned_terminal_spawned(tag_id)` — mirrors
  `manage_seats`'s own lock/mutate/drop/fresh-borrow sequencing (Story
  2.1's deadlock hazard).
- `wm/src/main.rs` (Task 3): `Action::OpenTagPicker`'s spawn now appends
  `output_id.0.to_string()` before the existing conditional output-name
  arg, whenever `active_output_id` is `Some`; `None` still spawns with zero
  args exactly as before — mirrors `Action::TagSwitch`'s existing
  `<output_id> [<output_name>]` order.
- `tag-picker/src/mode.rs` (Task 4, real RED/GREEN TDD): `Mode::Assign`
  extended to `{ output_id: Option<u64>, output_name: Option<String> }`.
  Verified RED by constructing a temporary file combining the *old*
  single-field `Assign`/`parse_args` implementation with the *new* test
  module and running `cargo test -p tag-picker mode::` in-container,
  confirming 3 real `E0559` compile errors ("variant `Mode::Assign` has no
  field named `output_id`"). Then installed the extended implementation
  (`[]` / `[id]` / `[mode, id] if mode == "switch"` / `[id, name]` /
  `[mode, id, name] if mode == "switch"` match arms, in that order so the
  literal `"switch"`-guarded arms are tried before the generic two-element
  assign arm) and confirmed all 10 tests GREEN.
- `tag-picker/src/main.rs` (Task 5): removed `should_open_picker`'s
  enforcement block; `run_assign_mode`'s `focused_view: Option<u64>`
  parameter renamed to `view_id: Option<u64>` (no `.expect()` needed
  anymore) and gained a new `output_id: Option<u64>` parameter, threaded
  from `main`'s `Mode::Assign { output_id, output_name }` destructure.
  `current_tags`'s computation changed from `views.iter().find(|v| v.id ==
  view_id)` to `view_id.and_then(|view_id| views.iter().find(|v| v.id ==
  view_id))`, naturally empty when `view_id` is `None`. Both outcome arms
  (`Toggled`, `CreateTag`'s post-`TagCreated` handling) forked on `match
  view_id { Some(view_id) => ..., None => ... }`: the `Some` arms are the
  pre-existing code moved under the match with no logic changes (confirmed
  via diff review — only re-indentation); the `None` arms send
  `Request::SwitchTag { output_id, tag_id }` instead of
  `Request::ToggleTag`/chained-`ToggleTag`, handle the response the same
  shape as `run_switch_mode`'s own `Selected` arm, then `break` the loop
  (one-shot, not toggle-and-reopen). Per Task 5.4, an `output_id == None`
  reached at a switch-style outcome prints `"tag-picker: no output to
  switch to"` and `std::process::exit(1)` before ever building a request.
- `tag-picker/src/checklist.rs` (Task 5.6): deleted `should_open_picker`
  and its two tests (`should_open_picker_true_when_focused_view_is_some`,
  `should_open_picker_false_when_focused_view_is_none`) — no remaining
  call sites.

**Deviations from the story's literal wording** (flagged explicitly, per
this project's established norm — see Stories 2.7/2.8/2.9's own Dev Agent
Records):

1. **No shared helper for the two `SwitchTag`-send-and-report blocks.** The
   `None`-branch logic in `Toggled` and in `CreateTag`'s `TagCreated` arm
   is textually identical (build `Request::SwitchTag`, send, match the
   response into the same three outcomes). The story's wording ("handle it
   the same shape... handled the same way as the forked `Toggled` arm
   above") could be read as inviting a shared function. Left as two
   inlined copies instead, per this project's own three-strike DRY
   convention (extract after ~3 occurrences, not two) — also avoids a
   helper that would need to either take a mutable `writer`/`reader` pair
   by `&mut` (adding an extra layer of indirection for a five-line body)
   or duplicate `break`'s loop-exiting behavior awkwardly across a
   function boundary.
2. **Task 5.3's literal `output_id.unwrap_or(...)` phrasing not used.**
   The task text says to build the `SwitchTag` request "using
   `output_id.unwrap_or(...)`", but Task 5.4 immediately clarifies the
   real intent: never send a request with a fabricated id when `output_id`
   is `None`. Implemented as an explicit `let Some(output_id) = output_id
   else { ...; std::process::exit(1) };` guard before building the
   request at all, which satisfies 5.4's requirement directly rather than
   computing then discarding a placeholder value. Functionally identical
   outcome to what 5.3+5.4 together specify; only the literal
   implementation shape differs.
3. **`mode.rs`'s module/`Mode` doc comments were substantially rewritten,
   not lightly edited.** Task 4.2 says to "update the usage string and the
   module/`Mode` doc comments" — the actual diff replaces most of the
   `parse_args` doc comment's prose (previous grammar description no
   longer applies verbatim once the one-arg/two-arg meaning changed) and
   trims the Story 2.9 code-review-follow-up paragraph to reflect that its
   original edge case (a connector literally named `"switch"` breaking
   assign mode) is now moot under the numeric-id grammar. Judged this
   necessary for the comments to stay accurate rather than merely
   patched — flagged since "update" could be read as a smaller edit than
   what was done.

No other deviations identified; the implementation otherwise matches
Tasks 1-6's plans, including exact field names, method signatures, and
match-arm shapes as written.

**Verification** (all inside the devcontainer via `devpod ssh buoy-wm` —
worked reliably every call this session; the `podman exec` fallback was
never needed):
- `cargo fmt --all -- --check`: clean, no drift.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace`: `buoy-wm` (wm) 176 (174 baseline + 2 new
  `tag_count` tests), `status-bar` 27 (unaffected), `tag-picker` 65 (63
  baseline + 2 net new — `mode.rs` went from 8 to 10 tests, `checklist.rs`
  lost 2 `should_open_picker` tests: +2 −2 net for that file, +2 net for
  `mode.rs` = +2 total for the crate) = **268 total, 0 failures, 0
  regressions** (266 baseline + 2 net, matching the story's Task 6.1
  expectation).
- `pre-commit run --all-files`: both configured hooks (`cargo fmt --check`,
  `cargo clippy`) passed.

**Task 6.2 (manual live-verification)**: **deferred**, not performed by
this agent — no access to a live compositor, a physical output, or `river`
itself in this sandbox, same boundary as every prior story's
live-compositor gap (most recently Story 2.9's own Task 6.2). Confirming a
fresh login actually lands on a usable "default" tag with its terminal
running, and that `Mod4+A` with nothing focused actually creates-and-
switches to a new tag against a real `river` session, still needs to be
done by a human before this story's Description's originating
live-testing report can be considered resolved end-to-end.

**File List**:
- `wm/src/wm_core/state.rs`
- `wm/src/main.rs`
- `tag-picker/src/mode.rs`
- `tag-picker/src/main.rs`
- `tag-picker/src/checklist.rs`

## Code Review

HIGH-effort workflow-backed review. Three findings, all fixed:

1. **`CreateTag` could be sent, and permanently mutate the registry, before
   ever checking whether there was an output to switch the new tag to.**
   In the no-focus fallback's `PickerAction::CreateTag` arm, the
   `output_id.is_none()` check (Task 5.4) only ran *after* the `CreateTag`
   round-trip had already succeeded — in the rare startup-race window
   where both `view_id` and `output_id` are `None`, this created a tag
   with no way to ever apply or remove it (no delete-tag API exists),
   then exited with an error. **Fixed**: reinstated `checklist::
   should_open_picker`, generalized to `(view_id, output_id) -> bool`
   (`view_id.is_some() || output_id.is_some()`), checked once at the very
   top of `run_assign_mode` before any wire traffic at all. This also
   makes every `output_id` unwrap inside the `None`-view arms provably
   safe (`.expect()` with a comment explaining why, rather than a
   redundant per-arm check).
2. **Deleting `should_open_picker`'s two tests removed real coverage with
   nothing replacing it.** Restoring the (generalized) function brought
   back dedicated coverage: `should_open_picker_true_when_view_focused`,
   `_true_when_output_known_even_without_view`, `_true_when_both_...`,
   `_false_when_neither_...` — the last of these is exactly the case
   Finding 1 depends on staying refused.
3. **The `SwitchTag` send/read/report block was duplicated three times**
   (`run_switch_mode`'s `Selected` arm, and both of `run_assign_mode`'s
   new no-focus arms) — past this project's own three-strike DRY
   threshold. **Fixed**: extracted `send_switch_tag_or_exit`, used by all
   three call sites.

**Re-verification after fixes**: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `pre-commit run
--all-files` all clean; `cargo test --workspace` — 272 total (`buoy-wm`
176, `status-bar` 27, `tag-picker` 69 — up 4 from the 268 baseline for
Finding 2's new tests), 0 failures.

**Code review: PASS**.
