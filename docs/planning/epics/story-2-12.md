---
baseline_commit: a139a09
---

# Story 2.12: Move focus to the new tag instead of stranding it on a hidden window

Epic: 2 | Priority: H | Status: done

## Description
Live testing reported, verbatim: *"when switching tag views, focus remains
on the last window with focus, even if it is no longer in view."*
Keypresses after a tag switch went to a window the user could no longer
see — typing into an invisible editor, `Mod4+Q` closing something off
screen — with no way back except clicking a window on the new tag.

Expected behaviour, confirmed with the user:
1. If the focused window is **still visible** after the switch (it carries
   the newly-shown tag too — tags are a many-to-many membership, not an
   exclusive assignment), focus stays exactly where it is. A switch that
   doesn't hide your window shouldn't move your cursor.
2. Otherwise focus moves to the **new tag** — specifically to the topmost
   window that is actually visible there.
3. If the new tag has no ordinary windows at all, focus falls back to
   **that tag's background pinned terminal**, which is always present
   (Story 1.5/1.7) and is the natural "you are here, start typing" target.

Diagnosis found **three independent root causes**. Each one alone was
sufficient to reproduce the symptom, and all three had to be fixed before
the switch actually landed — which is why the earlier Story 2.7
code-review follow-up, which addressed a real part of this, did not
resolve the user-visible behaviour.

**Cause 1 — `focus_top` tested exactly one candidate.** It looked at
`windows.back()` (the most-recently-interacted-with window WM-wide) and
filtered *that single window* through `is_view_visible`. The Story 2.7
code-review fix had correctly stopped the **wrong** window from keeping
focus — before it, a hidden `windows.back()` was re-affirmed as focused —
but it never added a fall-through. A hidden top candidate took the `None`
arm and called `clear_focus()`, dropping focus entirely rather than
passing it to something the user could actually see. So the bug's
observable shape after Story 2.7 was "focus goes nowhere", and before it
"focus stays on the hidden window"; neither is the required "focus moves
to the new tag".

**Cause 2 — the `terminal_intentionally_focused` latch suppressed
`focus_top` outright.** That latch was added by an earlier code-review
follow-up to stop `focus_top` from stealing focus back off a pinned
terminal the user had *deliberately clicked* (`windows.back()` still
pointed at whatever was focused before the click, so the next manage pass
silently undid it). It works by skipping the `focus_top()` call entirely
while set. But the latch was unscoped in time: switching tags away from a
deliberately-clicked pinned terminal hid that terminal without touching
focus, and the still-set latch then suppressed the very call that would
have repaired it — **permanently**, for every subsequent pass. Cause 1's
fix is invisible in this state, because `focus_top` is never reached.

**Cause 3 — ordering inside `manage_seats`.** `do_action` (which performs
`Action::CycleTag`, the keybind path that switches tags) runs **after**
the focus block. A tag-switching bind therefore hides the focused window a
step too late for that pass's `focus_top` to have noticed it, and nothing
schedules another pass: the main loop is a `blocking_dispatch`, so `wm`
sits idle until some unrelated compositor event arrives. Focus would stay
on the outgoing tag's window for an arbitrarily long time — in practice
until the user's next keypress, which is exactly when they notice it went
to the wrong place. The IPC/picker-driven switch path
(`wm/src/ipc/dispatch.rs`'s `Request::SwitchTag`) is worse still: it
mutates `wm_core` from the IPC thread with no focus handling of any kind,
so `Mod4+S`'s switch had never once moved focus.

## Fix
`WmCore::topmost_visible_view()` — a new pure query — replaces
`focus_top`'s single-candidate test with a back-to-front scan of
`stacking_order` for the highest view `is_view_visible` accepts.
`focus_top` resolves that view id to its `Window` and focuses it; `None`
still clears focus, but now only when *nothing at all* is visible rather
than merely when the top candidate isn't. The latch is expired the moment
its focused window stops being visible (new `Seat::focused_view_is_visible`
helper), and a tag change across `do_action` is detected after the fact by
comparing the active output's current tag before and after, then repaired
by re-running `focus_top` in the same pass.

**Why `wm_core.stacking_order` is the order scanned, not `self.windows`.**
The two deques are maintained by different mechanisms and genuinely
disagree. `self.windows` is only ever *appended to* (on map) and
*reordered on interaction* (click-to-focus, `FocusNext`) — it is a
recency-of-interaction list, not a z-order. A pinned terminal that maps
after a floating window therefore sits **behind** that window in
`self.windows`, which is precisely backwards for a fallback scan. By
contrast, `stacking_order` is the order `lower_view`/`raise_view` actively
maintain, with the pinned terminal pinned at the front (bottom) by
`lower_view` and held there by `raise_view`'s no-op guard (Story 1.5,
FR4). A back-to-front scan of `stacking_order` therefore reaches ordinary
windows first and the pinned terminal last — exactly the desired fallback
ordering, for free, with no special-casing in the scan itself.

**FR4 consequence.** Because the scan can now legitimately return the
pinned terminal, `focus_top` had to gain the split `manage_seats`'
click-to-focus path already makes: the pinned terminal may be **focused**
but must **never be raised**, so `place_top()` is now skipped when
`window.app_id == PINNED_TERM_APP_ID`. This case simply could not arise
before — `focus_top` only ever saw `windows.back()`, which the pinned
terminal is deliberately never pushed to.

**Two `.expect()` calls retired.** The old `focus_top` unwrapped
`window.view_id` twice with an `expect` asserting that `init_new_windows`
had registered it earlier in the same `handle_manage_start` call. The
rewrite takes the `ViewId` directly from the stacking order, so both are
gone; the `Window` lookup uses `and_then`, degrading to the same "nothing
to focus" clear as an empty stacking order rather than panicking (NFR2).

## Acceptance criteria
**Given** a focused window that carries the newly-shown tag as well as the
outgoing one
**When** the tag is switched
**Then** that window is still visible, `topmost_visible_view` still
resolves to it, and focus does not move — the switch is a no-op for focus

**Given** a focused window that does **not** carry the newly-shown tag
**When** the tag is switched
**Then** focus moves to the topmost view that *is* visible on the new tag,
and Wayland focus, `Seat::focused`, and `wm_core::focused_view` all agree
on it

**Given** the newly-shown tag has no ordinary windows on it — only its
background pinned terminal
**When** the tag is switched
**Then** focus lands on **that tag's** pinned terminal (reached last by
the back-to-front scan, because `lower_view` keeps it at the front/bottom
of `stacking_order`), and the terminal is focused **without** being raised
— `place_top()` is not called for it, preserving FR4's bottom-of-render-
order invariant

**Given** a pinned terminal belonging to some *other*, not-currently-shown
tag
**When** `topmost_visible_view` scans
**Then** it is not picked either — every candidate goes through the same
`is_view_visible` gate, so "no view is visible anywhere" resolves to
`None` and focus is cleared, rather than silently resolving to an off-tag
terminal

**Given** `terminal_intentionally_focused` is set (the user deliberately
clicked the pinned terminal) and the tag is then switched away from it
**When** the next manage pass runs
**Then** the latch is expired because its focused window is no longer
visible, `focus_top` is allowed to run, and focus is repaired — instead of
being suppressed indefinitely and parked on a terminal the user can no
longer see

**Given** the tag switch is driven by a **keybind** (`Action::CycleTag`,
handled inside `do_action`, which runs after the focus block)
**When** that same `manage_seats` pass completes
**Then** the change is detected by the active output's current tag
differing before and after `do_action`, and focus is repaired **in the
same pass** — not left until some unrelated compositor event happens to
wake the `blocking_dispatch` main loop

- [x] Tests pass (unit + integration where applicable)

## Tasks / Subtasks

- [x] **Task 1: `WmCore::topmost_visible_view` — the pure query (AC 1, 2, 3, 4)**
  - [x] 1.1 RED — Add five tests to `wm/src/wm_core/state.rs`'s test module, beside the existing `cycle_focus` tests (they share the same stacking-order/visibility fixtures and read as a contrasting pair):
    - `topmost_visible_view_returns_none_for_an_empty_core`
    - `topmost_visible_view_returns_the_back_of_stacking_order_when_visible`
    - `topmost_visible_view_skips_views_hidden_on_another_tag` (the tag-switch case this query exists for)
    - `topmost_visible_view_falls_back_to_the_visible_pinned_terminal`
    - `topmost_visible_view_returns_none_when_every_view_is_hidden` (an off-tag pinned terminal must not be picked)

    Confirm all five fail to compile — `topmost_visible_view` doesn't exist yet.
  - [x] 1.2 GREEN — Implement `pub fn topmost_visible_view(&self) -> Option<ViewId>` on `WmCore`: `self.stacking_order.iter().rev().copied().find(|&id| self.is_view_visible(id).unwrap_or(false))`. `///` doc comment covering: why back-to-front (a floating window on the newly-shown tag outranks that tag's pinned terminal, which `lower_view` keeps at the front); why — unlike `cycle_focus`, which deliberately *skips* the pinned terminal so repeated `FocusNext` presses round-robin the real windows — the pinned terminal is a **legitimate** result here, being the whole point of the fallback; and why `unwrap_or(false)` is safe-by-construction rather than a silently-wrong swallow (an id in `stacking_order` always has a matching entry in `views`, so `is_view_visible`'s `UnknownView` arm is unreachable — the same precedent `cycle_focus` already sets for the same call, NFR2). Pure query, never mutates `self`. Confirm all five GREEN.

- [x] **Task 2: Rewrite `Seat::focus_top` around the new query (AC 1, 2, 3, 4)**
  - [x] 2.1 Replace the `windows.back().filter(...)` single-candidate test with `wm_core.topmost_visible_view().and_then(|view_id| windows.iter().find(|w| w.view_id == Some(view_id)).map(|w| (view_id, w)))`. The `Some` arm now destructures `(view_id, window)`, so the two `.expect("every window reaches focus_top only after init_new_windows registered it…")` unwraps are deleted — `view_id` comes from the stacking order directly.
  - [x] 2.2 Guard `window.node.place_top()` behind `window.app_id != PINNED_TERM_APP_ID` (FR4) — mirrors the split `manage_seats`' click-to-focus path already makes. Comment why this case is *newly* reachable at all.
  - [x] 2.3 Rewrite the function's leading comment block: keep the Story 2.7 history (why `windows.back()` needed visibility filtering in the first place), then explain that filtering alone only got as far as *dropping* focus, and record why `stacking_order` — not `self.windows` — is the scanned order.
  - [x] 2.4 No RED/GREEN — `focus_top` issues real `river_seat_v1`/`river_node_v1` proxy calls and holds a live `WmCore` borrow from `manage_seats`; untestable without a compositor, the project's standing build + `clippy -D warnings` + manual structural-review carve-out for Wayland-proxy code (same as Stories 1.7, 2.6, 2.8, 2.9, 2.10 Task 2.2/3.2/5.5). Review specifically for: the `None` arm still calls `clear_focus()` unchanged; `set_focus`'s `Err` is still logged, not swallowed; the pinned-terminal branch still performs `focus_window` + `set_focus` and skips only `place_top`.

- [x] **Task 3: `Seat::focused_view_is_visible` — the latch's expiry predicate (AC 5)**
  - [x] 3.1 Add `fn focused_view_is_visible(&self, windows: &VecDeque<Window>, wm_core: &WmCore) -> bool`: `None` focus → `false`; focus on a window no longer in `windows` → `false`; focus on a window whose `view_id` is `None` or whose `is_view_visible` returns `Err` → `false`; otherwise the visibility answer. Every "can't tell" case answers `false`, which *releases* the latch — the safe direction, since a released latch only means `focus_top` gets to run and re-decide.
  - [x] 3.2 `///` doc comment stating its one caller and purpose: drives `terminal_intentionally_focused`'s expiry, because a latch that outlives its window suppresses the very `focus_top` call that would repair focus.
  - [x] 3.3 No RED/GREEN — takes `&VecDeque<Window>` (Wayland proxies, not constructible in a unit test), same carve-out as Task 2.4.

- [x] **Task 4: Expire the latch in `manage_seats` (AC 5)**
  - [x] 4.1 Immediately before the existing `if any_new_windows || !seat.terminal_intentionally_focused` block, add: `if seat.terminal_intentionally_focused && !seat.focused_view_is_visible(&self.windows, wm_core) { seat.terminal_intentionally_focused = false; }`. Placed *before*, not inside, so the existing condition is evaluated against the already-expired latch and `focus_top` runs on this same pass rather than the next one.
  - [x] 4.2 Extend `terminal_intentionally_focused`'s own doc comment (on `struct Seat`) to state the new scoping rule: the latch is scoped to its terminal's *visibility*, and the tag-switch-away-from-a-clicked-terminal case is exactly why.
  - [x] 4.3 Extend the existing comment above the focus block to explain that this is the half `focus_top`'s own visibility scan **cannot** fix on its own, because it never gets called.
  - [x] 4.4 No RED/GREEN, same carve-out as Task 2.4.

- [x] **Task 5: Repair focus after a `do_action`-driven tag change (AC 6)**
  - [x] 5.1 Capture `let tag_before = active_output_id.and_then(|id| wm_core.output_current_tag(id));` immediately before the `seat.do_action(...)` call, and `tag_after` with the identical expression immediately after it. `output_current_tag` already exists (Story 2.7) and is a pure lookup — no new `wm_core` API needed for this half.
  - [x] 5.2 If `tag_after != tag_before`: clear `seat.terminal_intentionally_focused` (a deliberate terminal focus does not survive the tag it was made on — the same expiry rule as Task 4.1, applied eagerly because the visibility recompute for the new tag hasn't happened yet at this point in the pass) and call `seat.focus_top(&self.windows, wm_core)`.
  - [x] 5.3 Comment the ordering constraint in full: why `do_action` runs after the focus block, why reordering them is not an option, and why nothing else will schedule a second pass (`blocking_dispatch`).
  - [x] 5.4 No RED/GREEN, same carve-out as Task 2.4. Review specifically that `tag_before`/`tag_after` are computed with the *same* expression against the *same* `active_output_id`, so the comparison can only report a real change, and that the repair is inside the per-seat loop (each seat has its own focus to repair).

- [x] **Task 6: Correct the stale comment in the `Action::FocusNext` arm**
  - [x] 6.1 The comment claimed `focus_top` "issues the real focus_window/place_top proxy calls against `windows.back()`, which is now guaranteed to be the same window `cycle_focus` just chose" — no longer true after Task 2. Reworded to: `focus_top` acts against the top of `wm_core`'s stacking order, which `cycle_focus`'s own `raise_view` just set to this same window, and the arm's `windows.remove`/`push_back` reorder keeps `windows` agreeing with it. **No behaviour change** — `FocusNext` deliberately keeps `windows` and `stacking_order` in sync (Story 1.4 code-review follow-up), which is exactly why it keeps working unmodified under the new scan.

- [x] **Task 7: Full in-container verification gate (AC: all)**
  - [x] 7.1 Inside the devcontainer: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pre-commit run --all-files`. Confirm the 272-test baseline plus Task 1's 5 new tests all pass, with no regressions in the existing `cycle_focus`/`is_view_visible`/`lower_view`/`raise_view` suites (this story adds a query beside them and must not perturb them).
  - [x] 7.2 Manual live-verification note (not automated, not a story blocker — same boundary as every prior story): confirm against a real `river` session that switching tags moves focus to the new tag, that a still-visible focused window keeps focus, and that an empty new tag lands focus on its pinned terminal without raising it. Record in the Dev Agent Record whether this was confirmed live or deferred.

## Technical notes

**Why reordering `focus_top` and `do_action` was not an option.**
The obvious "fix" for Cause 3 is to run `do_action` first and the focus
block second, so a tag switch is already applied by the time focus is
resolved. That breaks a load-bearing invariant: `do_action` **reads the
focus this pass's `focus_top` just resolved**. That read is what makes
click-then-`Mod4+Q` close the window you actually clicked, and what makes
`Action::FocusNext` operate from the correct starting point. Swapping the
two would make every action operate on the *previous* pass's focus — a
far worse and much subtler regression than the one being fixed. Detecting
the tag change **after the fact**, by comparing the active output's
current tag across the `do_action` call and re-running `focus_top` when it
differs, gets the same end state without disturbing that ordering, and
costs one cheap pure lookup per seat per pass.

**Why the tag change is detected by comparison rather than reported by
`do_action`.** `do_action` already has a return channel — it returns
`Option<TagId>` for pending pinned-terminal spawns — but that channel is
semantically "this tag needs its terminal spawned", populated on both the
switch path *and* the create path, and consumed after the seat loop under
a dropped lock. Overloading it, or adding a second return value, would
couple focus repair to the spawn bookkeeping. The before/after comparison
is independent of *how* the tag changed, so it also covers any future
action that switches tags without going through the same return path.

**What this story does not fix: the IPC path.**
`wm/src/ipc/dispatch.rs`'s `Request::SwitchTag` mutates `wm_core` from the
IPC thread and never touches seat focus at all — there is no `Seat` and no
Wayland proxy reachable from there. In practice the picker-driven switch
still ends up repaired, because dismissing `fuzzel` and the resulting
window teardown wake the main loop into a manage pass, at which point
Cause 1's and Cause 2's fixes (both of which live on the *pass* side, not
the keybind side) resolve focus correctly. The keybind path needed Cause
3's explicit repair precisely because it can complete a switch **without**
any such follow-up event. A first-class "wake the main loop after an IPC
mutation" mechanism is out of scope here (YAGNI) and would be the right
shape only if a switch is ever observed leaving focus stale in practice.

**Why the latch is cleared in two places rather than one.** Task 4.1's
check reads *actual* visibility, which is correct for every pass where the
tag change already happened. Task 5.2's clear is unconditional on the tag
having changed, because at that point in the pass the visibility recompute
for the new tag has not run yet, so a visibility query would still answer
about the *old* tag. The two are the same rule ("a deliberate terminal
focus does not survive its window becoming invisible") applied at the two
points in the pass where that can be known — not redundant, and not a
DRY violation worth extracting at two occurrences (three-strike rule).

**Scope boundary.** No change to `cycle_focus`'s behaviour, to
`is_view_visible`, to `lower_view`/`raise_view`, or to the pinned
terminal's spawn/geometry lifecycle. No change to what "visible" means —
this story only changes *which* views are consulted when focus needs a new
home. No new IPC request or response, no `wire.rs` change, no
`tag-picker`/`status-bar` change of any kind.

## Test plan
1. **`wm_core` unit tests (TDD, real RED before GREEN)**: `topmost_visible_view`'s five cases (Task 1.1) — empty core, top-of-order-when-visible, skip-hidden-on-another-tag, fall-back-to-the-pinned-terminal, and none-when-everything-is-hidden (including an off-tag pinned terminal). Fully testable: `WmCore` is pure state with no Wayland types.
2. **Regression gate on the neighbouring queries**: the existing `cycle_focus` suite (including `cycle_focus_skips_pinned_terminal_and_round_robins_through_others` and `cycle_focus_skips_views_not_visible_on_any_output_current_tag`) must stay green untouched — the two queries deliberately disagree about the pinned terminal, and that contrast is the point.
3. **Build/lint gate**: `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` clean with the rewritten `focus_top`, the new `focused_view_is_visible`, and both `manage_seats` blocks compiled in.
4. **Structural review gate** (the `main.rs` half, per the Wayland-proxy carve-out): `focus_top`'s `None` arm unchanged; `place_top` skipped only for `PINNED_TERM_APP_ID`; the latch expiry placed before the existing focus condition; `tag_before`/`tag_after` computed identically; the repair inside the per-seat loop.
5. **Live gate (manual, not automated)**: per Task 7.2 — the gate that reported this bug in the first place, and the only one that can confirm the three causes are jointly resolved rather than individually plausible.

## FR coverage
FR2 (at most one tag displayed per output) and FR4 (pinned terminal always
rendered at the bottom) — both already implemented, both newly relevant:
FR2's visibility filtering is what strands focus in the first place, and
FR4 is what forces `focus_top`'s new no-`place_top` branch now that the
pinned terminal is a reachable focus target. Not a new FR; this closes a
focus-management defect in the tag-switching behaviour Stories 2.7-2.11
built out, which the PRD's acceptance criteria implicitly assumed worked.

## Dev Agent Record

**What was done**:

- `wm/src/wm_core/state.rs` (Task 1, real RED/GREEN TDD): added the five
  `topmost_visible_view_*` tests beside the existing `cycle_focus` tests,
  confirmed all five failed to compile in-container with `E0599: no method
  named topmost_visible_view found for struct WmCore`, then implemented
  `pub fn topmost_visible_view(&self) -> Option<ViewId>` as a reversed
  `stacking_order` scan filtered by `is_view_visible(id).unwrap_or(false)`,
  and confirmed all five GREEN. The doc comment records the three
  non-obvious points: back-to-front ordering is what makes a floating
  window outrank the tag's pinned terminal; the pinned terminal is a
  *legitimate* result here in explicit contrast to `cycle_focus`, which
  skips it; and `unwrap_or(false)` is safe-by-construction (an id in
  `stacking_order` always has a `views` entry) rather than a silent
  swallow, matching `cycle_focus`'s precedent.
- `wm/src/main.rs` (Task 2): `Seat::focus_top` rewritten — target
  resolution is now `wm_core.topmost_visible_view()` piped through an
  `and_then` `Window` lookup yielding `(view_id, window)`. Both
  `.expect("every window reaches focus_top only after init_new_windows
  registered it earlier in the same handle_manage_start call")` calls are
  gone; the `and_then` degrades a missing `Window` to the same "clear
  focus" path as an empty stacking order (NFR2). `place_top()` is now
  guarded by `window.app_id != PINNED_TERM_APP_ID` (FR4). The `None` arm,
  the `clear_focus()` call, and the `set_focus` error logging are
  unchanged; only `set_focus`'s justifying comment was reworded, since the
  `view_id` now provably comes from the same `WmCore`'s stacking order
  rather than from `init_new_windows`' registration.
- `wm/src/main.rs` (Task 3): new `Seat::focused_view_is_visible(&self,
  &VecDeque<Window>, &WmCore) -> bool` — a `let ... else` early `false` on
  no focus, then `find` the window by proxy equality, `and_then` its
  `view_id`, `and_then` `is_view_visible(...).ok()`, `unwrap_or(false)`.
  Every uncertain case answers `false`, which releases the latch (the safe
  direction — a released latch only lets `focus_top` re-decide).
- `wm/src/main.rs` (Task 4): `manage_seats` expires
  `terminal_intentionally_focused` immediately before the existing
  `any_new_windows || !terminal_intentionally_focused` condition, so the
  repair happens on the same pass. `Seat::terminal_intentionally_focused`'s
  doc comment gained a paragraph scoping the latch to its terminal's
  visibility, and the focus block's own comment gained the explanation
  that this is the half `focus_top`'s visibility scan cannot fix alone,
  since it never gets called.
- `wm/src/main.rs` (Task 5): `tag_before`/`tag_after` snapshots of
  `active_output_id.and_then(|id| wm_core.output_current_tag(id))` bracket
  the `seat.do_action(...)` call; on inequality the latch is cleared and
  `seat.focus_top(&self.windows, wm_core)` re-runs, inside the same
  per-seat iteration. The block carries the full ordering rationale
  (why `do_action` must stay after the focus block; why nothing schedules
  a second pass under `blocking_dispatch`).
- `wm/src/main.rs` (Task 6): corrected the now-stale `Action::FocusNext`
  comment — `focus_top` no longer acts against `windows.back()`. No
  behaviour change; `FocusNext` deliberately keeps `windows` and
  `stacking_order` in sync, which is exactly why it needed no code change.

**Deviations from the story's literal wording**: none identified. The
implementation matches Tasks 1-7 as written, including the exact method
signature, the five test names, and the placement of both `manage_seats`
blocks.

**Verification** (all inside the devcontainer via `podman exec -u vscode
-w /workspaces/buoy-wm bold_vaughan <cmd>`):
- `cargo fmt --all -- --check`: clean, no drift.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace`: `buoy-wm` (wm) 181 (176 baseline + 5 new
  `topmost_visible_view` tests), `status-bar` 27 (unaffected),
  `tag-picker` 69 (unaffected — this story is entirely `wm`-local) =
  **277 total, 0 failures, 0 regressions** (272 baseline + 5).
- `pre-commit run --all-files`: both configured hooks (`cargo fmt
  --check`, `cargo clippy`) passed.

**Task 7.2 (manual live-verification)**: **deferred**, not performed by
this agent — no `river` session, no real seat, and no physical output in
this sandbox, the same boundary as every prior story's live-compositor gap
(most recently Stories 2.10 and 2.11). The originating report was found by
live testing, and confirming the three causes are *jointly* resolved —
still-visible window keeps focus, hidden one hands off to the new tag,
empty tag falls back to its pinned terminal without raising it, and a
keybind switch repairs within the same pass — still needs a human at a
real session before this story's symptom can be considered closed
end-to-end.

**File List**:
- `wm/src/wm_core/state.rs`
- `wm/src/main.rs`

## Code Review

HIGH-effort workflow-backed review (37 agents: per-angle finders, then an
independent verifier per finding location). One finding against this
story, fixed:

1. **The focus repair was WM-wide, not scoped to the output whose tag
   changed.** `WmCore::topmost_visible_view` answers "is this view visible
   *anywhere*", because `is_view_visible` is true when **any** registered
   output shows one of a view's tags. On a multi-output session that is the
   wrong question for this story's repair path: with output 1 showing
   "web" and output 2 showing "email", pressing `Mod4+Tab` with the pointer
   on output 1 could resolve to a window on output 2 — still visible, and
   at the back of `stacking_order` if it was clicked most recently — and
   hand it keyboard focus plus a `place_top()`. The user watches output 1
   while their keystrokes go to the other monitor. **Fixed**: added
   `WmCore::topmost_visible_view_on(output_id)`, which resolves that
   output's `current_tag` and scans `stacking_order` back-to-front for a
   view carrying it; `Seat::focus_top` gained a `scope: Option<OutputId>`
   parameter. The tag-change repair passes the output it just switched;
   the routine every-pass call and `Action::FocusNext`'s re-affirmation
   both pass `None`, where any visible window is a legitimate target and
   `cycle_focus` has already chosen. Three new unit tests cover
   cross-output isolation (asserting the unscoped and scoped queries
   genuinely disagree), an output showing no tag, an unregistered output
   id, and that the pinned-terminal last-resort fallback survives the
   scoping.

**Re-verification after the fix**: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `pre-commit run
--all-files` all clean; `cargo test --workspace` — 322 total (`buoy-wm`
226, `status-bar` 27, `tag-picker` 69), 0 failures.

**Code review: PASS**.
