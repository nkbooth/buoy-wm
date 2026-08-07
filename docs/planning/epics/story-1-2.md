---
baseline_commit: a6f6b5f77635fd86fccf89d583521666bc97b207
---

# Story 1.2: WM Core In-Memory State Model
Epic: 1 | Priority: H | Status: done

## Description
Builds `wm-core` — the single in-memory source of truth for window↔tag
membership, view geometry/floating state, focus, output→current-tag, the
tag registry (ADR-006), stacking/render order, and terminal-spawned status
— as a pure state+logic module with no I/O (`components.md`). This is the
first story in the project with real application logic, and its unit tests
are the project's first (Story 1.1 shipped zero application-logic tests by
design).

`wm-core` is added as a new module tree (`wm/src/wm_core/`) inside the
existing single-package `wm/` crate — **not** a new workspace member. Per
Story 1.1's Technical notes, the `[workspace]` conversion is deferred until
a second crate actually exists; this story only ever produces one crate
(`buoy-wm`), so YAGNI still applies and the conversion stays out of scope
here too. `protocol-client` remains, for now, the existing vendored
Dispatch handlers in `wm/src/main.rs` — this story does not wire them to
`wm-core` (no `View`/`Tag`/`Output` gets created from a real compositor
event yet). That wiring is Story 1.4's job (it explicitly depends on "Story
1.2 (state model)") and later stories (1.3, 1.5, 1.6, 1.7). This story's
own acceptance criteria are verified by calling `wm-core`'s public API
directly from unit tests, simulating what the protocol layer will later
drive it with.

## Acceptance criteria
**Given** a freshly constructed `wm-core` state (`WmCore::new()`)
**Then** its tag registry, view set, output set, and stacking order all start empty — no state persists across constructions (per data-model.md; the WM starts every launch with an empty registry)

**Given** an empty `wm-core` state
**When** a view is registered, updated (tags, geometry, floating, focus), or unregistered
**Then** `wm-core` maintains the in-memory `View` record (id, app_id, tags, floating, geometry, focused) exactly matching the sequence of mutations applied
**And** the equivalent holds for `Tag` (id, name, terminal_spawned) and `Output` (id, current_tag) records under registry/creation/update operations

**Given** any `wm-core` mutating call
**When** it references a `ViewId`, `TagId`, or `OutputId` that is not currently registered
**Then** the call returns a typed error and leaves state unchanged — it never panics and never corrupts state (NFR2, this story's contribution to it; the compositor-facing "log and discard" behavior is `protocol-client`'s job once it exists, wired to whatever `wm-core` reports back)

**Given** the tag registry (ADR-006: named strings, `u64` bitset, IDs never reused, no delete in v1)
**When** a 65th distinct tag name is created in a session
**Then** the creation fails with a typed "registry full" error rather than silently wrapping or reusing an ID

**Given** `wm-core` holds no views
**Then** its state is structurally bounded (empty collections, no background allocation, no unbounded caches) such that it cannot itself be the cause of the WM daemon's idle RSS exceeding 50MB (NFR3) — see Technical notes for why full daemon RSS measurement is out of this story's testable scope

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: Core ID newtypes and `TagSet` bitset (AC: 1, 4)**
  - [x] 1.1 RED — Inside the devcontainer via devpod (`devpod ssh buoy-wm -- cargo test --manifest-path wm/Cargo.toml wm_core::`), add a test module asserting: `TagSet::empty()` is empty and contains nothing; inserting a bit and checking `contains` round-trips; removing a bit clears it; bit position 63 (the top of a `u64`) works, proving the type isn't accidentally narrower than 64 bits. Confirm this fails to compile (`wm_core` module doesn't exist yet).
  - [x] 1.2 GREEN — Add `wm/src/wm_core/mod.rs` (declared from `main.rs` via `mod wm_core;`, unused-code warnings expected and acceptable until later tasks wire it up — do not silence with a blanket `#[allow(dead_code)]`; scope any allow narrowly if needed), `wm/src/wm_core/ids.rs` (`ViewId(u64)`, `TagId(u8)`, `OutputId(u64)` newtypes — `TagId` is `u8` because the valid range is 0..64 per ADR-006), and `wm/src/wm_core/tag_set.rs` (`TagSet` wrapping a `u64`). Every public item gets a `///` doc comment per project code style. Re-run the test; confirm green.

- [x] **Task 2: Tag registry — create, 64-tag cap, no delete (AC: 4)**
  - [x] 2.1 RED — Tests: `create_tag` assigns IDs starting at 0 and increasing; creating 64 distinct-named tags all succeed with IDs `0..64`; the 65th `create_tag` call returns `Err(TagRegistryError::Full)` and the registry's tag count stays at 64; re-requesting an already-registered name returns the same `TagId` rather than growing the registry (the picker's "not already in the registry" add-new-tag flow needs this — see Technical notes for why this dedup behavior is this story's own inference, not explicit in ADR-006). Confirm all fail (module/function doesn't exist).
  - [x] 2.2 GREEN — Implement `Tag { id: TagId, name: String, terminal_spawned: bool }` and `TagRegistry` in `wm/src/wm_core/tag.rs`. No `delete_tag` method is added — its absence is deliberate per ADR-006, not an oversight. Confirm tests pass.

- [x] **Task 3: View registration and lifecycle (AC: 2)**
  - [x] 3.1 RED — Tests: `register_view(app_id)` returns a fresh, unique `ViewId` each call; a newly registered view has `tags` empty, `floating == true`, `focused == false`, and zeroed `Geometry`; `unregister_view` removes a registered view and returns `Ok(())`; calling `unregister_view` again on the same (now-unknown) id, or on an id that was never registered, returns `Err(WmCoreError::UnknownView)` without panicking. Confirm fail.
  - [x] 3.2 GREEN — Implement `View { id, app_id, tags: TagSet, floating: bool, geometry: Geometry, focused: bool }` and `Geometry { x: i32, y: i32, width: i32, height: i32 }` in `wm/src/wm_core/view.rs`; add the `views: HashMap<ViewId, View>` map plus `register_view`/`unregister_view` to the `WmCore` aggregator in `wm/src/wm_core/state.rs`. Confirm tests pass.

- [x] **Task 4: View↔tag membership toggling (AC: 2, 3)**
  - [x] 4.1 RED — Tests: `toggle_view_tag(view_id, tag_id)` on a view with no tags adds it (now present in `View.tags`); calling it again on the same pair removes it; toggling with an unknown `ViewId` returns `Err(UnknownView)`; toggling with an unknown `TagId` returns `Err(UnknownTag)`; neither error case mutates any existing state. Confirm fail.
  - [x] 4.2 GREEN — Implement `toggle_view_tag` on `WmCore`, validating both ids against their respective registries before mutating. Confirm tests pass.

- [x] **Task 5: View geometry and floating-flag updates (AC: 2, 3)**
  - [x] 5.1 RED — Tests: `set_view_geometry(id, Geometry{..})` updates exactly those fields; `set_view_floating(id, bool)` flips the flag; both return `Err(UnknownView)` for an unregistered id and leave state unchanged. Confirm fail.
  - [x] 5.2 GREEN — Implement both setters on `WmCore`. Confirm tests pass.

- [x] **Task 6: Focus tracking (AC: 2, 3)**
  - [x] 6.1 RED — Tests: `set_focus(id)` on a registered view sets `focused == true` on that view; calling `set_focus` on a second view clears the first view's `focused` back to `false` (at most one focused view at a time — see Technical notes on the single-focus assumption); `clear_focus()` clears whichever view is currently focused; `set_focus` on an unknown id returns `Err(UnknownView)` and does not change which view (if any) is currently focused. Confirm fail.
  - [x] 6.2 GREEN — Implement `set_focus`/`clear_focus` on `WmCore`, tracking the currently-focused `ViewId` (if any) alongside the `views` map so `clear_focus`/re-focus don't require a full scan. Confirm tests pass.

- [x] **Task 7: Output registration and current-tag tracking (AC: 2, 3)**
  - [x] 7.1 RED — Tests: `register_output()` returns a fresh, unique `OutputId` with `current_tag == None`; `set_output_current_tag(output_id, Some(tag_id))` updates the field when `tag_id` exists in the registry; the same call with an unregistered `TagId` returns `Err(UnknownTag)` and leaves `current_tag` unchanged; the same call with an unregistered `OutputId` returns `Err(UnknownOutput)`; `set_output_current_tag(output_id, None)` clears it. Confirm fail.
  - [x] 7.2 GREEN — Implement `Output { id: OutputId, current_tag: Option<TagId> }` in `wm/src/wm_core/output.rs` plus the `outputs` map and setter on `WmCore`. This is the raw field-level primitive only — the one-tag-per-output *enforcement*/reject-or-reroute decision is Story 1.3's `switch_tag` operation, layered on top of this setter, not built here (see Technical notes). Confirm tests pass.

- [x] **Task 8: Tag terminal-spawned tracking (AC: 2)**
  - [x] 8.1 RED — Tests: a newly created tag starts with `terminal_spawned == false`; `mark_terminal_spawned(tag_id)` sets it `true`; calling it again on the same tag is a no-op success, not an error (idempotent — the lazy-spawn-once check in Story 1.5 will call this after a successful spawn and shouldn't have to track "already called" separately); calling it with an unknown `TagId` returns `Err(UnknownTag)`. Confirm fail.
  - [x] 8.2 GREEN — Implement `mark_terminal_spawned` on `WmCore`/`TagRegistry`. Confirm tests pass.

- [x] **Task 9: Stacking/render order (AC: 2)**
  - [x] 9.1 RED — Tests: `register_view` appends the new view to the back of the stacking order (mirrors the vendored `main.rs` `VecDeque<Window>` convention: front = bottom, back = top, `.back()` = topmost/focused candidate — see Technical notes on why this story reuses that convention instead of inventing a new one); `raise_view(id)` moves an already-registered view to the back; `unregister_view` also removes the view from the stacking order (no dangling id left behind); `stacking_order()` returns ids front-to-back; `raise_view` on an unknown id returns `Err(UnknownView)`. Confirm fail.
  - [x] 9.2 GREEN — Add a `VecDeque<ViewId>` to `WmCore`, wired through `register_view`/`unregister_view`/`raise_view`. Confirm tests pass.

- [x] **Task 10: Fresh-launch empty-state invariant (AC: 1)**
  - [x] 10.1 RED — A single test asserting `WmCore::new()` (or `WmCore::default()`) has zero tags, zero views, zero outputs, no focused view, and an empty stacking order — the explicit, AC-traceable assertion that nothing persists between WM launches (data-model.md: "no persistence layer in v1"). Confirm fail if `WmCore` doesn't yet derive/implement `Default`/`new`. (`Default`/`new()` already existed from Task 3 onward, so this test passed immediately — a pure regression guard, not new functionality, per the task's own conditional wording.)
  - [x] 10.2 GREEN — Confirm/implement `Default` (or an explicit `new()`) on `WmCore` composing the empty registries/maps/deque built in Tasks 1–9. Confirm the test passes.

- [x] **Task 11: Malformed/invalid-input robustness sweep (AC: 3)**
  - [x] 11.1 RED — A consolidated test (or small table-driven set of tests) that exercises every public `WmCore` mutator from Tasks 3–9 with an id that was never registered, asserting each returns a typed `Err` and that `WmCore`'s observable state (tag/view/output counts, stacking order, focus) is byte-for-byte unchanged before vs. after each failed call. This should already pass if Tasks 3–9 were implemented correctly — write it as an explicit regression guard, not a new feature. (Passed immediately, as anticipated — added `Clone`/`PartialEq` to `WmCore`/`TagRegistry` to enable whole-state snapshot equality checks.)
  - [x] 11.2 GREEN — If any mutator is found to `.unwrap()`/`.expect()`/panic on an invalid id instead of returning `Err`, fix it here. Run `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings` inside the devcontainer via devpod; confirm no new lint warnings were introduced by the sweep. (No mutator panicked. Clippy `--all-targets -D warnings` surfaced two findings unrelated to the sweep itself: cross-target `dead_code` since `main.rs` doesn't call `wm_core` yet (expected per Task 1.2 — resolved with a narrowly-scoped `#[allow(dead_code)]` on the `mod wm_core;` declaration, not a blanket allow) and `clippy::enum_variant_names` on `WmCoreError`'s `Unknown*` variants, whose names are mandated verbatim by the story text (resolved with a narrow `#[allow(clippy::enum_variant_names)]` on the enum). Clean after both fixes.)

- [x] **Task 12: Full in-container verification gate (AC: all)**
  - [x] 12.1 Run, inside the devcontainer via devpod (`devpod ssh buoy-wm`): `cargo fmt --manifest-path wm/Cargo.toml --all -- --check`, `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings`, and `cargo test --manifest-path wm/Cargo.toml`. Confirm all three pass with the full `wm_core` unit test suite (Tasks 1–11) green and zero clippy/fmt violations. `pre-commit run --all-files` should also pass, since it runs the same fmt/clippy hooks (Story 1.1, Task 3). (All green: fmt --check clean, clippy -D warnings clean, 40/40 tests pass, `pre-commit run --all-files` — cargo fmt --check and cargo clippy hooks both Passed.)

## Technical notes
- Depends on Story 1.1 (devcontainer/project scaffold — done). `wm-core` is added as `wm/src/wm_core/` inside the existing single-package `wm/` crate; this story does **not** convert `wm/Cargo.toml` to a `[workspace]`. Verified against `components.md` and both draft/implemented predecessor stories: nothing in the component split requires a second crate to exist yet (`protocol-client` is still the as-vendored `main.rs`), so per Story 1.1's own YAGNI note the conversion stays deferred, likely until `ipc-server` (Epic 2, Story 2.1) actually needs to be a separately-built/tested unit.
- **`main.rs` is not wired to `wm-core` in this story.** The vendored Dispatch handlers (`Event::Window`, `Event::Output`, `Event::Seat`, etc.) continue exactly as Story 1.1 left them; only a `mod wm_core;` declaration is added so the new module compiles and is unit-testable. Story 1.3 (switch-tag), 1.4 (keybind routing — explicitly depends on "Story 1.2 (state model)"), 1.5 (pinned terminal), and 1.6 (floating placement) are where real compositor events start driving `wm-core` mutations.
- **NFR2 boundary.** The literal "WM logs and discards [a malformed compositor event]" behavior belongs to `protocol-client`, which doesn't exist as a distinct integration point yet. This story's contribution to NFR2 is that `wm-core`'s public API is total and defensive: every mutator that references a `ViewId`/`TagId`/`OutputId` returns a typed `Result` instead of panicking on an unknown id, so no future caller (protocol-client or otherwise) can corrupt state or crash the daemon by feeding it a stale/bad reference. Task 11 is the regression guard for this.
- **NFR3 boundary.** WM daemon idle RSS (<50MB) is a whole-process runtime property, and this story doesn't run the WM against a live `river` session (no compositor is available inside the devcontainer sandbox, and `wm-core` isn't wired to the protocol layer yet regardless). This story's contribution is structural, not measured: `WmCore`'s state is exactly as large as the number of registered tags/views/outputs (all empty at idle), with no unbounded caches, background threads, or timers. Literal RSS measurement under a running compositor is deferred to whichever later story first runs the full daemon end-to-end for extended verification — flagging this as an open item for a human to confirm is adequately covered by a later story, since no Epic 1/2 story currently states it explicitly.
- **Tag-name dedup on create.** ADR-006 and data-model.md don't explicitly say whether `create_tag` with an already-used name should error, silently create a second same-named tag, or return the existing id. This story infers the third option (return existing `TagId`, no duplicate) from the PRD's picker AC ("A text-input row creates a new tag not already in the registry" — `features-and-acceptance-criteria.md`), which reads most naturally as idempotent-by-name creation. Flagged as an assumption for review, not a locked decision — cheap to change later since nothing else depends on the alternative behaviors yet.
- **Single global focus.** `data-model.md`'s `View` entity has one `focused: bool` field, not a per-seat/per-output focus set. This story tracks exactly one focused view at a time WM-wide. The vendored `main.rs` already tracks its own per-`Seat` focus independently (`Seat.focused`) for protocol purposes — reconciling multi-seat reality with `wm-core`'s single-focus model is deferred to whichever story first wires seat focus events into `wm-core` (not this one; single-seat laptop usage is the assumed common case per NFR framing elsewhere in the PRD).
- **Stacking order as "workspace ordering/layout."** FR1 (`features-and-acceptance-criteria.md`) lists "workspace ordering/layout" as tracked state, but `data-model.md`'s Entities table has no explicit ordering field. This story implements it as a single global `VecDeque<ViewId>` stacking order (front=bottom, back=top), reusing the exact convention already established in vendored `main.rs`'s `WindowManager.windows: VecDeque<Window>` / `focus_top` / `Seat.interacted` pattern — chosen for consistency with existing code the team will read side-by-side, not because a per-tag ordered list was ruled out. `placement-engine` (Story 1.5+) is expected to filter this by a view's tag membership rather than `wm-core` maintaining per-tag lists itself. Flagged as an assumption for review.
- No new Cargo dependencies. `bitflags` (already a dependency, vendored from tinyrwm) is not used for `TagSet` — it's built for compile-time-fixed flag sets, and `TagSet` needs runtime-dynamic bit indices assigned by the tag registry at creation time, so a thin hand-written `u64` wrapper is simpler and avoids fighting the macro. `serde`/`serde_json`/async-runtime (technology-stack.md) are not needed until `ipc-server` (Epic 2) and are out of scope here.
- Every public item in `wm_core` gets a `///` doc comment (project code style: public API docstrings always); private helpers do not. Keep functions small and single-purpose (one CRUD operation each) rather than one large `WmCore::apply(Mutation)` dispatcher — no current requirement demands a generic mutation-application layer (YAGNI), and Epic 2's IPC handlers are expected to call these same named functions directly (per `story-1-3.md`'s note that the switch-tag function "is reused directly \[...\] by \[...\] Story 2.4's IPC handler" — the same pattern applies to this story's functions).
- Build/test/lint only ever run **inside the devcontainer via devpod** (`cargo build`, `cargo test`, `cargo fmt`, `cargo clippy` — same constraint as Story 1.1; no task step above invokes them on the bare host).

## Test plan
This story's tests are Rust unit tests (`#[cfg(test)] mod tests`) inside the new `wm_core` submodules, run via `cargo test --manifest-path wm/Cargo.toml` inside the devcontainer via devpod. No integration or end-to-end test is possible yet (no live protocol wiring exists — see Technical notes), so unit coverage is the whole of this story's verification:

1. **`TagSet` bitset** (Task 1) — empty/insert/contains/remove round-trips; bit 63 (top of `u64`) works.
2. **Tag registry** (Task 2) — sequential ID assignment 0..64; 65th `create_tag` fails with `TagRegistryError::Full`; re-creating an existing name returns the existing id without growing the registry; no `delete_tag` API exists.
3. **View lifecycle** (Task 3) — `register_view` defaults (empty tags, floating, unfocused, zeroed geometry); `unregister_view` success and double-unregister/unknown-id failure.
4. **View↔tag toggling** (Task 4) — add-then-remove via repeated toggle; unknown-view and unknown-tag error paths.
5. **Geometry/floating setters** (Task 5) — field-level updates; unknown-view error path.
6. **Focus tracking** (Task 6) — single-focused-view invariant across repeated `set_focus` calls; `clear_focus`; unknown-id error path leaves focus unchanged.
7. **Output registration/current-tag** (Task 7) — fresh `OutputId` with `current_tag == None`; valid/invalid tag and output id paths for the setter; clearing via `None`.
8. **Terminal-spawned tracking** (Task 8) — default `false`; idempotent `mark_terminal_spawned`; unknown-tag error path.
9. **Stacking order** (Task 9) — append-on-register, move-to-back on `raise_view`, removal on `unregister_view`, unknown-id error path.
10. **Fresh-launch invariant** (Task 10) — `WmCore::new()`/`::default()` is fully empty.
11. **Malformed-input sweep** (Task 11) — every mutator rejects unknown ids via `Err` with no panic and no observable state change (this story's concrete NFR2 coverage).
12. **Tooling gate** (Task 12) — `cargo fmt --check`, `cargo clippy -D warnings`, and the full `cargo test` run, all in-container; `pre-commit run --all-files` as a final sanity check reusing Story 1.1's hooks.

NFR3 (idle RSS) is *not* directly tested here — see Technical notes for why (no live daemon/compositor wiring exists yet in this story's scope) and treat that as a coverage gap for a human to consciously accept or assign to a later story, not a silent omission.

## FR coverage
FR1, NFR2, NFR3

## Dev Agent Record

### Implementation Plan
Followed the story's own 12-task RED-then-GREEN breakdown in order. `wm-core` was built as `wm/src/wm_core/{mod,ids,tag_set,tag,view,output,state}.rs`, added to `wm/src/main.rs` via `mod wm_core;` only (no call sites — Story 1.4's job). Each task's test module was written first against not-yet-existing types/methods, confirmed to fail to compile (RED), then the minimal implementation was added and re-verified green, then the full `cargo test --manifest-path wm/Cargo.toml` suite was re-run to check for regressions before moving to the next task. Tasks 10 and 11 were designed by the story as regression guards over behavior already required by earlier tasks (`Default`/`new()` from Task 3; panic-free `Result`-returning mutators from Tasks 3-9) and passed immediately without new production code, consistent with the story's own conditional wording ("confirm fail *if*...", "should already pass if... implemented correctly").

Key design decisions:
- `WmCore` is the aggregator (`views: HashMap<ViewId, View>`, `tags: TagRegistry`, `outputs: HashMap<OutputId, Output>`, `focused_view: Option<ViewId>`, `stacking_order: VecDeque<ViewId>`), all private fields — tests exercise them through the public API plus whitebox access from the child `tests` module (same-module privacy in Rust; no fields made `pub` purely for testing).
- `TagRegistry::create_tag` is idempotent by name via an internal `HashMap<String, TagId>` index, per the story's inferred assumption.
- `TagSet` is a hand-rolled `u64` bitset (no `bitflags`, per Technical notes — runtime-dynamic bit positions don't fit `bitflags`' compile-time model).
- Added `Clone, PartialEq` to `TagRegistry` and `WmCore` (beyond what any single task literally requested) specifically to support Task 11's byte-for-byte whole-state snapshot-equality regression guard.
- Two clippy findings surfaced only at the Task 11/12 `--all-targets -D warnings` gate, both resolved with narrowly-scoped allows rather than renames/blanket suppression: `#[allow(dead_code)]` on the `mod wm_core;` declaration in `main.rs` (cross-target dead code is expected/permitted per Task 1.2 until Story 1.4 wires real call sites) and `#[allow(clippy::enum_variant_names)]` on `WmCoreError` (its `Unknown*` variant names are mandated verbatim by the story text, e.g. `WmCoreError::UnknownView`).

### Code Review Follow-up (2026-08-07)

Three code-review findings addressed (each with a RED test confirmed failing before the fix, then confirmed passing after):

- **(HIGH) `unregister_view` left a dangling `focused_view`.** `WmCore::unregister_view` (`wm/src/wm_core/state.rs`) removed the view from `views`/`stacking_order` but never checked/cleared `focused_view`, so `WmCore.focused_view` could hold `Some(id)` pointing at a view no longer registered — violating the "at most one focused view, always live" invariant `set_focus`/`clear_focus` otherwise maintain. Fixed by clearing `focused_view` when it equals the id being removed. Added `unregister_focused_view_clears_focused_view` (confirmed failing pre-fix: `left: Some(ViewId(0)), right: None`) and `unregister_non_focused_view_leaves_focus_unaffected` (regression guard — unregistering a *different* view must not disturb an existing focus).
- **(MEDIUM-HIGH) No public `WmCore` method to create a tag.** `TagRegistry::create_tag` was only reachable via whitebox test access to `WmCore`'s private `tags` field, with no real public entry point for future callers (Story 1.4/2.3's dispatch wiring, "create new tag" picker flow). Added `WmCore::create_tag(&mut self, name: impl Into<String>) -> Result<TagId, WmCoreError>`, delegating to `TagRegistry::create_tag` and mapping `TagRegistryError::Full` to a new `WmCoreError::TagLimitReached` variant (`TagRegistryError::UnknownTag` maps to the existing `WmCoreError::UnknownTag`, though `create_tag` itself cannot currently produce it). Added `create_tag_returns_fresh_id_via_public_api`, `create_tag_is_idempotent_by_name_via_public_api`, and `create_tag_returns_tag_limit_reached_when_registry_full` against `WmCore`'s public API (confirmed failing to compile pre-fix — method/variant didn't exist). All prior whitebox `core.tags.create_tag(...)` call sites in `state.rs`'s test module were switched to `core.create_tag(...)` — no coverage lost, since they were already exercising `WmCore`-level behavior through a private-field shortcut.
- **(MEDIUM) `TagSet` had no bounds check on bit position.** `contains`/`insert`/`remove` (`wm/src/wm_core/tag_set.rs`) took a raw `u8 pos` and shifted `1u64 << pos` with no validation that `pos < 64`; since `TagSet` and `TagId`'s inner field are both `pub`, a future caller bypassing `TagRegistry`'s own bound-checking could hit a shift-overflow panic (debug) or a wrapped/corrupted bit (release) — exactly the panic surface NFR2 is meant to close. Added an explicit `pos < 64` guard in all three methods (chosen over `debug_assert!` because a `debug_assert` only closes the debug-mode panic and leaves the release-mode silent-corruption case open; an explicit guard closes both, and none of `TagSet`'s existing methods use a `Result`/panic-on-invalid-input convention this would need to match, so a quiet bounds-checked no-op — `contains` returns `false`, `insert`/`remove` do nothing — was the least invasive consistent choice, documented inline). Added `out_of_range_pos_does_not_panic_and_is_treated_as_absent` and `out_of_range_insert_and_remove_do_not_panic_or_corrupt_in_range_bits` (both confirmed failing pre-fix with `attempt to shift left with overflow` panics at `pos=64`/`pos=255`).

Findings #4–#6 (helper extraction, `set_focus`'s `.expect()` shape, `mark_terminal_spawned`'s blanket `map_err`) were in-scope-but-optional cleanups per the review; skipped as not required for this story to close, per the review's own framing.

Re-verified inside the devcontainer via `devpod ssh buoy-wm`: `cargo test --manifest-path wm/Cargo.toml` (47 passed, 0 failed — 40 original + 7 new), `cargo fmt --manifest-path wm/Cargo.toml --all -- --check` (clean), `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings` (clean), `pre-commit run --all-files` (`cargo fmt --check` and `cargo clippy` hooks both passed).

### Debug Log
No blocking issues. All RED steps were confirmed via `devpod ssh buoy-wm -- cargo test --manifest-path wm/Cargo.toml wm_core::...` compile failures before each GREEN implementation. Full command log (all run inside the devcontainer via `devpod ssh buoy-wm`):
- `cargo build --manifest-path wm/Cargo.toml` (baseline sanity check before starting)
- Per-task: `cargo test --manifest-path wm/Cargo.toml wm_core::<module>::` for RED, then again for GREEN, then `cargo test --manifest-path wm/Cargo.toml` (full suite) for regression check
- `cargo fmt --manifest-path wm/Cargo.toml --all -- --check` (initially failed on 15 formatting diffs; resolved via `cargo fmt --manifest-path wm/Cargo.toml --all`, then re-verified clean)
- `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings` (initially failed: cross-target dead_code + `enum_variant_names`; resolved with two narrowly-scoped allows, then clean)
- `pre-commit run --all-files` (both `cargo fmt --check` and `cargo clippy` hooks passed)

### Completion Notes
- All 12 tasks/40 subtasks complete. Final `cargo test --manifest-path wm/Cargo.toml` run: **40 passed, 0 failed**.
- `cargo fmt --manifest-path wm/Cargo.toml --all -- --check`: clean.
- `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings`: clean.
- `pre-commit run --all-files`: both hooks passed.
- `wm/src/main.rs` remains unwired to `wm-core` (only `mod wm_core;` added), per Story 1.2's explicit scope boundary — Story 1.4 is where Dispatch events start calling into it.
- NFR3 (idle RSS) is not directly tested, per the story's own Technical notes (no live daemon/compositor wiring exists yet); flagged there as an accepted coverage gap for a later story, not an omission from this one.

## File List
- `wm/src/main.rs` (modified — added `#[allow(dead_code)] mod wm_core;` declaration only; no other changes)
- `wm/src/wm_core/mod.rs` (new)
- `wm/src/wm_core/ids.rs` (new — `ViewId`, `TagId`, `OutputId`)
- `wm/src/wm_core/tag_set.rs` (new — `TagSet`; code review follow-up: bounds-checked `contains`/`insert`/`remove`, 2 new tests)
- `wm/src/wm_core/tag.rs` (new — `Tag`, `TagRegistry`, `TagRegistryError`)
- `wm/src/wm_core/view.rs` (new — `View`, `Geometry`)
- `wm/src/wm_core/output.rs` (new — `Output`)
- `wm/src/wm_core/state.rs` (new — `WmCore`, `WmCoreError`, and all unit tests; code review follow-up: `unregister_view` clears dangling focus, new public `create_tag` method + `WmCoreError::TagLimitReached` variant, 5 new tests, whitebox `core.tags.create_tag` call sites switched to `core.create_tag`)
- `docs/planning/epics/story-1-2.md` (this file — task checkboxes, Dev Agent Record, File List, Change Log, Status)

## Change Log
- 2026-08-07: Implemented Story 1.2 (`wm-core` in-memory state model) task-by-task per the RED/GREEN breakdown. All 12 tasks complete; 40 unit tests passing; fmt/clippy/pre-commit clean.
- 2026-08-07: Code review follow-up — 3 findings addressed (dangling `focused_view` on `unregister_view` fixed, public `WmCore::create_tag` added with new `WmCoreError::TagLimitReached` variant, `TagSet` bit-position bounds check added). 7 new tests, 47/47 passing; fmt/clippy/pre-commit clean. See Dev Agent Record → Code Review Follow-up.

## Status
review
