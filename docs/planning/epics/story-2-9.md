---
baseline_commit: c9ef85c
---

# Story 2.9: Target fuzzel at the real active output

Epic: 2 | Priority: H | Status: done

## Description
Live testing after Story 2.8 (2026-08-09) found that with the laptop lid
closed and the pointer over the external monitor, `fuzzel` (the picker
`tag-picker` spawns for both assign mode, `Mod4+A`, and switch mode,
`Mod4+S`) still always renders on the laptop panel — which, with the lid
closed, makes it completely inaccessible. Story 2.8 fixed *`buoy-wm`'s own*
active-output resolution (tag-switch/auto-tag keybinds), but that fix never
reaches `fuzzel` at all: `fuzzel` is a separate process, spawned by
`tag-picker`, a separate binary — nothing in that chain ever told it which
monitor to appear on.

Confirmed via `fuzzel`'s own manual: it supports `-o`/`--output=OUTPUT`
("specifies the monitor to display the window on... default: let the
compositor choose output"). Nothing calls it today
(`tag-picker/src/main.rs`'s `run_fuzzel` passes only `--dmenu`/
`--with-nth`/`--accept-nth`/`--nth-delimiter`/`--placeholder`/`--search`).
The value it needs is a real Wayland connector name (e.g. `"eDP-1"`,
`"DP-2"`) — and confirmed via code search, **`buoy-wm` has never captured
that string anywhere.** The only output identity that exists today is
`wm_core`'s own internal `OutputId` (an arbitrary sequential integer),
already passed to `tag-picker`'s switch mode as a CLI arg, but that's
meaningless to `fuzzel` or any other real client.

**Where the real name actually comes from.** `river_output_v1`'s `WlOutput`
event ("the global name of the wl_output advertised with
`wl_registry.global`") only carries a `u32` *registry* name — not the
connector string. The connector string only exists on the `wl_output`
global itself, via its own `name` event (`wl_output` protocol version 4+,
a core Wayland protocol already available through `wayland_client::
protocol::wl_output` — no new XML vendoring needed, unlike Story 2.6's
`river_layer_shell_v1`). `buoy-wm` has never bound `wl_output` directly
(it has only ever used river's own abstractions) — this story is the first
time it does.

**Why this needs a two-step correlation, not a direct lookup.** The
`wl_output` global for a given monitor is guaranteed to be advertised
*before* the `river_output_v1::WlOutput` event that references it by
registry name — but nothing guarantees the bound `wl_output`'s own `name`
event has already arrived by that point. So: bind every `wl_output` global
as soon as it's advertised (regardless of which `river_output_v1` it'll
later correspond to), keyed by its registry name; when `river_output_v1`'s
`WlOutput{name}` event arrives, look up that registry name to find which
bound `wl_output` object corresponds to this `Output`, and remember *that
object's own id* (not a name string yet, since it might not have arrived).
Read the actual connector name from a separate map, populated whenever a
`wl_output`'s `name` event fires — populated independently, and always
looked up fresh at the point a name is actually needed (never cached
prematurely as "known" when it might still be pending).

## Acceptance criteria
**Given** a `wl_output` global is advertised and its `river_output_v1`
counterpart's `WlOutput` event has fired, and the `wl_output`'s own `name`
event has arrived
**When** `Mod4+A` (assign mode) or `Mod4+S` (switch mode) spawns `tag-picker`
**Then** the real active output's connector name (e.g. `"eDP-1"`) is passed
to `tag-picker`, which passes `fuzzel --output=<name>`

**Given** the connector name isn't yet known (its `wl_output.name` event
hasn't arrived, or the output's `wl_output` binding failed/never
correlated)
**When** `tag-picker`/`fuzzel` is spawned
**Then** no `--output` flag is passed at all — `fuzzel` falls back to its
own "let the compositor choose" default, same as today's behavior (never a
crash, never a bogus/empty `--output=` argument)

**Given** `fuzzel` is spawned with a correct `--output=<name>` for the
output the pointer is currently over (the same output Story 2.8's
`active_output_id` resolves to)
**When** the picker opens with the laptop lid closed and the pointer on
the external monitor
**Then** it renders on the external monitor, not the (inaccessible) laptop
panel

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: Bind every `wl_output` global as it's advertised (AC 1, 2)**
  - [x] 1.1 In `wm/src/main.rs`'s `wl_registry::Event::Global` handler, add a `"wl_output"` match arm: bind via `registry.bind::<wayland_client::protocol::wl_output::WlOutput, _, _>(name, version.min(4), qh, ())` (capped at 4 — the version that adds the `name` event; no fatal exit on a lower version, this is purely best-effort cosmetic data, same optional-global shape as Story 2.6's `river_layer_shell_v1`, not the required-global shape `river_xkb_bindings_v1` uses). Store the bound proxy in a new `WindowManager` field `pending_wl_outputs: HashMap<u32, WlOutput>`, keyed by the registry `name: u32` this bind call was made for.
  - [x] 1.2 Add `wayland_client::delegate_noop!` is NOT sufficient here (we need real event handling, not zero-event ignoring) — add a real `impl Dispatch<wl_output::WlOutput, ()> for AppData`, matching `wl_output::Event::Name { name } => { ... }` and ignoring `Geometry`/`Mode`/`Scale`/`Done`/`Description` (not needed). On `Name`, insert into a new `WindowManager` field `wl_output_names: HashMap<ObjectId, String>`, keyed by the `wl_output` proxy's own `.id()`.
  - [x] 1.3 No RED/GREEN — Wayland registry/event glue, untestable without a live compositor, same carve-out as every prior protocol-binding story (1.4, 1.7, 2.4, 2.6, 2.7, 2.8). Verify via `cargo build`/`cargo clippy --all-targets -- -D warnings`.

- [x] **Task 2: Correlate `river_output_v1`'s `Output` to its real connector name (AC 1, 2)**
  - [x] 2.1 Add `wl_output_object_id: Option<ObjectId>` to `main.rs`'s `Output` struct, defaulting to `None` in `Output::new`.
  - [x] 2.2 In `Dispatch<RiverOutputV1, ()>`'s handler, change `Event::WlOutput { name: _ } => {}` to look up `name` (the registry name) in `pending_wl_outputs`; if found, set this `Output`'s `wl_output_object_id` to that proxy's `.id()`. If not found (the `wl_output` global somehow wasn't bound — shouldn't happen per the protocol's own ordering guarantee, but handled gracefully, not panicking, per NFR2), leave it `None` — AC 2's fallback covers this.
  - [x] 2.3 Add a `WindowManager` method `fn output_name(&self, output_id: OutputId) -> Option<&str>`: find the `Output` whose `output_id` field matches (same linear-scan shape `output_proxy_for_id` already uses — `outputs` is keyed by `ObjectId`, not `OutputId`), read its `wl_output_object_id`, look that up in `wl_output_names`. Returns `None` at any missing step, never panics.
  - [x] 2.4 No RED/GREEN, same reasoning as Task 1.3 — `Output`/`WindowManager` structurally require live Wayland proxy fields, the same constraint that has kept every `main.rs` struct untestable throughout this project. Verify via build/clippy/structural review.

- [x] **Task 3: Thread the resolved name into both `tag-picker` spawn sites (AC 1, 2, 3)**
  - [x] 3.1 In `WindowManager::manage_seats`, alongside the existing `let active_output_id = self.active_output_id();` (computed before the `wm_core` mutable borrow begins, same reasoning that line's own comment already documents), add `let active_output_name = active_output_id.and_then(|id| self.output_name(id)).map(str::to_owned);`.
  - [x] 3.2 Change `Seat::do_action`'s signature to accept a new `active_output_name: Option<&str>` parameter (alongside the existing `active_output_id: Option<OutputId>`); update its one call site in `manage_seats` to pass `active_output_name.as_deref()`.
  - [x] 3.3 In `Action::OpenTagPicker`'s arm, append `.arg(name)` to the `tag-picker` `Command` when `active_output_name` is `Some(name)`; no extra arg when `None` (preserves today's zero-args default exactly).
  - [x] 3.4 In `Action::TagSwitch`'s arm, after the existing `.arg(output_id.0.to_string())`, append `.arg(name)` when `active_output_name` is `Some(name)`; no extra arg when `None` (preserves today's two-arg `switch <id>` shape exactly).
  - [x] 3.5 No RED/GREEN — Wayland-glue orchestration, same boundary as Task 1/2. Verify via build/clippy/structural review that both arms only add the trailing arg conditionally and never break the existing zero-arg/two-arg shapes when no name is known.

- [x] **Task 4: `tag-picker`'s argv grammar — extend, don't replace (AC: all)**
  - [x] 4.1 RED — In `tag-picker/src/mode.rs`, extend `Mode`: `Assign { output_name: Option<String> }`, `Switch { output_id: u64, output_name: Option<String> }`. Add tests: `parse_args_with_no_arguments_is_assign_mode_with_no_output_name` (existing test, updated for the new field shape), `parse_args_with_one_argument_is_assign_mode_with_output_name`, `parse_args_with_switch_and_valid_output_id_is_switch_mode_with_no_output_name` (existing test updated), `parse_args_with_switch_valid_output_id_and_name_is_switch_mode_with_output_name`, plus the existing rejection tests (`parse_args_rejects_switch_with_missing_output_id`, `..._non_numeric_output_id`, `..._too_many_arguments`, `..._unknown_first_argument`) updated for the new one-extra-optional-arg grammar (now up to 3 args for switch mode, up to 1 for assign mode — anything beyond that is still rejected). Confirm all fail to compile/fail against the current 2-variant `Mode` enum.
  - [x] 4.2 GREEN — Implement the extended `parse_args`: `[]` → `Assign { output_name: None }`; `[name]` → `Assign { output_name: Some(name.clone()) }`; `[mode, id] if mode == "switch"` → `Switch { output_id, output_name: None }` (existing numeric-parse-and-map-err shape, unchanged); `[mode, id, name] if mode == "switch"` → `Switch { output_id, output_name: Some(name.clone()) }`; everything else still `Err`. Update the usage string. Confirm all Task 4.1 tests pass.

- [x] **Task 5: Wire the output name into `run_fuzzel` (AC 1, 2, 3)**
  - [x] 5.1 Add an `output_name: Option<&str>` parameter to `run_fuzzel`; when `Some(name)`, append `.arg(format!("--output={name}"))` to the `fuzzel` `Command` before spawning. No RED/GREEN — `run_fuzzel` is real-process-spawning glue, already explicitly documented as untestable in this sandbox (its own doc comment: "never exercised live in this sandbox — no `fuzzel` binary and no Wayland session"), same carve-out this function already has.
  - [x] 5.2 Thread `output_name` through `run_assign_mode`/`run_switch_mode` (new parameter on each, passed to every `run_fuzzel` call site — `run_assign_mode`'s loop passes the same value on every reopen) and `main`'s `match mode { ... }` (destructure the new `output_name` field from each `Mode` variant, pass `.as_deref()` through).
  - [x] 5.3 No RED/GREEN, same reasoning as 5.1. Verify via `cargo build`/`cargo clippy --all-targets -- -D warnings` and structural review that a `None` output name never appends `--output=` at all (must never send `fuzzel` an empty/malformed value).

- [x] **Task 6: Full in-container verification gate (AC: all)**
  - [x] 6.1 Inside the devcontainer (`devpod ssh buoy-wm`, or the `podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>` fallback): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pre-commit run --all-files`. Confirm the existing 268-test suite plus this story's new `tag-picker` tests all pass, 0 regressions.
  - [x] 6.2 Manual live-verification note (not automated, not a story blocker — same boundary as every prior story's live-compositor gap): once rebuilt, confirm `fuzzel` actually opens on the monitor the pointer is over, including the docked/lid-closed case this story exists to fix. Record in the Dev Agent Record whether this was actually confirmed live or deferred.

## Technical notes

**Why not just reuse `wm_core`'s `OutputId` for this?** `fuzzel` (and
`wlr-randr`, and every other real Wayland client) has no concept of
`buoy-wm`'s internal, arbitrary `OutputId` — it only understands real
compositor-assigned connector names. This is the same category of gap
Story 2.8 already flagged in a comment (`main.rs`'s `status-bar` launch
note: "`status-bar` takes `buoy-wm`'s raw numeric `OutputId` as its only
arg, NOT a Wayland connector name... there is no mapping between the two
anywhere in `buoy-wm`") — this story is what finally builds that mapping,
for `fuzzel`'s benefit specifically. (`status-bar`'s own similar gap is
not touched by this story — YAGNI until a real multi-output status-bar
story needs it; out of scope here.)

**Why bind `wl_output` directly instead of extending `river_output_v1`.**
`river_output_v1`'s own `WlOutput` event is explicit that it only carries
"the global name... advertised with `wl_registry.global`" — a registry
bookkeeping number, not the connector string. The protocol's own rationale
comment says exactly why a WM might want the real `wl_output` object:
"such as the name/description... also may need the `wl_output` object to
start screencopy for example." Binding it ourselves is the intended,
documented way to get this — not a workaround.

**Scope boundary.** This story does not add `wl_output`'s `description`
event, `xdg_output_manager` binding, or any output-geometry duplication
(`position`/`dimensions` are already tracked via `river_output_v1`, Story
2.8 — `wl_output`'s own `geometry`/`mode` events are redundant with that
and intentionally ignored here). It does not change `status-bar`'s spawn
args. It does not attempt to disable/reconfigure any output — that's a
`wlr-output-management-unstable-v1` concern, explicitly out of
`river-window-management-v1`'s (and thus this WM's) scope entirely; the
correct tool for that is an external output-management daemon (`kanshi`),
which is a separate, system-level concern outside this repo.

## Test plan
1. **`tag-picker` unit tests (TDD, RED before GREEN)**: `mode::parse_args`'s
   extended grammar — all four new/updated success cases plus the existing
   rejection cases re-verified against the wider argument-count range.
   Fully testable — pure argv parsing, no Wayland types involved.
2. **Build/lint gate**: `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` clean with the new `wl_output` binding, `Output`/`WindowManager` fields, and `do_action` signature change compiled in.
3. **Regression gate**: existing 268-test suite (`wm` 178, `status-bar` 27, `tag-picker` 63) unaffected beyond Task 4's intentional test updates.
4. **Live gate (manual, not automated)**: `fuzzel` opens on the pointer's actual monitor, including with the laptop lid closed, per Task 6.2.

## FR coverage
Tag-manager popup (assign/switch-mode picker) made correct under real
multi-output conditions Epic 1/2's sandbox testing couldn't exercise — not
a new FR, same framing as Stories 2.6/2.8.

## Dev Agent Record

**What was done**:

- `wm/src/main.rs` (Tasks 1-3):
  - `WindowManager` gained two new fields: `pending_wl_outputs:
    HashMap<u32, wl_output::WlOutput>` (keyed by registry name) and
    `wl_output_names: HashMap<ObjectId, String>` (keyed by the `wl_output`
    proxy's own id) (Task 1.1/1.2).
  - `wl_registry::Event::Global`'s handler gained a `"wl_output"` match
    arm: binds every advertised `wl_output` global at `version.min(4)`
    (non-fatal on a lower version) and inserts it into
    `pending_wl_outputs` (Task 1.1).
  - A new `impl Dispatch<wl_output::WlOutput, ()> for AppData` handles only
    `Event::Name { name }`, inserting into `wl_output_names` keyed by the
    proxy's `.id()`; every other `wl_output` event is ignored (Task 1.2).
  - `Output` gained `wl_output_object_id: Option<ObjectId>`, defaulting to
    `None` in `Output::new` (Task 2.1).
  - `impl Dispatch<RiverOutputV1, ()>`'s `Event::WlOutput { name }` arm
    (previously discarded) now looks `name` up in `pending_wl_outputs` and,
    if found, sets `wl_output_object_id` to that proxy's id; left `None`
    otherwise, no panic (Task 2.2).
  - A new `WindowManager::output_name(&self, output_id: OutputId) ->
    Option<&str>` method resolves an `OutputId` to its real connector name
    by scanning `outputs` for a matching `output_id`, then chaining through
    `wl_output_object_id` into a fresh `wl_output_names` lookup — `None` at
    any missing step, never cached, never panics (Task 2.3).
  - `manage_seats` now computes `active_output_name` (an owned `String`,
    for lifetime reasons) alongside `active_output_id`, before the
    `wm_core` mutable borrow begins, and passes
    `active_output_name.as_deref()` into `do_action` (Task 3.1).
  - `Seat::do_action` gained a new `active_output_name: Option<&str>`
    parameter (Task 3.2). `Action::OpenTagPicker` now appends the name as
    an extra `.arg()` when `Some`, with no extra arg when `None` (Task
    3.3). `Action::TagSwitch` appends it after the existing `switch
    <output_id>` args, same conditional shape (Task 3.4).
- `tag-picker/src/mode.rs` (Task 4, real RED/GREEN TDD): extended `Mode` to
  `Assign { output_name: Option<String> }` and `Switch { output_id: u64,
  output_name: Option<String> }`. Wrote/updated all eight tests first
  (`parse_args_with_no_arguments_is_assign_mode_with_no_output_name`,
  `parse_args_with_one_argument_is_assign_mode_with_output_name`,
  `parse_args_with_switch_and_valid_output_id_is_switch_mode_with_no_output_name`,
  `parse_args_with_switch_valid_output_id_and_name_is_switch_mode_with_output_name`,
  plus the four updated rejection tests) against the still-2-field-less
  enum, then actually verified RED before writing the extended
  implementation: temporarily restored the pre-story `Mode`
  enum/`parse_args` body underneath the new tests and ran `cargo test -p
  tag-picker mode::` in-container, confirming 6 real compile errors
  (`E0559`/`E0026`: "variant `Mode::Assign`/`Mode::Switch` has no field
  named `output_name`", both in the new tests and in `main.rs`'s existing
  `match mode` call sites). Then restored the extended implementation
  (`[]` / `[name]` / `[mode, id]` / `[mode, id, name]` match arms) and
  confirmed all 8 tests GREEN via `cargo test --workspace`, plus the 57
  other pre-existing `tag-picker` tests (63 baseline − 6 in `mode.rs`)
  unaffected.
- `tag-picker/src/main.rs` (Task 5): `run_fuzzel` gained an `output_name:
  Option<&str>` parameter; appends `.arg(format!("--output={name}"))` only
  when `Some`. Threaded through `run_assign_mode`/`run_switch_mode` (new
  parameter on each, passed to every `run_fuzzel` call site) and `main`'s
  `match mode { ... }` (destructures the new `output_name` field from each
  `Mode` variant, passes `.as_deref()` through).

**Deviations from the story's literal wording** (flagged explicitly, per
this project's established norm — see Story 2.7/2.8's own Dev Agent
Records):

1. **`parse_args_rejects_unknown_first_argument`'s test body changed, not
   just its assertion.** The story lists this among the "existing rejection
   tests... updated for the new one-extra-optional-arg grammar" without
   specifying new argument data. Its original input, a single `"frobnicate"`
   argument, is no longer rejectable at all under the extended grammar — a
   lone non-`"switch"` argument is now a *valid* assign-mode output name by
   design (Task 4.2's `[name] if name != "switch"` arm). Changed the test's
   input to two unrecognized arguments (`["frobnicate", "extra"]`), which is
   still rejected (2-length slices only match the `["switch", id]` shape),
   preserving the test's original intent ("garbage input is rejected")
   under the new grammar. Documented inline in the test itself.
2. **Edge case: a real connector literally named `"switch"` would fail to
   parse in assign mode.** `parse_args`'s `[name] if name != "switch"` guard
   means a single-argument invocation whose value is exactly the string
   `"switch"` falls through to the catch-all `Err` arm instead of being
   treated as an output name. Real Wayland connector names (`"eDP-1"`,
   `"HDMI-A-1"`, `"DP-2"`, etc.) never take this form, so this is
   considered unreachable in practice, but it is a real ambiguity in the
   grammar as specified (the story's own Task 4.2 plan already implies this
   guard is needed to keep `"switch"` unambiguously starting switch mode) —
   flagged here rather than silently accepted.
3. **`active_output_name` is computed as an owned `String`, not `&str`, in
   `manage_seats`.** The story's Task 3.1 literal code sketch
   (`.map(str::to_owned)`) already specifies this, so this isn't a
   deviation in outcome — noted only because the resulting `.as_deref()`
   dance at both the `do_action` call site and now also inside
   `Action::OpenTagPicker`/`Action::TagSwitch` (which receive `Option<&str>`
   and call `.arg(name)` directly, no further conversion needed) is worth
   confirming reads correctly: it does, confirmed via `cargo build`/
   clippy passing with no lifetime errors.

No other deviations identified; the implementation otherwise matches
Tasks 1-5's plans, including exact field names, method signatures, and
match-arm shapes as written.

**Verification** (all inside the devcontainer via `devpod ssh buoy-wm` —
worked reliably every call this session; the `podman exec` fallback was
never needed):
- `cargo fmt --all -- --check`: found formatting drift in the new
  multi-line signatures/enum variants; fixed via `cargo fmt --all`, then
  clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, no
  changes needed.
- `cargo test --workspace`: `buoy-wm` (wm) 178, `status-bar` 27,
  `tag-picker` 65 (63 baseline + 2 net new — Task 4's `mode.rs` tests went
  from 6 to 8: 4 renamed/updated in place for the new field shapes, 2
  genuinely new) = **270 total, 0 failures, 0 regressions**.
- `pre-commit run --all-files`: both configured hooks (`cargo fmt --check`,
  `cargo clippy`) passed, exit 0.

**Task 6.2 (manual live-verification)**: **deferred**, not performed by
this agent — no access to a live compositor, a physical second monitor, or
a laptop lid in this sandbox, same boundary as every prior story's
live-compositor gap (most recently Story 2.8's own Task 6.2). Confirming
`fuzzel` actually opens on the pointer's monitor with the lid closed,
against a real `river` session, still needs to be done by a human before
this story's Description's originating live-testing report can be
considered resolved end-to-end.

**File List**:
- `wm/src/main.rs`
- `tag-picker/src/mode.rs`
- `tag-picker/src/main.rs`

## Code Review

HIGH-effort workflow-backed review. Confirmed exactly the ambiguity the
implementer's own Dev Agent Record had already flagged as deviation #2,
plus two more findings. Three findings, all fixed:

1. **A connector literally named `"switch"` broke assign mode entirely.**
   The implementer's own flagged deviation turned out to be a real
   correctness bug, not just a theoretical ambiguity: `wm` now always
   spawns assign-mode `tag-picker` with the active output's real connector
   name as its single argument, and `parse_args`'s `[name] if name !=
   "switch"` guard meant that specific value fell through to the catch-all
   `Err` arm — `tag-picker` would `exit(1)` with no picker opening at all,
   silently, for any output happening to be named exactly `"switch"`.
   **Fixed**: the guard is removed; a single argument is unconditionally
   an assign-mode output name now, including the literal string
   `"switch"`. The one trade-off is losing a clearer error message for a
   hand-typed `tag-picker switch` missing its id (now interpreted as
   assign mode targeting an output named "switch", which — finding none —
   `fuzzel` simply falls back to its own default placement for). Given
   `wm` is the only real caller and real DRM connector names never
   actually collide with this in practice, prioritizing correctness for
   the real path over a hypothetical manual-typo's error message is the
   right trade. Test `parse_args_rejects_switch_with_missing_output_id`
   replaced with `parse_args_with_single_argument_literally_switch_is_assign_mode_with_that_name`,
   documenting the new intentional behavior.
2. **No defense against an empty output name producing a malformed
   `--output=` flag.** Not proven reachable today, but nothing validated
   non-emptiness anywhere in the chain, contradicting AC 2's explicit
   guarantee. **Fixed**: `run_fuzzel` now filters out an empty string
   before appending the flag, defending the boundary directly rather than
   trusting every upstream caller.
3. **`pending_wl_outputs`/`wl_output_names` leaked on every output
   removal.** `remove_outputs` (Story 2.8's removal path) never pruned
   either of this story's new maps, unlike the symmetric
   `wm_core.unregister_output` cleanup it already performs for the same
   event — an unbounded (if slow) memory leak across dock/undock cycles
   in this long-running daemon. **Fixed**: `pending_wl_outputs` is now
   drained via `.remove()` (not `.get()`) at the moment of successful
   correlation, since the entry serves no purpose afterward; `remove_outputs`
   additionally prunes the matching `wl_output_names` entry when an output
   is torn down.

**Re-verification after fixes**: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `pre-commit run
--all-files` all clean; `cargo test --workspace` — 270 total (`buoy-wm`
178, `status-bar` 27, `tag-picker` 65), 0 failures, unchanged (fix #1
replaced one test 1:1 with another documenting the corrected behavior;
fixes #2/#3 are `main.rs`-level Wayland-glue and untestable-without-a-
live-compositor changes, same carve-out as the rest of Tasks 1-3/5).

**Code review: PASS**.
