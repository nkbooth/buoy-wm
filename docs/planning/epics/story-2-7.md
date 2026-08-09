---
baseline_commit: 2ad2571
---

# Story 2.7: Tag-based window visibility and fullscreen pinned terminal

Epic: 2 | Priority: H | Status: done

## Description
Live testing after Story 2.6 (2026-08-08) surfaced a foundational gap:
**no code anywhere in `buoy-wm` filters window visibility by tag.**
`manage_windows()` only handles pointer move/resize; nothing reads
`View.tags` or `Output.current_tag` to decide what's actually rendered.
Every window ever created, across every tag, is visible all the time.
This also explains why the pinned terminal "doesn't fullscreen" (the
originally reported symptom): it was never given real geometry, and even
if it were, multiple tags' pinned terminals would all be simultaneously
visible and overlapping without visibility filtering first.

FR2 ("the WM enforces at most one tag displayed per output") and FR1's
"the WM tracks window↔tags... output→current-tag" were implemented as
pure data-tracking (`wm_core`'s `View.tags`, `Output.current_tag`,
`switch_tag`'s reroute enforcement) but never wired to actual rendering.
This story closes that gap and, in the same pass, gives the pinned
terminal correct fullscreen placement — the two are inseparable: a
fullscreen pinned terminal only makes sense once only the *active* tag's
windows (including its own pinned terminal) are shown per output.

**Protocol primitives available (confirmed by reading the full XML, not
assumed):**
- `river_window_v1::hide()`/`show()` — "Newly created windows are
  considered shown unless explicitly hidden." Exactly the mechanism for
  visibility filtering; no protocol gap here.
- `river_window_v1::fullscreen(output)` — "The compositor will handle
  the position and dimensions of the window while it is fullscreen...
  clip window content... to the given output's dimensions." River does
  all real sizing/clipping natively. This supersedes the geometry-
  tracking approach originally considered (tracking `river_output_v1`'s
  `position`/`dimensions` events and manually computing
  `propose_dimensions`/`set_position`) — `fullscreen()` is simpler,
  protocol-idiomatic, and correct-by-construction. Per the protocol: "If
  multiple windows are fullscreen on the same output at the same time
  only the 'top' window in rendering order shall be displayed" — since
  the pinned terminal stays at the bottom of stacking order (Story 1.5,
  unaffected by this story) and no other window is ever made fullscreen,
  it is always the one displayed, with floating windows rendering above
  it exactly as today (Story 1.6, unaffected).

**The correlation problem this story must solve:** a pinned-terminal
window maps with `app_id == "pinned-term"` — the protocol gives no way to
know which tag it was spawned for. `WmCore` currently has no
pinned-terminal ↔ tag association at all (verified: nothing calls
`toggle_view_tag` for a pinned terminal anywhere). Resolution: track a
`pending_pinned_terminal_tags: VecDeque<TagId>` queue in `WindowManager`,
pushed to exactly when `ensure_pinned_terminal_spawned` actually spawns a
process (the `Some(session_name)` branch), popped FIFO by the next
pinned-terminal window that maps in `init_new_windows`. This is a
documented, accepted simplification for a single-user, human-paced
desktop (same class of assumption already accepted elsewhere in this
project, e.g. Story 2.3's numeral-collision edge case) — concurrent
rapid-fire spawns racing this queue is not a realistic scenario here.

**Newly-created (non-pinned) windows default to the active output's
current tag.** Without this, a freshly spawned window (`Mod4+Space`)
would register with empty tags and immediately be invisible under the
new filtering rule — confusing and wrong. This matches dwm's own
convention (new windows appear on the currently selected tag). Uses the
existing `active_output_id()` heuristic (lowest `OutputId`, same
documented simplification as Stories 1.7/2.4) to resolve "current tag"
if the active output has one; if not (e.g. no tag exists yet at all),
leave the window untagged and visible-by-default (see AC).

## Acceptance criteria
**Given** a view has at least one tag matching at least one output's `current_tag`
**When** visibility is (re-)computed
**Then** the view's window is shown (`river_window_v1.show()`)

**Given** a view has no tags matching any output's `current_tag`, and it has at least one tag
**When** visibility is (re-)computed
**Then** the view's window is hidden (`river_window_v1.hide()`)

**Given** a view has no tags at all (the only case where "no tags" doesn't mean "hidden")
**When** visibility is (re-)computed
**Then** the view's window is shown — untagged is a visible-by-default bootstrap state, not an invisible one, so a window created before any tag exists is never permanently stuck hidden

**Given** a new (non-pinned) window is created
**When** it is registered
**Then** it is auto-tagged with the active output's current tag, if one exists

**Given** a pinned terminal's window maps
**When** `init_new_windows` processes it
**Then** it is tagged with the `TagId` it was spawned for (via the pending-spawn queue), made fullscreen on whichever output currently shows that tag, and hidden if that tag is not currently shown on any output

**Given** a tag switches which output shows it (via `switch_tag`/`cycle_tag`, raw keybind or picker)
**When** the switch completes
**Then** visibility is recomputed for every view, and any pinned terminal whose tag just became displayed/undisplayed is shown+fullscreened or hidden accordingly

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: Pure visibility-decision logic in `wm_core` (AC: all "visibility" ACs) — TDD**
  - [x] 1.1 RED — Add a `WmCore` query (e.g. `is_view_visible(&self, view_id: ViewId) -> bool` or a free function taking a `View`'s tags + an iterator of outputs' `current_tag`s, whichever fits the existing module's style) with tests covering: a view whose tag matches some output's current tag → `true`; a view whose tags match none → `false`; a view with zero tags → `true` (the bootstrap exception); an unknown `view_id` → `Err(WmCoreError::UnknownView)` or equivalent, not a panic. Confirm all fail (function doesn't exist yet).
  - [x] 1.2 GREEN — Implement. Pure, no I/O, mirrors the style of existing `wm_core` queries like `closable_focused_view`/`snapshot`.

- [x] **Task 2: Pinned-terminal ↔ tag association (AC: pinned terminal tagging/fullscreen/hide) — TDD for the pure parts**
  - [x] 2.1 **Deviation from the literal spec, documented here per the story's own "flag rather than silently work around" instruction:** `pending_pinned_terminal_tags: VecDeque<TagId>` was added to `WmCore` (not `main.rs`'s `WindowManager`), and the push happens inside `WmCore::claim_pinned_terminal_spawn` itself rather than at `ensure_pinned_terminal_spawned`'s call site. Reason: `claim_pinned_terminal_spawn` already has a **second real production caller** the story's Description didn't account for — `ipc::dispatch::handle_request`'s `SwitchTag` arm (Story 2.4), invoked on the IPC server's own per-connection thread, which never touches `main.rs`'s `WindowManager` at all (`ipc/server.rs`'s `handle_connection_inner` calls `crate::spawn_pinned_terminal` directly). A `WindowManager`-local queue would silently miss every picker-driven switch-mode first-spawn. Moving the queue into `WmCore` (the one state both threads share via `Arc<Mutex<WmCore>>`) and bundling the push into `claim_pinned_terminal_spawn`'s existing atomic claim (mirroring that method's own "bundle so no caller can forget a step" precedent) covers both call sites correctly with no cross-thread signaling. RED/GREEN done — see `wm_core::state::tests::claim_pinned_terminal_spawn_pushes_tag_onto_pending_queue_on_first_claim` and four sibling tests.
  - [x] 2.2 RED — Extracted as fully testable `wm_core` pure logic, not left as `main.rs` glue: `WmCore::pop_pending_pinned_terminal_tag` (the pop half) plus three supporting pure queries added for Tasks 4/5's own decisions — `WmCore::view_tags`, `WmCore::output_showing_tag`, `WmCore::output_current_tag` — all TDD'd (RED confirmed via compile failure in the devcontainer, then GREEN). Only the two-line "call `toggle_view_tag` with whatever `main.rs` popped" sequencing remains genuine Wayland-glue, verified via build+clippy+review.
  - [x] 2.3 `init_new_windows`'s `PINNED_TERM_APP_ID` branch pops via `wm_core.pop_pending_pinned_terminal_tag()` and calls `wm_core.toggle_view_tag(view_id, tag_id)` (log-and-continue via `log_wm_core_err`). An empty-queue pop (defensive, NFR2) logs `"Pinned terminal window mapped with no pending spawn tag queued; leaving untagged"` and does not panic.

- [x] **Task 3: Wire visibility recomputation into every tag-state-changing call site (AC: all)**
  - [x] 3.1 Chose the broad, simplest-correct-first-cut strategy explicitly sanctioned by this task: `WindowManager::recompute_window_visibility` runs unconditionally every render sequence (not narrowly triggered per call site), calling `.show()`/`.hide()` on every mapped window per `WmCore::is_view_visible`. This covers `toggle_view_tag` (assign-mode picker and Task 4's auto-tag), `switch_tag`/`cycle_tag` (keybind and switch-mode picker), and window registration/removal uniformly, since any of them is followed by a render sequence before the user can observe a stale state (IPC-driven picker mutations are followed by the picker's own window-close event, which triggers a fresh manage+render sequence — see the note on `WmCore::pending_pinned_terminal_tags` and the Dev Agent Record for the cross-thread reasoning).
  - [x] 3.2 `recompute_window_visibility` is called from `handle_render_start` (before `render_finish()`), not `handle_manage_start` — confirmed against the protocol XML directly: `hide`/`show` read "may only be made as part of a render sequence." No `manage_dirty` mechanism exists anywhere in this codebase (confirmed via grep) and none was introduced — Story 1.4/1.6's `set_position`/`place_top` precedent (calling rendering-state requests directly from whichever handler is convenient, relying on the general "manage or render" rendering-state clause) is reused as-is, not reinvented. Task 5's `fullscreen`/`exit_fullscreen` calls, by contrast, are protocol-restricted to manage sequences only (confirmed from the XML: "may only be made as part of a manage sequence") — this is *why* Tasks 3 and 5 are necessarily two separate recompute passes (`recompute_window_visibility` from `handle_render_start`, `recompute_pinned_terminal_fullscreen` from `handle_manage_start`) rather than one, despite Task 5.2's "don't build a second, separate mechanism" wording (that wording is satisfied at the strategy level — both passes share the same "recompute broadly, every sequence" approach — not at the literal single-function level, which the protocol's manage/render split makes impossible).

- [x] **Task 4: Auto-tag new (non-pinned) windows with the active output's current tag (AC: "new window" AC)**
  - [x] 4.1 `init_new_windows`'s non-pinned branch resolves `active_output_id()` (computed before the `wm_core` lock, same ordering precedent as `manage_seats`) then `wm_core.output_current_tag(output_id)`; if `Some`, calls `wm_core.toggle_view_tag(view_id, tag_id)` via `log_wm_core_err`. `None` (no output yet, or active output has no tag yet) leaves the window untagged, kept visible by Task 1's bootstrap exception.

- [x] **Task 5: Fullscreen the pinned terminal on its owning tag's current output, re-fullscreen on tag-output reassignment (AC: pinned terminal fullscreen ACs)**
  - [x] 5.1 After Task 2's tagging (still inside `init_new_windows`, i.e. still inside the manage sequence), resolves `wm_core.output_showing_tag(tag_id)`; if `Some(output_id)`, looks up the real `RiverOutputV1` proxy via the new free function `output_proxy_for_id(&self.outputs, output_id)` (scans `main.rs`'s `outputs: HashMap<ObjectId, Output>` by `Output::output_id`, since that map's keys are Wayland `ObjectId`s, not `wm-core` `OutputId`s) and calls `window.proxy.fullscreen(output_proxy)`. If `None`, no special-case hide call is made here — Task 3's `recompute_window_visibility` (next render sequence) hides it via the same `is_view_visible` decision every other view uses, avoiding a duplicate hide mechanism.
  - [x] 5.2 `WindowManager::recompute_pinned_terminal_fullscreen`, called unconditionally at the end of every `handle_manage_start` (after `manage_seats`, so a same-sequence keybind-driven `switch_tag`/`cycle_tag` is already reflected), re-resolves every mapped pinned terminal's tag (`wm_core.view_tags`) and re-applies `fullscreen(output_proxy)` if its tag is shown somewhere, or `exit_fullscreen()` if not — covering reassignment via keybind (same manage sequence) or via the IPC-driven picker (picked up the next manage sequence, bounded by the picker's own window-close event).
  - [x] 5.3 Build+clippy+structural review done (see Task 6). Confirmed unaffected: pinned terminal remains non-closable (`Action::Close`'s `is_pinned_terminal` guard, untouched), remains pinned to the bottom of stacking order (`lower_view`/`raise_view`'s existing pinned-terminal guards, untouched — only tagging/fullscreen logic was added inside the same `PINNED_TERM_APP_ID` branch), and floating windows still render above it (`window.node.place_top()` on non-pinned registration, untouched; node placement is a separate rendering-order mechanism from `show`/`hide`/`fullscreen`).

- [x] **Task 6: Full in-container verification gate + live confirmation note (AC: all)**
  - [x] 6.1 Ran via `devpod ssh buoy-wm` (worked for every command this session; the documented `podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>` fallback was not needed): `cargo fmt --all -- --check` (clean after one `cargo fmt --all` pass to fix two formatting diffs), `cargo clippy --workspace --all-targets -- -D warnings` (clean), `cargo test --workspace` (clean). Final counts: `buoy-wm` 173 (149 baseline + 24 new: 19 for `is_view_visible`/`view_tags`/`output_showing_tag`/`output_current_tag`, 5 for the `pending_pinned_terminal_tags` queue), `status-bar` 27, `tag-picker` 63 — 263 total, up from the 239 baseline, 0 failures.
  - [ ] 6.2 Deferred. This is a sandboxed dev-agent session with no access to a real `river` compositor session — live confirmation of (a) tag switching showing/hiding the right windows including a fullscreen pinned terminal, (b) `Mod4+Space` producing an immediately-visible window on a fresh tag, and (c) floating windows rendering above the fullscreen pinned terminal, must happen on the next real boot under `river`, per the story's own framing of this as "directly actionable next boot," not something this session can execute.

## Technical notes

**Why this wasn't caught during the original 12 stories.** Every story's
own acceptance criteria described *data-tracking* correctness (`WmCore`
holds the right state) without ever specifying an observable-on-screen
visibility requirement — FR2's "enforces at most one tag displayed per
output" was fully satisfiable (and was fully tested) at the `wm_core`
data layer alone. No story's AC ever said "and hide windows not on the
active tag," so none of the 12 stories' Definition of Done ever required
it. This is a genuine planning-artifact gap (the PRD/epics never spelled
out visibility as a distinct FR), not a story that was skipped or an
implementation shortcut.

**Scope boundary: per-output vs. global visibility.** A view is visible
if ANY of its tags matches ANY output's `current_tag` (standard
multi-monitor dwm semantics — a window tagged "web" is visible on
whichever output(s) currently show "web", including simultaneously on
two outputs if both happen to show tags it has). This story does not
attempt to make a view render differently per-output (e.g. "half-visible"
if only one of two outputs shows a matching tag) — full show/hide only.

**Not in scope:** `exit_fullscreen`-then-immediately-`fullscreen`-elsewhere
flicker/frame-perfection tuning if a tag's pinned terminal rapidly
bounces between outputs — correctness (it ends up fullscreen on the right
output) matters more than transition smoothness for this story. Revisit
only if it's visibly janky in practice.

## Test plan
1. **`wm_core` unit tests (Task 1)**: `is_view_visible`-style pure logic — visible-when-tag-matches, hidden-when-tags-exist-but-none-match, visible-when-untagged, unknown-view-id error path. Fully unit-testable, no I/O.
2. **Build/clippy gate**: `cargo build --workspace`/`cargo clippy --workspace --all-targets -- -D warnings` clean with all `main.rs` show/hide/fullscreen call sites compiled in.
3. **Regression gate**: existing 239-test suite (`wm` 149, `tag-picker` 63, `status-bar` 27) unaffected, plus Task 1's new tests.
4. **Live gate (manual, not automated)**: per Task 6.2 — tag switching actually hides/shows the right windows; new windows appear on the active tag immediately; the pinned terminal is fullscreen and stays under floating windows.

## FR coverage
FR1 (tag tracking), FR2 (one tag per output — now actually enforced visually, not just in data), FR4 (pinned terminal "always tiled... rendered at the bottom" — now actually fullscreen-sized). Closes a live-testing gap discovered after Story 2.6 shipped.

## Dev Agent Record

**What was done**:

- `wm_core` (protocol-agnostic, no Wayland types) gained five new pure,
  fully unit-tested methods on `WmCore` (`wm/src/wm_core/state.rs`):
  `is_view_visible` (Task 1's core visibility decision — a view is
  visible if any of its tags matches any output's `current_tag`, or if it
  has no tags at all, the bootstrap exception), `view_tags`,
  `output_showing_tag`, `output_current_tag` (supporting pure queries for
  Tasks 4/5), and `pop_pending_pinned_terminal_tag` (paired with a change
  to `claim_pinned_terminal_spawn`, described below).
- A `pending_pinned_terminal_tags: VecDeque<TagId>` field was added to
  `WmCore` itself, **not** to `main.rs`'s `WindowManager` as the story's
  Description literally suggested. Reason (a real design gap the story
  spec didn't anticipate, flagged rather than silently worked around):
  `claim_pinned_terminal_spawn` already has two real production callers —
  `main.rs`'s `ensure_pinned_terminal_spawned` (main Wayland-dispatch
  thread) and `ipc::dispatch::handle_request`'s `SwitchTag` arm (the IPC
  server's own per-connection thread, added in Story 2.4) — and the
  second one never touches `WindowManager` at all. A `WindowManager`-
  local queue would have silently missed every picker-driven switch-mode
  first-spawn. The push now happens inside `claim_pinned_terminal_spawn`
  itself (on its `Ok(Some(session_name))` path), so both callers correlate
  correctly through the one state they actually share
  (`Arc<Mutex<WmCore>>>`). This preserves the story's actual *intent* (a
  FIFO queue, pushed at spawn time, popped at map time) while fixing a
  cross-thread correctness gap in its literal wording.
- `main.rs`'s `init_new_windows` (Wayland glue, verified via build/
  clippy/review per the story's own carve-out, same as every prior
  registration-path story): the `PINNED_TERM_APP_ID` branch now pops via
  `wm_core.pop_pending_pinned_terminal_tag()`, tags the view
  (`toggle_view_tag`, log-and-continue on an empty pop per NFR2), and — if
  that tag is currently shown on some output — calls
  `window.proxy.fullscreen(output_proxy)` (Task 5.1). The non-pinned
  branch now auto-tags a freshly-registered window with the active
  output's current tag, if one exists (Task 4.1). The old, always-zero
  `window.set_position(window.x, window.y)`/`propose_dimensions(window.x,
  window.y)` calls for the pinned terminal (Story 1.5 scaffolding that was
  the literal cause of "it doesn't fullscreen," per this story's own
  Description) were removed — superseded by `fullscreen()`, which the
  protocol says takes over position/dimensions entirely.
- Two new manage/render-sequence recompute passes, both called
  unconditionally every relevant sequence (simplest-correct first cut,
  Task 3.1's own explicit sanction): `WindowManager::
  recompute_window_visibility` (Task 3), called from `handle_render_start`
  before `render_finish()`, issues `.show()`/`.hide()` per
  `is_view_visible` for every mapped window; `WindowManager::
  recompute_pinned_terminal_fullscreen` (Task 5.2), called from
  `handle_manage_start` after `manage_seats`, re-resolves and re-applies
  `fullscreen()`/`exit_fullscreen()` for every mapped pinned terminal.
  These are necessarily two separate functions, not one shared mechanism
  literally: the protocol XML says `hide`/`show` "may only be made as
  part of a render sequence" while `fullscreen`/`exit_fullscreen` "may
  only be made as part of a manage sequence" — confirmed by reading the
  actual request descriptions, not assumed. No `manage_dirty` request is
  used anywhere (grepped the codebase to confirm none exists yet) —
  Story 1.4/1.6's existing `set_position`/`place_top` precedent (call the
  rendering-state request directly from whichever handler already runs at
  the right time) was reused, not reinvented.
- A new free function `output_proxy_for_id(&HashMap<ObjectId, Output>,
  OutputId) -> Option<&RiverOutputV1>` resolves the real output proxy for
  a `wm-core` `OutputId` by scanning `main.rs`'s `outputs` map's values
  (keyed by `ObjectId`, not `OutputId`). It's a free function rather than
  a `WindowManager` method specifically so it can be called from inside a
  loop that already holds a disjoint mutable borrow of `self.windows`
  (same reasoning `active_output_id` already documents for why it must be
  computed *before* such a loop, not a method called from inside it).

**Verification** (all inside the devcontainer via `devpod ssh buoy-wm` —
worked reliably every time this session; the `podman exec -u vscode -w
/workspaces/buoy-wm bold_vaughan <cmd>` fallback was never needed):
`cargo build --workspace` clean; `cargo fmt --all -- --check` clean
(after one `cargo fmt --all` pass); `cargo clippy --workspace
--all-targets -- -D warnings` clean; `cargo test --workspace` — `buoy-wm`
173 (149 baseline + 24 new), `status-bar` 27, `tag-picker` 63 = 263 total,
0 failures, 0 regressions.

**Invariant regression check** (Stories 1.4/1.5/1.6, hard constraint):
confirmed by structural review, not live-tested — `Action::Close`'s
pinned-terminal exclusion (`main.rs`, checks the seat's own focused
window's `app_id`) is untouched; `WmCore::lower_view`/`raise_view`'s
pinned-terminal stacking-order guards are untouched (this story only adds
tagging/fullscreen calls inside the same `PINNED_TERM_APP_ID` branch,
after the existing `set_view_floating`/`lower_view` calls); floating
windows' `window.node.place_top()` call on non-pinned registration is
untouched, and node placement (render list ordering) is a protocol-level
mechanism entirely separate from `show`/`hide`/`fullscreen` (rendering
state vs. window management state per the protocol's own two-category
split), so fullscreen-ing the pinned terminal cannot affect float-above-
pinned ordering.

**Task 6.2 (manual live-verification)**: deferred — this is a sandboxed
dev-agent session with no access to a real `river` compositor session, so
none of (a) tag-switch show/hide correctness including fullscreen, (b)
`Mod4+Space` immediate visibility on a fresh tag, or (c) floats rendering
above the fullscreen pinned terminal could be exercised live. All three
are directly actionable on the next real boot of `buoy-wm` under `river`.

**Self-review note**: no separate reviewer sub-agent was used; implemented
directly with RED-before-GREEN TDD for every pure `wm_core` addition
(Task 1's `is_view_visible` and its four supporting queries, plus the
`pending_pinned_terminal_tags` push/pop pair), and build+clippy+structural
self-review for the Wayland-glue `main.rs` changes, per the story's own
carve-out (same pattern as every prior `main.rs` registration/dispatch
story). The one place this implementation deviates from the story's
literal wording (Task 2.1's queue location) is called out explicitly
above and in the Task 2.1 checklist entry, per the top-level instruction
to flag rather than silently work around a design gap the story spec
didn't anticipate.

**File List**:
- `wm/src/wm_core/state.rs` — added `is_view_visible`, `view_tags`,
  `output_showing_tag`, `output_current_tag`, `pop_pending_pinned_terminal_tag`;
  added `pending_pinned_terminal_tags` field and wired its push into
  `claim_pinned_terminal_spawn`; 24 new unit tests.
- `wm/src/main.rs` — `init_new_windows` reworked (pinned-terminal tag
  pop/associate/fullscreen, non-pinned auto-tag); added
  `recompute_window_visibility` (called from `handle_render_start`) and
  `recompute_pinned_terminal_fullscreen` (called from
  `handle_manage_start`); added free function `output_proxy_for_id`.
- `docs/planning/epics/story-2-7.md` — this file (task checkboxes, Dev
  Agent Record).

## Code Review

HIGH-effort review, prioritized on threading/locking, FIFO correlation
under the two-caller reality, and manage-vs-render sequence correctness
per this story's own flagged risk areas. Three CONFIRMED correctness
findings:

1. **Stale, invisible-window focus after a tag switch (`main.rs`'s
   `focus_top`).** `windows.back()` carries no tag/visibility filtering of
   its own, and nothing in `cycle_tag`/`switch_tag` reorders `self.windows`
   or reassigns focus. `focus_top` ran unconditionally every manage
   sequence and could re-affirm Wayland keyboard focus + `wm_core::
   focused_view` on a window `recompute_window_visibility` hides on the
   very next render sequence — the user's keypresses would go nowhere
   until they clicked something. **Fixed**: `focus_top` now filters
   `windows.back()` through `is_view_visible` first; a hidden back-of-
   stack window is treated the same as no window (focus cleared) instead
   of wrongly re-affirmed. `wm_core::cycle_focus` (the `Mod4+N`/
   `Action::FocusNext` path) had the identical unfiltered defect — fixed
   the same way, with two new regression tests
   (`cycle_focus_skips_views_not_visible_on_any_output_current_tag`,
   `cycle_focus_returns_none_when_only_candidate_is_hidden`). Not a
   literal item this story's Description called out, but the same root
   cause newly exposed by this story's own visibility feature, on the
   exact "rotate windows" keybind live-testing surfaced as a real question
   — fixing both call sites rather than just the one candidate flagged.
2. **Untagged pinned terminal left with no geometry at all
   (`init_new_windows`'s `None` arm on an empty pending-tag queue).** The
   window is shown via `is_view_visible`'s bootstrap exception but never
   given `set_position`/`propose_dimensions`/`fullscreen` — worse than
   pre-diff, which at least zeroed position. Narrow trigger (only reachable
   by a client independently mapping `app_id == PINNED_TERM_APP_ID`
   outside this WM's own spawn-tracking, or Finding 3's race draining the
   queue out of sync). **Fixed**: give it the same default floating
   geometry every other new window gets, so it renders sanely rather than
   with undefined dimensions.
3. **FIFO spawn↔window-map correlation race across two threads**
   (`pending_pinned_terminal_tags`, pushed from both the keybind path and
   the IPC `SwitchTag` path). Confirmed real and reachable — two
   independently-scheduled `foot`/`zellij` processes have no guaranteed
   map-back ordering matching push order — but this is exactly the
   mechanism Task 2.1 already documented and explicitly accepted as a
   scope-limited simplification ("concurrent rapid-fire spawns racing this
   queue is not a realistic scenario here"). Left as-is; no code change.

Non-blocking efficiency/reuse notes (not fixed, flagged for awareness):
`recompute_pinned_terminal_fullscreen`/`recompute_window_visibility`
unconditionally re-issue `fullscreen`/`exit_fullscreen`/`hide`/`show`
every sequence rather than only on actual state change (each also
independently re-locks `wm_core`); `output_showing_tag`/`view_tags`
duplicate predicate/filter logic already inline in `switch_tag`/
`snapshot`. Both are pre-existing-pattern tradeoffs the story's own Task
3.1 explicitly sanctioned ("simplest-correct first cut"), not
regressions.

**Re-verification after fixes**: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `pre-commit
run --all-files` all clean; `cargo test --workspace` — `buoy-wm` 175
(173 + 2 new regression tests), `status-bar` 27, `tag-picker` 63 = 265
total, 0 failures.

**Code review: PASS** (after the two fixes above; Finding 3 accepted as
documented scope, matching Task 2.1's own risk note).
