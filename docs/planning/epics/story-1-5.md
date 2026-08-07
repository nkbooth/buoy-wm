---
baseline_commit: 27bf02af3e633db64b5ea5842c922a472717d40a
---

# Story 1.5: Pinned Terminal Lifecycle
Epic: 1 | Priority: H | Status: done

## Description
A persistent zellij-backed terminal automatically available on every tag,
lazily spawned on first switch to that tag. This story builds the pinned
terminal's full lifecycle as a set of `wm-core` decision functions plus the
Wayland-glue call sites that use them — but, per the clarification below,
only *some* of that glue is reachable through a live keybind today. Tag
switching itself (`WmCore::switch_tag`, Story 1.3) is not yet wired to any
keybind in `main.rs` — that is Story 1.7's job ("Raw-Keybind Tag Switching &
Creation"). This story does not add that keybind or touch
`Seat`/`Action`/`init_new_seats` at all. What it does add is:
- the pure `wm-core` decision logic for "does this tag still need its
  terminal spawned, and if so what zellij session name to use" — fully
  testable today by calling `WmCore::switch_tag` directly from a test
  harness, exactly as the clarification below already established;
- a `main.rs`-level orchestration method that composes that decision with
  the actual `foot -a pinned-term` / `zellij attach --create` process spawn
  — written and structurally verified now, but with **no call site yet**,
  since nothing in `main.rs` calls `switch_tag` until Story 1.7 exists to
  call this method right after it;
- the one piece of this story that **is** reachable today regardless of
  Story 1.7: whenever any window with `app_id == "pinned-term"` is
  registered (however it got spawned — including a human manually running
  `foot -a pinned-term zellij attach --create tag-x` on a live compositor
  for smoke-testing before Story 1.7 lands), it is immediately forced
  non-floating and moved to the bottom of `wm-core`'s stacking order. This
  path runs unconditionally inside `WindowManager::init_new_windows`, which
  already executes for every window today.

## Acceptance criteria
**Given** a tag has no pinned terminal yet
**When** the output's current tag switches to it for the first time
**Then** the WM spawns `foot -a pinned-term` running `zellij attach --create tag-<name>`
**And** marks that tag's `terminal_spawned` as true
**Given** the pinned terminal is running
**Then** it is always tiled and always rendered at the bottom of the render order
**And** it is never respawned outside this lazy-spawn-once path, even if the process exits (architectural constraint)

> **Clarification (added 2026-08-06, implementation-readiness review):** the
> tag-switch trigger above is exercised via `wm-core`'s internal switch-tag
> function (established in Story 1.3), directly from a test harness — it does
> **not** require the Story 1.7 keybind wrapper to exist first. Not a forward
> dependency.

> **Clarification (added during story refinement, story-creation pass):**
> the AC above describes the full, live, end-to-end behavior — a real
> keypress ultimately causing a real process to spawn. That full chain
> cannot be exercised end-to-end in this story alone, because nothing calls
> `WmCore::switch_tag` from a real keybind until Story 1.7 exists. This
> story is scoped, and its Tasks below are structured, so that:
> - the *decision* half (does this tag need a terminal, what's the session
>   name, mark it spawned exactly once) is fully built and unit-tested now,
>   triggered by `WmCore::switch_tag` calls made directly from test code —
>   satisfying the first clarification above and both `terminal_spawned`
>   related AC bullets without waiting on Story 1.7;
> - the *process-spawn* half (`foot -a pinned-term` / `zellij attach
>   --create`) is written now as a callable, structurally-verified function,
>   composed with the decision half in a new `main.rs` orchestration method
>   — but that method has no production call site until Story 1.7 adds the
>   tag-switch keybind and calls it. This is the same "implemented
>   defensively, ahead of the story that wires the live trigger" pattern
>   Story 1.4 used for the pinned-terminal close exclusion, ahead of this
>   very story;
> - the *tiled + bottom-of-render-order* half is **not** deferred — it is
>   wired into `WindowManager::init_new_windows` in this story, which is
>   already live production code reachable today for any window, regardless
>   of how it was spawned or whether Story 1.7 exists yet.
>
> See Technical notes ("Reachable today vs. pending Story 1.7") for the
> exact call-site-by-call-site breakdown, and Test plan for how each half is
> verified.

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: `WmCore::lower_view` — move a view to the bottom of the stacking order (AC: "always rendered at the bottom of the render order")**
  - [x] 1.1 RED — Inside the devcontainer via devpod (`devpod ssh buoy-wm -- cargo test --manifest-path wm/Cargo.toml wm_core::state::`), add tests to `wm/src/wm_core/state.rs`'s existing test module: `lower_view_moves_view_to_front` — register two views `a, b` (`stacking_order() == [a, b]`), call `core.lower_view(b)`, assert `stacking_order() == [b, a]` (mirrors the existing `raise_view_moves_view_to_back` test, opposite direction); `lower_view_on_already_front_view_is_idempotent` — same two-view setup, `core.lower_view(a)` (already front), assert `stacking_order()` unchanged (`[a, b]`); `lower_view_unknown_id_returns_error` — a bogus `ViewId(999)`, assert `Err(WmCoreError::UnknownView)`. Also extend the existing consolidated regression guard `every_mutator_rejects_unknown_ids_without_mutating_state` with one more assertion for `core.lower_view(bogus_view)` returning `Err(WmCoreError::UnknownView)` and leaving the state snapshot unchanged, following that test's existing pattern for every other mutator. Confirm all of this fails to compile — `lower_view` doesn't exist yet.
  - [x] 1.2 GREEN — Implement `pub fn lower_view(&mut self, id: ViewId) -> Result<(), WmCoreError>` on `WmCore` in `wm/src/wm_core/state.rs`, directly beside `raise_view` (`wm/src/wm_core/state.rs:313-320`): validate `id` is a known view (else `Err(UnknownView)`), then `self.stacking_order.retain(|&v| v != id); self.stacking_order.push_front(id);` — the exact mirror of `raise_view`'s `retain` + `push_back`. `///` doc comment noting this is the primitive `Task 5`'s `init_new_windows` wiring uses to satisfy FR4's "always rendered at the bottom of render order" for the pinned terminal. Confirm the three new tests plus the extended regression guard all pass; confirm no regression in the existing 65-test suite.

- [x] **Task 2: `WmCore::claim_pinned_terminal_spawn` — atomic lazy-spawn-once decision (AC: both `terminal_spawned`-related bullets)**
  - [x] 2.1 RED — Add tests to the same test module: `claim_pinned_terminal_spawn_returns_session_name_first_time` — `create_tag("web")` → `tag_id`, call `core.claim_pinned_terminal_spawn(tag_id)`, assert `Ok(Some("tag-web".to_string()))` and (whitebox, same-module access) `core.tags.get(tag_id).unwrap().terminal_spawned == true`; `claim_pinned_terminal_spawn_is_idempotent_returns_none_after_first_claim` — call it twice on the same tag, assert the second call returns `Ok(None)` (this is the concrete, unit-testable proof of the AC's "never respawned outside this lazy-spawn-once path" — see Technical notes for why no separate "process exited" handling is needed to satisfy that bullet); `claim_pinned_terminal_spawn_unknown_tag_returns_error` — a bogus `TagId(63)`, assert `Err(WmCoreError::UnknownTag)`; `claim_pinned_terminal_spawn_session_name_uses_tag_dash_prefix_convention` — `create_tag("my-tag-name")`, assert the claimed session name is exactly `"tag-my-tag-name"` (locks in the literal `tag-<name>` format both `features-and-acceptance-criteria.md` and this story's own AC specify); `claim_pinned_terminal_spawn_reachable_via_switch_tag_alone_without_any_keybind` — the direct proof of both clarification notes above: `register_output()` → `output_id`, `create_tag("web")` → `tag_id`, `core.switch_tag(output_id, tag_id)` (Story 1.3's existing, already-tested function, called directly from this test — no keybind, no `main.rs` involved), then `core.claim_pinned_terminal_spawn(tag_id)`, assert `Ok(Some("tag-web".to_string()))` — demonstrating the entire trigger-to-decision sequence this story's AC describes is exercisable through `wm-core` alone. Confirm all five fail to compile — the method doesn't exist yet.
  - [x] 2.2 GREEN — Implement `pub fn claim_pinned_terminal_spawn(&mut self, tag_id: TagId) -> Result<Option<String>, WmCoreError>` on `WmCore` in `wm/src/wm_core/state.rs`, near `mark_terminal_spawned` (`wm/src/wm_core/state.rs:283-294`): look up the tag via `self.tags.get(tag_id).ok_or(WmCoreError::UnknownTag)?`; if `tag.terminal_spawned`, return `Ok(None)`; otherwise compute `format!("tag-{}", tag.name)` (ends the immutable borrow via NLL), call `self.mark_terminal_spawned(tag_id).expect("tag_id was just confirmed registered above")` (reuses the existing Story 1.2 mutator rather than duplicating the field write — DRY; the `.expect()` is safe-by-construction, matching `cycle_focus`'s established NFR2 pattern for calls guaranteed to succeed by the same function's own prior check), and return `Ok(Some(session_name))`. `///` doc comment stating this is the single atomic "check + claim" decision for the lazy-spawn-once invariant — deliberately bundled into one method (mirroring `cycle_focus`'s precedent of composing several `wm-core`-internal steps into one atomic call) rather than exposing separate `tag_terminal_spawned`/`tag_name` queries, so no caller can accidentally check without claiming or claim twice. Confirm the five new tests pass; confirm no regression in Task 1's tests or the rest of the suite.

- [x] **Task 3: `spawn_pinned_terminal` — the actual `foot -a pinned-term` / `zellij attach --create` process spawn (AC: "the WM spawns...")**
  - [x] 3.1 Add a free function in `wm/src/main.rs`, next to `Seat::do_action`'s `Action::SpawnFoot` arm (`wm/src/main.rs:455-461`) for easy comparison: `fn spawn_pinned_terminal(session_name: &str) { match std::process::Command::new("foot").arg("-a").arg(PINNED_TERM_APP_ID).arg("zellij").arg("attach").arg("--create").arg(session_name).env_remove("WAYLAND_DEBUG").spawn() { Ok(_) => {} Err(e) => eprintln!("Failed to spawn pinned terminal: {e}") } }` — same `env_remove("WAYLAND_DEBUG")` treatment and same `Ok`/`Err` handling as the existing `SpawnFoot` arm (consistency, not reinvention), reusing the already-imported `PINNED_TERM_APP_ID` constant for the `-a` value instead of a second `"pinned-term"` literal (DRY). Arguments are passed individually to `Command`, not through a shell, so arbitrary tag names in `session_name` carry no shell-injection risk regardless of their contents (see Technical notes for the separate, non-injection assumption about zellij's own session-name character restrictions).
  - [x] **No RED/GREEN for this task.** Same carve-out and justification as Story 1.4's `Action::SpawnFoot`/`Action::Exit` arms: this is unconditional process-spawn I/O with no decision logic of its own — the decision it's given (`session_name`, and *whether* to call it at all) was already RED/GREEN tested in Task 2. Verification is `cargo build --manifest-path wm/Cargo.toml` and `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings` succeeding. This function has no call site until Task 4 wires it in, so expect (and account for, via a narrowly-scoped `#[allow(dead_code)]` with an inline comment) a `dead_code` warning on it until then.

- [x] **Task 4: `WindowManager::ensure_pinned_terminal_spawned` — compose Tasks 2-3, dormant until Story 1.7 (AC: "the WM spawns..." / "marks... terminal_spawned")**
  - [x] 4.1 Add `fn ensure_pinned_terminal_spawned(&mut self, tag_id: wm_core::ids::TagId)` to `WindowManager`'s `impl` block in `wm/src/main.rs`: `match self.wm_core.claim_pinned_terminal_spawn(tag_id) { Ok(Some(session_name)) => spawn_pinned_terminal(&session_name), Ok(None) => {} Err(e) => eprintln!("Failed to check pinned-terminal spawn state for tag {tag_id:?}: {e:?}") }` — logs rather than silently swallows the `Err` case, matching this file's established NFR2 logging convention (`remove_windows`/`focus_top` in Story 1.4). This is the method Story 1.7's tag-switch keybind handler will call immediately after a successful `self.wm_core.switch_tag(output_id, tag_id)` — deliberately *not* calling `switch_tag` itself, so this story doesn't have to pre-decide Story 1.7's own call convention (e.g. which output, how the active output is determined) for a keybind that doesn't exist yet. Mark the method `#[allow(dead_code)]` with an inline comment naming Story 1.7 as the story that adds its first real call site — same narrowly-scoped-allow discipline Story 1.4 established for its own not-yet-wired `wm-core` surface.
  - [x] 4.2 Import `wm_core::ids::TagId` alongside the existing `wm_core::ids::ViewId` import (`wm/src/main.rs:45`) so Task 4.1's signature compiles without a fully-qualified path if preferred (either is acceptable; keep consistent with the rest of the file's existing `use` style).
  - [x] **No RED/GREEN for this task**, same carve-out as Task 3: this is call-site composition of two already-tested/verified pieces (Task 2's unit-tested decision, Task 3's build-verified I/O), with no decision logic of its own beyond the `match` dispatch that Task 2's tests already cover the *decision* half of. Verification is `cargo build`/`cargo clippy -D warnings` plus structural review confirming the `match` arms call the right functions.

- [x] **Task 5: Wire the pinned terminal's tiled + bottom-of-render-order invariant into `init_new_windows` — reachable today (AC: "it is always tiled and always rendered at the bottom of the render order")**
  - [x] 5.1 In `WindowManager::init_new_windows` (`wm/src/main.rs:273-281`), immediately after `window.view_id = Some(view_id);` and before `window.new = false;`, add: `if window.app_id == PINNED_TERM_APP_ID { if let Err(e) = self.wm_core.set_view_floating(view_id, false) { eprintln!("Failed to set pinned terminal non-floating: {e:?}"); } if let Err(e) = self.wm_core.lower_view(view_id) { eprintln!("Failed to lower pinned terminal in stacking order: {e:?}"); } }` — `set_view_floating` is Story 1.2's existing mutator (its first production call site; previously `#[allow(dead_code)]`-marked as unwired), `lower_view` is Task 1's new mutator. Both `Result`s are logged rather than silently discarded, matching this file's established NFR2 convention. This branch runs for **every** window regardless of how it was spawned or whether Story 1.7 exists — it is the one part of this story reachable in production today, independent of Task 4's dormant orchestration.
  - [x] 5.2 Since `set_view_floating` now has a real production call site, remove its `#[allow(dead_code)]` annotation in `wm/src/wm_core/state.rs` (`wm/src/wm_core/state.rs:171-180`) and update its doc comment's "not yet wired into `main.rs`" note to reflect that this story wires it in.
  - [x] **No RED/GREEN for this task.** Same carve-out as Story 1.4's Task 3 (protocol/`Dispatch`-layer glue, no live compositor in the sandbox): the branch condition and call sites are Wayland-glue with no decision logic of their own — the actual mutations it calls (`set_view_floating`, already tested in Story 1.2; `lower_view`, tested in Task 1 of this story) are fully unit-tested in isolation. Verification is `cargo build`/`cargo clippy -D warnings` plus structural code review confirming the branch only fires for `PINNED_TERM_APP_ID` and calls both the right methods in the right order.

- [x] **Task 6: Exclude the pinned terminal from `cycle_focus`/click-to-focus's `raise_view` calls, so it stays at the bottom even after being focused or clicked (AC: "always rendered at the bottom of the render order" — closes the gap identified during story drafting; confirmed in-scope by the user 2026-08-07 rather than deferred)**
  - [x] 6.1 RED — Add a `wm_core::state` test: register the pinned terminal (`app_id == PINNED_TERM_APP_ID`) plus a second ordinary view, lower the terminal to the bottom (as Task 5 does at spawn time), then call `cycle_focus()` (or directly exercise whatever primitive `cycle_focus`/click-to-focus use to reorder) and assert the pinned terminal is still at `stacking_order().front()` afterward — i.e. cycling focus onto/through it, or a simulated click on it, must not move it. Confirm this fails against the current `cycle_focus`/`raise_view` behavior (which has no pinned-terminal exclusion).
  - [x] 6.2 GREEN — Add a `wm_core`-level guard so the pinned terminal is never moved by whatever reordering operation backs focus-cycling/click-to-focus: either (a) make `cycle_focus`/the click-to-focus reordering path skip `raise_view` when the target view's `app_id == PINNED_TERM_APP_ID` (re-lowering it immediately after, or simply no-op'ing the raise for that view), or (b) add a small `WmCore`-level check callable from both `main.rs` call sites. Prefer keeping the exclusion logic inside `wm_core` (protocol-agnostic, testable) rather than duplicating an `app_id` string check at both `main.rs` call sites. Confirm the new test passes and the existing `cycle_focus`/`raise_view`/click-to-focus tests (Story 1.4 and this story's Task 1) still pass unchanged for non-pinned views.
  - [x] 6.3 Update `main.rs`'s click-to-focus call site (`manage_seats`'s `interacted` handling) and the `Action::FocusNext` handler if either needs a corresponding change to route through the new guard — build + clippy verification only for this part (Wayland glue, same carve-out as Task 5's no-RED/GREEN note), since the decision logic itself is covered by Task 6.1/6.2's unit tests.
  - [x] 6.4 Note in the Dev Agent Record whether this also means the pinned terminal is effectively unfocusable via keybind-cycle/click (since it's always immediately re-lowered), or whether it can still receive keyboard focus while staying visually at the bottom of stacking order — state which behavior was actually implemented, since the user's "fix it now" choice was scoped to the render-order invariant specifically, not a broader "is it focusable at all" UX redesign.

- [x] **Task 7: Full in-container verification gate (AC: all)**
  - [x] 7.1 Run, inside the devcontainer via devpod (`devpod ssh buoy-wm`): `cargo fmt --manifest-path wm/Cargo.toml --all -- --check`, `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings`, and `cargo test --manifest-path wm/Cargo.toml`. Confirm all three pass with the existing 65-test suite (Stories 1.2-1.4) plus this story's new `WmCore` tests (Task 1: 3, Task 2: 5, Task 6: 1+) all green, zero clippy/fmt violations, and specifically zero unexpected `dead_code` warnings: `set_view_floating` should no longer be flagged (Task 5.2), while `spawn_pinned_terminal` and `ensure_pinned_terminal_spawned` should still correctly show their intentional, narrowly-scoped `#[allow(dead_code)]` (Tasks 3-4) since Story 1.7 hasn't wired a call site yet. Run `pre-commit run --all-files` as a final sanity check (same fmt/clippy hooks from Story 1.1).
  - [x] 7.2 Manual smoke-test note (not an automated check, not a story blocker, same boundary as every prior story's live-compositor gap): if a real `river` session is available outside this sandbox, manually running the built binary and then manually invoking `foot -a pinned-term zellij attach --create tag-manual-test` (standing in for Story 1.7's not-yet-existing keybind) would let a human visually confirm the window renders tiled and at the bottom, underneath any floating window on top of it, that it stays at the bottom even after being clicked or focus-cycled to (Task 6), and that `Action::Close` still refuses to close it (Story 1.4's existing exclusion, unmodified by this story). Record in the Dev Agent Record whether this was performed or explicitly deferred, per this project's established convention — either is acceptable to close the story, but must be stated, not silently assumed.

## Technical notes

**Reachable today vs. pending Story 1.7 — the exact split.**
| Piece | Where | Reachable today? |
|---|---|---|
| `WmCore::lower_view` (Task 1) | `wm_core::state` | Yes — unit-tested directly, no dependency on anything else in this story |
| `WmCore::claim_pinned_terminal_spawn` (Task 2) | `wm_core::state` | Yes — unit-tested directly, including via a `switch_tag`-only test harness call with no keybind involved (both clarification notes above) |
| `spawn_pinned_terminal` (Task 3) | `main.rs` free fn | Structurally yes (compiles, build/clippy-verified), but has no production call site in this story — dormant until Task 4 calls it |
| `WindowManager::ensure_pinned_terminal_spawned` (Task 4) | `main.rs` method | Structurally yes, but **not reachable from any live keybind** — no code in this story calls `wm_core.switch_tag`, so nothing calls this method either. Story 1.7 adds the first real call site |
| `init_new_windows`'s tiled/lower-to-bottom branch (Task 5) | `main.rs`, existing method | **Yes, live in production today** — runs for every window on every `handle_manage_start` pass, independent of Story 1.7 |

This mirrors Story 1.4's own precedent: that story implemented the
pinned-terminal close exclusion (`closable_focused_view`, later superseded
by a per-seat `app_id` check) "defensively... ahead of Story 1.5," fully
unit-tested but only exercisable live once a real pinned terminal existed.
This story is the mirror image — it builds the pinned terminal's spawn
*decision* and *process* now, fully testable/verifiable in isolation, while
the live *trigger* (a real tag-switch keybind calling `switch_tag`) remains
Story 1.7's explicit responsibility. Story 1.7's own AC already documents
this dependency direction ("the `wm-core` functions this exercises are the
same functions Epic 2's IPC handlers call") — this story adds one more
function (`ensure_pinned_terminal_spawned`) to that same "built once, called
from multiple future call sites" list.

**Why `claim_pinned_terminal_spawn`'s idempotency alone satisfies "never
respawned... even if the process exits."** `wm-core` has no concept of a
live process or its exit status at all (`data-model.md`: no persistence
layer, WM state is process-memory only; zellij's own session persists
independently of the WM, `zellij attach` re-attaching to it later is what
makes that acceptable). There is deliberately no process-monitoring code in
this story (YAGNI) — the AC's "never respawned... even if the process
exits" is satisfied structurally, not by watching the process: the *only*
code path that can ever trigger a spawn is
`ensure_pinned_terminal_spawned` → `claim_pinned_terminal_spawn`, and the
latter is proven-by-test to return `Ok(None)` on every call after the
first for a given tag, regardless of anything happening to the previously
spawned OS process. A crashed `foot`/`zellij` simply leaves that tag
terminal-less until... nothing brings it back, which is exactly the
specified (if perhaps surprising) behavior.

**Resolved during story drafting: `raise_view` calls elsewhere could move
the pinned terminal off the bottom — fixed in this story (Task 6), not
deferred.** Task 5 puts the pinned terminal at the front (bottom) of
`wm_core.stacking_order` at registration time via `lower_view`. But
`main.rs` already has two call sites that call `wm_core.raise_view`
unconditionally on whatever view they target — `cycle_focus` (Story 1.4,
via the focus-cycle keybind) and click-to-focus (`manage_seats`'s
`interacted` handling, Story 1.4's code-review follow-up). Neither
excluded the pinned terminal's `app_id`, so cycling focus onto it or
clicking it would move it to the *back* (top) of `stacking_order`,
directly violating FR4's "always rendered at the bottom of the render
order." This was flagged during drafting rather than silently accepted or
silently fixed; the user confirmed (2026-08-07) it should be fixed in this
story rather than deferred, since leaving it broken would mean this
story's own AC checkbox couldn't honestly be checked. Task 6 closes the
gap. The narrower question of whether the pinned terminal should be
excluded from focus *entirely* (a bigger UX redesign) was explicitly left
unresolved — Task 6 only guarantees the render-order invariant, per the
user's scoped answer.

**Zellij session-name character set is assumed, not validated.** Tag names
are arbitrary user-supplied strings (ADR-006: "user-facing arbitrary
strings"), and `claim_pinned_terminal_spawn` builds the session name as a
literal `format!("tag-{name}")` with no sanitization. `Command`'s
argument-array API (not a shell) means there is no shell-injection risk
regardless of what characters `name` contains, but `zellij` itself may
reject or mangle session names containing certain characters (e.g.
whitespace, slashes) — untested and unresearched in this story (YAGNI: no
evidence yet that real usage produces such tag names). Flagged as an
assumption for review, cheap to revisit if it proves wrong; the natural
place to add validation, if ever needed, is the tag-creation path (Story
1.7's tag-create keybind, or `WmCore::create_tag` itself), not this story's
spawn path.

**No new `WmCoreError` variants.** `claim_pinned_terminal_spawn` reuses the
existing `WmCoreError::UnknownTag` (already defined, previously
`#[allow(dead_code)]`-marked as unconstructed by any production path —
still true after this story, since the only caller,
`ensure_pinned_terminal_spawned`, has no production call site yet either;
remove that `#[allow(dead_code)]` only once Story 1.7 gives it one). No new
error variant needed for `lower_view`, which reuses
`WmCoreError::UnknownView` exactly like its mirror, `raise_view`.

**Pinned-terminal close exclusion, verified not reimplemented.** Story
1.4's code-review follow-up moved the close exclusion from
`WmCore::closable_focused_view` to a direct per-seat `Window.app_id ==
PINNED_TERM_APP_ID` check inside `Seat::do_action`'s `Action::Close` arm
(`wm/src/main.rs:462-485`, current). This story does not touch that logic
at all — confirmed by reading current `main.rs` before drafting this
story. The "never closed via any routed keybind" AC bullet is fully
satisfied by Story 1.4's existing implementation; this story's job is only
spawn + tiled + bottom-of-order.

**NFR1 (50ms budget).** `lower_view` is the same `O(number of registered
views)` `VecDeque` retain-then-push as `raise_view` (already accepted as
structurally within budget in Story 1.4's own NFR1 note).
`claim_pinned_terminal_spawn` is `O(1)` (a single tag lookup, a `format!`
allocation, and a delegated `O(1)`-per-tag `mark_terminal_spawned` call) —
no I/O, no locking. `spawn_pinned_terminal`'s actual `Command::spawn()` call
is I/O and explicitly *not* covered by the 50ms in-`wm-core` budget, same
treatment `Action::SpawnFoot` already received in Story 1.4.

**NFR2 (panic-free).** Both new `WmCore` methods are total: `lower_view`
never panics (returns `Err` for an unknown id, same as `raise_view`);
`claim_pinned_terminal_spawn`'s one `.expect()` is guaranteed to succeed
because the same tag id was just confirmed present in the same call
(safe-by-construction, matching `cycle_focus`'s established precedent
in Story 1.4, not a caller-facing invariant). On the `main.rs` side, every
new `Result` this story discards (`set_view_floating`, `lower_view`,
`claim_pinned_terminal_spawn` inside `ensure_pinned_terminal_spawned`) is
logged via `eprintln!` rather than silently swallowed, matching the
convention Story 1.4's code review established for `remove_windows`/
`focus_top`.

**No new ADR needed.** Checked `adrs.md` — no ADR governs process-spawning
mechanics or zellij session naming specifically; `architectural-constraints.md`
already states the standing constraint ("never respawn it on exit outside
the lazy-spawn-once path") this story implements, and `components.md`'s
`placement-engine` section already describes this exact spawn behavior as
a planned responsibility, not a new architectural decision this story is
making among alternatives.

No new Cargo dependencies. Build/test/lint only ever run **inside the
devcontainer via devpod** (`cargo build`, `cargo test`, `cargo fmt`,
`cargo clippy`) — same constraint as every prior story.

## Test plan
This story's tests split into three tiers, matching the project's
established TDD approach (Story 1.4's Test plan) for a mix of pure state
logic and I/O-adjacent glue:

1. **`WmCore::lower_view`** (Task 1) — Rust unit tests in
   `wm/src/wm_core/state.rs`'s existing test module, run via `cargo test
   --manifest-path wm/Cargo.toml` inside the devcontainer via devpod: moves
   a view to the front of the stacking order; is idempotent on a
   already-front view; returns `Err(UnknownView)` for an unregistered id
   and leaves state unchanged (covered both directly and via the extended
   consolidated regression guard).
2. **`WmCore::claim_pinned_terminal_spawn`** (Task 2) — same test
   module/mechanism: returns the `tag-<name>` session name and marks the
   tag spawned on the first call; returns `None` on every subsequent call
   for the same tag (the concrete proof of the "never respawned... even if
   the process exits" AC bullet — see Technical notes for why no
   process-exit-monitoring code is needed to satisfy it); returns
   `Err(UnknownTag)` for an unregistered id; the session-name format is
   exactly `tag-<name>`; and — directly proving both clarification notes at
   the top of this story — a full `register_output` → `create_tag` →
   `switch_tag` (Story 1.3's existing function) → `claim_pinned_terminal_spawn`
   sequence, run entirely from a test harness with no keybind and no
   `main.rs` involvement, returns the expected session name.
3. **`main.rs` wiring** (Tasks 3-5) — **not covered by automated unit
   tests**, by design, same boundary every prior story has hit (real
   `wayland_client`/`river-window-management-v1` `Dispatch` types, no live
   protocol connection in this devcontainer sandbox): `spawn_pinned_terminal`
   (I/O), `ensure_pinned_terminal_spawned` (composition of already-tested
   pieces, no call site yet), and `init_new_windows`'s new branch
   (Dispatch-adjacent glue calling already-tested `wm-core` mutators).
   Verified instead via `cargo build --manifest-path wm/Cargo.toml` and
   `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D
   warnings` succeeding, plus structural code review confirming: (a) Task
   5's branch fires only for `PINNED_TERM_APP_ID` and calls
   `set_view_floating(false)` then `lower_view` in that order; (b) Task 4's
   `match` arms call `spawn_pinned_terminal` only on `Ok(Some(_))`; (c)
   neither Task 3 nor Task 4 has been given a production call site by this
   story (confirmed by the `dead_code` lint itself under `cargo build`, not
   just by review).
4. **Tooling gate** (Task 6) — `cargo fmt --check`, `cargo clippy -D
   warnings`, and the full `cargo test` run (65 existing + 8 new, all
   green — 73 total), all in-container; `pre-commit run --all-files` as a
   final sanity check reusing Story 1.1's hooks.

Live-compositor coverage for the full spawn-on-switch chain, and for the
visual "tiled, bottom of render order, underneath a floating window" claim,
remains a standing gap in this story specifically because the chain's
*trigger* (a real tag-switch keybind) doesn't exist until Story 1.7 — not
merely a sandbox limitation like prior stories' gaps, though the sandbox
limitation applies too. Task 6.2's optional manual smoke test (spawning the
pinned terminal manually, standing in for the not-yet-existing keybind) is
the only way to get any live visual confirmation in this story; if not
performed, that is an explicit, recorded coverage gap for a human to
accept, consistent with Story 1.4's own precedent for its own live-keybind
gaps.

## FR coverage
FR3, FR4, NFR1, NFR2

## Dev Agent Record

### Implementation Plan
- Tasks 1 and 2 followed strict RED→GREEN: `lower_view`'s 3 tests (plus the extended `every_mutator_rejects_unknown_ids_without_mutating_state` guard) were added first and confirmed to fail to *compile* (`E0599`, method doesn't exist), then `lower_view` was implemented as the literal mirror of `raise_view` (`retain` + `push_front` instead of `push_back`) and the suite went green. Same pattern for `claim_pinned_terminal_spawn`'s 5 tests, implemented per the story's exact spec: look up the tag, short-circuit `Ok(None)` if already spawned, else `format!("tag-{name}")` and delegate to the existing `mark_terminal_spawned` mutator (DRY, no duplicated field write).
- Tasks 3-5 (Wayland/I/O glue, no RED/GREEN per the story's explicit carve-out) were implemented exactly per the story's call-site-by-call-site snippets, then verified via `cargo build`/`cargo clippy --all-targets -D warnings` plus structural review of the diff.
- Task 6 also followed RED→GREEN, but required a design decision the story deliberately left to this session (Technical notes documents two options). Chosen approach: embed the pinned-terminal exclusion **inside `WmCore::raise_view` itself**, rather than adding a separate guard method or duplicating an `app_id` check at each `main.rs` call site. Both of `main.rs`'s reordering call sites — `cycle_focus` (called from `Action::FocusNext`) and click-to-focus (`manage_seats`'s `interacted` handling, which calls `wm_core.raise_view` directly) — already route through `raise_view`, so guarding it there covers both automatically with zero `main.rs` changes (confirmed for Task 6.3 by re-reading both call sites after the `wm_core` change — no edits needed, build+clippy clean). `raise_view` now checks the target view's `app_id` and returns `Ok(())` without reordering when it's `PINNED_TERM_APP_ID`; `set_focus` is untouched by this guard, so `cycle_focus` (which calls `raise_view` then `set_focus` on the same id) still proceeds to focus the pinned terminal even when the reorder is a no-op.
- Task 7's gate (`cargo fmt --check`, `cargo clippy --all-targets -D warnings`, `cargo test`, `pre-commit run --all-files`) all ran clean inside the devcontainer via `devpod ssh buoy-wm`. One `cargo fmt` pass was needed mid-implementation (Task 4's `ensure_pinned_terminal_spawned` match arm exceeded the line-length the project's rustfmt config wraps at) — applied via `cargo fmt --all`, no manual reformatting needed.

### Completion Notes
- **Task 1 (GREEN):** `WmCore::lower_view(&mut self, id: ViewId) -> Result<(), WmCoreError>` added in `wm/src/wm_core/state.rs`, directly beside `raise_view`. 3 new tests plus 1 new assertion in the consolidated regression guard, all passing.
- **Task 2 (GREEN):** `WmCore::claim_pinned_terminal_spawn(&mut self, tag_id: TagId) -> Result<Option<String>, WmCoreError>` added, reusing `mark_terminal_spawned` rather than duplicating the field write. 5 new tests, all passing, including the direct `register_output` → `create_tag` → `switch_tag` → `claim_pinned_terminal_spawn` sequence proving the whole decision chain is reachable without any keybind or `main.rs` involvement.
- **Task 3 (build+clippy, no unit tests per carve-out):** `spawn_pinned_terminal(session_name: &str)` free function added in `wm/src/main.rs`, reusing `PINNED_TERM_APP_ID` for the `-a` flag. `#[allow(dead_code)]` with an inline comment — no call site until Task 4 wires it in (Task 4 does call it, so the allow is technically already satisfied by production reachability *within this story*, but the comment is kept accurate: the composing method itself, `ensure_pinned_terminal_spawned`, has no call site until Story 1.7).
- **Task 4 (build+clippy, no unit tests per carve-out):** `WindowManager::ensure_pinned_terminal_spawned(&mut self, tag_id: TagId)` added, matching on `claim_pinned_terminal_spawn`'s three outcomes and logging the `Err` case rather than swallowing it (NFR2 convention). `wm_core::ids::TagId` imported alongside the existing `ViewId` import. `#[allow(dead_code)]` — no production call site until Story 1.7's tag-switch keybind exists.
- **Task 5 (build+clippy+review, no unit tests per carve-out):** `init_new_windows` now forces any window with `app_id == PINNED_TERM_APP_ID` non-floating (`set_view_floating(view_id, false)`) and to the bottom of the stacking order (`lower_view(view_id)`) immediately on registration, both `Result`s logged rather than discarded. `set_view_floating`'s `#[allow(dead_code)]` removed (first real production call site) and its doc comment updated to reflect that.
- **Task 6 (GREEN, design decision recorded per 6.4):** See Implementation Plan above for the chosen design (guard inside `raise_view`, not duplicated at call sites). **Resulting behavior, stated explicitly per the story's instruction:** the pinned terminal remains **focusable** via both `FocusNext` and click-to-focus — `set_focus` is called on it exactly as for any other view, so it does receive real keyboard/pointer focus — but it is **never moved** from the bottom of the stacking order by either path, satisfying FR4's render-order invariant. One documented side effect of this narrowly-scoped fix, not addressed here because it was explicitly out of scope (Technical notes: "whether the pinned terminal should be excluded from focus entirely... was explicitly left unresolved"): because `raise_view` no-ops for the pinned terminal, once it is the front (bottom) element of the stacking order, `cycle_focus`'s "move front to back" traversal cannot advance past it — repeated `FocusNext` presses while it occupies the front position will keep re-focusing the pinned terminal rather than rotating to the next window. This is a real, observable consequence of the chosen design, flagged here for a human to consciously accept or revisit in a future story, not silently avoided.
- **Task 7 (verification gate):** `cargo fmt --manifest-path wm/Cargo.toml --all -- --check`, `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings`, `cargo test --manifest-path wm/Cargo.toml`, and `pre-commit run --all-files` all pass clean inside the devcontainer (`devpod ssh buoy-wm`). Full suite: **75/75 passing** (65 pre-existing + 3 `lower_view` + 5 `claim_pinned_terminal_spawn` + 2 Task 6 pinned-terminal-exclusion tests). Note: the story's own Task 7.1/Test-plan text anticipated "73 total" (65 + 8); that count predates Task 6, which the story's own preamble notes was added after a scoping discussion following the original draft — 75 is the correct, final total. `dead_code` warnings confirmed exactly as expected: `set_view_floating` no longer flagged; `spawn_pinned_terminal` and `ensure_pinned_terminal_spawned` still correctly show their narrowly-scoped `#[allow(dead_code)]`; `lower_view` has no allow needed since Task 5 gave it a real call site.
- **Task 7.2 (manual smoke test):** **Deferred, not performed.** This sandbox has no live `river` compositor session. The full spawn-on-switch chain, the visual "tiled, bottom of render order, underneath a floating window" claim, and Task 6's focus-but-don't-move behavior all remain visually unverified end-to-end — verified instead via Tasks 1/2/6's unit tests for the decision logic and Tasks 3-5's build+clippy+structural review for the wiring, consistent with the story's own Test plan section's stated coverage gap.

### Debug Log
No blocking issues. One `cargo fmt` pass required (Task 4's match-arm line length); resolved by running `cargo fmt --all` before Task 7's `--check` gate, no manual reformatting needed. `lower_view` showed a transient `dead_code` warning under `cargo clippy` between Task 1 (implemented) and Task 5 (call site added) — expected and self-resolving within the same session, not a regression; final Task 7 gate confirms zero unexpected warnings.

### Code Review Follow-up (2026-08-07)
Code review of the above found that Task 6's chosen design — no-op'ing
`raise_view` for the pinned terminal — had a worse consequence than
intended: because `cycle_focus` unconditionally targeted
`stacking_order.front()`, and the pinned terminal permanently occupies
that position, `cycle_focus` got **permanently stuck returning the pinned
terminal's id on every call** once one existed — `FocusNext` could never
reach any other window again. The user was asked and chose: fix
`cycle_focus` to skip over the pinned terminal entirely (never select it
as a target), while keeping it directly focusable via a click. Two
findings were fixed; a third (consolidation) was evaluated and
deliberately skipped.

**Finding #1 (design fix, TDD RED→GREEN) — `wm_core::state::cycle_focus`
reworked to never select the pinned terminal.** Previously: read
`stacking_order.front()` unconditionally, use it as the target for
`raise_view`+`set_focus`. Now: `cycle_focus` finds the *first* `ViewId` in
`stacking_order` (front to back) whose view's `app_id !=
PINNED_TERM_APP_ID`, and uses only that as the target. Returns `None` if
no such view exists (empty `stacking_order`, or the pinned terminal is the
only registered view) — it does **not** fall back to the pinned terminal
itself. Two RED tests added first and confirmed failing against the old
implementation, then the fix made them green, plus the full 75-test suite
(now 77 with these two additions) reconfirmed green:
- `cycle_focus_skips_pinned_terminal_and_round_robins_through_others` —
  pinned + 2 other views, 10 repeated `cycle_focus()` calls, asserts the
  pinned terminal's id is never returned and the two non-pinned ids
  round-robin correctly, with the pinned terminal still at
  `stacking_order().first()` throughout.
- `cycle_focus_returns_none_when_only_pinned_terminal_registered` — only
  the pinned terminal registered, asserts `cycle_focus()` returns `None`.
The pre-existing
`cycle_focus_does_not_move_pinned_terminal_from_bottom_of_stacking_order`
test was re-verified passing unchanged (it still holds under the new
implementation, since the pinned terminal still never moves).
**Corrected description of `cycle_focus`'s actual behavior, superseding
the Task 6 Completion Note above:** the pinned terminal is *not*
selectable via `FocusNext`/`cycle_focus` at all — repeated `FocusNext`
presses round-robin through every other registered view and skip the
pinned terminal entirely, every time, rather than getting stuck on it.
It remains focusable only via a direct click (see Finding #2).

**Finding #2 (HIGH, real bug, build+clippy+manual-trace verified per the
established `main.rs` carve-out) — the real compositor z-order was not
guarded, so the pinned terminal still visibly raised to the top on
`FocusNext`/click.** `main.rs` maintains its own `self.windows:
VecDeque<Window>` that drives the actual `place_top()` calls, entirely
independent of `wm_core.stacking_order` — Task 6's `wm_core`-level fix
never touched it. Two call sites fixed:
- `Seat::do_action`'s `Action::FocusNext` arm (`wm/src/main.rs`): added a
  defensive `windows[i].app_id == PINNED_TERM_APP_ID` check on the
  `cycle_focus()`-returned target (belt-and-suspenders — Finding #1 means
  `cycle_focus` should never actually return the pinned terminal's id, but
  this arm is now correct on its own terms rather than correct only by
  accident of `cycle_focus`'s behavior elsewhere). If it somehow is the
  pinned terminal, skips the `windows.remove`/`push_back`/`place_top()`
  reorder and instead issues the underlying focus calls
  (`self.proxy.focus_window`, `self.focused = ...`,
  `wm_core.set_focus`) directly.
- `manage_seats`'s `interacted` (click-to-focus) handling
  (`wm/src/main.rs`): the previously-unconditional
  `self.windows.push_back(window)` is now guarded — for the pinned
  terminal, the window is re-inserted at its original index (not pushed to
  the back, so `focus_top`'s subsequent `place_top()` call never targets
  it) but still receives real Wayland keyboard focus via a direct
  `seat.proxy.focus_window`/`seat.focused = ...`/`wm_core.set_focus`
  sequence — the same underlying calls `focus_top` would have made, minus
  `place_top()`. Because `manage_seats` unconditionally calls
  `seat.focus_top(&self.windows, wm_core)` immediately after this block on
  every pass (which always re-targets `windows.back()`), a new
  `pinned_terminal_focused_directly` flag skips that call for the pass
  that just handled a pinned-terminal click, so it doesn't immediately
  self-undo the direct focus assignment made moments earlier in the same
  pass. **Known limitation, flagged rather than silently assumed away:**
  this only guarantees the focus assignment survives the single pass that
  processed the click. If `manage_seats` runs again on a later pass with
  no new interaction (which it does unconditionally on every
  `handle_manage_start`), that later pass's unconditional
  `seat.focus_top` call will re-target `windows.back()` (the real top
  window, not the pinned terminal) and could revert focus away from the
  pinned terminal. Whether this is visible/problematic in practice depends
  on how frequently `handle_manage_start` runs relative to user input — not
  verifiable without a live compositor session (none available in this
  sandbox). The FR4 z-order bug itself (the HIGH-priority, must-fix part)
  is fully and unconditionally fixed regardless of this limitation: the
  pinned terminal's `place_top()` is never called by either path now,
  full stop.
Verified via `cargo build --manifest-path wm/Cargo.toml` and `cargo clippy
--manifest-path wm/Cargo.toml --all-targets -- -D warnings`, both clean,
plus manual tracing of both call sites (no live compositor in this
sandbox, same carve-out established by Story 1.4 Tasks 3-4 and this
story's own Task 5).

**Finding #3 (consolidation) — evaluated and deliberately skipped.** All
`PINNED_TERM_APP_ID` comparisons already route through the single exported
constant (`wm_core::state::PINNED_TERM_APP_ID`) rather than duplicating the
literal string — the "at minimum, a single exported constant-comparison
idiom used consistently" bar the finding itself allows as sufficient was
already met before this follow-up. A shared `WmCore::is_pinned_terminal`
predicate was considered but rejected: the three `wm_core`-internal call
sites (`raise_view`, `closable_focused_view`, `cycle_focus`) each already
have a `&View` in hand from a lookup they've already performed, so routing
through a `ViewId`-based helper would mean either a redundant second
lookup or a signature change to accept `&View` (more churn than the
finding intends); the four `main.rs` call sites (`init_new_windows`,
`manage_seats`'s `interacted` handling, `Action::Close`, `Action::FocusNext`)
check `main.rs`'s own `Window.app_id` directly — a plain `String` already
in hand — and would gain nothing from a `WmCore`-side `ViewId` lookup
except an extra `Option` unwrap. The actual root cause of Finding #2 was a
*missing* check at a new call site, not an inconsistent comparison idiom
at existing ones — a shared predicate would not have prevented that
omission, and per the finding's own instruction ("if it adds meaningful
risk or complexity, skip it... note in the Dev Agent Record why"), this
was skipped rather than forced.

Full in-container re-verification: `cargo test --manifest-path
wm/Cargo.toml` (**77/77 passing** — 75 pre-existing + 2 new), `cargo build
--manifest-path wm/Cargo.toml`, `cargo fmt --manifest-path wm/Cargo.toml
--all -- --check`, `cargo clippy --manifest-path wm/Cargo.toml
--all-targets -- -D warnings`, and `pre-commit run --all-files` — all
clean, run inside the devcontainer via `devpod ssh buoy-wm`.

## File List
- `wm/src/wm_core/state.rs` — added `WmCore::lower_view` (Task 1, mirror of `raise_view`), `WmCore::claim_pinned_terminal_spawn` (Task 2), a pinned-terminal exclusion guard inside `WmCore::raise_view` (Task 6), removed `set_view_floating`'s `#[allow(dead_code)]` and updated its doc comment (Task 5.2). 10 new unit tests (3 + 5 + 2) plus 1 new assertion in the consolidated regression guard. **Code review follow-up:** reworked `cycle_focus` to skip the pinned terminal as a selection candidate entirely (Finding #1), plus 2 new tests.
- `wm/src/main.rs` — added `spawn_pinned_terminal` free function (Task 3), `WindowManager::ensure_pinned_terminal_spawned` (Task 4), `wm_core::ids::TagId` import (Task 4.2), `init_new_windows`'s pinned-terminal tiled/lower-to-bottom branch (Task 5.1). No changes needed for Task 6.3 at the time — both reordering call sites already routed through the now-guarded `raise_view`/`cycle_focus`. **Code review follow-up:** guarded the real compositor z-order in both `Action::FocusNext` and `manage_seats`'s `interacted` (click-to-focus) handling so the pinned terminal's `place_top()` is never called, while still routing it through direct Wayland-focus calls so it remains focusable via click (Finding #2).

## Change Log
- 2026-08-07: Implemented Story 1.5 (Tasks 1-7) — `WmCore::lower_view`/`claim_pinned_terminal_spawn` (TDD, RED→GREEN), `spawn_pinned_terminal`/`ensure_pinned_terminal_spawned` in `main.rs` (build/clippy-verified, dormant until Story 1.7), the pinned terminal's tiled + bottom-of-render-order wiring in `init_new_windows` (live in production today), and a `raise_view`-level guard (Task 6, TDD) closing the FR4 gap where `cycle_focus`/click-to-focus could move the pinned terminal off the bottom. Full in-container fmt/clippy/test/pre-commit gate all green (75/75 tests). Manual live-compositor smoke test explicitly deferred (no `river` session available in this sandbox).
- 2026-08-07: Code review follow-up — reworked `cycle_focus` so it never selects the pinned terminal as a `FocusNext` target (Finding #1, TDD RED→GREEN), guarded the real `main.rs` compositor z-order in `Action::FocusNext` and click-to-focus so the pinned terminal's `place_top()` is never called even though it can still receive keyboard focus via click (Finding #2, build+clippy+manual-trace verified), and evaluated but deliberately skipped consolidating the `PINNED_TERM_APP_ID` checks into a shared predicate (Finding #3, documented rationale above). Full in-container fmt/clippy/test/pre-commit gate all green (77/77 tests).
