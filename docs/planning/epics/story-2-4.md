---
baseline_commit: 2bc0587
---

# Story 2.4: Tag-Switching via Picker
Epic: 2 | Priority: H | Status: done

## Description
Adds a second hotkey, `Mod4+S` ("Switch"), that spawns the same `tag-picker`
binary Story 2.2/2.3 already built — not a new binary, not a second
rendering path — in a new **switch mode**: a plain, un-checkboxed list of
every registry tag, single-select-by-position (choosing a row *is* the
action; there is no separate confirm and no reopen loop, unlike assign
mode's toggle-and-reopen shape). Selecting a tag sends `wm`'s already-
existing `{"type":"switch-tag","output_id":...,"tag_id":...}` IPC request
(`Request::SwitchTag`, `wm/src/ipc/protocol.rs:23`, dispatched by
`dispatch::handle_request` to `WmCore::switch_tag` since Story 2.1 — this
story adds **zero** new WM-side IPC request types), changing the target
output's displayed tag under the existing one-tag-per-output/reroute
enforcement (Story 1.3/ADR-005).

`output_id` is never independently computed by `tag-picker` itself. `wm`
resolves it once, at spawn time, the exact same way `Action::TagCycle`
already does (`WindowManager::active_output_id`, `wm/src/main.rs:399` — the
deterministic lowest-`OutputId` rule established in Story 1.7), and passes
it to the spawned `tag-picker switch <output_id>` process as a CLI
argument. This is the concrete answer to "reuse that same resolution, don't
re-derive a different one": there is exactly one place in this codebase
that decides what "the active output" means, and this story threads its
existing answer across a process boundary rather than growing a second,
independently-arrived-at answer inside `tag-picker`.

**One genuine functional gap, closed here rather than silently shipped
half-done** (see Technical notes for full rationale): `wm/src/ipc/
dispatch.rs`'s `SwitchTag` arm currently only calls `WmCore::switch_tag`
and returns `Response::Ok`/`Error` — unlike `Action::TagCycle`'s keybind
path (Story 1.7, via `WindowManager::manage_seats` →
`ensure_pinned_terminal_spawned`), it never claims or spawns that tag's
pinned terminal. The PRD's own top-level "Pinned terminal lifecycle" AC
("First switch to a tag with no terminal yet spawns `foot -a pinned-term`
running `zellij attach --create tag-<name>`") is mechanism-agnostic, and
`EXPERIENCE.md`'s Flow B — the exact switch-hotkey flow this story
implements — narrates it explicitly: "if `chat`'s pinned zellij session
hasn't been spawned yet this session, the WM lazily spawns it." Since this
story is the first to make `switch-tag` reachable by a real user action
(previously only reachable via a raw IPC call, e.g. `nc`/`socat` per
ADR-007, or transitively via `cycle_tag`'s own already-wired keybind path),
it is the correct, and only remaining, place to close this gap — not a
scope-creeping addition invented here, but a direct, necessary consequence
of wiring a second live caller onto `switch_tag` for the first time.

## Acceptance criteria

**Given** at least one output is registered (`WindowManager::active_output_id`
returns `Some`)
**When** I press `Mod4+S` from anywhere — no window needs to be focused, and
none needs to exist (unlike `Mod4+A`'s "no window focused" no-op case)
**Then** `wm` resolves the active output via the same deterministic
lowest-`OutputId` rule `Action::TagCycle` already uses, and spawns the same
`tag-picker` binary (resolved via the existing `tag_picker_path` sibling-
executable lookup, `wm/src/main.rs:551`) with args `["switch",
"<output_id>"]`
**And** `tag-picker` opens `fuzzel --dmenu` showing one plain row per
registry tag — name only, no `[✓]`/`[ ]` checkbox glyph prefix, and no
free-text/create-tag row — per `EXPERIENCE.md` Component Patterns: "not
shown at all in switch mode... Not present in switch mode (switching only
operates on existing tags)"

**Given** no output is registered yet (`active_output_id` returns `None` —
e.g. a spurious keypress before the compositor has announced any output)
**When** I press `Mod4+S`
**Then** `wm` logs and takes no action — `tag-picker` is never spawned — the
same defensive shape as `Action::TagCycle`'s own `None` arm
(`wm/src/main.rs:764-767`)

**Given** the switch-mode picker is open, showing the full tag registry
**When** I select an existing tag (a real, matched row — `fuzzel`'s
`--accept-nth=2` prints that tag's bare id to stdout on exit success)
**Then** `tag-picker` sends `{"type":"switch-tag","output_id":<the id it was
launched with>,"tag_id":<selected id>}` over a fresh IPC connection and,
regardless of the response, exits — there is no reopen loop in switch mode
(EXPERIENCE.md: "single-select-by-position... no separate confirm")
**And** on `{"type":"ok"}`, the target output's displayed tag has changed,
subject to the existing one-tag-per-output/reroute enforcement
(`WmCore::switch_tag`, unchanged, `wm/src/wm_core/state.rs:325`) — no new
`wm-core` logic of any kind is added by this story
**And** if `tag_id`'s pinned terminal has not yet been spawned this
session, `wm` lazily spawns it (`foot -a pinned-term zellij attach --create
tag-<name>`) as a direct, necessary side effect of `switch-tag` now being
reachable by a real user action for the first time (Description's "genuine
functional gap," closed by Task 2) — the same terminal-spawn behavior
`Action::TagCycle`'s keybind path already guarantees, now also guaranteed
via this picker path
**And** the whole round trip (one `wm-core` mutator call, one already-
tested `claim_pinned_terminal_spawn` call, one JSON line each way) is
structurally bounded well under the 50ms budget (NFR1) — see Technical
notes "NFR1," same non-benchmark structural argument as every prior Epic 2
story

**Given** the switch-mode picker is open
**When** I press Escape, or the `fuzzel` process otherwise exits
unsuccessfully, or its stdout is empty
**Then** `tag-picker` sends no `switch-tag` request and exits — no output's
displayed tag changes

**Given** `wm` returns any `switch-tag` error (`"unknown output"`/`"unknown
tag"` — both practically unreachable here since `output_id` comes from
`wm`'s own live `active_output_id` and `tag_id` comes from a row `tag-picker`
itself just rendered from a fresh `get-state`, but not literally
impossible under a pathological race)
**Then** `tag-picker` prints it to stderr and exits non-zero — no silent
failure, same generic-error handling precedent as assign mode's
`toggle-tag`/`create-tag` error paths (Stories 2.2/2.3)

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: Resolve the mode-encoding, active-output-sourcing, and pinned-terminal-gap design decisions (no RED/GREEN — a documented design decision gating every later task, same class as Story 2.2's Task 1 spike and Story 2.3's Task 1)**
  - [x] 1.1 Record the mode-encoding rule: `tag-picker` invoked with zero CLI args (`std::env::args().skip(1).collect::<Vec<_>>().is_empty()`) is assign mode — Story 2.2/2.3's existing toggle-and-reopen loop, byte-for-byte unchanged, so `Mod4+A`'s existing behavior and every existing assign-mode test keeps passing with no changes. Invoked as `tag-picker switch <output_id>` (exactly two args, literal `"switch"` then a `u64`) is switch mode. Any other argument shape (wrong count, first arg not literally `"switch"`, second arg not a valid `u64`) is a startup error — `tag-picker` prints to stderr and exits non-zero before ever connecting to the socket, the same "fail closed on malformed input" discipline `wm`'s own `parse_request` applies (NFR2-style, extended to this client per Story 2.2/2.3 precedent).
  - [x] 1.2 Record why `output_id` is a CLI argument, not something `tag-picker` derives itself from `get-state`: `wm`'s own `WindowManager::active_output_id` (`wm/src/main.rs:392-401`) is already the single, deterministic, documented "active output" resolution (lowest-`OutputId`, a conscious scope boundary pending real focused-output tracking). Adding a second, independent "which output" computation inside `tag-picker` — even one that happened to produce the same answer today by reading `OutputDto`s from `get-state` and taking the min — would be exactly the kind of re-derivation the orchestrator's own instruction warns against, and would silently diverge the day `active_output_id`'s rule changes (e.g. once real focused-output tracking lands) since nothing would force the two copies to change together. `wm` computes the answer once, at spawn time, and hands it across the process boundary as a plain argument; `tag-picker` treats it as opaque.
  - [x] 1.3 Record the pinned-terminal-lazy-spawn gap and its resolution (Description's "genuine functional gap"): `dispatch::handle_request`'s `SwitchTag` arm (`wm/src/ipc/dispatch.rs:83-90`) calls `wm_core.switch_tag(...)` and returns `Response::Ok`/`Error` — it never calls `wm_core.claim_pinned_terminal_spawn`. `Action::TagCycle`'s keybind path (`wm/src/main.rs:755-768`, Story 1.7) returns `Some(tag_id)` back up to `manage_seats`, which calls `WindowManager::ensure_pinned_terminal_spawned(tag_id)` (`wm/src/main.rs:330-338`) afterward. No such follow-up exists on the IPC path. The PRD's "Pinned terminal lifecycle" AC and `EXPERIENCE.md`'s Flow B both describe this as expected regardless of *how* the switch happened; `wm/src/wm_core/state.rs`'s own test `claim_pinned_terminal_spawn_reachable_via_switch_tag_alone_without_any_keybind` (added in an earlier story, before any real caller existed) already anticipates exactly this reachability at the `wm-core` layer. **Resolution (Task 2):** extend `handle_request`'s `SwitchTag` arm itself, rather than inventing a new WM-side request type or pushing this decision into `server.rs`. `handle_request`'s signature stays `fn handle_request(wm_core: &mut WmCore, request: Request) -> Response` — unchanged — so no existing call site (`server.rs`'s `handle_connection_inner`, every existing `dispatch.rs` test) needs updating. `crate::spawn_pinned_terminal` (`wm/src/main.rs:566-580`) is a private fn at the binary crate's root module; per Rust's visibility rule that a private item is visible in its defining module and all descendants, `wm::ipc::dispatch` (a descendant of the crate root) can already call `crate::spawn_pinned_terminal(&session_name)` directly with **no visibility changes anywhere** — confirmed by inspection, not assumed. `dispatch.rs`'s own module doc comment ("so it can be reviewed and tested with zero socket/thread machinery in the loop") is corrected to note this one process-spawn exception: it is not "socket/thread machinery" in the sense that sentence means (no socket, no new thread), it is the same class of fire-and-forget `Command::spawn()` call this codebase already treats as untested I/O glue everywhere else (`Action::SpawnFoot`, `Action::OpenTagPicker`, `ensure_pinned_terminal_spawned` itself).
  - [x] 1.4 Record the accepted "picker follows focus" assumption, flagged not hidden: `EXPERIENCE.md`'s Interaction Primitives states "the picker always opens on the currently-focused output, never a fixed 'home' output" as a universal rule across both modes. Neither this story nor Story 2.2/2.3 adds an `--output=<name>` flag to `run_fuzzel`'s `Command::new("fuzzel")` invocation — assign mode never added one and was never challenged on this point, so switch mode reuses `run_fuzzel` completely unmodified here too (consistent with FR9's "reused unmodified"). The working assumption, carried forward unverified (no live Wayland/fuzzel session exists in this sandbox — same disclosed-gap class as every prior Epic 2 story): a layer-shell client spawned with no explicit output binding renders on the compositor's own default choice, which for a seat-focused-output selection is `river`'s own responsibility, not `tag-picker`'s. If a future live smoke test finds this assumption wrong, adding an explicit output-targeting flag is a follow-up, not a regression introduced here.
  - [x] **No RED/GREEN** — documented design decisions only, gating Tasks 2-7's implementation, same carve-out class as every prior Epic 2 story's own Task 1.

- [x] **Task 2: `wm/src/ipc/dispatch.rs` — close the pinned-terminal-lazy-spawn gap on the `SwitchTag` path (AC: "if `tag_id`'s pinned terminal has not yet been spawned... `wm` lazily spawns it")**
  - [x] 2.1 RED — In `dispatch.rs`'s existing `#[cfg(test)] mod tests`: `switch_tag_success_claims_pinned_terminal_spawn_for_the_target_tag` — register an output and a tag, call `handle_request(&mut core, Request::SwitchTag{output_id, tag_id})`, assert the response is `Response::Ok`, then assert `core.claim_pinned_terminal_spawn(tag_id) == Ok(None)` — proving `handle_request` itself already consumed the claim as a side effect (the tri-state's second call always returns `Ok(None)` once claimed, `wm/src/wm_core/state.rs`'s existing `claim_pinned_terminal_spawn_is_idempotent_returns_none_after_first_claim` precedent), without needing to intercept the real process spawn. `switch_tag_error_does_not_claim_pinned_terminal_spawn` — call `handle_request` with a bogus `tag_id` (expect `Response::Error`), then assert `core.claim_pinned_terminal_spawn(<the same bogus tag_id>) == Err(WmCoreError::UnknownTag)` (unchanged from calling it directly — proving the claim is never attempted when `switch_tag` itself failed, since claiming for a tag the switch never actually touched would be a spurious side effect on an error path). Confirm both fail to compile/fail as expected (the second currently trivially "passes" only because there is no claim call yet either way — restate its assertion as the *absence* of a claim: additionally assert the *same* tag's terminal is still claimable exactly once afterward via a direct `claim_pinned_terminal_spawn` call succeeding with `Ok(Some(_))`, which would already be true before this task's GREEN and must remain true after — this is the regression guard against a hypothetical future bug that claims on the error path too).
  - [x] 2.2 GREEN — In `handle_request`'s `Request::SwitchTag` arm, on `Ok(())` from `wm_core.switch_tag(...)`: call `wm_core.claim_pinned_terminal_spawn(tag_id)`; on `Ok(Some(session_name))`, call `crate::spawn_pinned_terminal(&session_name)` (no visibility change needed — Task 1.3); on `Ok(None)` (already spawned this session), do nothing further; a `claim_pinned_terminal_spawn` `Err` here is unreachable in practice (`tag_id` was just proven valid by the preceding successful `switch_tag` call against the same registry) but if it somehow occurred, `eprintln!` and continue — never let a terminal-spawn-claim failure turn a successful tag switch into an error response, since the switch itself already fully succeeded and reporting it as failed would be a lie to the client. Return `Response::Ok` exactly as before. Update `handle_request`'s and the module's own doc comments per Task 1.3's correction. Confirm both new tests pass; confirm zero regressions in the existing suite (every other `Request` variant's arm is untouched).

- [x] **Task 3: `tag-picker/src/wire.rs` — add `SwitchTag` to the client's protocol mirror (AC: "`tag-picker` sends `switch-tag`...")**
  - [x] 3.1 RED — In `wire.rs`'s existing `#[cfg(test)] mod tests`: `serializes_switch_tag_request` — `serialize_request(&Request::SwitchTag{output_id:0,tag_id:2})` equals `r#"{"type":"switch-tag","output_id":0,"tag_id":2}"#` (byte-for-byte matching `wm`'s own `protocol.rs`'s `parses_switch_tag` fixture, so the two ends are provably compatible). Confirm it fails to compile (`Request::SwitchTag` doesn't exist yet on this client's narrower `Request` enum).
  - [x] 3.2 GREEN — Add `SwitchTag { output_id: u64, tag_id: u8 }` to the client's `Request` enum (`wire.rs:23-27`), mirroring `wm`'s own shape exactly. Update the module's own doc comment, which currently states "`tag-picker` never sends `switch-tag` (Story 2.4's job)" — that sentence is now wrong and must say the client sends `get-state`/`toggle-tag`/`create-tag`/`switch-tag` and understands `state`/`ok`/`tag-created`/`error`. No new `Response` variant needed — `switch-tag` only ever produces `Ok`/`Error`, both already modeled. Confirm the new test passes; confirm zero regressions.

- [x] **Task 4: `tag-picker` — a new pure CLI-argument-to-mode decision (AC: mode dispatch, the malformed-arguments startup-error path)**
  - [x] 4.1 RED — In a new `tag-picker/src/mode.rs` (or, if the team prefers fewer files, a new `mod tests` block appended to `main.rs` guarded the same way `checklist.rs`'s own tests are — either is acceptable, this task assumes the dedicated-module form for testability without `main.rs`'s own no-RED/GREEN carve-out swallowing it): `#[derive(Debug, Clone, Copy, PartialEq)] pub enum Mode { Assign, Switch { output_id: u64 } }`. Tests: `parse_args_with_no_arguments_is_assign_mode` — `parse_args(&[])` equals `Ok(Mode::Assign)`. `parse_args_with_switch_and_valid_output_id_is_switch_mode` — `parse_args(&["switch".into(), "3".into()])` equals `Ok(Mode::Switch{output_id:3})`. `parse_args_rejects_switch_with_non_numeric_output_id` — `parse_args(&["switch".into(), "not-a-number".into()])` is `Err(_)`. `parse_args_rejects_switch_with_missing_output_id` — `parse_args(&["switch".into()])` is `Err(_)`. `parse_args_rejects_unknown_first_argument` — `parse_args(&["frobnicate".into()])` is `Err(_)`. `parse_args_rejects_too_many_arguments` — `parse_args(&["switch".into(), "3".into(), "extra".into()])` is `Err(_)` (defensive — no legitimate reason for a third argument to ever appear from `wm`'s own spawn call, but malformed/attacker-adjacent input is never silently ignored, same NFR2-style discipline as `wm`'s own `parse_request`). Confirm all fail to compile.
  - [x] 4.2 GREEN — `pub fn parse_args(args: &[String]) -> Result<Mode, String>`: `match args { [] => Ok(Mode::Assign), [mode, id] if mode == "switch" => id.parse::<u64>().map(|output_id| Mode::Switch{output_id}).map_err(|_| format!("invalid output id: {id}")), _ => Err(format!("usage: tag-picker [switch <output_id>], got: {args:?}")) }`. `///` doc comments on `Mode` and `parse_args` explaining the zero-args-is-assign-mode backward-compatibility property (Task 1.1). Confirm all new tests pass.

- [x] **Task 5: `tag-picker/src/checklist.rs` — switch-mode rendering and output-parsing (AC: plain list with no checkbox/free-text row, single-select decision)**
  - [x] 5.1 RED — Extend `checklist.rs`'s existing `#[cfg(test)] mod tests`: `render_switch_list_formats_one_plain_row_per_tag` — `render_switch_list(&[tag(0,"web"), tag(1,"chat")])` equals `"web\t0\nchat\t1\n"` (name + tab + bare id, no `[✓]`/`[ ]` glyph prefix — contrast directly with `render_fuzzel_input`'s existing glyph-prefixed format, doc comment cross-referencing why the two renderers deliberately differ). `render_switch_list_of_empty_tags_is_empty_string`. `render_switch_list_replaces_embedded_tab_and_newline_in_tag_name` (same corruption-avoidance rationale as `render_fuzzel_input`'s existing equivalent test — this is the second real occurrence of that sanitization rule, not yet a three-strike DRY extraction candidate on its own, though Task 5.2's implementation may still call a small shared sanitizer helper if that reads more clearly — see 5.2). `parse_switch_selection_returns_selected_for_a_known_tag_id` — `parse_switch_selection(true, "1\n", &[0,1,2])` equals `SwitchAction::Selected(1)`. `parse_switch_selection_returns_cancelled_for_nonzero_exit` — `parse_switch_selection(false, "1\n", &[0,1,2])` equals `Cancelled` (exit status still wins, same principle as `parse_fuzzel_output`). `parse_switch_selection_returns_cancelled_for_empty_stdout` — `parse_switch_selection(true, "", &[0,1,2])` equals `Cancelled`. `parse_switch_selection_returns_cancelled_for_freeform_text_not_a_known_id` — `parse_switch_selection(true, "deploy-watch\n", &[0,1,2])` equals `Cancelled` — the deliberate, load-bearing difference from assign mode's `parse_fuzzel_output`: switch mode has no create-tag branch at all (EXPERIENCE.md: "switching only operates on existing tags"), so anything that isn't a real matched row is simply cancelled, never treated as a create attempt. `parse_switch_selection_returns_cancelled_for_a_numeral_not_among_known_ids` — `parse_switch_selection(true, "99\n", &[0,1,2])` equals `Cancelled` (same reasoning — an in-range but unregistered numeral is not a valid selection in switch mode, unlike assign mode where it would become a `CreateTag` attempt). `parse_switch_selection_trims_whitespace_consistently_with_toggle_parsing` — `parse_switch_selection(true, " 1 \n", &[0,1,2])` equals `Selected(1)`. Confirm all fail to compile.
  - [x] 5.2 GREEN — `#[derive(Debug, Clone, Copy, PartialEq)] pub enum SwitchAction { Selected(u8), Cancelled }`. `pub fn render_switch_list(tags: &[TagDto]) -> String`: `tags.iter().map(|t| format!("{}\t{}\n", t.name.replace(['\t','\n'], " "), t.id)).collect()` (reusing the identical embedded-tab/newline sanitization `render_fuzzel_input` already applies — a small private `fn sanitize_name(name: &str) -> String { name.replace(['\t','\n'], " ") }` helper factored out and called from both renderers is the cleaner, non-premature choice here, since this is exactly the three-strike threshold's second real occurrence and a one-line, obviously-correct extraction, not speculative abstraction). `pub fn parse_switch_selection(exit_success: bool, stdout: &str, known_tag_ids: &[u8]) -> SwitchAction`: `if !exit_success { return Cancelled }`; `let trimmed = stdout.trim(); if trimmed.is_empty() { return Cancelled }`; `match trimmed.parse::<u8>() { Ok(id) if known_tag_ids.contains(&id) => Selected(id), _ => Cancelled }` — structurally the same shape as `parse_fuzzel_output` minus the `CreateTag` catch-all arm, doc comment cross-referencing that deliberate difference. `///` doc comments throughout. Confirm all new tests pass; confirm zero regressions in `parse_fuzzel_output`'s own existing tests (untouched — assign mode's decision logic is not modified by this task).

- [x] **Task 6: `tag-picker/src/main.rs` — branch on `Mode`, add the switch-mode single-shot flow (AC: all — process-spawn/live-socket glue, same carve-out class as Story 2.2's Task 6/Story 2.3's Task 4)**
  - [x] 6.1 Extract the existing `main()` body's connection setup (socket connect, `get-state` send/receive) into a small helper reused by both modes — e.g. `fn connect_and_get_state() -> (UnixStream-derived writer/reader, wire::Response's tags/views/focused_view fields)` — so switch mode doesn't duplicate that boilerplate. Assign mode's own loop (the existing `pending_rejected_name`/`pending_create_apply_failed`/toggle-and-reopen logic) is otherwise moved into its own function, e.g. `fn run_assign_mode(writer, reader, tags, views, focused_view)`, with **no behavioral change whatsoever** — this is a pure mechanical extraction, verified by the fact that every existing Story 2.2/2.3 unit test in `checklist.rs`/`wire.rs` (which this task does not touch) keeps passing unmodified.
  - [x] 6.2 Add `fn run_switch_mode(writer, reader, tags: Vec<wire::TagDto>, output_id: u64)`: compute `known_ids: Vec<u8> = tags.iter().map(|t| t.id).collect()`; render `checklist::render_switch_list(&tags)`; call `run_fuzzel(&input, None)` (no `initial_search` — switch mode never has a rejection row to restore); call `checklist::parse_switch_selection(exit_success, &stdout, &known_ids)`. On `SwitchAction::Cancelled`, return (no request sent, no reopen). On `SwitchAction::Selected(tag_id)`: send `wire::Request::SwitchTag{output_id, tag_id}` on the connection; on `Response::Ok`, return (success, no further action needed client-side — the pinned-terminal spawn, if any, happens entirely `wm`-side per Task 2); on `Response::Error{message}`, `eprintln!("tag-picker: {message}")` and exit non-zero; on an unexpected response or a send/read failure, `eprintln!` and exit non-zero — same generic-error handling shape as every existing assign-mode error path, just with no loop to `break` out of since this is inherently single-shot.
  - [x] 6.3 In `main()`: call `mode::parse_args(&std::env::args().skip(1).collect::<Vec<_>>())`; on `Err(message)`, `eprintln!("tag-picker: {message}")` and `std::process::exit(1)` **before connecting to the socket at all** (Task 1.1 — malformed invocation is a pure argument-shape problem, not something a live connection could fix). On `Ok(Mode::Assign)`, call `connect_and_get_state()` then the existing focused-view-required check (`checklist::should_open_picker`, unchanged) then `run_assign_mode(...)` — byte-for-byte the same runtime behavior as before this story, just reorganized into functions. On `Ok(Mode::Switch{output_id})`, call `connect_and_get_state()` then `run_switch_mode(writer, reader, tags, output_id)` directly — switch mode has no focused-view precondition at all (AC: "no window needs to be focused, and none needs to exist"), so `should_open_picker`'s check is deliberately not applied on this branch.
  - [x] **No RED/GREEN** — process-spawn and live-socket I/O glue, same carve-out class as Story 2.2's Task 6/Story 2.3's Task 4: every decision this composes (`parse_args`, `render_switch_list`, `parse_switch_selection`, `serialize_request`/`parse_response`) is unit-tested in Tasks 3-5; this task is pure composition plus the mechanical Task 6.1 extraction. Verified via `cargo build --workspace`/`cargo clippy --workspace --all-targets -- -D warnings` and structural code review against 6.1-6.3's description above.

- [x] **Task 7: `wm/src/main.rs` — the `Mod4+S` keybind and `Action::TagSwitch` (AC: "I press `Mod4+S`...", the no-output-registered no-op)**
  - [x] 7.1 Add `Action::TagSwitch` to the `Action` enum (`wm/src/main.rs:63-74`), alongside `TagCycle`/`TagCreate`/`OpenTagPicker`.
  - [x] 7.2 In `init_new_seats` (`wm/src/main.rs:340-371`): add `const S: u32 = 0x73;` (see `xkbcommon/xkbcommon-keysyms.h`, same single-letter-mnemonic convention this file already documents for `A`/"Assign") and `seat.create_xkb_binding(river_xkb, qh, mods, S, Action::TagSwitch);`.
  - [x] 7.3 In `Seat::do_action`'s `Action::TagSwitch` arm (mirroring `Action::TagCycle`'s existing `match active_output_id` shape at `wm/src/main.rs:755-768`, and `Action::OpenTagPicker`'s existing `tag_picker_path`/spawn/`env_remove("WAYLAND_DEBUG")` shape at `wm/src/main.rs:792-808`): `match active_output_id { Some(output_id) => { match std::env::current_exe() { Ok(wm_exe) => { match std::process::Command::new(tag_picker_path(&wm_exe)).arg("switch").arg(output_id.0.to_string()).env_remove("WAYLAND_DEBUG").spawn() { Ok(_) => {}, Err(e) => eprintln!("Failed to spawn tag-picker in switch mode: {e}") } }, Err(e) => eprintln!("Failed to resolve wm's own executable path: {e}") } }, None => eprintln!("Tag-switch keybind pressed but no output is registered yet") }; None` — always returns `None` (never itself mutates `wm_core` or triggers `manage_seats`' pinned-terminal-spawn signal; the eventual `switch-tag` IPC call and Task 2's follow-up pinned-terminal spawn both happen later, asynchronously, once the user picks a tag in the spawned process — the same "fire-and-forget spawn, no wm_core access at spawn time" shape `Action::OpenTagPicker` already established).
  - [x] **No RED/GREEN** — Wayland-`Dispatch`/keybind-registration/process-spawn glue, the same carve-out class as `Action::OpenTagPicker`'s own Story 2.2 Task 7 and every other `wm/src/main.rs` action arm. Verified via `cargo build`/`cargo clippy --all-targets -D warnings` and structural review confirming the arm's shape matches `Action::TagCycle`'s `None`-handling and `Action::OpenTagPicker`'s spawn-error-handling precedents exactly.

- [x] **Task 8: Full in-container verification gate, across the workspace (AC: all)**
  - [x] 8.1 Run, inside the devcontainer — `devpod ssh buoy-wm -- <cmd>` first; fall back to `podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>` if that transport is unreliable again (confirmed unreliable in every prior Epic 2 story's own session) — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, all from the repo root. Confirm the full pre-existing `wm` (147) + `tag-picker` (47, per Story 2.3's final count) suite stays green apart from this story's own additions — 2 new `dispatch.rs` tests, 1 new `wire.rs` test, 6 new `tag-picker::mode` tests, 9 new `checklist.rs` tests (18 new tests total: 149 `wm` + 63 `tag-picker`, expected) — and the Task 6.1 mechanical extraction (no test-visible behavior change).
  - [x] 8.2 Run `pre-commit run --all-files` as the `vscode` user inside the container — confirm the workspace-wide `cargo fmt --check`/`cargo clippy` hooks (already covering both members since Story 2.2) still pass with no further hook changes needed (this story adds no new Cargo targets).
  - [x] 8.3 Manual smoke-test note (same explicit, not-downplayed gap class as every prior Epic 2 story): no live `fuzzel`/Wayland session exists in this sandbox. Beyond Stories 2.2/2.3's already-carried-forward gaps, this story adds three more live-only unknowns: the real plain-list rendering (no checkbox glyphs) actually displaying legibly under `--with-nth=1`, the real single-shot exit-without-reopen behavior, and Task 1.4's "picker follows focus" assumption (no explicit `--output` flag). Every *decision* is unit-tested (Tasks 2-5, 8.1); the live gap is recorded here and carried forward.

## Technical notes

**No new WM-side IPC request type (constraint from the story brief,
confirmed by inspection, not assumed).** `wm/src/ipc/protocol.rs:23`
already declares `SwitchTag { output_id: u64, tag_id: u8 }` on `Request`,
and `dispatch::handle_request`'s existing `Request::SwitchTag` arm
(`wm/src/ipc/dispatch.rs:83-90`) already dispatches it to
`WmCore::switch_tag` with full test coverage
(`switch_tag_calls_wm_core_switch_tag_and_returns_ok`,
`switch_tag_unknown_output_returns_error`,
`switch_tag_unknown_tag_returns_error`, plus the cross-variant
never-panics sweep) — all landed in Story 2.1, ahead of any real caller,
per that story's own AC requiring IPC mutations reuse the exact functions
Story 1.7's raw keybinds already exercise. This story's only WM-side
`dispatch.rs` change (Task 2) is additive behavior on the *existing* arm
(the pinned-terminal-spawn follow-up), not a new request/response shape —
the wire contract between `tag-picker` and `wm` for `switch-tag` itself is
already fully specified and needs no story-level design work here.

**The pinned-terminal-lazy-spawn gap — why this story, not a separate one,
closes it.** Before this story, `switch_tag`'s only real caller was
`Action::TagCycle`'s keybind path (Story 1.7), which already gets its
pinned-terminal follow-up via `manage_seats`
(`ipc::lock_recovering(&self.wm_core).claim_pinned_terminal_spawn(tag_id)`
→ `spawn_pinned_terminal`, `wm/src/main.rs:330-338`) — a call site entirely
outside `dispatch.rs`/`ipc::server`. The IPC path (`dispatch::
handle_request`'s `SwitchTag` arm) was reachable in isolation only via a
raw hand-crafted socket write (`nc`/`socat`, ADR-007's own acknowledged
debugging affordance) — no real, user-facing feature ever drove it until
this story adds `tag-picker switch`. Closing the gap anywhere *other* than
this story would mean either (a) never closing it, silently shipping a
user-visible regression relative to `Mod4+Tab`'s existing behavior the
first time someone switches to a fresh tag via the picker, or (b) a
separate bugfix story with no real trigger to justify it before this one
lands. Fixing it here, as a documented, load-bearing part of wiring
`switch-tag`'s second real caller, is the correct scope boundary — the
same "close the gap in the story that first makes it reachable" judgment
this project already applied to Story 2.3's own four numbered gaps.

**Why `handle_request`'s signature doesn't need to change.** An
alternative design would have `handle_request` return `(Response,
Option<String>)` (the pinned-terminal session name, when one needs
spawning) and push the actual `Command::spawn()` call out to
`server.rs`'s `handle_connection_inner`, keeping `dispatch.rs` itself
`Command`-free. This was considered and rejected: it would force every
existing `dispatch.rs` test and both of `server.rs`'s and `main.rs`'s call
sites to be rewritten for a distinction (which module literally contains
the `Command::new` call) that doesn't change what gets tested or how —
either way, `claim_pinned_terminal_spawn`'s *decision* is `wm-core`-level
and already fully tested, and the actual OS process spawn is, either way,
untested I/O glue verified only by build/clippy/structural review. Calling
`crate::spawn_pinned_terminal` directly from `dispatch.rs`'s existing
`fn handle_request(wm_core: &mut WmCore, request: Request) -> Response`
achieves the identical effect with a strictly smaller diff and zero
call-site churn. `dispatch.rs`'s own doc comment is updated (Task 2.2) to
disclose this one exception rather than leaving its "zero socket/thread
machinery" claim quietly inaccurate.

**Switch mode's single-shot shape vs. assign mode's toggle-and-reopen
loop — not the same interaction pattern, deliberately.** Story 2.2's own
Technical notes ("Spike finding") established the toggle-and-reopen loop
specifically because assign mode needs to reflect a live checkbox-state
change and let the user keep toggling further tags without re-invoking the
hotkey. `EXPERIENCE.md`'s switch-mode row description — "is not shown at
all in switch mode (single-select-by-position — selecting *is* the action,
no separate confirm)" — describes a fundamentally different interaction:
there is nothing to toggle, membership isn't the concept at all, and the
*only* thing that can happen after a real selection is the terminal action
itself. Building switch mode on top of the toggle-and-reopen loop's
machinery (e.g. by treating every selection as "toggle then immediately
break") would work but would carry along state (`pending_rejected_name`,
`pending_create_apply_failed`, a `current_tags`/membership vector) that
means nothing in this mode and would need to be defensively never-touched
throughout — a worse shape than the two modes' `main.rs` functions simply
being separate, small, and independently readable, which Task 6.1's
extraction achieves without duplicating anything: both modes still share
`connect_and_get_state`, `run_fuzzel`, and the wire-level request/response
plumbing, only the reopen-loop-vs-single-shot control flow differs.

**NFR1 (50ms budget).** No literal benchmark is added, consistent with
every prior story's own treatment (`docs/planning/epics/story-2-1.md`'s
Technical notes "NFR1 (50ms budget)" — a sandboxed CI timing assertion
would be flaky, not diagnostic). The structural argument, extended from
Story 2.1's own: Task 2's addition to `dispatch::handle_request`'s
`SwitchTag` arm is at most one already-NFR1-accepted `wm_core.switch_tag`
call, plus one already-tested O(1) `claim_pinned_terminal_spawn` call
(a single `HashMap` lookup and flag flip — `wm/src/wm_core/state.rs:396+`),
plus, only on the rare first-switch-to-a-fresh-tag case, one
fire-and-forget `Command::spawn()` call that does not block on the
child process (the same non-blocking shape `ensure_pinned_terminal_spawned`
already relies on for the keybind path, never re-litigated as a new
concern here) — no new blocking I/O, no unbounded work, mutex held only
for the one `handle_request` call exactly as before.

**Consistency with the existing three-strike DRY rule (Task 5.2's
`sanitize_name` extraction).** `render_fuzzel_input`'s existing embedded-
tab/newline sanitization (`.replace(['\t', '\n'], " ")`, added as a Story
2.2 code-review follow-up) and `render_switch_list`'s new equivalent need
are the same one-line operation solving the same real corruption risk
(`wm_core::create_tag` accepts any string with no charset validation,
ADR-006/YAGNI) against the same tab-delimited `--with-nth`/`--accept-nth`
wire format both renderers emit. This is the second real occurrence, not
the third — strictly, the project's own three-strike rule would tolerate
a second inline duplicate a while longer — but a private, zero-ambiguity,
one-line helper shared by two functions already living in the same file is
not "premature abstraction" in the sense YAGNI/three-strike guards against
(no speculative generality, no new public surface, no interface designed
for a hypothetical third caller); it is the same judgment call this
project already made for other same-file, same-shape helpers. If this
reads as over-cautious during actual implementation, inlining the
duplicate `.replace(...)` call a second time instead is an acceptable,
equally-conforming alternative — this note flags the choice, it does not
mandate the extraction.

No new ADR. Every decision above is an implementation-decision gap-fill
within this story's own existing scope (matching Story 2.1's four, Story
2.2's five, and Story 2.3's four precedents), not a decision among
architecturally significant alternatives `adrs.md` needs to record — in
particular, the pinned-terminal-spawn wiring reuses `wm_core::
claim_pinned_terminal_spawn` and `spawn_pinned_terminal` completely
unmodified, and the IPC wire contract (`SwitchTag`) was already decided in
Story 2.1/ADR-007.

Build/test/lint only ever run **inside the devcontainer**, `devpod ssh
buoy-wm` first, falling back to `podman exec -u vscode -w
/workspaces/buoy-wm bold_vaughan <cmd>` if that transport is unreliable —
confirmed unreliable in every prior Epic 2 story's own session, expect the
same here.

## Test plan
This story's tests split into five tiers:

1. **Design resolution** (Task 1) — no automated tests; the deliverable is
   four documented decisions (mode-encoding, active-output sourcing, the
   pinned-terminal-gap closure design, the "picker follows focus"
   assumption), each gating a later task's implementation, sourced from
   direct inspection of the existing codebase (`wm/src/main.rs`,
   `wm/src/ipc/dispatch.rs`, `wm/src/wm_core/state.rs`) rather than
   invented fresh.
2. **`wm/src/ipc/dispatch.rs`** (Task 2) — two new unit tests exercising
   the `SwitchTag` arm's new pinned-terminal-claim side effect
   (`switch_tag_success_claims_pinned_terminal_spawn_for_the_target_tag`,
   `switch_tag_error_does_not_claim_pinned_terminal_spawn`), run via
   `cargo test --workspace`, no sockets, no process spawning — the claim
   is asserted via `WmCore`'s own already-tested idempotent tri-state
   (`claim_pinned_terminal_spawn` returning `Ok(None)` the second time),
   never by intercepting a real `Command::spawn()` call.
3. **`tag-picker` wire protocol and mode parsing** (Tasks 3-4) —
   `tag-picker::wire`, one new unit test
   (`serializes_switch_tag_request`, cross-checked byte-for-byte against
   `wm`'s own `protocol.rs` fixture for the same request shape);
   `tag-picker::mode` (new module), six new unit tests covering every
   `parse_args` branch (zero-args-is-assign, valid switch, non-numeric
   output id, missing output id, unknown first argument, too many
   arguments) — all pure, no sockets, no `fuzzel` process.
4. **`tag-picker` switch-mode decision logic** (Task 5) —
   `tag-picker::checklist`, new unit tests covering: the plain-list
   renderer's exact format and its difference from assign mode's
   glyph-prefixed format
   (`render_switch_list_formats_one_plain_row_per_tag`,
   `render_switch_list_of_empty_tags_is_empty_string`,
   `render_switch_list_replaces_embedded_tab_and_newline_in_tag_name`),
   and the switch-selection parser's full decision table
   (`parse_switch_selection_returns_selected_for_a_known_tag_id`,
   `..._returns_cancelled_for_nonzero_exit`,
   `..._returns_cancelled_for_empty_stdout`,
   `..._returns_cancelled_for_freeform_text_not_a_known_id` — the
   load-bearing difference from assign mode's `CreateTag` branch,
   `..._returns_cancelled_for_a_numeral_not_among_known_ids`,
   `..._trims_whitespace_consistently_with_toggle_parsing`). Every
   pre-existing `checklist.rs`/`wire.rs` test for assign mode is left
   unmodified — this story adds new functions, it does not change
   `parse_fuzzel_output`'s or `render_fuzzel_input`'s existing behavior or
   signatures.
5. **`tag-picker` orchestration and `wm` keybind wiring** (Tasks 6-7) —
   **not** covered by automated tests, the same boundary class as every
   prior story's process-spawn/socket-I/O glue and Wayland-`Dispatch`
   registration: the real `Command::new("fuzzel")` invocation in switch
   mode, the real single-shot exit-without-reopen behavior, the real
   `Mod4+S` keybind registration and `tag-picker switch <output_id>` spawn
   from `wm`, and the real appearance of the plain list are unverified
   beyond Task 1's design rationale and structural review against Tasks
   6.1-6.3/7.1-7.3's explicit descriptions. Verified via `cargo build
   --workspace`/`cargo clippy --workspace --all-targets -- -D warnings`
   succeeding and the full `wm`+`tag-picker` suite staying green (Task 8).
6. **Tooling gate** (Task 8) — `cargo fmt --all -- --check`, `cargo clippy
   --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
   all in-container; `pre-commit run --all-files` against the already-
   widened workspace-wide hook definitions (no further hook changes
   needed — this story adds no new Cargo targets).

**Remaining, explicitly-flagged coverage gaps, carried forward from Stories
2.2/2.3 and widened by this one:** the real `fuzzel` process is still not
exercised anywhere in this sandbox, live or otherwise. This story adds
three more live-only unknowns on top of Stories 2.2/2.3's own unverified
surface: the real plain-list (no-checkbox) rendering's legibility under
`--with-nth=1`/`--accept-nth=2`, the real single-shot exit-without-reopen
control flow (a materially different runtime shape from every prior
story's loop), and Task 1.4's "picker follows focus"-via-fuzzel-defaults
assumption (no explicit `--output` flag added, and none has ever been
verified live for assign mode either). Every *decision* around these has
full unit-test coverage; the real subprocess's behavior and the real
visual placement remain unverified beyond structural review. A live smoke
test (`river` running, `Mod4+S` with at least one existing tag, a real
`fuzzel` on screen, selecting a tag and observing the target output's
displayed tag and bar module change, and — for a genuinely first-time tag
— observing the pinned terminal actually spawn) is recommended before this
interaction pattern is relied upon in daily use, same recommendation
Story 2.3 carried into this one.

## FR coverage
FR9, NFR1

**Also closes an outstanding gap in the PRD's "Pinned terminal lifecycle"
AC** ("First switch to a tag with no terminal yet spawns..."), which until
this story was only actually guaranteed via the `Mod4+Tab` keybind path,
not via IPC-driven `switch-tag` (Task 2) — see Description and Technical
notes for the full gap analysis.

## Dev Agent Record

### Implementation Plan

Followed the story's own task breakdown exactly, in order, RED before
GREEN for every pure-logic task:

- **Task 2** (`wm/src/ipc/dispatch.rs`): added
  `switch_tag_success_claims_pinned_terminal_spawn_for_the_target_tag` and
  `switch_tag_error_does_not_claim_pinned_terminal_spawn` first — the
  success test failed against the pre-existing arm (`Ok(Some("tag-web"))`
  where `Ok(None)` was expected, proving no claim was happening yet).
  GREEN: the `SwitchTag` arm's `Ok(())` branch now calls
  `wm_core.claim_pinned_terminal_spawn` and, on `Ok(Some(session_name))`,
  `crate::spawn_pinned_terminal(&session_name)` directly — no visibility
  changes needed, `handle_request`'s signature unchanged. Module and
  function doc comments updated to disclose the one process-spawn
  exception.
- **Task 3** (`tag-picker/src/wire.rs`): added
  `serializes_switch_tag_request` against a `Request` enum with no
  `SwitchTag` variant (confirmed fails to compile), then added
  `SwitchTag { output_id: u64, tag_id: u8 }` mirroring `wm`'s shape
  exactly. Updated the module's stale "`tag-picker` never sends
  `switch-tag`" doc comment.
- **Task 4** (new `tag-picker/src/mode.rs`): wrote all 6 `parse_args`
  tests against a `todo!()`-stubbed function first (confirmed all 6
  panic), then implemented the real match arms.
- **Task 5** (`tag-picker/src/checklist.rs`): wrote all 9
  `render_switch_list`/`parse_switch_selection` tests against
  `todo!()`-stubbed functions first (confirmed all 9 panic), then
  implemented both, extracting a private `sanitize_name` helper shared
  with `render_fuzzel_input`'s pre-existing identical
  `.replace(['\t','\n'], " ")` call (Technical notes' explicitly-flagged,
  non-mandatory DRY note — taken, since it was a clean one-line, zero-
  ambiguity extraction).
- **Task 6** (`tag-picker/src/main.rs`): mechanical extraction only, no
  RED/GREEN. Pulled the pre-existing connection/`get-state` boilerplate
  into `connect_and_get_state()` and the existing toggle-and-reopen loop
  verbatim into `run_assign_mode()`; added `run_switch_mode()` (single-shot,
  no reopen) and a `main()` that dispatches on `mode::parse_args`. One
  small deliberate deviation from the story text's literal phrasing: the
  `should_open_picker` focused-view check lives inside `run_assign_mode`
  itself rather than inline in `main()` before calling it — functionally
  identical (still assign-mode-only, still runs before any loop iteration),
  just keeps `main()` a thin mode dispatcher. Runtime behavior is otherwise
  byte-for-byte unchanged for assign mode.
- **Task 7** (`wm/src/main.rs`): added `Action::TagSwitch`, the `Mod4+S`
  binding (`const S: u32 = 0x73`), and the `do_action` arm mirroring
  `Action::TagCycle`'s `None`-output handling and `Action::OpenTagPicker`'s
  spawn/error-handling shape exactly, passing `"switch"` and the resolved
  `output_id` as CLI args. No RED/GREEN — Wayland-`Dispatch`/process-spawn
  glue, same carve-out class as every other `main.rs` action arm.
- **Task 8**: full verification gate, in-container only (`podman exec -u
  vscode -w /workspaces/buoy-wm bold_vaughan <cmd>` — `devpod ssh buoy-wm`
  errored with "Error tunneling to container" on the first attempt this
  session, consistent with every prior Epic 2 story's own note).

### Completion Notes

- Zero-arg assign mode is byte-for-byte unchanged: every pre-existing
  `checklist.rs`/`wire.rs` unit test for `render_fuzzel_input`,
  `parse_fuzzel_output`, `build_checklist_entries`,
  `should_toggle_after_create`, `should_add_to_tag_mirror`,
  `toggle_local_membership`, `should_open_picker`,
  `render_rejection_row`/`render_create_apply_failed_row`, and every
  `wire.rs` `Request`/`Response` (de)serialization test, passed unmodified
  throughout — none of those functions' signatures or bodies changed
  (`render_fuzzel_input`'s body changed to call the new `sanitize_name`
  helper, but produces byte-identical output, confirmed by its own
  pre-existing tests still passing unmodified).
- No new WM-side IPC request type was added, per the story's explicit
  constraint — `Request::SwitchTag` already existed since Story 2.1.
- Final counts, matching the story's own Task 8.1 prediction exactly: 149
  `wm` tests (147 baseline + 2 new `dispatch.rs` tests), 63 `tag-picker`
  tests (47 baseline + 1 `wire.rs` + 6 `mode.rs` + 9 `checklist.rs` = 63).
  212 total, all green.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --
  -D warnings`, and `pre-commit run --all-files` (both hooks: `cargo fmt
  --check`, `cargo clippy`) all pass clean with zero warnings.
- Live-only gaps carried forward unverified, per Task 8.3 and Technical
  notes: no real `fuzzel`/Wayland session in this sandbox, so the actual
  plain-list rendering's legibility, the real single-shot
  exit-without-reopen control flow, and the "picker follows focus"
  assumption (no explicit `--output` flag) remain unverified beyond unit
  tests and structural review. A live smoke test is recommended before
  relying on `Mod4+S` in daily use, same recommendation as Stories 2.2/2.3.

## File List

- `wm/src/ipc/dispatch.rs` — modified (Task 2: pinned-terminal-spawn claim
  on `SwitchTag` success; module/function doc comments)
- `wm/src/main.rs` — modified (Task 7: `Action::TagSwitch`, `Mod4+S`
  binding, `do_action` arm)
- `tag-picker/src/wire.rs` — modified (Task 3: `Request::SwitchTag`
  variant; doc comments)
- `tag-picker/src/mode.rs` — new (Task 4: `Mode`, `parse_args`)
- `tag-picker/src/checklist.rs` — modified (Task 5: `SwitchAction`,
  `render_switch_list`, `parse_switch_selection`, `sanitize_name`)
- `tag-picker/src/main.rs` — modified (Task 6: `connect_and_get_state`,
  `run_assign_mode`, `run_switch_mode`, dispatching `main`)
- `docs/planning/epics/story-2-4.md` — this file (task checkboxes, Dev
  Agent Record, File List, Change Log, Status)

## Change Log

- 2026-08-08: Implemented Story 2.4 (Tag-Switching via Picker) per the
  bmad-dev-story workflow — all 8 tasks complete, 18 new tests added (149
  `wm` + 63 `tag-picker` = 212 total), zero regressions, `cargo
  fmt`/`clippy`/`pre-commit` all clean. Status moved to `review`.

### Code Review Follow-up

Code review found a real, confirmed bug in this story's Task 2 change: the
`SwitchTag` success arm called `crate::spawn_pinned_terminal(&session_name)`
directly, a real `Command::new("foot").arg(...).spawn()`, from inside
`handle_request` itself. That meant `cargo test --workspace` launched a real
`foot`/`zellij` process as a side effect of the two dedicated tests
(`switch_tag_success_claims_pinned_terminal_spawn_for_the_target_tag`) and
also, incidentally, of any other test exercising a first-time successful
`SwitchTag` (`switch_tag_calls_wm_core_switch_tag_and_returns_ok`,
`handle_request_never_panics_regardless_of_which_ids_are_bogus`'s
`(valid_output, valid_tag)` combination) — directly contradicting this
module's own doc comment claiming "zero socket/thread machinery" and the
Test plan's "no sockets, no process spawning" claim. This is exactly the
alternative design the story's own Technical notes section ("Why
`handle_request`'s signature doesn't need to change") considered and
rejected; that rejection is superseded by this follow-up.

**Fix applied:**
- `wm/src/ipc/dispatch.rs`'s `handle_request` signature changed from
  `fn handle_request(wm_core: &mut WmCore, request: Request) -> Response`
  to `fn handle_request(wm_core: &mut WmCore, request: Request) -> (Response,
  Option<String>)`. The `SwitchTag` success arm still calls
  `wm_core.claim_pinned_terminal_spawn(...)` (a pure `wm-core` state
  mutation, unchanged), but on `Ok(Some(session_name))` now returns that
  session name as the tuple's second element instead of calling
  `crate::spawn_pinned_terminal` itself. Every other arm returns `None` for
  the second element. No other dispatch logic (the `switch_tag`/
  `claim_pinned_terminal_spawn` call sequence, error handling, `Response`
  construction) changed.
- `wm/src/ipc/server.rs`'s `handle_connection_inner` now destructures
  `(response, pending_spawn)` from `handle_request`, writes the response to
  the client first, and only afterward — outside the `wm-core` mutex guard —
  calls `crate::spawn_pinned_terminal(&session_name)` if `pending_spawn` is
  `Some`. This ordering means a slow or failing spawn can never block the
  client waiting on its response. `crate::spawn_pinned_terminal` is a
  private fn at the binary crate's root module and `ipc::server` is a
  descendant module, so no visibility change was needed — the same
  Rust-visibility argument `dispatch.rs`'s prior doc comment already made,
  just applied to `server.rs` instead.
- Updated tests: `switch_tag_success_claims_pinned_terminal_spawn_for_the_target_tag`
  now asserts `pending_spawn == Some("tag-web".to_string())` in addition to
  the pre-existing `claim_pinned_terminal_spawn(tag_id) == Ok(None)`
  regression check. `switch_tag_error_does_not_claim_pinned_terminal_spawn`
  additionally asserts `pending_spawn == None`. Every other test destructures
  the new tuple and either asserts `pending_spawn == None` or (for
  `switch_tag_calls_wm_core_switch_tag_and_returns_ok` and the
  `handle_request_never_panics...` sweep) deliberately discards it with a
  comment explaining that discarding is sufficient to guarantee no real
  process spawn, since `handle_request` itself never calls
  `crate::spawn_pinned_terminal`.
- `dispatch.rs`'s module doc comment and `handle_request`'s own doc comment
  rewritten to describe the new contract and retract the stale "considered
  but rejected" framing.
- No behavioral change to `wm_core::switch_tag`/`claim_pinned_terminal_spawn`
  themselves, no change to `tag-picker`, no change to any other
  lower-priority review finding (all explicitly deferred, out of scope for
  this fix).

**Verification (in-container, `podman exec -u vscode -w /workspaces/buoy-wm
bold_vaughan <cmd>`, `devpod ssh buoy-wm` unreliable this session as in every
prior story):**
- `cargo test --workspace`: 149 `wm` + 63 `tag-picker` = 212 tests, all
  green — same counts as before this fix (no tests added/removed, only
  reshaped). Confirmed via `ps aux | grep -E 'foot|zellij'` immediately
  after the run that no such process exists — the real spawn call is now
  structurally outside `handle_request`'s tested code path (it lives in
  `server.rs`, exercised only by a live socket connection, never by
  `dispatch.rs`'s unit tests).
- `cargo build --workspace`: clean.
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `pre-commit run --all-files`: both hooks (`cargo fmt --check`, `cargo
  clippy`) passed.

## Status

review
