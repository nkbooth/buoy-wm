---
baseline_commit: d9fc572
---

# Story 2.8: Real active-output tracking and output-removal reroute

Epic: 2 | Priority: H | Status: done

## Description
Live testing after Story 2.7 (2026-08-08, first real multi-monitor session —
laptop docked to an external monitor) found the pinned terminal (and every
tag-switch/auto-tag keybind) permanently stuck on the laptop panel, and asked
that undocking/lid-close be handled too. Both are the same root cause,
already known and explicitly flagged as an accepted gap rather than an
oversight:

**Gap 1 — `active_output_id()` is a placeholder, not a real signal.**
`wm/src/main.rs`'s `WindowManager::active_output_id` (added Story 1.7) is
`self.outputs.values().map(|o| o.output_id).min()` — "the lowest-`OutputId`
(first-registered) output," documented at the time as "correct-by-construction
for the single-output case (ADR-005)... Real focused-output tracking has no
live protocol signal to compute from yet." Since the laptop's built-in panel
enumerates before an externally-connected dock monitor essentially always,
every `TagCycle`/auto-tag/pinned-terminal-spawn keybind action resolves to
the laptop panel's `OutputId`, regardless of where the user actually is.

**Gap 2 — output removal never reaches `wm_core`.** Story 1.7's own Dev Agent
Record flagged this explicitly at the time: *"`wm_core` has no
`unregister_output` method; `remove_outputs` ... destroys the real Wayland
proxy on hotplug-removal but never removes the corresponding `wm_core`
output. A removed real output leaves a stale, permanently-registered
`wm_core` output behind, eligible forever after to be selected by
`active_output_id`... Flagged for a human to consciously accept, not
silently omitted."* That human decision is this story: fix it now that real
multi-monitor/hotplug use (dock connect/disconnect, laptop lid close) has
made it a live problem rather than a theoretical one.

**Both gaps have a real fix available now that didn't exist at Story 1.7
time** (or rather, existed in the protocol but wasn't wired up): reading the
actual protocol XML in full (not just the summary) shows `river_output_v1`
already sends `position`/`dimensions` events carrying each output's real
rectangle in the compositor's global coordinate space, and `river_seat_v1`
(protocol v4, already bound at v4 via `river_window_manager_v1`) sends a
`pointer_position` event (`since="2"`) carrying the pointer's current global
coordinates. **Both events already arrive at `buoy-wm` today and are
silently discarded** (`main.rs`'s `Event::Position { x: _, y: _ } => {}` /
`Event::Dimensions { width: _, height: _ } => {}` for outputs, and
`Event::PointerPosition { x: _, y: _ } => {}` for seats) — no new protocol
version negotiation is needed, only wiring up data that's already flowing.

## Acceptance criteria
**Given** the pointer's last-known global position falls within a registered
output's rectangle (`position`/`dimensions`)
**When** any keybind action needs "the active output" (`TagCycle`,
`TagCreate`, `OpenTagPicker`/`TagSwitch`'s output argument, new-window
auto-tagging)
**Then** that output is used, not the lowest-`OutputId` placeholder

**Given** no seat has yet reported a pointer position (startup, before the
first `pointer_position` event), or the last-known position falls outside
every currently-registered output's rectangle
**When** the active output is needed
**Then** fall back to the existing deterministic lowest-`OutputId` behavior — never panic, never silently guess something undocumented

**Given** a registered output is removed (`river_output_v1.removed` — dock
disconnect or laptop lid close)
**When** `buoy-wm` processes the removal
**Then** `wm_core` forgets that output entirely (no permanent ghost entry with a stale `current_tag` claim)

**Given** the removed output had a tag currently displayed on it, and at least one other output survives
**When** the removal is processed
**Then** that tag is rerouted onto the (post-removal) active output, using the same `switch_tag` reroute enforcement every other tag-assignment path already goes through — not a second, parallel implementation

**Given** the removed output had a tag displayed on it, and no other output survives
**When** the removal is processed
**Then** the tag becomes displayed on no output (its windows correctly go through `is_view_visible` → hidden) — no crash, no panic, no orphaned reference

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: Track real output geometry (AC 1, 2)**
  - [x] 1.1 Add `position: (i32, i32)` and `dimensions: (i32, i32)` fields to `main.rs`'s `Output` struct (both default `(0, 0)` at construction — matches the struct's existing all-zero `Window` field-init precedent). No RED/GREEN: this is a plain data field with no decision logic of its own, same carve-out as every prior struct-field addition in this project.
  - [x] 1.2 In `Dispatch<RiverOutputV1, ()>`'s event handler, wire `Event::Position { x, y }` to `output.position = (x, y)` and `Event::Dimensions { width, height }` to `output.dimensions = (width, height)`, replacing the current discard arms. No RED/GREEN (Wayland event glue, untestable without a live compositor — same boundary as every prior `main.rs` protocol-handler story).

- [x] **Task 2: Track the pointer's last-known global position (AC 1, 2)**
  - [x] 2.1 Add `pointer_position: Option<(i32, i32)>` to `Seat`, defaulting to `None` (no position known yet — the AC 2 fallback case).
  - [x] 2.2 In `Dispatch<RiverSeatV1, ()>`'s event handler, wire `Event::PointerPosition { x, y }` to `seat.pointer_position = Some((x, y))`, replacing the current discard arm. No RED/GREEN, same reasoning as 1.2.

- [x] **Task 3: Replace the `active_output_id` placeholder with a real computation (AC 1, 2)**
  - [x] 3.1 Rewrite `WindowManager::active_output_id`: for each seat with `pointer_position == Some((x, y))`, find an output whose rectangle (`position.0..position.0+dimensions.0`, `position.1..position.1+dimensions.1`) contains `(x, y)`; return its `output_id` on the first match found (iteration order over `self.seats`/`self.outputs` — document as deterministic-but-arbitrary under the practically-never-hit multi-seat-disagreement case, same "deterministic, not a hidden guess" standard ADR-005 already set for the code it replaces). If no seat has a known position, or the known position(s) don't fall within any registered output's rectangle (e.g. an output's geometry events haven't arrived yet, or the pointer's last-known position referenced an output that's since been removed — see Task 4), fall back to the existing `self.outputs.values().map(|o| o.output_id).min()`.
  - [x] 3.2 Update the function's doc comment: this is no longer a documented permanent placeholder (ADR-005's "no live protocol signal to compute from yet" no longer applies) — it's now a real signal with a documented, deterministic fallback for the genuinely-ambiguous/not-yet-known cases.
  - [x] 3.3 No RED/GREEN — this method lives in `main.rs` and is pure `WindowManager`-field arithmetic (rectangle-contains-point over `Copy` structs), not `wm_core` decision logic, matching Story 1.7's own carve-out for the code being replaced. Verify via `cargo build`/`cargo clippy --all-targets -- -D warnings` and structural review that the rectangle-containment check is off-by-one-correct (`position.0..position.0 + dimensions.0`, half-open, matching how `dimensions` is documented as a width/height extent from `position`).

- [x] **Task 4: `wm_core::unregister_output` (AC 3, 4, 5)**
  - [x] 4.1 RED — Add tests to `wm/src/wm_core/state.rs`: `unregister_output_removes_the_output_and_returns_none_when_it_had_no_current_tag`; `unregister_output_removes_the_output_and_returns_its_current_tag`; `unregister_output_unknown_id_returns_error_and_leaves_state_unchanged` (`Err(WmCoreError::UnknownOutput)`, whole-state snapshot equality per Story 1.2's established pattern); extend the existing `every_mutator_rejects_unknown_ids_without_mutating_state` regression guard with one more assertion for `unregister_output(bogus_output)`. Confirm all fail to compile — `unregister_output` doesn't exist yet.
  - [x] 4.2 GREEN — Implement `pub fn unregister_output(&mut self, output_id: OutputId) -> Result<Option<TagId>, WmCoreError>` on `WmCore`, near `register_output`: `let output = self.outputs.remove(&output_id).ok_or(WmCoreError::UnknownOutput)?;` then `Ok(output.current_tag)`. Deliberately does **not** decide where an orphaned tag goes — `wm_core` stays protocol-agnostic (it has no way to know which *other* output should inherit the tag; that's a `main.rs`-level, pointer-position-derived decision, Task 5) — it only forgets the removed output and reports what it was showing, mirroring `unregister_view`'s "report enough for the caller to act, don't decide for them" shape. `///` doc comment stating this explicitly, referencing Story 1.7's flagged gap this closes. Confirm all Task 4.1 tests plus the extended guard pass.

- [x] **Task 5: Wire removal + reroute into `main.rs` (AC 3, 4, 5)**
  - [x] 5.1 In `WindowManager::remove_outputs`, before dropping each `removed`-flagged output from `self.outputs`, call `wm_core.unregister_output(output.output_id)` (lock `wm_core` once for the whole pass, same `ipc::lock_recovering` pattern every other `WindowManager` method already uses) and collect any `Ok(Some(tag_id))` into a `Vec<TagId>` of orphaned tags — log-and-continue on the structurally-unreachable `Err(UnknownOutput)` case (NFR2 pattern, same as `remove_windows`' `unregister_view` call).
  - [x] 5.2 After all removals are applied to `self.outputs` (so `active_output_id()` sees only survivors), if there's a live active output (`self.active_output_id()` returns `Some`), call `wm_core.switch_tag(new_active_output_id, orphaned_tag_id)` for each orphaned tag collected in 5.1 — reusing the existing ADR-005-enforcing reroute mutator, not a second implementation. If multiple tags were orphaned in the same pass (removing more than one output at once — rare, but not impossible), each is migrated in turn onto the same surviving active output; since `switch_tag` enforces one-tag-per-output, only the last one processed ends up actually displayed there — document this as an accepted, narrow edge case rather than silently mishandling it (mirrors this project's existing "documented FIFO race, accepted scope" precedent from Story 2.7's review). If no output survives, do nothing further — the orphaned tag(s) are simply displayed nowhere, which `is_view_visible` already handles correctly (hidden, not a crash).
  - [x] 5.3 No RED/GREEN for this task (Wayland-glue orchestration calling already-tested `wm_core` mutators, same boundary as every prior `main.rs` story). Verify via `cargo build`/`cargo clippy --all-targets -- -D warnings` and structural review: confirm `unregister_output` is called before the output is dropped from `self.outputs` (not after — `active_output_id`'s Task 3 fallback needs the survivor set already accurate), and confirm no double-lock/deadlock is introduced (one `lock_recovering` for the whole `remove_outputs` pass, released before any `switch_tag` call needs its own separate lock — or folded into the same guard if lifetimes allow; match whichever existing pattern `manage_seats`/`ensure_pinned_terminal_spawned` already establishes for "mutate wm_core, then use a fresh borrow after" sequencing).

- [x] **Task 6: Full in-container verification gate (AC: all)**
  - [x] 6.1 Inside the devcontainer (`devpod ssh buoy-wm`, or the `podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>` fallback): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pre-commit run --all-files`. Confirm the existing 265-test suite plus this story's new `wm_core` tests all pass, 0 regressions.
  - [x] 6.2 Manual live-verification note (not automated, not a story blocker — same boundary as every prior story's live-compositor gap): once rebuilt, docking/undocking and lid-close should be directly observable to confirm the pinned terminal and tag-switch keybinds follow the pointer's actual monitor, and that closing the lid while docked correctly migrates any tag that was showing on the laptop panel onto the external monitor instead of hiding it. Record in the Dev Agent Record whether this was actually confirmed live or deferred.

## Technical notes

**Why "pointer position" and not "last-focused window's output" or
"whichever output most recently had a manage/render event."** The protocol
gives a first-class, always-current signal for exactly this
(`river_seat_v1.pointer_position`) — this is the same signal dwm/i3/sway-
style WMs use for "focused monitor follows mouse," a well-understood and
expected convention, not a novel design. No other candidate signal in the
protocol is as direct; window focus can lag behind which monitor the user is
actually looking at/reaching for next (e.g. right after a fresh `TagCycle`
onto an output with no windows yet).

**Why `unregister_output` returns `Option<TagId>` instead of also doing the
reroute itself.** Every prior story in this project has kept `wm_core`
strictly protocol-agnostic — `is_view_visible`'s own doc comment states this
explicitly ("this method itself has no knowledge of the Wayland protocol").
Picking *which* surviving output should inherit an orphaned tag requires the
pointer-position-derived `active_output_id()`, which is `main.rs`-level,
Wayland-derived state `wm_core` has no access to and shouldn't be given
access to just for this. Keeping the split matches every existing
`wm_core`/`main.rs` boundary in the codebase.

**Reuse, not reinvention.** The reroute step (Task 5.2) is a direct call to
the existing `WmCore::switch_tag`, the same mutator `cycle_tag`/the
switch-mode picker already go through — this is deliberately the "shared
mutator, new call site" pattern this project's own retrospectives have
flagged as its highest-real-bug-rate category (Epic 1: 3/7 stories, Epic 2:
5/5 stories touching `Arc<Mutex<WmCore>>`-shared state). Reviewers: this is
the single highest-priority thing to verify in this story — confirm
`switch_tag`'s existing ADR-005 cross-output reroute enforcement behaves
correctly when called from this new removal-triggered path, not just from
its two existing keybind/IPC call sites.

**Scope boundary.** This story does not change `switch_tag`/`cycle_tag`
themselves, does not add any new `WmCoreError` variant (removal of an
unknown output is the only new failure mode, and `UnknownOutput` already
exists), and does not attempt "smartest possible" output selection (e.g.
largest output, output nearest the removed one's old position) — pointer
position is the one real, protocol-native signal available, and building
anything more elaborate now would be speculative (YAGNI) for what is still
a single-user WM.

## Test plan
1. **`wm_core` unit tests (TDD, RED before GREEN)**: `unregister_output`'s three cases (Task 4.1) plus the extended cross-mutator invalid-id regression guard. Fully testable — no Wayland types involved.
2. **Build/lint gate**: `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` clean with the new `Output`/`Seat` fields and rewritten `active_output_id` compiled in.
3. **Regression gate**: existing 265-test suite (`wm` 175, `status-bar` 27, `tag-picker` 63) unaffected.
4. **Live gate (manual, not automated)**: dock/undock and lid-close correctly follow the pointer and reroute orphaned tags, per Task 6.2.

## FR coverage
Existing behavior (`Output.current_tag` tracking, one-tag-per-output
enforcement via `switch_tag`/ADR-005) made correct under real multi-output
and hotplug conditions that Epic 1/2's sandbox testing couldn't exercise —
not a new FR, same framing as Story 2.6's layer-shell fix.

## Dev Agent Record

**What was done**:

- `wm/src/main.rs`'s `Output` struct gained `position: (i32, i32)` and
  `dimensions: (i32, i32)`, both defaulting to `(0, 0)` in `Output::new`
  (Task 1.1). `impl Dispatch<RiverOutputV1, ()>`'s event handler now wires
  `Event::Position { x, y }`/`Event::Dimensions { width, height }` into
  those fields instead of discarding them (Task 1.2).
- `Seat` gained `pointer_position: Option<(i32, i32)>`, defaulting to
  `None` in `Seat::new` (Task 2.1). `impl Dispatch<RiverSeatV1, ()>`'s
  event handler now wires `Event::PointerPosition { x, y }` into
  `seat.pointer_position = Some((x, y))` instead of discarding it (Task
  2.2).
- `WindowManager::active_output_id` was rewritten (Task 3): it now scans
  `self.seats.values()` for a known `pointer_position`, and for the first
  one found, looks for a registered output whose rectangle
  (`position.0..position.0 + dimensions.0`, half-open, same for the y
  axis) contains that point, returning its `output_id` on the first match.
  If no seat has a known position, or no output's rectangle contains it,
  it falls back to the old Story 1.7 behavior
  (`self.outputs.values().map(|o| o.output_id).min()`). The doc comment
  was rewritten to state this is no longer a permanent placeholder.
- `WmCore::unregister_output` (Task 4) was added to
  `wm/src/wm_core/state.rs` via real RED/GREEN TDD: three new tests
  (`unregister_output_removes_the_output_and_returns_none_when_it_had_no_current_tag`,
  `unregister_output_removes_the_output_and_returns_its_current_tag`,
  `unregister_output_unknown_id_returns_error_and_leaves_state_unchanged`)
  plus an extra assertion on
  `every_mutator_rejects_unknown_ids_without_mutating_state` were written
  first and confirmed to fail to compile (`cargo test -p buoy-wm
  unregister_output` → 4× "no method named `unregister_output`"), then the
  implementation was added (`self.outputs.remove(&output_id).ok_or(...)`,
  returning the removed output's `current_tag`) and all four assertions
  confirmed green.
- `WindowManager::remove_outputs` (Task 5) now locks `wm_core` once for
  the whole removal pass, calling `wm_core.unregister_output` for each
  `removed`-flagged output inside the same `retain` closure that drops it
  from `self.outputs`, collecting any `Ok(Some(tag_id))` into a
  `Vec<TagId>` (log-and-continue on the structurally-unreachable
  `Err(UnknownOutput)`, matching `remove_windows`' `unregister_view`
  pattern). The lock is released (block scope ends) before
  `self.active_output_id()` is called against the now-settled survivor
  set; if it returns `Some`, a second, fresh `wm_core` lock is taken to
  call `wm_core.switch_tag(active_output_id, tag_id)` for each orphaned
  tag in turn, reusing the existing ADR-005-enforcing mutator rather than
  a second implementation. If no output survives, the orphaned tags are
  simply dropped (no `switch_tag` call), which `is_view_visible` already
  renders as "hidden," not a crash.

**Deviations from the story's literal wording** (flagged explicitly, per
this project's established norm — see Story 2.7's own Dev Agent Record):
none identified. The implementation matches Tasks 1-5's plans as written,
including the accepted "last orphaned tag wins" edge case under
`switch_tag`'s one-tag-per-output enforcement when more than one output is
removed in the same pass (documented in-line in `remove_outputs`'s
comments, not silently mishandled).

**Verification** (all inside the devcontainer via `devpod ssh buoy-wm` —
worked reliably every call this session, the `podman exec` fallback was
never needed):
- `cargo fmt --all -- --check`: found one line-length violation in the new
  `unregister_output` signature; fixed via `cargo fmt --all`, then clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, no
  changes needed.
- `cargo test --workspace`: `buoy-wm` 178 (175 baseline + 3 new), `status-
  bar` 27, `tag-picker` 63 = **268 total, 0 failures, 0 regressions**.
- `pre-commit run --all-files`: both configured hooks (`cargo-fmt-check`,
  `cargo-clippy`) passed.

**Task 6.2 (manual live-verification)**: **deferred**, not performed by
this agent — no access to a live compositor or a physical dock/lid in this
environment, same boundary as every prior story's live-compositor gap.
Docking/undocking and lid-close behavior against a real `river` session
still needs to be confirmed by a human before this story's Description's
originating live-testing report can be considered resolved end-to-end.

**File List**:
- `wm/src/main.rs`
- `wm/src/wm_core/state.rs`

## Code Review

HIGH-effort workflow-backed review, prioritized on the story's own flagged
highest-risk area (reusing `switch_tag` via a new call site). Three CONFIRMED
findings:

1. **Output-removal reroute destroyed the surviving output's own tag.** The
   implementation as first written called `wm_core.switch_tag(active_output_id,
   orphaned_tag)` unconditionally whenever any output survived — but
   `switch_tag` always overwrites its target's `current_tag`, so if the
   surviving output was already displaying something (the realistic docked
   case this story exists for: external monitor showing real work, laptop
   panel showing something else), closing the lid would silently blow away
   the external monitor's tag and replace it with the laptop's orphaned one.
   This is exactly the failure mode AC 4 didn't anticipate (it only covers
   "the removed output had a tag" and "at least one other output survives,"
   not "and that survivor already has its own tag"). **Fixed**:
   `remove_outputs` now checks `wm_core.output_current_tag(active_output_id)`
   first and only reroutes if that output isn't already displaying
   something; otherwise the orphaned tag is left displayed nowhere (the same
   safe "hidden, not a crash" outcome the no-survivor case already had). The
   multi-orphan edge case's documented behavior changed accordingly: only
   the first orphaned tag processed can land on an empty survivor; every
   subsequent one then finds it occupied and is left hidden, rather than
   the previous "last one wins" overwrite chain.
2. **`active_output_id`'s multi-seat tie-break claimed a determinism it
   didn't have.** The doc comment stated "the first match in `self.seats`'
   iteration order wins — deterministic, not a hidden guess," but
   `self.seats` is a `HashMap`, whose iteration order is not guaranteed
   stable across insertions/removals or process runs. Real-world impact is
   low (this is a single-user, practically-single-seat WM), but the
   claim was false as written. **Fixed**: rewritten to collect the output
   id under every seat's pointer position and take the minimum — a
   genuinely deterministic tie-break independent of `HashMap` iteration
   order — with the doc comment corrected to describe it accurately.
3. **Stale `output_id` race in the switch-mode picker.** `Action::TagSwitch`
   captures `active_output_id` once and passes it to the spawned
   `tag-picker` process; if that output is unplugged while the picker is
   still open, the later `SwitchTag` IPC call now hits a genuinely
   unregistered id (this story's own `unregister_output` is what makes this
   possible — previously the ghost output would have silently "succeeded"
   into nothing). Confirmed real, but checked against `tag-picker`'s actual
   client code (`run_switch_mode`): a `Response::Error` from `SwitchTag` is
   already handled cleanly — logs the message to stderr and exits — the
   same graceful-failure shape every other IPC error path in this project
   already uses. **No change needed**: this is an honest error replacing a
   silent no-op on an already-narrow race, not a regression in robustness.

**Re-verification after fixes**: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `pre-commit run
--all-files` all clean; `cargo test --workspace` — 268 total (`buoy-wm`
178, `status-bar` 27, `tag-picker` 63), 0 failures, unchanged from before
the fixes (both fixes are `main.rs`-level Wayland-glue logic, same
untestable-without-a-live-compositor boundary as the rest of Tasks 1/2/3/5
— verified via build/clippy/structural review, matching the story's own
carve-out).

**Code review: PASS**.
