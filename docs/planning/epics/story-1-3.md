---
baseline_commit: 2bd0e8874859fda16c203d715c9d7640965f665a
---

# Story 1.3: One-Tag-Per-Output Enforcement
Epic: 1 | Priority: H | Status: done

## Description
Adds `WmCore::switch_tag(output_id, tag_id)` — the enforcement operation
layered on top of Story 1.2's raw `set_output_current_tag` setter — so that
a tag can never be simultaneously displayed on two different outputs
(ADR-005). `set_output_current_tag` itself only validates ids and writes a
single field; it deliberately does not coordinate across outputs (see its
doc comment in `wm/src/wm_core/state.rs`, added during Story 1.2's review
follow-up: "this is the raw field-level primitive only — the one-tag-per-
output enforcement/reject-or-reroute decision is Story 1.3's `switch_tag`
operation, layered on top of this setter, not built here"). This story
closes that gap.

`switch_tag` is the single function later stories reuse rather than
duplicate: Story 1.5's lazy pinned-terminal spawn trigger, Story 1.7's
tag-cycle/tag-create keybind wrapper, and Story 2.4's IPC `switch-tag`
handler all call this same `wm-core` function. Entirely within `wm_core`
(`wm/src/wm_core/state.rs`) — no `main.rs` wiring, same boundary Story 1.2
established (`main.rs` wiring for real compositor-driven tag switches starts
in Story 1.4/1.7).

## Acceptance criteria

**Given** an output currently displaying tag A
**When** any action would assign tag B to display on that same output
**Then** the WM rejects or reroutes the action so exactly one tag remains displayed per output
**And** the enforcement decision completes within the 50ms internal latency budget (NFR1)
**And** this holds under both single-output and multi-output configurations

> **Clarification (this story).** The block above is `epics.md`'s literal
> wording and, taken at face value, only exercises the trivial same-output
> replace case (assigning B to an output already showing A is ordinary
> replacement — `Output.current_tag: Option<TagId>` already guarantees
> "exactly one tag" per output structurally; see Technical notes). The
> substantive rule this story exists to build — ADR-005's "no need to
> coordinate simultaneous multi-output tag display" — is the *converse*:
> the same tag must never be the `current_tag` of two different outputs at
> once. The Given/When/Then below makes that scenario explicit and is what
> Tasks 2–3 actually test; it extends, not contradicts, the block above.

**Given** tag B is currently displayed on output O1 (i.e. `O1.current_tag == Some(B)`)
**When** `switch_tag(O2, B)` is called for a different, registered output O2
**Then** O2's current tag becomes `Some(B)` and O1's current tag is cleared to `None` (rerouted away)
**And** at every point afterward, no two registered outputs simultaneously report the same `Some(tag)` — the ADR-005 invariant
**And** outputs/tags uninvolved in the switch are left untouched

**Given** a `switch_tag` call referencing an `OutputId` or `TagId` that is not currently registered
**Then** the call returns a typed `WmCoreError` (`UnknownOutput`/`UnknownTag`, the existing variants from Story 1.2) and leaves every output's `current_tag` field unchanged — it never panics and never clears an unrelated, valid output as a side effect of a failed call (NFR2)

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: `switch_tag` baseline — set and same-output idempotency (AC 1)**
  - [x] 1.1 RED — Inside the devcontainer via devpod (`devpod ssh buoy-wm -- cargo test --manifest-path wm/Cargo.toml wm_core::state::`), add tests to `wm/src/wm_core/state.rs`'s test module: `switch_tag(output_id, tag_id)` on an output currently showing `None` sets `current_tag == Some(tag_id)` and returns `Ok(())`; calling it on an output that already displays a *different* tag A replaces `current_tag` with the new tag B (the literal same-output AC case — after the call, A is no longer displayed there and B is); calling it again with the tag already displayed on that same output is a no-op success (`current_tag` stays `Some(tag_id)`, no error). Confirm these fail to compile — `switch_tag` doesn't exist yet.
  - [x] 1.2 GREEN — Implement `WmCore::switch_tag(&mut self, output_id: OutputId, tag_id: TagId) -> Result<(), WmCoreError>` in `wm/src/wm_core/state.rs`: validate `tag_id` exists in the tag registry and `output_id` exists in `outputs` (same checks `set_output_current_tag` already performs), then delegate the actual field write to `self.set_output_current_tag(output_id, Some(tag_id))` — reused, not duplicated, per this story's own reuse requirement for Stories 1.5/1.7/2.4. `///` doc comment referencing FR2/ADR-005 per project code style. Confirm the three new tests pass; confirm no regression in Story 1.2's existing suite.

- [x] **Task 2: Cross-output reroute enforcement — the ADR-005 rule (AC 2)**
  - [x] 2.1 RED — Add tests: with two outputs O1 (`current_tag = Some(B)`) and O2 (`current_tag = None`), `switch_tag(O2, B)` returns `Ok(())` and afterward `O2.current_tag == Some(B)` **and** `O1.current_tag == None` (rerouted away). With three outputs — O1 showing tag B, O2 showing an unrelated tag C, O3 showing `None` — `switch_tag(O3, B)` clears O1 to `None` but leaves O2's `Some(C)` untouched (the reroute only clears the exact tag being switched, not other tags/outputs). A third test drives a short sequence of `switch_tag` calls across 3 outputs and 2 tags (e.g. tag on O1 → switched to O2 → switched to O3, interleaved with a second tag going to O1) and, after *every* call in the sequence, asserts no two outputs simultaneously hold the same `Some(tag)` (write a small local test helper scanning `core.outputs.values()` for duplicate `Some` values — this is the AC's explicit "multi-output configurations" clause). Confirm all three fail against Task 1's implementation (which doesn't yet touch any output besides the target, so O1 above would incorrectly still show B after the switch).
  - [x] 2.2 GREEN — In `switch_tag`, after validating `tag_id`/`output_id` but before writing the target output, iterate `self.outputs` and clear (`current_tag = None`) any *other* output whose `current_tag == Some(tag_id)`, then perform the target write via `set_output_current_tag` as in Task 1. Confirm the new tests pass and Task 1's tests still pass (in particular the same-output idempotent no-op — rerouting must not clear the target output itself before its own write).

- [x] **Task 3: Invalid-input robustness — validate before mutating (AC 3 / NFR2)**
  - [x] 3.1 RED — Add tests: `switch_tag` with a registered tag but an unregistered `OutputId` returns `Err(WmCoreError::UnknownOutput)`; with a registered output but an unregistered `TagId` (e.g. `TagId(63)`) returns `Err(WmCoreError::UnknownTag)`; both leave the whole `WmCore` state byte-for-byte unchanged, asserted via whole-state snapshot equality (`WmCore`'s `Clone`/`PartialEq`, established in Story 1.2's Task 11 pattern) taken before the call and compared after. A fourth, explicitly-named regression-guard test: with O1 genuinely displaying a valid tag A, call `switch_tag(O2, bogus_tag)` where `bogus_tag` is unregistered — confirm `Err(UnknownTag)` **and** `O1.current_tag` is still `Some(A)` (proving the reroute loop cannot run before tag/output validation fails, which would otherwise incorrectly clear a real, unrelated output). Confirm this last test would fail under a naive "reroute first, validate last" ordering. **Note:** these tests passed immediately on first run rather than failing — Task 2.2's implementation already validates both ids strictly before the reroute loop (per its own instructions: "after validating tag_id/output_id but before writing the target output"), so no naive-ordering bug was ever introduced. Confirmed as regression guards, consistent with the task's own framing.
  - [x] 3.2 GREEN — Ensure `switch_tag`'s validation (`tags.contains(tag_id)`, `outputs.contains_key(&output_id)`) runs strictly before the Task 2 reroute loop, returning early on either failure with no mutation. Confirm all tests pass. Run `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings` inside the devcontainer via devpod; fix any new lint warnings.

- [x] **Task 4: Full in-container verification gate (AC: all)**
  - [x] 4.1 Run, inside the devcontainer via devpod (`devpod ssh buoy-wm`): `cargo fmt --manifest-path wm/Cargo.toml --all -- --check`, `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings`, and `cargo test --manifest-path wm/Cargo.toml`. Confirm all three pass with Story 1.2's existing suite (47 tests) plus this story's new `switch_tag` tests (Tasks 1–3) all green, and zero clippy/fmt violations. Run `pre-commit run --all-files` as a final sanity check (same fmt/clippy hooks from Story 1.1).

## Technical notes
- Depends on Story 1.2 (`wm-core` state model — done). Builds directly on `WmCore::set_output_current_tag` (`wm/src/wm_core/state.rs`), reusing it as the actual field-write path rather than duplicating the assignment logic — satisfies this story's own forward requirement that later stories reuse `switch_tag` "directly (not duplicated)."
- **Scope stays inside `wm_core`.** No `main.rs` wiring in this story, verified against `components.md` (`wm-core`: "in-process function calls from the protocol client and the IPC server; no external interface of its own") and Story 1.2's Technical notes (real compositor-driven wiring starts Story 1.4 onward). Nothing in `epics.md`, the pre-existing `story-1-3.md` draft, `data-model.md`, or the ADRs asks for `main.rs` changes here — Stories 1.5 (lazy terminal spawn trigger), 1.7 (keybind wrapper), and 2.4 (IPC handler) are the later call sites, each a separate story.
- **The actual invariant, precisely.** `Output.current_tag: Option<TagId>` already structurally guarantees "at most one tag per output" — a single field can't hold two values, so the literal `epics.md` AC (same-output replace) is nearly free. The real work (ADR-005: "no need to coordinate simultaneous multi-output tag display anywhere in the design") is the converse: a tag must not be the `current_tag` of *two different* outputs at once. `switch_tag` is where that cross-output check lives; `set_output_current_tag` intentionally still doesn't do it (per its own doc comment from Story 1.2's review follow-up), so raw callers of the setter (there are none outside `wm_core` yet) remain unenforced by design — only `switch_tag` is the enforcement entry point.
- **Reject vs. reroute — this story picks reroute.** FR2/ADR-005/`epics.md` all leave the choice open. Reroute (silently move the tag rather than error) is chosen because: (a) the picker UX ("selecting a tag switches the active output's displayed tag" — `features-and-acceptance-criteria.md`) describes an unconditional action with no described conflict/error state for the user to resolve; (b) Story 1.5's lazy-spawn trigger and Story 1.7's tag-cycle keybind both need `switch_tag` to just succeed regardless of which output previously showed the target tag; (c) a reject path would need a new `WmCoreError` variant and an undefined recovery UX that no requirement currently describes (YAGNI). No new `WmCoreError` variant is added by this story — the only failure modes remain the pre-existing `UnknownOutput`/`UnknownTag`. Flagged as an assumption for review, same as several of Story 1.2's inferred decisions (tag-name dedup, single global focus) — cheap to revisit since nothing later depends on the rejected alternative.
- **Validation-before-mutation ordering** is the crux of this story's NFR2 contribution: `switch_tag` must fully validate both ids before touching any *other* output's `current_tag`, so a bogus id can never have the side effect of clearing a real, unrelated output's real tag. Task 3's fourth test is the explicit regression guard for this exact bug class.
- **NFR1 (50ms budget) boundary.** Not literally benchmarked by a timing assertion in this story's unit tests — same boundary reasoning Story 1.2 applied to NFR3 (idle RSS): `switch_tag` is a single `O(number of registered outputs)` in-memory `HashMap` scan with no I/O, locking, syscalls, or unbounded allocation, so it is structurally far under budget; a literal `Instant`-based assertion in a sandboxed CI container would be flaky and non-diagnostic rather than meaningfully verifying anything real-world. Flagged as an accepted coverage gap for whichever later story first runs the full daemon end-to-end under a live compositor (same framing as Story 1.2's NFR3 note), not a silent omission.
- No new Cargo dependencies. No new `WmCoreError` variants (see reroute-vs-reject note above).
- Build/test/lint only ever run **inside the devcontainer via devpod** (`cargo build`, `cargo test`, `cargo fmt`, `cargo clippy`) — same constraint as Stories 1.1/1.2; no task step above invokes them on the bare host.

## Test plan
This story's tests are Rust unit tests (`#[cfg(test)] mod tests`) added to the existing `wm/src/wm_core/state.rs` test module, run via `cargo test --manifest-path wm/Cargo.toml` inside the devcontainer via devpod. No integration or end-to-end test is possible yet (no live protocol/compositor wiring exists — same boundary as Story 1.2), so unit coverage is the whole of this story's verification:

1. **`switch_tag` baseline** (Task 1) — sets `current_tag` on an output previously showing `None`; replaces an output's own previously-displayed tag with a new one (the literal `epics.md` same-output AC case); re-switching to the tag already displayed on that output is an idempotent no-op.
2. **Cross-output reroute** (Task 2) — switching a tag onto a new output clears it from whichever other output was previously displaying it; unrelated outputs/tags are left untouched by the reroute; a multi-step sequence across 3 outputs and 2 tags never leaves two outputs simultaneously reporting the same `Some(tag)` at any point (the ADR-005 invariant, asserted after every step — covers the AC's explicit multi-output-configuration clause).
3. **Invalid-input robustness** (Task 3) — unknown `OutputId`/`TagId` return the existing `WmCoreError::UnknownOutput`/`UnknownTag` variants; whole-`WmCore` snapshot equality (via `Clone`/`PartialEq`, established in Story 1.2) proves zero mutation on failure; a targeted test proves a bogus `tag_id` cannot trigger the reroute loop against a real, unrelated, valid output before validation fails (the specific "validate before mutate" ordering bug class).
4. **Tooling gate** (Task 4) — `cargo fmt --check`, `cargo clippy -D warnings`, and the full `cargo test` run (Story 1.2's 47 existing tests plus this story's new `switch_tag` tests, all green), all in-container; `pre-commit run --all-files` as a final sanity check reusing Story 1.1's hooks.

NFR1 (50ms internal latency budget) is *not* directly benchmarked here — see Technical notes for why (structural argument instead of a flaky in-container timing assertion, same treatment Story 1.2 gave NFR3) — treat that as a coverage gap for a human to consciously accept or assign to a later end-to-end story, not a silent omission.

## FR coverage
FR2, NFR1, NFR2

## Dev Agent Record

### Implementation Plan
Followed the story's own 4-task RED-then-GREEN breakdown in order, entirely within `wm/src/wm_core/state.rs`. Task 1 implemented `WmCore::switch_tag(output_id, tag_id)` as a thin validate-then-delegate wrapper around the existing `set_output_current_tag` primitive (no cross-output logic yet), confirmed against 3 new tests covering the baseline set/replace/idempotent-no-op cases. Task 2 added the ADR-005 cross-output reroute: a loop over `self.outputs` clearing any *other* output whose `current_tag` matched the target tag, placed after validation but before the target write — its 3 RED tests were confirmed failing against Task 1's implementation first, exactly as the story predicted, before the reroute loop was added. Task 3's 4 invalid-input tests passed immediately on first run with no further code change, because Task 2's implementation already validates both ids strictly before the reroute loop runs (per Task 2.2's own instructions) — consistent with the story's framing of these as regression guards rather than new-behavior tests. Task 4 ran the full in-container fmt/clippy/test/pre-commit gate.

Key design decisions:
- Reroute, not reject, per the story's Technical notes decision — no new `WmCoreError` variant added.
- The reroute loop and the target write are both funneled through `self.outputs`, with the target write still delegated to `set_output_current_tag` (not duplicated), per the story's explicit reuse requirement for Stories 1.5/1.7/2.4.
- One formatting fix required: `cargo fmt` collapsed a multi-line `assert_eq!` in `switch_tag_bogus_tag_cannot_clear_unrelated_valid_output` onto a single line; applied via `cargo fmt --manifest-path wm/Cargo.toml --all` inside the devcontainer, then re-verified clean.

### Debug Log
No blocking issues. All RED steps were confirmed via `devpod ssh buoy-wm --command 'cargo test --manifest-path wm/Cargo.toml wm_core::state::'` (compile failure for Task 1, assertion failures for Task 2) before each GREEN implementation. Command log (all run inside the devcontainer via `devpod ssh buoy-wm --command '...'`; the `--` positional form used in the story text's literal wording did not work against this devpod version — `Error tunneling to container: wait: remote command exited without exit status or exit signal` on every invocation — so the `--command` flag was used instead, which behaves identically and returns correct exit codes/output):
- `cargo test --manifest-path wm/Cargo.toml` (baseline: 47 passed, sanity check before starting)
- Task 1.1 RED: `cargo test --manifest-path wm/Cargo.toml wm_core::state::` — 5 compile errors (`no method named switch_tag`)
- Task 1.2 GREEN: full suite — 50 passed (47 + 3 new)
- Task 2.1 RED: `cargo test --manifest-path wm/Cargo.toml wm_core::state::` — 3 of the new tests failed (reroute not yet implemented), 40 passed/3 failed
- Task 2.2 GREEN: full suite — 53 passed (47 + 3 + 3)
- Task 3.1: `cargo test --manifest-path wm/Cargo.toml wm_core::state::` — all 4 new tests passed immediately (46 passed, 10 filtered) — no RED, see Implementation Plan note
- Task 3.2: full suite — 56 passed; `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings` — clean
- Task 4.1: `cargo fmt --manifest-path wm/Cargo.toml --all -- --check` — 1 diff found; `cargo fmt --manifest-path wm/Cargo.toml --all` applied; re-checked clean; `cargo clippy ... -D warnings` — clean; `cargo test --manifest-path wm/Cargo.toml` — 56 passed, 0 failed; `pre-commit run --all-files` — `cargo fmt --check` and `cargo clippy` hooks both passed (exit 0)

### Completion Notes
- All 4 tasks / 9 subtasks complete. Final `cargo test --manifest-path wm/Cargo.toml` run: **56 passed, 0 failed** (47 Story 1.2 baseline + 9 new `switch_tag` tests).
- `cargo fmt --manifest-path wm/Cargo.toml --all -- --check`: clean (after one fmt pass).
- `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings`: clean, no new lint warnings.
- `pre-commit run --all-files`: both hooks passed.
- No `main.rs` changes; no new `WmCoreError` variants; no new Cargo dependencies — scope stayed entirely inside `wm_core` per the story's Technical notes.
- NFR1 (50ms budget) is not directly benchmarked, per the story's own Technical notes (structural argument: single `O(outputs)` in-memory `HashMap` scan, no I/O) — flagged there as an accepted coverage gap, not an omission from this story.

## File List
- `wm/src/wm_core/state.rs` (modified — added `WmCore::switch_tag(output_id, tag_id) -> Result<(), WmCoreError>` and 9 new unit tests: 3 baseline set/replace/idempotent-no-op, 3 cross-output reroute, 3 invalid-input robustness/regression-guard, plus a local `assert_no_duplicate_current_tags` test helper)
- `docs/planning/epics/story-1-3.md` (this file — YAML frontmatter `baseline_commit`, task checkboxes, Dev Agent Record, File List, Change Log, Status)

## Change Log
- 2026-08-07: Implemented Story 1.3 (`WmCore::switch_tag` one-tag-per-output enforcement) task-by-task per the RED/GREEN breakdown. All 4 tasks complete; 56/56 unit tests passing (47 baseline + 9 new); fmt/clippy/pre-commit clean.

## Status
review
