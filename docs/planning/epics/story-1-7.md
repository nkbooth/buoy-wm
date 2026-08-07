---
baseline_commit: 1e9b394a34a51c353df60dadfba3f8dd8128f714
---

# Story 1.7: Raw-Keybind Tag Switching & Creation
Epic: 1 | Priority: H | Status: done

## Description
Keybind-driven tag switching and creation, sufficient to validate the
multi-tag model end-to-end before the fuzzel-based picker exists (FR13).
This is the last story in Epic 1 — an Epic 1 retrospective follows its
completion.

This is also the story that finally gives `WmCore::switch_tag` (Story 1.3)
and `WmCore::create_tag` (Story 1.2) their first live keybind, and — per
Story 1.5's Technical notes ("Reachable today vs. pending Story 1.7") — the
trigger point for `WindowManager::ensure_pinned_terminal_spawned`, which has
sat structurally complete but uncalled since Story 1.5. Confirmed by reading
current `wm/src/main.rs` before drafting this story: **no code anywhere
calls `wm_core.switch_tag`, `wm_core.create_tag`, `wm_core.register_output`,
or `WindowManager::ensure_pinned_terminal_spawned` today** — all four are
either `#[allow(dead_code)]`-marked or (for `register_output`) never called
at all. This story wires all four for real.

**Two gaps neither the PRD nor the ADRs resolve, closed here rather than
silently invented or deferred:**

1. **How does a raw keybind supply a tag *name* with no text-input UI yet
   (fuzzel picker is Epic 2)?** `features-and-acceptance-criteria.md`'s
   tag-manager-popup section is explicit that free-text tag naming is a
   picker feature; no ADR addresses a keybind-only equivalent.
   **Resolution:** the tag-create keybind generates a placeholder name
   deterministically (`format!("tag{n}")`, where `n` is the registry's
   current tag count at the moment of creation — `tag0`, `tag1`, `tag2`,
   ...), reusing the existing `format!("tag{i}")` convention already used by
   this codebase's own tests (`wm/src/wm_core/state.rs`'s
   `create_tag_returns_tag_limit_reached_when_registry_full`). This
   satisfies FR13's literal "sufficient to validate multi-tag behavior
   end-to-end" without inventing any UI. Flagged as an assumption — see
   Technical notes.
2. **Which output does "the active output" mean, given `main.rs` has no
   concept of a focused/active output anywhere today?** Neither
   `RiverOutputV1` nor `RiverSeatV1` events carry any output↔seat
   association in the vendored protocol bindings (confirmed by reading
   `wm/protocol/river-window-management-v1.xml` and the current
   `Dispatch<RiverSeatV1, ...>`/`Dispatch<RiverOutputV1, ...>` impls in
   `main.rs`) — there is no live signal to compute a real "currently
   focused output" from yet. **Resolution:** this story registers every
   real output into `wm_core` and deterministically targets the
   lowest-`OutputId` (i.e. first-registered) output for both keybinds. This
   is correct-by-construction for the single-output case (ADR-005: "~95% of
   real usage is single-monitor") and deterministic (not silently
   inconsistent) under multi-output. Real focused-output tracking is
   explicitly out of scope here — `EXPERIENCE.md`'s "picker follows focus...
   opens on the currently-focused output" is Epic 2's problem once picker
   invocation gives a real seat/pointer signal to key off of. Flagged as an
   assumption — see Technical notes.

## Acceptance criteria
**Given** at least one tag exists in the registry
**When** I invoke the tag-cycle keybind
**Then** the active output's current tag advances to the next tag in the registry, subject to one-tag-per-output enforcement (Story 1.3)
**And** the switch completes within the 50ms latency budget (NFR1)
**And** if this is the first switch to the newly-current tag this session, the pinned terminal for it is spawned (Story 1.5's `ensure_pinned_terminal_spawned`, now live)

**When** I invoke the tag-create keybind
**Then** a new `Tag` is added to the registry, named `tag<N>` (a generated placeholder — see Description; no text-input UI exists yet), with a freshly assigned bitmask ID, never reusing a previously assigned ID (ADR-006)
**And** creating a tag does not switch any output to it — a separate tag-cycle press is required to make it visible

**Given** 64 tags already exist in the registry
**When** I attempt to create another
**Then** creation fails and the failure is surfaced (logged, per this codebase's established NFR2 error-visibility convention — no richer UI surfacing exists until Epic 2's picker) rather than silently ignored

**And** the `wm-core` functions this exercises (`switch_tag`, `create_tag`) are the same functions Epic 2's IPC handlers (Story 2.1) call — no duplicate logic path: the tag-cycle keybind composes `switch_tag`, the tag-create keybind composes `create_tag`, neither reimplements the other's decision logic

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: `TagRegistry::ids` — ordered tag-id accessor, the data `cycle_tag` needs (AC: "advances to the next tag in the registry")**
  - [x] 1.1 RED — Inside the devcontainer via devpod (`devpod ssh buoy-wm -- cargo test --manifest-path wm/Cargo.toml wm_core::tag::`), add tests to `wm/src/wm_core/tag.rs`'s existing test module: `ids_returns_empty_vec_when_registry_is_empty` — a fresh `TagRegistry::new()`, assert `registry.ids() == Vec::<TagId>::new()`; `ids_returns_ids_in_creation_order` — `create_tag("a")`, `create_tag("b")`, `create_tag("c")`, assert `registry.ids() == vec![id_a, id_b, id_c]` (creation order, not alphabetical or any other order). Confirm both fail to compile — `ids` doesn't exist yet.
  - [x] 1.2 GREEN — Add `pub fn ids(&self) -> Vec<TagId> { self.tags.iter().map(|t| t.id).collect() }` to `TagRegistry` in `wm/src/wm_core/tag.rs`, directly below `count()`. `///` doc comment: pure query, returns tag ids in creation (registration) order — the order `WmCore::cycle_tag` treats as canonical. Confirm both new tests pass; confirm no regression in the existing 4-test `tag` module suite.

- [x] **Task 2: `WmCore::cycle_tag` — the tag-cycle decision, composing `switch_tag` (not duplicating it) (AC: tag-cycle bullets)**
  - [x] 2.1 RED — Add tests to `wm/src/wm_core/state.rs`'s existing test module (import `TagRegistry`/`ids` is not needed here — this exercises `WmCore` only): `cycle_tag_from_none_current_selects_first_tag_in_registry` — `register_output()` → `output_id`, `create_tag("a")` → `tag_a`, `create_tag("b")` → `tag_b`, output starts with `current_tag == None`; `core.cycle_tag(output_id)` returns `Ok(Some(tag_a))` and `core.outputs.get(&output_id).unwrap().current_tag == Some(tag_a)`. `cycle_tag_advances_to_next_tag_in_creation_order` — same setup, call `cycle_tag` twice; second call returns `Ok(Some(tag_b))`. `cycle_tag_wraps_around_to_first_tag_after_last` — same setup, call `cycle_tag` three times; third call returns `Ok(Some(tag_a))` again (wraps). `cycle_tag_returns_ok_none_when_no_tags_registered` — `register_output()` only, no tags created; `cycle_tag(output_id)` returns `Ok(None)` and does not panic, output's `current_tag` stays `None`. `cycle_tag_unknown_output_returns_error_and_leaves_state_unchanged` — `create_tag("a")`, a bogus `OutputId(999)`; assert `Err(WmCoreError::UnknownOutput)` and a whole-state `Clone`/`PartialEq` snapshot taken before the call is unchanged after (Story 1.2's established snapshot-equality pattern). `cycle_tag_reuses_switch_tag_enforcement_and_reroutes_away_from_other_output` — two outputs `o1`/`o2`, one tag `tag_a` already displayed on `o1` (`core.switch_tag(o1, tag_a).unwrap()`), `o2` has `current_tag == None`; call `core.cycle_tag(o2)` — asserts `o2.current_tag == Some(tag_a)` **and** `o1.current_tag == None` (rerouted away), directly proving `cycle_tag` goes through `switch_tag`'s existing ADR-005 enforcement rather than a separate, duplicated field write. Also extend the existing `every_mutator_rejects_unknown_ids_without_mutating_state` regression guard with one more assertion for `core.cycle_tag(bogus_output)` returning `Err(WmCoreError::UnknownOutput)` and leaving the snapshot unchanged. Confirm all six new tests plus the extended guard fail to compile — `cycle_tag` doesn't exist yet.
  - [x] 2.2 GREEN — Implement `pub fn cycle_tag(&mut self, output_id: OutputId) -> Result<Option<TagId>, WmCoreError>` on `WmCore` in `wm/src/wm_core/state.rs`, near `switch_tag`: validate `output_id` is registered (`self.outputs.get(&output_id).ok_or(WmCoreError::UnknownOutput)?`), read its `current_tag`, compute `let ids = self.tags.ids();` — if `ids.is_empty()`, return `Ok(None)` (no mutation); otherwise compute `next` as the tag immediately following `current_tag` in `ids` (wrapping to `ids[0]` after the last, and starting at `ids[0]` when `current_tag` is `None` or is a tag no longer found in `ids`, defensively — should not occur since tags are never deleted, ADR-006, but avoids an `.expect()`/panic on a hypothetically stale id, NFR2), then `self.switch_tag(output_id, next)?;` (reused, not duplicated — the AC's explicit dedup requirement) and return `Ok(Some(next))`. `///` doc comment stating this is the pure decision `main.rs`'s tag-cycle keybind will call, and that it deliberately delegates the actual field write + ADR-005 enforcement to `switch_tag` rather than reimplementing it. Confirm the six new tests plus the extended guard all pass (87 total: 79 baseline + 2 from Task 1 + 6 here — the extended guard is not a new `#[test]` fn); confirm no other regression.

- [x] **Task 3: `WmCore::create_tag_with_generated_name` — the tag-create decision, composing `create_tag` (not duplicating it) (AC: tag-create bullets)**
  - [x] 3.1 RED — Add tests to the same test module: `create_tag_with_generated_name_uses_tag_n_naming_convention` — on an empty registry, `core.create_tag_with_generated_name()` returns `Ok(id)` where `core.tags.get(id).unwrap().name == "tag0"`; a second call returns a fresh id named `"tag1"`. `create_tag_with_generated_name_returns_fresh_ids_on_repeated_calls` — three consecutive calls return three distinct `TagId`s. `create_tag_with_generated_name_fails_when_registry_full` — pre-fill the registry to 64 via 64 `create_tag(format!("tag{i}"))` calls (mirrors the existing `create_tag_returns_tag_limit_reached_when_registry_full` test's own setup), then `create_tag_with_generated_name()` returns `Err(WmCoreError::TagLimitReached)` and `core.tags.count()` stays 64. `create_tag_with_generated_name_delegates_to_create_tag_no_new_ids_by_construction` — whitebox: after a successful call, confirm the returned id is present via `core.tags.get(id)` with the expected generated name — proving the id came from the real `create_tag` path (ADR-006's assigned-bitmask-id + dedup-by-name machinery), not a separately-invented id allocator. Confirm all four fail to compile — the method doesn't exist yet.
  - [x] 3.2 GREEN — Implement `pub fn create_tag_with_generated_name(&mut self) -> Result<TagId, WmCoreError>` on `WmCore` in `wm/src/wm_core/state.rs`, near `create_tag`: `let name = format!("tag{}", self.tags.count()); self.create_tag(name)` — a thin one-line delegation, no duplicated id-assignment or dedup logic. `///` doc comment: this is the pure decision `main.rs`'s tag-create keybind will call in place of a text-input UI that doesn't exist yet (Epic 2); documents the `tag<N>` naming convention and flags the known, accepted collision risk (see Technical notes: "Generated-name collision risk") rather than silently hiding it. Confirm the four new tests pass; confirm no regression elsewhere.

- [x] **Task 4: Register real outputs into `wm_core` + a deterministic "active output" selector (AC: both keybinds need a live `OutputId` to act on — Wayland glue, no wm-core decision logic of its own)**
  - [x] 4.1 In `wm/src/main.rs`, add an `output_id: OutputId` field to the `Output` struct (`wm/src/main.rs:120-124`) — always present, not `Option`, since it is assigned at construction time (see 4.2). Update `Output::new` to `fn new(proxy: RiverOutputV1, output_id: OutputId) -> Self`, setting the new field.
  - [x] 4.2 In the `Dispatch<RiverWindowManagerV1, ()>` impl's `Event::Output { id }` arm (`wm/src/main.rs:842-844`), change `state.wm.outputs.insert(id.id(), Output::new(id));` to first call `let output_id = state.wm.wm_core.register_output();` then `state.wm.outputs.insert(id.id(), Output::new(id, output_id));` — registers a fresh `wm_core` `OutputId` the moment a real output appears, no two-phase "new" flag needed (unlike `Window`/`init_new_windows`) since `register_output` has no dependency on any other state that arrives later.
  - [x] 4.3 Add a private `WindowManager` method: `fn active_output_id(&self) -> Option<OutputId> { self.outputs.values().map(|o| o.output_id).min() }` — the lowest-`OutputId` (first-registered) output, deterministic under both single- and multi-output configurations (see Description's resolution #2 and Technical notes for the full rationale — this is a stated, accepted assumption, not a hidden guess).
  - [x] 4.4 Import `wm_core::ids::OutputId` in `main.rs`'s existing `use wm_core::ids::{TagId, ViewId};` line (`wm/src/main.rs:45`), becoming `use wm_core::ids::{OutputId, TagId, ViewId};`.
  - [x] **No RED/GREEN for this task.** Same carve-out and justification as every prior story's Wayland-glue tasks (Story 1.4 Tasks 3-4, Story 1.5 Tasks 3-5, Story 1.6 Task 2): `register_output` is pre-existing and already unit-tested (Story 1.2); `active_output_id` is a trivial, deterministic `Iterator::min()` over a `Copy` id type with no `wm-core` decision logic of its own. No live `river` compositor session exists in this sandbox to exercise real multi-output hotplug end-to-end (same boundary every prior story has hit). Verification is `cargo build --manifest-path wm/Cargo.toml` and `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings` succeeding, plus structural code review confirming `register_output` is called exactly once per real `Event::Output`.
  - [x] **Known, explicitly-flagged gap (not fixed by this story — see Technical notes "Output lifecycle: registration only, no unregister"):** `wm_core` has no `unregister_output` method; `remove_outputs` (`wm/src/main.rs:218-226`) destroys the real Wayland proxy on hotplug-removal but never removes the corresponding `wm_core` output. A removed real output leaves a stale, permanently-registered `wm_core` output behind, eligible forever after to be selected by `active_output_id`. Out of scope here: FR13 does not ask for output lifecycle cleanup, no ADR governs it, and building it now would be speculative (YAGNI) for a single-user, mostly-single-monitor project (ADR-005). Flagged for a human to consciously accept, not silently omitted.

- [x] **Task 5: `Action::TagCycle` / `Action::TagCreate` + raw keybinds (AC: "invoke the tag-cycle keybind" / "invoke the tag-create keybind" — Wayland glue, no wm-core decision logic of its own)**
  - [x] 5.1 Add two variants to the `Action` enum (`wm/src/main.rs:59-68`): `TagCycle`, `TagCreate`.
  - [x] 5.2 In `WindowManager::init_new_seats` (`wm/src/main.rs:337-359`), add two new keysym constants alongside the existing ones (`SPACE`/`N`/`Q`/`ESC`): `const TAB: u32 = 0xff09;` and `const T: u32 = 0x74;` (X11 keysym values — `T` follows this file's existing ASCII-lowercase-for-latin-letters convention, matching `N = 0x6e`/`Q = 0x71`). Add `seat.create_xkb_binding(river_xkb, qh, mods, TAB, Action::TagCycle);` and `seat.create_xkb_binding(river_xkb, qh, mods, T, Action::TagCreate);` alongside the existing bindings (same `Mod4` modifier as every other keybind in this file — no keybind-configuration system exists yet, same "hardcoded defaults" precedent as every existing binding).
  - [x] **No RED/GREEN for this task.** Same carve-out as Story 1.4's original keybind-registration task and every subsequent story that added an `Action` variant: this is enum/registration glue with no decision logic of its own — the decisions the new actions trigger (`cycle_tag`, `create_tag_with_generated_name`) are fully unit-tested in Tasks 2-3. Verification is `cargo build`/`cargo clippy --all-targets -D warnings` succeeding.

- [x] **Task 6: Wire `Seat::do_action`'s new arms + `manage_seats`' post-loop call to `WindowManager::ensure_pinned_terminal_spawned` — THE call site Story 1.5 has been dormant for two stories waiting on (AC: all — Wayland glue, no wm-core decision logic of its own)**
  - [x] 6.1 Change `Seat::do_action`'s signature (`wm/src/main.rs:561-566`) from returning `()` to `-> Option<TagId>`, and add a new `active_output_id: Option<OutputId>` parameter. Restructure the body's `match self.pending_action { ... }` so it is the function's tail expression (its value becomes the return value) instead of a statement: every existing arm (`None`, `SpawnFoot`, `Close`, `FocusNext`, `Move`, `Resize`, `Exit`) keeps its current body byte-for-byte, just followed by a trailing `None` so the arm's block type is `Option<TagId>` (mechanical change only — no behavior change to any existing arm). Add two new arms:
    ```rust
    Action::TagCycle => match active_output_id {
        Some(output_id) => match wm_core.cycle_tag(output_id) {
            Ok(Some(tag_id)) => Some(tag_id),
            Ok(None) => None,
            Err(e) => {
                eprintln!("Failed to cycle tag on output {output_id:?}: {e:?}");
                None
            }
        },
        None => {
            eprintln!("Tag-cycle keybind pressed but no output is registered yet");
            None
        }
    },
    Action::TagCreate => {
        if let Err(e) = wm_core.create_tag_with_generated_name() {
            eprintln!("Failed to create tag: {e:?}");
        }
        None
    }
    ```
    `TagCycle`'s `Some(tag_id)` return value signals "a tag switch to `tag_id` just happened — the caller should ensure its pinned terminal." `TagCreate` never returns `Some` — creating a tag never switches to it (AC: "creating a tag does not switch any output to it"), so it never needs a pinned-terminal check.
  - [x] 6.2 In `WindowManager::manage_seats` (`wm/src/main.rs:380-448`): compute `let active_output_id = self.active_output_id();` **before** `let wm_core = &mut self.wm_core;` (an immutable `self.outputs` borrow that completes before the mutable `wm_core` borrow begins — avoids a borrow-checker conflict, since `self.outputs` and `self.wm_core` are disjoint fields but `active_output_id()` is a whole-`&self` method call, which cannot run *during* the loop's `&mut self.wm_core` borrow). Add `let mut pending_terminal_spawns: Vec<TagId> = Vec::new();` before the `for seat in ...` loop. Change the `seat.do_action(&mut self.windows, wm_proxy, wm_core)` call to `if let Some(tag_id) = seat.do_action(&mut self.windows, wm_proxy, wm_core, active_output_id) { pending_terminal_spawns.push(tag_id); }`. After the `for` loop ends (so `wm_core`'s borrow has ended under NLL), add `for tag_id in pending_terminal_spawns { self.ensure_pinned_terminal_spawned(tag_id); }` — **this is the first production call site for `WindowManager::ensure_pinned_terminal_spawned`, dormant since Story 1.5.** Multiple seats cycling onto the same tag in one pass is harmless: `ensure_pinned_terminal_spawned` → `claim_pinned_terminal_spawn` is already idempotent (Story 1.5), so a duplicate entry in `pending_terminal_spawns` just resolves to a no-op `Ok(None)` on the second call — no dedup needed (YAGNI).
  - [x] **No RED/GREEN for this task.** Same carve-out as Story 1.5's Tasks 3-5 and Story 1.6's Task 2: this is call-site composition/glue with no new decision logic of its own — every actual decision (`cycle_tag`, `create_tag_with_generated_name`, `claim_pinned_terminal_spawn`, `switch_tag`) is unit-tested in isolation (Tasks 2-3 of this story, plus Stories 1.2/1.3/1.5). No live `river` compositor session exists in this sandbox (same boundary every prior story has hit). Verification is `cargo build`/`cargo clippy --all-targets -D warnings` succeeding, plus structural code review confirming: (a) every pre-existing `do_action` arm's behavior is byte-for-byte unchanged aside from the added trailing `None`; (b) `TagCycle` calls `cycle_tag` exactly once and only acts on `Ok(Some(_))`; (c) `TagCreate` never returns `Some` (never triggers a pinned-terminal check, per the AC); (d) `ensure_pinned_terminal_spawned` is called once per queued tag, after the seat loop, not inside it (confirming the `wm_core` mutable-borrow lifetime reasoning above is actually what `cargo build` accepts, not just what was intended).

- [x] **Task 7: Retire now-unnecessary `#[allow(dead_code)]` markers + stale doc comments across `wm_core` and `main.rs` (AC: none directly — hygiene closing out Story 1.5's explicitly-deferred cleanup)**
  - [x] 7.1 In `wm/src/wm_core/state.rs`, remove `#[allow(dead_code)]` from `create_tag`, `register_output`, `set_output_current_tag`, and `switch_tag` (all now reachable from `main()` via this story's wiring), and from `WmCoreError`'s `UnknownTag`, `UnknownOutput`, and `TagLimitReached` variants (all now constructible via a live path). Update each item's doc comment's "Not yet wired into `main.rs`" note to reflect this story wired it in, mirroring exactly how Stories 1.5/1.6 retired `set_view_floating`'s/`set_view_geometry`'s equivalent notes. **Leave `toggle_view_tag`'s `#[allow(dead_code)]` and its "Story 1.7" doc-comment reference untouched** — this story deliberately does not wire window-tag toggling (that's Epic 2's assign-mode picker); update its comment to say so explicitly rather than leaving a now-inaccurate "Story 1.7" forward-reference dangling.
  - [x] 7.2 In `wm/src/main.rs`, remove `#[allow(dead_code)]` from `spawn_pinned_terminal` and `WindowManager::ensure_pinned_terminal_spawned` (both now have real production call sites via Task 6.2) and update their doc comments accordingly.
  - [x] 7.3 In `wm/src/wm_core/tag.rs`, update the module-level doc comment (currently: "not yet wired into `main.rs` — that lands in Story 1.5 (pinned terminal) and Story 1.7 (tag switching)") to reflect that `create_tag`/`switch_tag`(-via-`cycle_tag`)/`mark_terminal_spawned` are now all wired, `toggle_view_tag` is not (deferred to Epic 2). Run `cargo build --manifest-path wm/Cargo.toml` to check whether the module-level `#![allow(dead_code)]` (`wm/src/wm_core/tag.rs:14`) is still needed at all now that most of the module's public API is reachable; if it is (e.g. `TagRegistry::new()` remains genuinely production-unreachable since `WmCore::default()`'s derive constructs `TagRegistry` via its own `Default`, not `TagRegistry::new()`), narrow it to a single `#[allow(dead_code)]` on just that item, mirroring `WmCore::new()`'s own existing narrow allow, rather than leaving an unnecessarily broad module-level allow in place. Leave `wm/src/wm_core/tag_set.rs`'s module-level allow untouched (still fully accurate — `toggle_view_tag`, its only caller, stays unwired) but update its comment's dangling "until Story 1.7" reference the same way as 7.1.
  - [x] **No RED/GREEN for this task** — pure hygiene, no behavior change. Verification: `cargo build --manifest-path wm/Cargo.toml` and `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings` show zero unexpected `dead_code` warnings (every item this task un-allows must show *no* warning; `toggle_view_tag` and whatever `tag.rs` narrowing lands on must show *exactly* their intentional, narrowly-scoped allow and nothing broader).

- [x] **Task 8: Full in-container verification gate (AC: all)**
  - [x] 8.1 Run, inside the devcontainer via devpod (`devpod ssh buoy-wm`): `cargo fmt --manifest-path wm/Cargo.toml --all -- --check`, `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings`, and `cargo test --manifest-path wm/Cargo.toml`. Confirm all three pass with the existing 79-test suite (Stories 1.2-1.6) plus this story's 12 new `wm_core` tests (Task 1: 2, Task 2: 6, Task 3: 4) all green — **91 total** — zero clippy/fmt violations, and specifically the exact `dead_code` state described in Task 7's verification note. Run `pre-commit run --all-files` as a final sanity check (same fmt/clippy hooks from Story 1.1).
  - [x] 8.2 Manual smoke-test note (not an automated check, not a story blocker, same boundary as every prior story's live-compositor gap): if a real `river` session is available outside this sandbox, manually pressing `Mod+Tab` (tag-cycle) with no tags yet created should no-op silently; pressing `Mod+T` (tag-create) a few times should silently populate the registry with `tag0`, `tag1`, `tag2`; pressing `Mod+Tab` afterward should cycle the output's displayed tag through them in that order, wrapping back to `tag0`, and the first time each tag is cycled onto, its pinned terminal (`foot -a pinned-term zellij attach --create tag-tag<N>`) should spawn tiled at the bottom, exactly once per tag even if cycled onto repeatedly. Record in the Dev Agent Record whether this was performed or explicitly deferred, per this project's established convention — either is acceptable to close the story, but must be stated, not silently assumed.

## Technical notes

**Generated-name collision risk (tag-create keybind), flagged not hidden.**
`create_tag_with_generated_name` derives its placeholder name from
`self.tags.count()` at call time. Since there is no tag-deletion feature in
v1 (ADR-006), `count()` only ever grows, so repeated presses of the
tag-create keybind alone can never collide with each other. The one real
collision risk is cross-source: once Epic 2's picker adds real free-text tag
naming, a user could manually create a tag literally named e.g. `"tag3"`
before the generator's own counter reaches 3 — `create_tag` is idempotent
by name (Story 1.2), so the generator's next call at that count would
silently return the *existing* `"tag3"`'s id rather than creating a new
tag, which would be a surprising (if harmless — no data corruption, no
panic) UX wrinkle. Untested and unresolved here: this story's keybind is the
only tag-creation path that exists today, so the collision precondition
(some *other* path creating a `tag<N>`-shaped name first) cannot occur yet.
Flagged as an assumption for Epic 2 to revisit if it proves to matter
(YAGNI) — the natural fix, if ever needed, is prefixing the generated name
distinctly from anything a human would plausibly type (e.g. a leading
character fuzzel's picker would never accept), not built now against a
precondition that cannot yet occur.

**Output lifecycle: registration only, no unregister.** See Task 4's
explicitly-flagged gap above. `wm_core` gains outputs but never loses them
in this story. Not a regression — no `main.rs` code path removed a `wm_core`
output before this story either, because none existed to remove.

**"Active output" is a real, accepted simplification, not a bug.** Neither
`river_seat_v1` nor `river_output_v1` (checked against
`wm/protocol/river-window-management-v1.xml`) carries any per-seat "which
output has input focus" signal — there is nothing today to compute a true
focused-output from, keybind-driven or otherwise. `active_output_id`'s
lowest-`OutputId` selection is deterministic and correct for the
dominant single-output case (ADR-005), and every multi-output outcome is at
least *consistent* (always the same output, not silently different per
keypress) rather than *correct* in the "whichever monitor my mouse is on"
sense `EXPERIENCE.md` describes for the eventual picker. This is a
conscious scope boundary for a story whose own FR (FR13) says its purpose is
only "to validate multi-tag behavior end-to-end before the fuzzel-based
picker exists" — not to solve output-focus tracking, which Epic 2 will need
new protocol-level signal (pointer/seat-enter events keyed to an output) to
solve properly regardless of what this story does.

**Why `cycle_tag` and not a `main.rs`-level loop over `TagRegistry::ids()`.**
Keeping the "what's next" decision inside `wm_core` (rather than having
`main.rs` read `ids()` + the output's `current_tag` and compute the next tag
itself before calling `switch_tag`) keeps all tag-cycling *decision* logic
unit-testable without a live compositor, matches this project's established
"`wm-core` owns decisions, `main.rs` is glue" boundary (`components.md`:
"`wm-core`... single source of truth for tag enforcement and placement
decisions"), and is the only way to satisfy this story's own AC clause
("the `wm-core` functions this exercises... are the same functions Epic 2's
IPC handlers call") without also inventing a second place the "next tag"
rule could drift from `switch_tag`'s own enforcement.

**No new `WmCoreError` variants.** `cycle_tag` reuses `UnknownOutput`
(pre-existing, Story 1.2/1.3). `create_tag_with_generated_name` reuses
`TagLimitReached` and delegates entirely to `create_tag`'s existing error
mapping — no new failure mode either function can produce that `create_tag`/
`switch_tag` didn't already have.

**NFR1 (50ms budget).** `TagRegistry::ids()` is `O(number of tags)` (≤ 64,
ADR-006), a single `Vec` allocation, no I/O. `cycle_tag` is that plus the
existing `O(number of outputs)` `switch_tag` reroute scan (already
budget-accepted, Story 1.3) — both bounded, small, in-memory. Same
structural-argument treatment (not a literal `Instant`-based benchmark) as
every prior story's NFR1 note, for the same reason (a sandboxed CI timing
assertion would be flaky, not diagnostic).

**NFR2 (panic-free).** `cycle_tag` never indexes `ids` without first
checking `is_empty()`; the "current tag not found in `ids`" defensive branch
(Task 2.2) means a hypothetically-stale `current_tag` falls back to
`ids[0]` rather than panicking — believed structurally unreachable today
(no delete-tag feature exists to make a tag id stale) but guarded anyway,
matching this codebase's established "safe-by-construction, but don't
`.expect()` where a cheap real check is available" discipline.
`create_tag_with_generated_name` cannot panic: `format!` never fails, and
`create_tag` itself is already total (`Ok`/`Err`, no panic path). On the
`main.rs` side, every new `Result` this story discards
(`cycle_tag`/`create_tag_with_generated_name`'s `Err` arms) is logged via
`eprintln!`, matching every prior story's convention.

**No new ADR.** Checked `adrs.md` — nothing governs raw-keybind tag-cycle
order, generated-tag-naming, or active-output selection specifically; this
story's two resolved ambiguities (Description, above) are implementation
decisions filling gaps neither the PRD nor the ADRs specify, not decisions
among architecturally significant alternatives an ADR would need to record
(consistent with how Stories 1.3/1.5/1.6 judged their own similarly-scoped
gap-filling decisions not to need new ADRs).

No new Cargo dependencies. Build/test/lint only ever run **inside the
devcontainer via devpod** (`devpod ssh buoy-wm -- cargo ...`) — same
constraint as every prior story.

## Test plan
This story's tests split into two tiers, matching every prior story's
established TDD approach for a mix of pure state logic and I/O-adjacent
glue:

1. **`wm_core` decision logic** (Tasks 1-3) — Rust unit tests added to
   `wm/src/wm_core/tag.rs`'s and `wm/src/wm_core/state.rs`'s existing test
   modules, run via `cargo test --manifest-path wm/Cargo.toml` inside the
   devcontainer via devpod:
   - `TagRegistry::ids` (Task 1, 2 tests): empty-registry case; creation-
     order preservation across multiple tags.
   - `WmCore::cycle_tag` (Task 2, 6 tests + 1 regression-guard extension):
     first-cycle-selects-first-tag from a `None` starting state; advances
     to the next tag in creation order on repeated calls; wraps back to the
     first tag after the last; returns `Ok(None)` (no panic) when the
     registry is empty; returns `Err(UnknownOutput)` and leaves the whole
     `WmCore` snapshot unchanged for an unregistered output; and — the
     direct proof of the AC's "same functions... no duplicate logic path"
     clause — reuses `switch_tag`'s existing cross-output reroute
     enforcement rather than a separate write path.
   - `WmCore::create_tag_with_generated_name` (Task 3, 4 tests):
     `tag<N>`-naming-convention on first/second calls; fresh distinct ids
     across repeated calls; `Err(TagLimitReached)` at the 64-tag cap with
     the count staying 64; and a whitebox check that the returned id is a
     real, present, correctly-named tag (proving delegation to `create_tag`
     rather than a separately-invented id allocator).
2. **`main.rs` wiring** (Tasks 4-6) — **not covered by automated unit
   tests**, by design, same boundary every prior story has hit (real
   `wayland_client`/`river-window-management-v1` `Dispatch` types, no live
   protocol connection in this devcontainer sandbox): output registration on
   `Event::Output`, `active_output_id`'s deterministic selection, the two
   new `Action` variants and their raw keybinds, `do_action`'s new arms, and
   `manage_seats`'s post-loop `ensure_pinned_terminal_spawned` call. Verified
   instead via `cargo build --manifest-path wm/Cargo.toml` and `cargo
   clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings`
   succeeding, plus structural code review per each task's own listed
   confirmation checklist (Tasks 4.4/5.2/6's no-RED/GREEN notes above) —
   most pointedly, that `ensure_pinned_terminal_spawned` (dormant since
   Story 1.5) is now actually called, exactly once per seat action that
   produced a real tag switch, after (not during) the seat loop that holds
   `wm_core`'s mutable borrow.
3. **Dead-code/hygiene verification** (Task 7) — `cargo build`/`cargo
   clippy -D warnings` output itself is the test: every un-allowed item
   must show zero warnings; `toggle_view_tag` and any narrowed `tag.rs`
   allow must show exactly their intentional, narrowly-scoped allow.
4. **Tooling gate** (Task 8) — `cargo fmt --check`, `cargo clippy -D
   warnings`, and the full `cargo test` run (79 existing + 12 new, all
   green — 91 total), all in-container; `pre-commit run --all-files` as a
   final sanity check reusing Story 1.1's hooks.

Live-compositor coverage for the actual end-to-end visual claim (pressing
real keys, seeing the bar/terminal respond) remains a standing gap in this
story, same class of sandbox limitation every prior story has documented (no
live `river` session available here). Task 8.2's optional manual smoke test
is the only way to get live visual confirmation; if not performed, that is
an explicit, recorded coverage gap for a human to accept, consistent with
every prior story's own precedent.

## FR coverage
FR13, NFR1, NFR2

## Dev Agent Record

### Implementation Plan
Followed the 8-task breakdown exactly, in order:
- Tasks 1-3: strict RED→GREEN TDD in `wm_core` (`tag.rs`'s `ids()`, `state.rs`'s
  `cycle_tag`/`create_tag_with_generated_name`). Each RED step confirmed via a
  compile failure before implementing; each GREEN step confirmed via the
  targeted test module passing, then the full suite re-run for regressions.
- Tasks 4-6: Wayland-glue wiring in `main.rs` (output registration,
  `active_output_id`, the two new `Action` variants + keybinds, `do_action`'s
  restructured return type, `manage_seats`'s post-loop
  `ensure_pinned_terminal_spawned` call). No RED/GREEN per the story's
  explicit carve-out; verified via `cargo build`/`cargo clippy --all-targets
  -D warnings` plus manual structural review against each task's own
  checklist.
- Task 7: retired `#[allow(dead_code)]` on `create_tag`/`register_output`/
  `set_output_current_tag`/`switch_tag`, `WmCoreError`'s `UnknownTag`/
  `UnknownOutput`/`TagLimitReached`, and `main.rs`'s
  `spawn_pinned_terminal`/`ensure_pinned_terminal_spawned`. Removed
  `tag.rs`'s module-level `#![allow(dead_code)]` entirely and re-added a
  narrow allow on just `TagRegistry::new()` after confirming via a real
  `cargo build` (no allow present) that it was the only remaining dead item
  in that module. `toggle_view_tag` and `tag_set.rs`'s module-level allow
  were left untouched per the story's explicit instruction, with their
  comments updated to point at Epic 2 instead of a dangling "Story 1.7"
  reference.
- Task 8: full fmt/clippy/test/pre-commit gate, all green.

### Devcontainer access note
`devpod ssh buoy-wm` failed to tunnel in this environment ("Error tunneling
to container: wait: remote command exited without exit status or exit
signal") on every attempt, including after `devpod up buoy-wm`. The
underlying devcontainer itself was confirmed running (`devpod status
buoy-wm` → Running; matched via `podman inspect` label
`dev.containers.id: default-bu-2a0fe` on container `bold_vaughan`). All
`cargo`/`pre-commit` commands for this story were run via `podman exec -u
vscode -w /workspaces/buoy-wm bold_vaughan ...` directly against that same
container instead — devcontainer-only, just bypassing the broken `devpod
ssh` transport layer rather than the sandbox constraint itself. Flagged
here rather than silently substituted. `pre-commit` additionally required
`git config --global --add safe.directory /workspaces/buoy-wm` inside the
container (dubious-ownership check, unrelated to this story's changes).

### Known, flagged gap not addressed by this task
Task 7.1 explicitly lists only `create_tag`/`register_output`/
`set_output_current_tag`/`switch_tag` and three `WmCoreError` variants for
`#[allow(dead_code)]` removal. `WmCore::mark_terminal_spawned` and
`WmCore::claim_pinned_terminal_spawn` (`state.rs`) are, by the same
"now reachable from `main()`" logic, also genuinely reachable as of this
story's Task 6.2 wiring (transitively via
`ensure_pinned_terminal_spawned`), but neither is named in Task 7.1's list,
so their `#[allow(dead_code)]` markers were left in place, following the
task breakdown literally rather than improvising a scope addition. This is
harmless (an unnecessary `#[allow(dead_code)]` on a live item produces no
warning — confirmed via the zero-warning `cargo build`/`clippy` runs above)
and does not affect any acceptance criterion or the Task 8 verification
gate, but is flagged here for a human to consciously accept or fold into a
follow-up hygiene pass.

### Manual smoke test (Task 8.2)
Explicitly deferred — no live `river` compositor session is available in
this sandbox, the same boundary every prior story in this epic has
documented. Not performed.

### Completion Notes
- 91/91 tests pass (`cargo test --manifest-path wm/Cargo.toml`): 79 baseline
  + 2 (`TagRegistry::ids`) + 6 (`WmCore::cycle_tag`) + 4
  (`WmCore::create_tag_with_generated_name`).
- `cargo build --manifest-path wm/Cargo.toml`: clean, zero warnings.
- `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings`:
  clean.
- `cargo fmt --manifest-path wm/Cargo.toml --all -- --check`: clean.
- `pre-commit run --all-files`: both hooks (`cargo fmt --check`, `cargo
  clippy`) passed.
- All four previously-dormant `wm_core` entry points
  (`switch_tag`/`create_tag`/`register_output`/
  `ensure_pinned_terminal_spawned`) now have real, wired production call
  sites for the first time since their respective stories introduced them.

## File List
- `wm/src/wm_core/tag.rs` — added `TagRegistry::ids()` + 2 tests; removed
  module-level `#![allow(dead_code)]`, added narrow allow on
  `TagRegistry::new()`; updated module-level wiring-status comment.
- `wm/src/wm_core/state.rs` — added `WmCore::cycle_tag` + 6 tests (+
  extended `every_mutator_rejects_unknown_ids_without_mutating_state`);
  added `WmCore::create_tag_with_generated_name` + 4 tests; removed
  `#[allow(dead_code)]` from `create_tag`/`register_output`/
  `set_output_current_tag`/`switch_tag` and `WmCoreError`'s `UnknownTag`/
  `UnknownOutput`/`TagLimitReached`, with doc comments updated; updated
  `toggle_view_tag`'s doc comment to reference Epic 2 instead of Story 1.7.
- `wm/src/wm_core/tag_set.rs` — updated dangling "until Story 1.7" comment
  reference; module-level allow left untouched.
- `wm/src/main.rs` — added `output_id: OutputId` field to `Output` +
  updated `Output::new`; wired `register_output` into the `Event::Output`
  handler; added `WindowManager::active_output_id`; added
  `Action::TagCycle`/`Action::TagCreate` + `Mod4+Tab`/`Mod4+T` keybinds;
  changed `Seat::do_action`'s signature to accept `active_output_id: Option<OutputId>`
  and return `Option<TagId>`, added the two new arms, gave every
  pre-existing arm a trailing `None`; changed `manage_seats` to compute
  `active_output_id` before the `wm_core` mutable borrow, collect
  `pending_terminal_spawns`, and call `ensure_pinned_terminal_spawned` once
  per queued tag after the seat loop; removed `#[allow(dead_code)]` from
  `spawn_pinned_terminal`/`ensure_pinned_terminal_spawned` with updated doc
  comments; added `OutputId` to the `wm_core::ids` import.
- `docs/planning/epics/story-1-7.md` — this story file: task/AC checkboxes,
  Status, Dev Agent Record, File List, Change Log.

## Change Log
- 2026-08-07: Implemented Story 1.7 — raw-keybind tag cycling (`Mod4+Tab`)
  and creation (`Mod4+T`), wiring `switch_tag`/`create_tag`/
  `register_output`/`ensure_pinned_terminal_spawned` into `main.rs` for the
  first time. 12 new `wm_core` unit tests (91 total, up from 79). All
  fmt/clippy/test/pre-commit gates green.
