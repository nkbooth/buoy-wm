---
baseline_commit: e7eede75c1816ce72add5e8ef9c4f76bcb2ea6fd
---

# Story 1.6: Floating Placement For Non-Pinned Windows
Epic: 1 | Priority: H | Status: done

## Description
Every window except the pinned terminal floats above it, so active work is
always visible over the terminal backdrop. `WmCore::register_view` (Story
1.2) already defaults every newly registered view to `floating: true`, and
`init_new_windows` (Story 1.5) never overrides that default for anything
except the pinned terminal — so the "defaults to floating" half of FR5 is
already fully satisfied by existing, tested code, with nothing new for this
story to do there (see Technical notes, "What's already done").

What is genuinely missing, confirmed by reading current `main.rs` and
`wm_core::state` before drafting this story:
- **Geometry.** `WmCore::set_view_geometry` (Story 1.2) exists, is unit
  tested, and its own doc comment says outright: *"Not yet wired into
  `main.rs` — floating placement/geometry lands in Story 1.6"*
  (`wm/src/wm_core/state.rs:162-163`). No call site anywhere calls it today.
  Every newly mapped `Window` in `main.rs` starts with `x: 0, y: 0, width:
  0, height: 0` (`Window::new`, `wm/src/main.rs:434-437`), and
  `init_new_windows` immediately calls `window.set_position(window.x,
  window.y)` / `window.proxy.propose_dimensions(window.width,
  window.height)` using those same untouched zero defaults
  (`wm/src/main.rs:275-276`) — i.e. every window, pinned or not, is
  currently placed at `(0, 0)` and told to self-size (per the protocol,
  `propose_dimensions(0, 0)` means "the window may decide its own
  dimensions" — not an error, but not a real placement decision either).
  Non-pinned windows get no distinct treatment from this at all.
- **Live render-order at creation time.** `wm_core`'s own
  `stacking_order` bookkeeping (Story 1.5) is internal state, not
  automatically reflected in the compositor's actual render list. The
  `river_node_v1` protocol interface says outright: *"The initial position
  of a node in the render list is undefined, the window manager client must
  use the place_above or place_below request to guarantee a specific
  rendering order"* (`wm/protocol/river-window-management-v1.xml:1231-1233`).
  Today, `place_top()` is the only such request ever made anywhere in
  `main.rs`, and only from `Seat::focus_top` (`wm/src/main.rs:683`) — i.e.
  only once a window is first focused. A brand-new, not-yet-focused
  non-pinned window has protocol-undefined real z-order relative to the
  pinned terminal until something focuses it. This is a real, citable gap
  against FR5's literal "rendered above the pinned terminal," not merely a
  cosmetic one (see Technical notes, "Render-order gap, closed").

## Acceptance criteria
**Given** a newly mapped view whose app-id is not `pinned-term`
**When** the WM places it
**Then** it defaults to floating and renders above the pinned terminal
**And** this holds regardless of which tag or output it's mapped on

- [ ] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Technical notes

**What's already done — not this story's job to redo.** `WmCore::register_view`
defaults every view (pinned or not) to `floating: true`
(`wm/src/wm_core/state.rs:105-121`), already locked in by Story 1.2's
existing `newly_registered_view_has_expected_defaults` test
(`wm/src/wm_core/state.rs:454-467`, asserts `view.floating` truthy).
`init_new_windows` (Story 1.5) only ever sets `floating` to `false` inside
its `app_id == PINNED_TERM_APP_ID` branch; it never touches `floating` for
anything else. So "any non-pinned-term window defaults to floating" is
fully satisfied today, for every `app_id` including ones that don't exist
yet — no code change, and Task 1's new integration test below re-confirms
it as a side effect of testing geometry, rather than duplicating a
dedicated test for something Story 1.2 already covers.

**Render-order gap, closed — and why the fix doesn't touch the pinned
terminal at all.** `river_node_v1.place_top` places a node "above all other
nodes in the compositor's render list," unconditionally, at the moment
it's called — it does not require knowing the pinned terminal's own node or
position. So calling `window.node.place_top()` once, for every non-pinned
window, immediately after it's registered, is sufficient by itself to
guarantee it renders above the pinned terminal (and above everything else
registered so far) at that moment — regardless of where the pinned
terminal's own node currently sits in the render list, and regardless of
whether the pinned terminal has been registered yet at all. This means the
fix is entirely scoped to the non-pinned branch of `init_new_windows`; it
requires **zero changes** to the `PINNED_TERM_APP_ID` branch or to any of
Story 1.5's already-shipped, already-tested pinned-terminal z-order
handling (`raise_view`'s pinned-terminal no-op guard, `cycle_focus`'s
skip-pinned logic, the `Action::FocusNext`/click-to-focus guards) — all of
that is untouched and unregressed by this story.

Calling `place_top()` from `init_new_windows` (which runs inside
`handle_manage_start`, i.e. a *manage* sequence, not a *render* sequence)
is protocol-legal: the protocol's own top-level description states
"Rendering state may be modified by the window manager during a manage
sequence or a render sequence" (`wm/protocol/river-window-management-v1.xml:59-61`),
and `main.rs` already calls the rendering-state `set_position` request from
this exact same manage-sequence method today (`wm/src/main.rs:275`,
pre-existing, unmodified by this story) — this story's `place_top()` call
follows the identical, already-proven-in-this-codebase pattern, not a new
one.

**Geometry: a fixed constant, not a placement algorithm — confirmed against
architecture docs, not assumed.** `components.md`'s `placement-engine`
section describes this story's scope as: *"Decides floating-vs-tiled and
render order per view — pinned-term app-id forced tiled and bottom-of-stack,
everything else floating above it"* — no mention of centering, cascading,
or overlap avoidance. `data-model.md`'s `Output` entity has only `id` and
`current_tag` fields — no width/height/position — so an output-size-aware
placement decision (e.g. "centered on the active output") is not buildable
today without first extending that entity, which is out of scope for this
story (YAGNI: FR5's literal text doesn't ask for it, and no other story or
ADR calls for it yet). This story instead adds one fixed
`wm_core::view::DEFAULT_FLOATING_GEOMETRY` constant (`x: 100, y: 100,
width: 800, height: 600` — a non-degenerate, visibly-offset-from-the-corner
default) applied to every non-pinned view via the already-existing,
already-tested `WmCore::set_view_geometry`. Every non-pinned window
currently lands at the exact same geometry as every other — cascading/
overlap-avoidance is an explicit non-goal here, flagged as a follow-up
candidate for a future story once output geometry is modeled, not a gap in
this one.

**NFR2 (panic-free) edge cases.** Zero windows: `init_new_windows`'s `for`
loop over `self.windows.iter_mut().filter(|w| w.new)` simply doesn't
iterate — no panic risk, unchanged from today. Output with no geometry
info: moot by construction — `DEFAULT_FLOATING_GEOMETRY` is a fixed
constant that never reads `Output` state at all, so there is no output
lookup that could fail or panic. `WmCore::set_view_geometry` itself already
returns `Err(WmCoreError::UnknownView)` rather than panicking for an
unregistered id (Story 1.2, unchanged); this story's call site logs that
`Err` via `eprintln!` rather than swallowing it, matching every prior
story's established NFR2 convention (`unwrap`-free error propagation with
visible logging) — though in practice this call is only ever made
immediately after `register_view` returns the same `view_id` in the same
loop iteration, so the error path is believed structurally unreachable in
production, same class of "safe-by-construction, log rather than panic or
silently swallow" precedent as `WindowManager::remove_windows`' existing
`unregister_view` logging (`wm/src/main.rs:246-248`).

**No new `WmCoreError` variants, no new ADR.** `set_view_geometry` already
exists and already returns the right error type; this story only removes
its `#[allow(dead_code)]` (it gets its first production call site) and
updates its doc comment, mirroring exactly how Story 1.5 Task 5.2 retired
`set_view_floating`'s own `#[allow(dead_code)]` when *it* got its first
call site. `adrs.md` has no entry governing default floating-window size or
position; nothing here is a new architectural decision among alternatives,
just closing a gap two existing components (`wm_core::set_view_geometry`,
`river_node_v1.place_top`) already anticipated.

**NFR1 (50ms budget).** `set_view_geometry` is an `O(1)` `HashMap` lookup
and field write (Story 1.2, already budget-accepted). `place_top()` and
`propose_dimensions`/`set_position` are the same class of Wayland protocol
request already made elsewhere in this file today (`focus_top`,
pre-existing) — not new I/O this story introduces, just one more call site
of an already-accepted-cost operation.

No new Cargo dependencies. Build/test/lint only ever run **inside the
devcontainer via devpod** (`devpod ssh buoy-wm -- cargo ...`) — same
constraint as every prior story.

## Tasks / Subtasks

- [x] **Task 1: `wm_core::view::DEFAULT_FLOATING_GEOMETRY` — the fixed default geometry for non-pinned floating windows (AC: "renders above the pinned terminal" geometry half + "regardless of which tag or output")**
  - [x] 1.1 RED — Inside the devcontainer via devpod (`devpod ssh buoy-wm -- cargo test --manifest-path wm/Cargo.toml wm_core::state::`), add tests to `wm/src/wm_core/state.rs`'s existing test module (import `DEFAULT_FLOATING_GEOMETRY` alongside the module's existing `use super::{PINNED_TERM_APP_ID, WmCore, WmCoreError};` / `use crate::wm_core::view::Geometry;` lines): `default_floating_geometry_has_expected_literal_value` — `assert_eq!(DEFAULT_FLOATING_GEOMETRY, Geometry { x: 100, y: 100, width: 800, height: 600 })`, locking in the exact chosen constant so any future change to it is a conscious, reviewed edit rather than a silent drift; `default_floating_geometry_is_non_degenerate` — `assert!(DEFAULT_FLOATING_GEOMETRY.width > 0)` and `assert!(DEFAULT_FLOATING_GEOMETRY.height > 0)`, guarding against a future edit accidentally reintroducing a zero-size (invisible) default regardless of what the literal x/y/width/height values become; `default_floating_geometry_applies_identically_regardless_of_tag_or_output` — the direct proof of the AC's "regardless of which tag or output" clause: `register_output()` twice (`output_a`, `output_b`), `create_tag()` twice with different names, `switch_tag(output_a, tag_a)` and `switch_tag(output_b, tag_b)` (Story 1.3's existing, already-tested function) to put each output/tag pair into a genuinely different live state, `register_view()` a non-pinned view under each, call `set_view_geometry(view, DEFAULT_FLOATING_GEOMETRY)` for both, then assert both views' resulting `.geometry` (whitebox, same-module access to `core.views`) equal `DEFAULT_FLOATING_GEOMETRY` and both `.floating` remain `true` — demonstrating the default is applied identically no matter which tag/output context was live at registration time. Confirm all three fail to compile — the constant doesn't exist yet.
  - [x] 1.2 GREEN — In `wm/src/wm_core/view.rs`, directly below the `Geometry` struct definition, add: `pub const DEFAULT_FLOATING_GEOMETRY: Geometry = Geometry { x: 100, y: 100, width: 800, height: 600 };` with a `///` doc comment stating this is the fixed default position/size applied to every newly registered non-pinned view (FR5), and explicitly noting *why* it's a fixed constant rather than computed from output geometry (`data-model.md`'s `Output` entity has no width/height fields yet — output-aware placement is out of scope until a future story extends it; see this story's Technical notes for the full rationale). Confirm the three new tests pass; confirm no regression in the existing 77-test suite.

- [x] **Task 2: Wire `DEFAULT_FLOATING_GEOMETRY` + real render-order into `WindowManager::init_new_windows` for non-pinned windows (AC: all — this is the live production wiring)**
  - [x] 2.1 In `wm/src/main.rs`, add `use wm_core::view::DEFAULT_FLOATING_GEOMETRY;` alongside the existing `use wm_core::state::{PINNED_TERM_APP_ID, WmCore};` line (`wm/src/main.rs:46`). In `WindowManager::init_new_windows` (`wm/src/main.rs:273-289`), add an `else` branch to the existing `if window.app_id == PINNED_TERM_APP_ID { ... }` block, leaving that `if` branch's own body completely untouched (no regression to Story 1.5's shipped pinned-terminal behavior):
    ```rust
    } else {
        if let Err(e) = self.wm_core.set_view_geometry(view_id, DEFAULT_FLOATING_GEOMETRY) {
            eprintln!("Failed to set default floating geometry for view {view_id:?}: {e:?}");
        }
        window.set_position(DEFAULT_FLOATING_GEOMETRY.x, DEFAULT_FLOATING_GEOMETRY.y);
        window
            .proxy
            .propose_dimensions(DEFAULT_FLOATING_GEOMETRY.width, DEFAULT_FLOATING_GEOMETRY.height);
        window.node.place_top();
    }
    ```
    Both `Result`s this story discards are logged rather than silently swallowed, matching this file's established NFR2 convention (`remove_windows`/`focus_top`/Story 1.5's own new call sites). The pre-existing, unconditional `window.set_position(window.x, window.y)` / `window.proxy.propose_dimensions(window.width, window.height)` calls at the top of the loop (`wm/src/main.rs:275-276`, predating this story) are left exactly as-is — they still run first for every window (harmless no-op zero-geometry for the pinned terminal, preserving Story 1.5's exact existing behavior for it), and this new `else` branch's calls simply override them afterward for non-pinned windows only.
  - [x] 2.2 Since `set_view_geometry` now has a real production call site, remove its `#[allow(dead_code)]` annotation in `wm/src/wm_core/state.rs` (`wm/src/wm_core/state.rs:164`) and update its doc comment (`wm/src/wm_core/state.rs:160-163`) to state that this story (1.6) wires it into `main.rs` for non-pinned windows, replacing the "Not yet wired into `main.rs` — floating placement/geometry lands in Story 1.6" note — mirroring exactly how Story 1.5 Task 5.2 retired `set_view_floating`'s equivalent note.
  - [x] **No RED/GREEN for this task.** Same carve-out and justification as Story 1.5's Task 5 (`init_new_windows`'s pinned-terminal branch): this is Wayland-glue call-site composition of already-tested/verified pieces — `set_view_geometry` (Story 1.2, re-verified by Task 1.1 above), `Window::set_position`/`RiverWindowV1::propose_dimensions` (pre-existing, used identically elsewhere in this same function), and `RiverNodeV1::place_top` (pre-existing, used identically in `Seat::focus_top`) — composed with no new decision logic of its own beyond "is this the pinned terminal or not," which the existing `if`/`else` branch condition already expresses. No live `river` compositor session exists in this sandbox to exercise the real protocol calls end-to-end (same boundary every prior story has hit). Verification is `cargo build --manifest-path wm/Cargo.toml` and `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings` succeeding, plus structural code review confirming: (a) the `else` branch fires only for non-`PINNED_TERM_APP_ID` windows; (b) it calls `set_view_geometry`, then `set_position`, then `propose_dimensions`, then `place_top()`, in that order; (c) the pre-existing `PINNED_TERM_APP_ID` branch (Story 1.5) is byte-for-byte unchanged; (d) `set_view_geometry`'s `dead_code` allow is confirmed gone under `cargo build` (not just by reading the source) and no *new*, unexpected `dead_code` warnings appear anywhere else.

- [x] **Task 3: Full in-container verification gate (AC: all)**
  - [x] 3.1 Run, inside the devcontainer via devpod (`devpod ssh buoy-wm`): `cargo fmt --manifest-path wm/Cargo.toml --all -- --check`, `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings`, and `cargo test --manifest-path wm/Cargo.toml`. Confirm all three pass with the existing 77-test suite (Stories 1.2-1.5) plus this story's 3 new `wm_core` tests (Task 1) all green — 80 total — zero clippy/fmt violations, and specifically: `set_view_geometry` no longer flagged `dead_code` (Task 2.2), no other unexpected `dead_code` warnings introduced. Run `pre-commit run --all-files` as a final sanity check (same fmt/clippy hooks from Story 1.1).
  - [x] 3.2 Manual smoke-test note (not an automated check, not a story blocker, same boundary as every prior story's live-compositor gap): if a real `river` session is available outside this sandbox, manually mapping a non-pinned window (e.g. plain `foot` via the existing `Mod+Space` `Action::SpawnFoot` keybind) while a pinned terminal is already running for the current tag would let a human visually confirm the new window appears at a fixed, visible, non-`(0,0)`-degenerate position/size and renders above the pinned terminal immediately on creation, without needing to be clicked or focus-cycled to first. Record in the Dev Agent Record whether this was performed or explicitly deferred, per this project's established convention — either is acceptable to close the story, but must be stated, not silently assumed.

## Test plan
This story's tests split into two tiers, matching the project's established
TDD approach (Story 1.5's Test plan) for a mix of pure state logic and
I/O-adjacent glue:

1. **`wm_core::view::DEFAULT_FLOATING_GEOMETRY`** (Task 1) — Rust unit
   tests added to `wm/src/wm_core/state.rs`'s existing test module, run via
   `cargo test --manifest-path wm/Cargo.toml` inside the devcontainer via
   devpod: the constant's exact literal value is locked in
   (`default_floating_geometry_has_expected_literal_value`); it is
   non-degenerate — strictly positive width and height, guarding future
   edits regardless of the specific literal chosen
   (`default_floating_geometry_is_non_degenerate`); and applying it via
   `set_view_geometry` produces identical, correct results under two
   different live tag/output contexts (`register_output` → `create_tag` →
   `switch_tag` → `register_view` → `set_view_geometry`, run twice with
   different tags/outputs), directly proving the AC's "regardless of which
   tag or output it's mapped on" clause and, as a side effect, re-confirming
   `floating` stays `true` for non-pinned views under both contexts
   (`default_floating_geometry_applies_identically_regardless_of_tag_or_output`).
   FR5's "defaults to floating" clause itself needs no *new* test beyond
   this confirmation — it's already covered by Story 1.2's existing
   `newly_registered_view_has_expected_defaults` test, unchanged by this
   story.
2. **`main.rs` wiring** (Task 2) — **not covered by automated unit tests**,
   by design, same boundary every prior story has hit (real
   `wayland_client`/`river-window-management-v1` `Dispatch` types, no live
   protocol connection in this devcontainer sandbox): the new `else` branch
   in `init_new_windows` calling `set_view_geometry`, `set_position`,
   `propose_dimensions`, and `place_top()` for non-pinned windows. Verified
   instead via `cargo build --manifest-path wm/Cargo.toml` and `cargo
   clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings`
   succeeding, plus structural code review confirming: (a) the branch fires
   only for non-`PINNED_TERM_APP_ID` windows; (b) call order is
   `set_view_geometry` → `set_position` → `propose_dimensions` →
   `place_top()`; (c) the pre-existing `PINNED_TERM_APP_ID` branch (Story
   1.5) is unmodified; (d) `set_view_geometry`'s `dead_code` lint is
   confirmed gone under `cargo build` itself, not just by reading the
   source.
3. **Tooling gate** (Task 3) — `cargo fmt --check`, `cargo clippy -D
   warnings`, and the full `cargo test` run (77 existing + 3 new, all
   green — 80 total), all in-container; `pre-commit run --all-files` as a
   final sanity check reusing Story 1.1's hooks.

Live-compositor coverage for the actual visual "non-pinned window appears
at the default geometry, above the pinned terminal, immediately on
creation" claim remains a standing gap in this story, same class of
sandbox limitation every prior story has documented (no live `river`
session available here). Task 3.2's optional manual smoke test is the only
way to get live visual confirmation; if not performed, that is an explicit,
recorded coverage gap for a human to accept, consistent with every prior
story's own precedent for its own live-compositor gaps.

## FR coverage
FR5, NFR1, NFR2

## Dev Agent Record

### Implementation Plan
Followed the story's 3-task breakdown exactly, TDD for Task 1 only (Task 2
is the established Wayland-glue carve-out, same as Story 1.5 Task 5):
1. Task 1 RED: added `use crate::wm_core::view::{DEFAULT_FLOATING_GEOMETRY, Geometry};`
   and three new tests to `wm_core::state`'s test module, referencing the
   not-yet-existing `DEFAULT_FLOATING_GEOMETRY` constant. Confirmed compile
   failure (`E0432: unresolved import`).
2. Task 1 GREEN: added `pub const DEFAULT_FLOATING_GEOMETRY: Geometry = Geometry { x: 100, y: 100, width: 800, height: 600 };`
   directly below `Geometry` in `wm_core::view`. Full suite went to 80/80
   green (77 existing + 3 new), no regressions.
3. Task 2: added the `else` branch to `init_new_windows`'s existing
   `if window.app_id == PINNED_TERM_APP_ID { ... }` block in `main.rs`,
   calling `set_view_geometry` → `set_position` → `propose_dimensions` →
   `node.place_top()` for non-pinned windows only. The pinned-terminal `if`
   branch body is byte-for-byte unchanged. Removed `set_view_geometry`'s
   `#[allow(dead_code)]` and updated its doc comment in `wm_core::state`.
4. Task 3: ran `cargo fmt --check`, `cargo clippy --all-targets -D warnings`,
   `cargo build`, `cargo test`, and `pre-commit run --all-files` inside the
   devcontainer via `devpod ssh buoy-wm`. All green.

### Completion Notes
- 80/80 tests pass (77 pre-existing + 3 new: `default_floating_geometry_has_expected_literal_value`,
  `default_floating_geometry_is_non_degenerate`, `default_floating_geometry_applies_identically_regardless_of_tag_or_output`).
- `cargo build`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo fmt --all -- --check` all clean inside the devcontainer.
  `set_view_geometry`'s `dead_code` allow is confirmed gone (no warning
  emitted by `cargo build`); no new/unexpected `dead_code` warnings appeared
  elsewhere.
- `pre-commit run --all-files` (both configured hooks: `cargo-fmt-check`,
  `cargo-clippy`) passed.
- One deviation from the story's literal test code: clippy's
  `assertions_on_constants` lint fired on
  `default_floating_geometry_is_non_degenerate`'s two `assert!` calls
  (compile-time-constant today by construction). Resolved with a targeted
  `#[allow(clippy::assertions_on_constants)]` on that test function plus a
  comment explaining the assertion is an intentional guard rail against a
  future edit to the constant reintroducing a zero-size default, not a
  runtime behavior check. No other deviations from the story spec.
- Story 1.5's pinned-terminal registration/z-order logic
  (`PINNED_TERM_APP_ID` branch body, `raise_view`'s no-op guard,
  `cycle_focus`'s skip-pinned logic, `Action::FocusNext`/click-to-focus
  guards) was not touched; full suite re-run after Task 2 confirms no
  regression in any of those tests.
- Task 3.2 (manual live-`river` smoke test): **explicitly deferred**, same
  sandbox limitation documented by every prior story (no live `river`
  compositor session available in this environment). Not performed.

### Debug Log
- RED confirmation: `devpod ssh buoy-wm --command "cd /workspaces/buoy-wm && cargo test --manifest-path wm/Cargo.toml wm_core::state::"` →
  `error[E0432]: unresolved import 'crate::wm_core::view::DEFAULT_FLOATING_GEOMETRY'`.
- GREEN confirmation: `cargo test --manifest-path wm/Cargo.toml` → `test result: ok. 80 passed; 0 failed`.
- `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings` initially failed
  with `error: this assertion has a constant value` (2x, `assertions_on_constants`)
  on the non-degenerate test; fixed via targeted `#[allow]` (see Completion Notes);
  re-run clean.
- `cargo fmt --manifest-path wm/Cargo.toml --all -- --check` initially failed
  (line-wrapping diff in the new `else` branch); resolved by running
  `cargo fmt --manifest-path wm/Cargo.toml --all` in-container; re-run of
  `--check` clean.
- Final full gate (`cargo fmt --check`, `cargo build`, `cargo clippy -D warnings`,
  `cargo test`, `pre-commit run --all-files`), all run via
  `devpod ssh buoy-wm --command "..."`: all green.

## File List
- `wm/src/wm_core/view.rs` — added `pub const DEFAULT_FLOATING_GEOMETRY: Geometry`.
- `wm/src/wm_core/state.rs` — added 3 new unit tests and the
  `DEFAULT_FLOATING_GEOMETRY` import to the test module; removed
  `set_view_geometry`'s `#[allow(dead_code)]` and updated its doc comment.
- `wm/src/main.rs` — added `use wm_core::view::DEFAULT_FLOATING_GEOMETRY;`;
  added the non-pinned `else` branch to `init_new_windows` wiring
  `set_view_geometry`, `set_position`, `propose_dimensions`, and
  `node.place_top()`.

## Change Log
- 2026-08-07: Story 1.6 implemented — `DEFAULT_FLOATING_GEOMETRY` constant
  (Task 1, TDD RED→GREEN), wired into `init_new_windows`'s non-pinned branch
  with `place_top()` closing the protocol-undefined initial z-order gap
  (Task 2), full in-container verification gate green at 80/80 tests
  (Task 3). Story 1.5's pinned-terminal logic untouched and unregressed.
