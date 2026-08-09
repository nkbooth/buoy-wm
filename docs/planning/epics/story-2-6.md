---
baseline_commit: e5c2670
---

# Story 2.6: Bind river_layer_shell_v1 so the status bar can render

Epic: 2 | Priority: H | Status: done

## Description
Live testing after Story 2.5 (2026-08-08, first-ever live boot of `buoy-wm`
under a real `river` session) found that waybar's status bar never actually
appeared on screen. `river`'s own log made the cause explicit:

```
info(wm): window manager did not bind river_layer_shell_v1, closing layer surface
```

`river_layer_shell_v1` is a small, optional companion protocol
(codeberg.org/river/river, `protocol/river-layer-shell-v1.xml`, © 2025 Isaac
Freund, MIT) that gates whether `river` allows any client to map a
`wlr-layer-shell-unstable-v1` layer surface at all. Per its own description:

> If the window manager does not bind this interface, the compositor should
> not allow clients to map layer surfaces. This can be achieved by closing
> layer surfaces immediately.

Waybar's bar is exactly such a layer surface. Since `buoy-wm` never bound
this global across all of Epic 1/2, `river` closed it immediately every
time — the status bar work from Story 2.5 was correct but structurally
unable to render under a real `river` session. This story closes that gap.

**Scope is deliberately minimal.** Reading the full protocol XML (not just
the summary), `river_layer_shell_v1` has exactly three requests on the
global itself (`destroy`, `get_output`, `get_seat`) and no per-surface
geometry/anchor/layer requests at all — `river` handles all real
layer-surface placement and rendering natively via its own
`wlr-layer-shell-unstable-v1` implementation. The global's sole job is the
gate described above: **binding it is what unlocks layer-shell surfaces
being allowed to map at all.** `get_output`/`get_seat` (exclusive-zone
hints via `non_exclusive_area`, keyboard-focus arbitration via
`focus_exclusive`/`focus_non_exclusive`/`focus_none`) are optional
refinements — a status-bar module requests no keyboard focus and needs no
exclusive-zone awareness from `buoy-wm` (waybar's own layer-shell surface
already declares its own anchor/exclusive-zone to `river` directly per the
standard `wlr-layer-shell-unstable-v1` protocol, independent of this
companion protocol). This story binds the global only — `get_output`/
`get_seat`/`set_default` are explicitly out of scope until a real use case
needs them (YAGNI; noted as a documented future gap, not silently built to
completeness nobody asked for).

## Acceptance criteria
**Given** `river` advertises `river_layer_shell_v1`
**When** `buoy-wm` starts
**Then** it binds the global (mirroring the existing `river_xkb_bindings_v1` binding pattern)
**And** `river` no longer logs "window manager did not bind river_layer_shell_v1, closing layer surface"
**And** a waybar `custom/tag` layer-shell surface is allowed to map (verifiable live; not testable in this sandbox — see Test plan)

**Given** an older/different compositor that does not advertise `river_layer_shell_v1`
**When** `buoy-wm` starts
**Then** it starts successfully without this optional global (unlike `river_xkb_bindings_v1`, which is treated as required and exits the process if missing) — layer-shell clients simply won't be able to map surfaces, exactly as before this story, not a regression

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: Vendor the protocol XML (AC: binds the global)**
  - [x] 1.1 Add `wm/protocol/river-layer-shell-v1.xml`, fetched verbatim from `codeberg.org/river/river`'s `protocol/river-layer-shell-v1.xml` (mirrors how `river-window-management-v1.xml`/`river-xkb-bindings-v1.xml` were vendored in Story 1.1). No modifications to the XML itself.
  - [x] 1.2 Add `wayland_scanner::generate_interfaces!`/`generate_client_code!` calls for it in `wm/src/main.rs`'s protocol-codegen block, alongside the existing two. No RED/GREEN — this is generated-code plumbing, verified by `cargo build` succeeding (a malformed/missing XML reference fails the build immediately, which is the test).

- [x] **Task 2: Bind the global as optional (AC: both — required-vs-optional distinction)**
  - [x] 2.1 Add a `river_layer_shell: Option<RiverLayerShellV1>` field to `AppData`, mirroring `river_wm`/`river_xkb`.
  - [x] 2.2 In the `wl_registry::Event::Global` match arm, add a `"river_layer_shell_v1"` case that binds the global (version 1) into `state.river_layer_shell`, mirroring the existing two cases' version-check-then-bind shape.
  - [x] 2.3 Unlike `river_xkb_bindings_v1` (whose absence is fatal — `std::process::exit(1)`), a missing `river_layer_shell_v1` must NOT be fatal — it's explicitly optional per the protocol's own description. If absent, log an informational message (mirroring the existing `river_xkb_bindings_v1` missing-global message style) and continue running with layer-shell surfaces simply unable to map, exactly as `buoy-wm`'s behavior was before this story (not a new regression, a pre-existing and now-explicit limitation).
  - [x] 2.4 No RED/GREEN — this is Wayland registry/global-binding glue, the same untestable-without-a-live-compositor category as every prior `main.rs` protocol-binding change in this project (Stories 1.4, 1.7, 2.4). Verify via `cargo build`/`cargo clippy --all-targets -- -D warnings` and structural review: confirm the new match arm compiles, binds the correct interface/version, and the optional-vs-required distinction from 2.3 is actually implemented (no `std::process::exit` on a missing `river_layer_shell_v1`).

- [x] **Task 3: Full in-container verification gate + live confirmation note (AC: all)**
  - [x] 3.1 Run, inside the devcontainer via `devpod ssh buoy-wm` (or the `podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>` fallback — `devpod ssh` has been intermittent all project): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Confirm the existing 238-test suite is unaffected (this story adds no new unit tests — see Test plan) and the workspace still builds clean with the new protocol module wired in.
  - [x] 3.2 Manual live-verification note (not an automated check, not a story blocker — same boundary as every prior story's live-compositor gap, but this one is directly actionable the next time `buoy-wm` is run for real): once rebuilt, launching `river -c <path-to-buoy-wm>` should no longer log `window manager did not bind river_layer_shell_v1, closing layer surface`, and waybar's bar should become visible. Record in the Dev Agent Record whether this was actually confirmed live or deferred.

## Technical notes

**Why this wasn't caught during Epic 2's original 5 stories.** No `waybar`/
`fuzzel`/live `river` session existed in the devcontainer sandbox at any
point during Epic 1 or Epic 2 — every story's Dev Agent Record explicitly
disclosed this gap rather than hiding it (a documented, accepted risk, not
an oversight in process). This story is a direct, first-time consequence of
that disclosed gap surfacing on the very first live boot (2026-08-08) —
exactly the outcome the disclosure was meant to flag as possible.

**Scope boundary, restated:** `get_output`/`get_seat`/`set_default`/focus
arbitration are real, documented parts of the protocol but serve use cases
`buoy-wm` doesn't have yet (exclusive-zone-aware window placement around
the bar, keyboard-focus handoff to a hypothetical future layer-shell
client that wants input, like a lock screen or launcher). Do not
implement these speculatively. If a future story needs them, it can
build on the `river_layer_shell: Option<RiverLayerShellV1>` field this
story adds.

**No `wm_core` changes.** This is purely `main.rs`-level Wayland registry
plumbing — `wm_core` stays protocol-agnostic, unchanged by this story.

## Test plan
No new unit tests — this story is Wayland global-binding glue with no
decision logic of its own (unlike, say, Story 2.4's `parse_args`, which
had real branching to unit-test). The verification is:
1. **Build gate**: `cargo build --workspace`/`cargo clippy --workspace --all-targets -- -D warnings` clean with the new protocol module compiled in.
2. **Regression gate**: existing 238-test suite (`wm` 149, `tag-picker` 63, `status-bar` 26) unaffected.
3. **Live gate (manual, not automated)**: `river`'s log no longer reports closing the layer surface; waybar's bar becomes visible.

## FR coverage
FR10 (status bar visibility) — closes a live-testing gap discovered after Story 2.5 shipped, not a new FR.

## Dev Agent Record

**What was done**: vendored `wm/protocol/river-layer-shell-v1.xml` verbatim
from `codeberg.org/river/river`'s `main` branch (fetched 2026-08-08). Added
a third protocol-codegen submodule (`rlayer`, mirroring `rxkb`) to
`wm/src/main.rs`'s existing `mod river { ... }` block. Added
`river_layer_shell: Option<RiverLayerShellV1>` to `AppData` with a doc
comment explaining the optional-vs-required distinction from
`river_xkb`. Added a `"river_layer_shell_v1"` match arm to the
`wl_registry::Event::Global` handler, alongside the existing
`river_window_manager_v1`/`river_xkb_bindings_v1` arms — binds the global
on success; on a version mismatch, logs an informational message and
`return`s (not `std::process::exit`, since this global is optional).
Added `wayland_client::delegate_noop!(AppData: ignore RiverLayerShellV1)`
(discovered via a build error: `RiverXkbBindingsV1`'s own zero-event
`Dispatch` satisfaction comes from the same macro, not a manual `impl`
that a first grep for it missed — `main.rs:1229` already had the
pattern for `RiverXkbBindingsV1`/`RiverNodeV1`).

**Verification** (all inside the devcontainer via `devpod ssh buoy-wm`):
`cargo build --workspace` clean; `cargo fmt --all -- --check` clean;
`cargo clippy --workspace --all-targets -- -D warnings` clean;
`cargo test --workspace` — 149 `wm` + 27 `status-bar` + 63 `tag-picker` =
239 total, all passing, zero regressions (this story adds no new tests,
matching the Test plan); `pre-commit run --all-files` — both hooks pass.

**Task 3.2 (manual live-verification)**: deferred, not performed as part
of this change — no live `river`/`waybar` session was exercised during
implementation (same devcontainer-sandbox boundary as every prior story).
The next real boot of `buoy-wm` (rebuilt with this change) is the actual
live confirmation; if `river`'s "closing layer surface" log line is gone
and the status bar becomes visible, this story's live AC is satisfied.

**Self-review note**: no separate reviewer sub-agent was used for a diff
this size (38 lines in one file, plus a vendored third-party XML and the
story doc) — implemented directly with the same TDD-where-testable
discipline as the rest of the project (this story's own Task 2.4
explicitly carves out RED/GREEN as inapplicable, matching every prior
`main.rs` registry/global-binding change), then self-reviewed against the
protocol's actual documented semantics (re-read in full, not just the
summary) before committing.

