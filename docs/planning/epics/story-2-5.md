---
baseline_commit: e87aba4ab94b95ae9a01422428c8b212deddf3f4
---

# Story 2.5: Per-Output Status Bar via Waybar Module
Epic: 2 | Priority: H | Status: done

## Description

An always-visible bar element per output showing the currently selected tag
— resolves the `wlr-layer-shell` rejection gap recorded in
`technical-constraints.md` (no layer-shell surface is tag-scoped) by having
`status-bar`, a new companion binary, drive a **waybar custom module**
instead of rendering its own surface (`components.md`,
`external-integrations.md`, both updated 2026-08-06).

**This story adds a third Cargo workspace member, `status-bar/`, and
touches nothing else in the repo except `Cargo.toml` (workspace member
list) and `.pre-commit-config.yaml` (hook file-glob, Task 7 — a real,
concrete gap found by inspection, not assumed; see Technical notes).
Zero changes to `wm/src/ipc/`, `wm/src/wm_core/`, or `wm/src/main.rs`.**
`Request::GetState`/`WmCore::snapshot()` (Story 2.1) already expose
everything this client needs — every output's `id` and `current_tag`, every
tag's `id` and `name` — so, per the orchestrating instruction's constraint
to keep `wm-core` protocol-agnostic and reuse existing IPC surface, this
story invents no new WM-side request/response shape. Confirmed by
inspection of `wm/src/ipc/protocol.rs`'s `OutputDto`/`TagDto`/
`Response::State`, not assumed.

**Waybar's custom-module mechanism (resolved, not TBD — the open item both
`external-integrations.md` and `components.md` flagged "exact mechanism
TBD at implementation").** Researched directly against waybar's own
`waybar-custom(5)` man page (`man.archlinux.org/man/waybar-custom.5.en`,
package version 0.15.0) rather than guessed, same rigor as Story 2.2's
fuzzel spike:

- A `custom/<name>` module's `interval` field polls `exec` repeatedly;
  `interval: "once"` runs `exec` once at startup and thereafter only
  updates on `signal` (an external `pkill -RTMIN+<N> waybar`). **If
  neither `interval` nor `signal` is set at all, waybar's own docs state:
  "it is assumed that the out script loops itself"** — i.e. `exec` is a
  long-running process, and waybar simply re-reads and re-renders on every
  new line that process writes to its own stdout, with no timer of
  waybar's own. This is the mechanism this story uses: `status-bar` is
  that self-looping script. From waybar's side this looks exactly like a
  push-based module (no polling delay imposed by waybar); underneath,
  `status-bar` implements the "push" by polling `wm`'s IPC socket on its
  own short internal timer (Task 1.4) — this satisfies the orchestrating
  instruction's hard constraint against adding any new WM-side
  subscribe/push IPC machinery (`wm-core` stays exactly as request/
  response as Story 2.1 left it) while still presenting waybar with a
  continuously-updating module.
- `return-type: "json"` tells waybar to parse each stdout line as
  `{"text": "...", "tooltip": "...", "class": "...", "percentage": ...}`
  rather than i3blocks-style newline-separated plain text. This story uses
  `text`+`class` only (Task 5) — no tooltip, no percentage, matching
  `EXPERIENCE.md`'s "single text element per output... display-only."
- **Per-output instancing** is a native waybar feature, not something this
  binary implements: a waybar config's top-level bar block accepts either
  a single object or **an array of bar-config objects**, each with its own
  `"output": "<connector-name>"` binding (`waybar(5)`'s own multi-output
  documentation, "Configuration of multiple outputs" — e.g. one block with
  `"output": "eDP-1"`, a second with `"output": "DP-2"`, each free to
  declare its own `modules-left`/`-center`/`-right` and its own
  `custom/tag` module definition). Each such per-output bar block spawns
  its own independent `exec` process — so **one `status-bar` process per
  physical output**, not one process serving all outputs. This is the
  concrete resolution to the orchestrating instruction's open question
  #2 ("how does the module know WHICH output it's reporting for"): **a
  plain CLI argument**, the target output's `id` exactly as it appears on
  `get-state`'s `outputs[].id` (`wm`'s own `OutputId`, a `u64`) —
  `status-bar <output_id>`. No env var, no waybar-side output-name
  threading of any kind.

**The output-identity mapping gap, flagged rather than silently
papered over.** Investigated directly against the codebase, not assumed:
`wm_core::OutputId` (`wm/src/wm_core/ids.rs`) is a bare newtype around a
monotonic counter; `WmCore::register_output` (`wm/src/wm_core/state.rs:271`)
takes **no arguments at all** and is called from exactly one site
(`wm/src/main.rs`'s `Dispatch<RiverWindowManagerV1>`'s `Event::Output`
arm) with no Wayland-level name/connector/`xdg-output` binding captured
anywhere in this codebase — `wm`'s wire-level `OutputDto`
(`wm/src/ipc/protocol.rs:44-48`) carries only `id`/`current_tag`, nothing
else. There is, today, no code path anywhere in this repo that could
translate waybar's own output identifier (`"eDP-1"`, assigned by the
compositor/`xdg-output`) into `wm`'s numeric `OutputId` — the two
namespaces are entirely unrelated, and this story does not attempt to
unify them. The accepted resolution (Task 1.2): the user hand-authors the
correspondence once, the same way Story 1.7 already accepted
"lowest-`OutputId`-is-the-active-output" as a documented scope boundary
rather than building real focused-output tracking. In practice, for the
laptop-plus-optional-external setup `EXPERIENCE.md` scopes this project
to ("Laptop-only and docked-to-42"-widescreen are both first-class"),
`OutputId`s are assigned in the stable order `river` announces outputs at
compositor startup for a given physical layout — the user runs `wm`,
inspects `get-state`'s `outputs` list once (or simply watches which bar
updates when switching tags on a given monitor) to learn which numeric id
is which physical screen, and writes that literal number into their own
waybar config's per-output `custom/tag` module `exec` line. If this
assumption breaks (e.g. a hotplug reorders outputs), the bar shows an
incorrect but harmless tag label, not a crash — recoverable via the
existing "`bar-tag-disconnected`"-adjacent `UnknownOutput` case this story
provides if the id becomes genuinely absent.

**The three-strike DRY rule, and why this story doesn't finally trigger
it.** `tag-picker/src/wire.rs`'s own doc comment already names this exact
moment: *"A shared crate is deferred to whichever story first gives
`status-bar` (Story 2.5) the same need, a genuine third consumer."*
`status-bar` genuinely is that third independent occurrence of the
wire-type/socket-path duplication (`wm/src/ipc/protocol.rs` +
`wm/src/ipc/server.rs::resolve_socket_path`, `tag-picker/src/wire.rs` +
`tag-picker/src/socket_path.rs`, and now `status-bar`'s own equivalents).
**Resolution, recorded as a deliberate decision (Task 1.3), not an
oversight:** the three-strike rule exists to bound future maintenance
drift across call sites that keep evolving independently — and this is
the last story in Epic 2 *and the last story in the entire currently
planned project* (per the orchestrating brief). There is no fourth
consumer this rule is protecting against, ever, under the current plan.
Extracting a shared `buoy-wm-protocol`/`buoy-wm-ipc-client` crate now,
purely to satisfy the letter of a rule whose entire purpose is amortizing
*future* drift, with no future call site left to drift, is the exact
shape of premature-generality YAGNI itself warns against — abstracting
for an audience of zero remaining callers. `status-bar` therefore gets its
own third, narrower copy of the wire types (`Request::GetState` only —
this client never mutates) and its own third copy of
`resolve_socket_path`/`default_socket_path`, following
`tag-picker/src/wire.rs`/`socket_path.rs`'s exact existing shape and doc-
comment convention. If a fourth consumer is ever added to this project in
the future, that is the correct point to finally extract a shared crate —
this story explicitly does not do so.

**Startup-default correction (a real inaccuracy found in the prior draft,
not assumed).** The pre-existing draft's AC text said an output's tag can
change "via keybind, picker, or WM startup default." Inspected against
`WmCore::register_output` (`wm/src/wm_core/state.rs:271-275`): a freshly
registered output's `current_tag` starts as `None` and **there is no
startup-default-tag mechanism anywhere in this codebase** — `current_tag`
stays `None` until the first real `switch_tag` call (via `Mod4+Tab`'s
keybind cycle or `Mod4+S`'s switch-mode picker). This story's ACs are
corrected accordingly (no "WM startup default" trigger), and the bar must
therefore render a genuine, non-error "no tag selected yet" state
distinctly from both the normal and disconnected states (Task 1.5) —
`DESIGN.md`/`EXPERIENCE.md` do not specify this third state explicitly
(they only name "normal" and "disconnected"), so its exact text is this
story's own literal, minimal-voice-and-tone-consistent choice, flagged as
an interpretation gap rather than silently invented.

## Acceptance criteria

**Given** `status-bar` is invoked with a valid numeric output id argument
matching a real, registered `OutputId` on `wm`'s side, and the IPC socket
is reachable
**When** that output has a current tag (`OutputDto.current_tag` is
`Some(tag_id)`, and `tag_id` resolves to a real entry in `get-state`'s
`tags` list)
**Then** `status-bar` prints one JSON line to stdout,
`{"text":"<tag name>","class":"normal"}`, and continues running
(it is the waybar-facing "script that loops itself" — see Technical
notes "Waybar's custom-module mechanism") — no `interval`/`signal` is
needed in the user's waybar config for this to update live

**Given** the same setup
**When** the target output's displayed tag changes (via `Mod4+Tab`'s
keybind cycle or `Mod4+S`'s switch-mode picker, both already implemented
and unchanged by this story)
**Then** `status-bar`'s next poll tick (within one `POLL_INTERVAL`,
Technical notes — no literal real-time bound is tested, see Test plan)
observes the new `current_tag` via a fresh `get-state` call and prints a
new JSON line reflecting the new tag name — this is `EXPERIENCE.md` Flow
B's "the bar module at the top of that output re-renders to show `chat`"
made concrete

**Given** a registered output that has never had `switch_tag` called for
it yet (`current_tag` is `None` — the corrected, no-startup-default case,
Description)
**When** `status-bar` polls it
**Then** it prints `{"text":"no tag","class":"normal"}` — a real,
non-error state, not treated as a disconnect or a fault

**Given** the numeric output id `status-bar` was invoked with does not
appear in `get-state`'s `outputs` list at all (misconfigured waybar
`exec` argument, or a genuinely-removed output)
**When** `status-bar` polls
**Then** it prints `{"text":"⚠ unknown output","class":"disconnected"}` —
reuses the disconnected CSS class (`DESIGN.md`'s `bar-tag-disconnected` is
the only "something is wrong, don't trust this" token this project
defines — Do's/Don'ts reserves `accent-error` for exactly the two named
error states, and this is treated as a member of the disconnect family,
not a third accent-error state) while keeping the stdout `text` itself
diagnostic

**Given** `status-bar` is running and successfully polling
**When** the IPC connection to `wm` cannot be established, or a
previously-working connect/send/read fails partway through a poll tick
(socket gone, `wm` restarted, `wm` not yet started)
**Then** `status-bar` does not crash or exit — it prints
`{"text":"⚠ disconnected","class":"disconnected"}` (dimmed text plus a
disconnect glyph carried in the text itself, per `DESIGN.md`
`bar-tag-disconnected` — color/dimming is the user's own waybar
`style.css`'s job against the `disconnected` CSS class this binary
emits, not this binary's) and keeps looping, retrying the connection on
every subsequent poll tick with no backoff (Technical notes)

**Given** `status-bar` was printing the disconnected line
**When** a subsequent poll tick's connect/send/read succeeds again
**Then** the very next printed line reflects the real, current state
(normal tag / no-tag / unknown-output, whichever applies) — this is
automatic; no user action, restart, or signal is required, satisfying the
reconnect-recovery AC the 2026-08-06 implementation-readiness review added
explicitly to the original draft

**Given** `status-bar` is invoked with zero arguments, more than one
argument, or a non-numeric single argument
**When** it starts
**Then** it prints a usage message to stderr and exits non-zero
**before** attempting to connect to the socket at all — the same
fail-closed-on-malformed-invocation discipline `tag-picker`'s own
`mode::parse_args` already established (Story 2.4 Task 1.1)

**Given** two consecutive poll ticks produce the identical waybar-facing
line (no real change occurred)
**When** `status-bar` would otherwise print again
**Then** it does not re-print — no duplicate stdout lines for an
unchanged state (Task 5's `changed` decision), reducing waybar's redraw
churn on a passive, "quiet" surface (`DESIGN.md`: "nothing here should
compete for attention")

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: Resolve every open design question before writing any implementation (no RED/GREEN — a documented design decision gating every later task, same class as Story 2.2/2.3/2.4's own Task 1)**
  - [x] 1.1 Record the waybar custom-module mechanism (Description's "resolved, not TBD" section): no `interval`/`signal` set on the `custom/tag` module — `status-bar` is the "script that loops itself" waybar's own `waybar-custom(5)` docs describe, `return-type: "json"`, `{"text":..,"class":..}` per line. Cite the man page directly (already quoted above) so this is a fact, not a recollection.
  - [x] 1.2 Record the per-output CLI-argument decision and its accepted scope boundary (Description's "output-identity mapping gap"): `status-bar <output_id>` takes `wm`'s own raw `OutputId` numeric value, one waybar bar-config block per physical output (native waybar feature, `"output":"<connector-name>"`), user hand-maps connector name → `OutputId` once by inspection — no code in this story performs that mapping, and none is added to `wm` to make it dynamic.
  - [x] 1.3 Record the three-strike-DRY non-extraction decision (Description): `status-bar` gets its own third, narrower `wire.rs`/`socket_path.rs` copy, following `tag-picker`'s existing shape exactly; no shared crate, with the specific "why not now, given this is the project's last story" rationale written into `status-bar/src/wire.rs`'s own module doc comment (mirroring `tag-picker/src/wire.rs`'s own doc-comment style, so a future reader hits the same reasoning at the same place).
  - [x] 1.4 Record the polling design: `status-bar` reconnects fresh every poll tick (`UnixStream::connect` → send `get-state` → read one response → drop the connection) rather than holding one long-lived connection across ticks — deliberately simpler and more robust to `wm` restarts than persistent-connection-state management, and cheap enough at a local Unix-socket, single-mutex-guarded-snapshot scale. `POLL_INTERVAL` is a hardcoded constant, `Duration::from_millis(250)` — comfortably under typical human just-noticeable-lag for a passive display, far below any NFR1-relevant threshold (NFR1 is about `wm`'s own 50ms *action* budget, not this passive bar's refresh cadence), not user-configurable in v1 (YAGNI — a single hobby user, no stated need for tuning).
  - [x] 1.5 Record the three-state render-decision table (Description's "startup-default correction"): `Normal(tag_name)` (class `normal`), `NoTagSelected` (class `normal`, literal text `"no tag"` — a resolved interpretation gap, `DESIGN.md`/`EXPERIENCE.md` do not name this state explicitly), `UnknownOutput` (class `disconnected`, text `"⚠ unknown output"` — folded into the disconnect family per `DESIGN.md`'s Do's/Don'ts restricting `accent-error`-class treatment to exactly its two named states), and the connect/send/read-failure case (class `disconnected`, text `"⚠ disconnected"`) which is not part of the pure decision function at all (Task 5) since it is purely a function of I/O outcome, not of any parsed `get-state` data.
  - [x] 1.6 Record the JSON-injection-safety decision: the waybar-facing line is built via a `#[derive(Serialize)]` struct through `serde_json::to_string`, never hand-formatted string interpolation — tag names are arbitrary user-authored strings with no charset restriction (ADR-006/YAGNI, same fact `tag-picker`'s own `sanitize_name` note already relies on), so a tag name containing a literal `"` or `\` must never corrupt the emitted JSON line. This is the one concrete correctness risk unique to this story (tag-picker's fuzzel-facing format is tab/newline-delimited text, not JSON, so it never faced this exact risk).
  - [x] **No RED/GREEN** — documented design decisions only, gating Tasks 2-8's implementation, same carve-out class as every prior Epic 2 story's own Task 1.

- [x] **Task 2: Scaffold `status-bar` as a third Cargo workspace member (mechanical, no RED/GREEN — same class as Story 2.2 Task 2's `tag-picker` scaffolding)**
  - [x] 2.1 Add `"status-bar"` to `Cargo.toml`'s `[workspace] members` array (currently `["wm", "tag-picker"]`).
  - [x] 2.2 Create `status-bar/Cargo.toml`: `name = "status-bar"`, `edition = "2024"`, `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"` — identical dependency shape to `tag-picker/Cargo.toml`, deliberately **no** dependency on anything `fuzzel`-related (this client never spawns a subprocess or renders any interactive UI).
  - [x] 2.3 Create a minimal `status-bar/src/main.rs` stub (`fn main() {}`) so the workspace builds before Tasks 3-6 add real content — confirm `cargo build --workspace` succeeds with the new empty member in place before proceeding.
  - [x] **No RED/GREEN** — pure scaffolding, verified by `cargo build --workspace` succeeding.

- [x] **Task 3: `status-bar/src/socket_path.rs` — mirror `wm`'s/`tag-picker`'s socket-path resolution (AC: connecting to the right socket at all)**
  - [x] 3.1 RED — port `tag-picker/src/socket_path.rs`'s existing four tests verbatim (`resolve_socket_path_uses_xdg_runtime_dir_when_set`, `resolve_socket_path_falls_back_to_tmp_user_when_xdg_runtime_dir_unset`, `resolve_socket_path_falls_back_to_logname_when_user_also_unset`, `resolve_socket_path_falls_back_to_literal_unknown_when_nothing_is_set`) against a not-yet-implemented `resolve_socket_path`/`default_socket_path` pair in the new module. Confirmed they fail to compile (`error[E0425]: cannot find function 'resolve_socket_path' in this scope`, x4).
  - [x] 3.2 GREEN — implemented `resolve_socket_path(xdg_runtime_dir: Option<&str>, user: Option<&str>, logname: Option<&str>) -> PathBuf` and `default_socket_path() -> PathBuf`, byte-for-byte identical logic to `tag-picker/src/socket_path.rs` (this is the third accepted copy, Task 1.3). `///` doc comments cross-referencing `tag-picker`'s copy and the three-strike-DRY decision. All four tests pass (a transient `dead_code` warning on `default_socket_path` is expected here — resolved once Task 6's `main.rs` calls it).

- [x] **Task 4: `status-bar/src/wire.rs` — this client's own minimal protocol mirror, needing `outputs` for the first time (AC: "reflects the new tag name," "unknown output")**
  - [x] 4.1 RED — wrote, against not-yet-existing types: `serializes_get_state_request` (`serialize_request(&Request::GetState)` equals `r#"{"type":"get-state"}"#`); `parses_state_response_with_tags_and_outputs_ignoring_unmodeled_fields` (a fixture line containing `tags`, `views`, `outputs`, `focused_view` — the full real `wm` response shape, byte-identical to `protocol.rs`'s own `serializes_state_response_shape` fixture — parses into a `Response::State{ tags, outputs }` that omits `views`/`focused_view` entirely); `parse_response_rejects_non_json_garbage`; `parse_response_rejects_invalid_utf8_bytes`; `parse_response_rejects_unknown_type`; `parse_response_rejects_object_missing_type_field`. Confirmed all fail to compile (`error[E0433]: cannot find type 'Request'/'Response'`, `error[E0425]: cannot find function 'serialize_request'/'parse_response'`, 10 errors total).
  - [x] 4.2 GREEN — `Request` enum with **only** `GetState`. `TagDto { id: u8, name: String }` (`Deserialize`). `OutputDto { id: u64, current_tag: Option<u8> }` (`Deserialize`). `Response::State { tags: Vec<TagDto>, outputs: Vec<OutputDto> }` / `Ok` / `TagCreated { tag_id: u8 }` / `Error { message: String }`. `ParseError` mirroring `wm`'s own. `serialize_request`/`parse_response` mirroring `tag-picker`'s own implementations exactly. Module doc comment records the Task 1.3 three-strike-DRY decision in full. All six new tests pass (10 total in the crate so far).

- [x] **Task 5: `status-bar/src/bar_line.rs` — the pure render-decision logic (AC: all four render states, the JSON-injection-safety guard, the no-duplicate-line guard)**
  - [x] 5.1 RED — `resolve_bar_line` tests written against a not-yet-implemented function (`resolve_bar_line_returns_normal_with_the_tags_name_when_output_has_a_current_tag`, `resolve_bar_line_returns_no_tag_selected_when_output_exists_with_none_current_tag`, `resolve_bar_line_returns_unknown_output_when_output_id_is_absent_from_the_list`, `resolve_bar_line_defensively_falls_back_when_current_tag_id_is_missing_from_the_tags_list`).
  - [x] 5.2 RED — `format_waybar_line` tests written (`format_waybar_line_renders_disconnected_json_for_none`, `format_waybar_line_renders_normal_tag_json`, `format_waybar_line_renders_no_tag_selected_json`, `format_waybar_line_renders_unknown_output_with_the_disconnected_class_and_distinct_text`, `format_waybar_line_escapes_a_tag_name_containing_a_double_quote_and_a_backslash`).
  - [x] 5.3 RED — `changed` tests written (`changed_is_true_when_previous_is_none`, `changed_is_true_when_previous_differs_from_next`, `changed_is_false_when_previous_equals_next`). Confirmed all 12 tests from 5.1-5.3 fail to compile (22 `E0425`/`E0433` errors against absent `BarLine`/`resolve_bar_line`/`format_waybar_line`/`changed`).
  - [x] 5.4 GREEN — implemented `BarLine { Normal(String), NoTagSelected, UnknownOutput }`, `resolve_bar_line`, private `WaybarLine<'a> { text: &'a str, class: &'static str }` + `format_waybar_line` (via `serde_json::to_string`, never hand-built), and `changed`. Module doc comment records Task 1.5's full render-decision table. All 12 new tests pass (22 total in the crate).

- [x] **Task 6: `status-bar/src/main.rs` — CLI-argument parsing and the poll loop (AC: the malformed-argument fail-closed path is unit-tested; the poll loop itself is I/O glue, same carve-out class as `tag-picker/src/main.rs`)**
  - [x] 6.1 RED — added a `#[cfg(test)] mod tests` in `main.rs`: `parse_output_id_accepts_a_single_numeric_argument`, `parse_output_id_rejects_zero_arguments`, `parse_output_id_rejects_more_than_one_argument`, `parse_output_id_rejects_a_non_numeric_argument`. Confirmed all fail to compile (`error[E0425]: cannot find function 'parse_output_id'`, x4).
  - [x] 6.2 GREEN — implemented `parse_output_id(args: &[String]) -> Result<u64, String>` exactly per spec. All four tests pass.
  - [x] 6.3 Implemented the untested I/O-glue poll loop: `try_get_state(socket_path: &Path) -> Option<(Vec<wire::TagDto>, Vec<wire::OutputDto>)>` connects fresh, sends `get-state`, reads/parses exactly one response line, returns `None` on any failure (connect/write/flush/EOF/parse/unexpected-variant). `main()` parses argv via `parse_output_id`, `eprintln!`+`exit(1)` on `Err` before touching the socket, then loops forever: poll, resolve `BarLine` (or `None` on poll failure), format, and only print+flush+update `last_printed` when `bar_line::changed` says the line differs, then `sleep(POLL_INTERVAL)`. The loop never calls `std::process::exit` — a stdout write/flush failure is `eprintln!`-logged and the loop continues. No RED/GREEN (I/O glue), verified via `cargo build --workspace`/`cargo clippy --workspace --all-targets -- -D warnings` (Task 8) and structural review against this subtask's description.

- [x] **Task 7: Close the pre-commit hook-coverage gap this new member introduces (a real, concrete gap found by inspection — AC: none directly, but a genuine "this story's own new files must be lint/fmt-gated" requirement)**
  - [x] 7.1 Widened both hooks' `files:` regex from `^(wm|tag-picker)/(src/.*\.rs|Cargo\.toml)$|^Cargo\.(toml|lock)$` to `^(wm|tag-picker|status-bar)/(src/.*\.rs|Cargo\.toml)$|^Cargo\.(toml|lock)$` so a `status-bar`-only change actually triggers `cargo fmt --check`/`cargo clippy` locally. Confirmed by inspection that `.github/workflows/ci.yml` needs no change — `cargo build --workspace`/`cargo test --workspace` already cover all three members unconditionally.
  - [x] 7.2 No RED/GREEN — verified in Task 8.2 (`pre-commit run --all-files` passing clean, confirming both hooks actually ran against the newly-added `status-bar/` files rather than silently skipping them).

- [x] **Task 8: Full in-container verification gate, across the now-three-member workspace (AC: all)**
  - [x] 8.1 Ran, inside the devcontainer — `devpod ssh buoy-wm` errored ("Error tunneling to container: wait: remote command exited without exit status or exit signal"), confirmed unreliable this session as in every prior Epic 2 story; fell back to `podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>` — `cargo fmt --all -- --check` (found and fixed one real formatting violation in `bar_line.rs`, then clean), `cargo clippy --workspace --all-targets -- -D warnings` (clean, zero warnings), `cargo test --workspace`. Final counts exactly matched the story's own prediction: 149 `wm` + 63 `tag-picker` + 26 `status-bar` (4 `socket_path` + 6 `wire` + 12 `bar_line` + 4 `parse_output_id`) = 238 total, all green. `cargo build --workspace` also clean.
  - [x] 8.2 Ran `pre-commit run --all-files` as the `vscode` user inside the container — both hooks (`cargo fmt --check`, `cargo clippy`) passed clean. Additionally confirmed the coverage fix itself (not just that the aggregate run passes) via `pre-commit run --files status-bar/src/main.rs status-bar/Cargo.toml` — both hooks fired and passed against `status-bar`-only paths, proving Task 7's regex widening is what makes this new member actually gated (it would have been silently skipped under the pre-fix two-name regex).
  - [x] 8.3 Manual smoke-test note (same explicit, not-downplayed gap class as every prior Epic 2 story): **no `waybar` binary and no live Wayland/`river` session exist in this sandbox** — this story's live-only unknowns are: whether waybar's "no `interval`/`signal` → script loops itself, updates on every new stdout line" behavior matches this story's understanding of the man page exactly (untested against a real waybar process anywhere in this project so far); the real per-output multi-bar-block waybar config actually producing one `status-bar <id>` process per physical output as expected; and the real visual legibility of the `⚠`/dimmed-text disconnected state once an actual `style.css` targets the `disconnected` CSS class this binary emits (no such stylesheet is written by this story — `waybar` styling is explicitly "external, user-configured" per `components.md`). Every *decision* has full unit-test coverage (Tasks 3-6); the real waybar process's behavior and the real visual result remain unverified beyond structural review and the man-page citation in Task 1.1. A live smoke test (`river` + a real `waybar` running the example config below, switching tags via `Mod4+S` and observing the correct per-output bar element update, then killing/restarting `wm` and observing the disconnected-then-recovered sequence) is recommended before this is relied upon in daily use — the same recommendation every prior Epic 2 story carried forward for its own live-only gaps.

## Technical notes

**Example waybar config snippet (documentation only — not a file this
story adds to the repo; the user's real waybar config is external,
`components.md`'s own framing, likely chezmoi-managed per this
environment's own dotfile convention, and out of this story's File
List).** Two physical outputs, `OutputId 0` and `OutputId 1` (learned by
inspection per Task 1.2, not computed by any code here):

```jsonc
// ~/.config/waybar/config.jsonc (illustrative, not shipped by this story)
[
  {
    "layer": "top",
    "output": "eDP-1",
    "modules-right": ["custom/tag"],
    "custom/tag": {
      "exec": "/path/to/status-bar 0",
      "return-type": "json"
      // Deliberately no "interval"/"signal": status-bar is the
      // self-looping script waybar-custom(5) describes; it updates
      // waybar on every new stdout line it prints, at its own
      // internal ~250ms poll cadence (Task 1.4).
    }
  },
  {
    "layer": "top",
    "output": "DP-2",
    "modules-right": ["custom/tag"],
    "custom/tag": {
      "exec": "/path/to/status-bar 1",
      "return-type": "json"
    }
  }
]
```

**No new WM-side IPC surface (constraint from the orchestrating brief,
confirmed by inspection).** `Request::GetState` and `WmCore::snapshot()`
(Story 2.1) already expose every field this story needs
(`OutputDto.id`/`current_tag`, `TagDto.id`/`name`). This story's entire
diff outside the new `status-bar/` crate is `Cargo.toml`'s member list
and `.pre-commit-config.yaml`'s hook globs (Task 7) — `wm/src/ipc/`,
`wm/src/wm_core/`, and `wm/src/main.rs` are untouched.

**Why `status-bar` polls instead of `wm` pushing.** `components.md`'s own
"subscribes to (or polls)" phrasing left this genuinely open. Adding a
server-push/subscribe mechanism to `wm`'s IPC protocol (a new request
type that keeps a connection open and streams unsolicited `state` lines
on every mutation) was considered and rejected here: it would require new
`wm`-side request/response variants and new per-connection subscriber
bookkeeping in `ipc::server`, directly contradicting the orchestrating
brief's explicit constraint to reuse `get-state` and avoid new WM-side
IPC surface "unless something is genuinely missing" — and nothing is
missing; a cheap, frequent poll against an already-`Mutex`-guarded,
O(views+tags+outputs) snapshot is well within a single hobby user's
performance envelope and requires zero `wm`-side code changes at all.

**Why reconnect-every-tick instead of one persistent connection.**
`wm/src/ipc/server.rs`'s `handle_connection_inner` does support many
sequential requests on one long-lived connection (confirmed by
inspection — `tag-picker`'s own assign-mode toggle-and-reopen loop
already relies on exactly this). A persistent-connection design was
considered for `status-bar` too, but reconnecting fresh every poll tick
was chosen instead: it collapses "was this connection previously good,
did it just drop, do I need to reconnect now or next tick" into a single
code path (Task 6.3's `try_get_state`, all-or-nothing per tick), which is
simpler and more directly matches the AC's disconnect/recovery framing
than a stateful connection-lifecycle machine would, at a connect-syscall
cost (a few Hz against a local Unix socket) this project's own NFR
posture has no stated concern with.

**No `restart-interval` needed in the user's waybar config.** Because the
poll loop (Task 6.3) never calls `std::process::exit` for any I/O
failure after argument parsing — including a failed write to its own
stdout — `status-bar` is a genuinely permanent "script that loops itself"
for the lifetime of the `wm` session, matching waybar's own
no-`interval`-no-`signal` contract exactly. `restart-interval` exists in
waybar for scripts that do legitimately exit and need respawning; this
one is designed not to.

**NFR1 (50ms budget) — not applicable to this story the way it was to
Stories 2.1-2.4.** NFR1 bounds `wm`'s own action-handling latency; this
story adds no code to `wm` at all (previous section). `status-bar`'s own
poll cadence (`POLL_INTERVAL`, Task 1.4) is a UX-freshness choice, not an
NFR1-governed one, and is treated as such — no benchmark, same
non-benchmark structural-argument precedent as every prior story, just
applied to a different concern here (perceived bar-update latency, not
`wm`'s action budget).

No new ADR. Every decision in this story's Description/Task 1 is an
implementation-decision gap-fill within this story's own existing scope
(matching Story 2.1's four, 2.2's five, 2.3's four, and 2.4's four
precedents) — the wire contract (`get-state` only, no new request types)
was already fully specified in Story 2.1/ADR-007, and no architecturally
significant alternative is left undecided by this story that would
warrant a new ADR entry.

Build/test/lint only ever run **inside the devcontainer**, `devpod ssh
buoy-wm` first, falling back to `podman exec -u vscode -w
/workspaces/buoy-wm bold_vaughan <cmd>` if that transport is unreliable —
confirmed unreliable in every prior Epic 2 story's own session, expect
the same here.

## Test plan

This story's tests split into four tiers:

1. **Design resolution** (Task 1) — no automated tests; the deliverable
   is six documented decisions (waybar mechanism, per-output CLI-argument
   identity, the three-strike-DRY non-extraction, the polling design and
   interval, the three-state render table plus the startup-default
   correction, and the JSON-injection-safety guard), each gating a later
   task's implementation, sourced from waybar's own `waybar-custom(5)`/
   `waybar(5)` man pages and direct inspection of this codebase
   (`wm/src/wm_core/state.rs`, `wm/src/ipc/protocol.rs`,
   `wm/src/ipc/server.rs`, `tag-picker/src/wire.rs`/`socket_path.rs`)
   rather than invented fresh.
2. **`status-bar::socket_path`** (Task 3) — four unit tests, ported
   verbatim from `tag-picker/src/socket_path.rs`'s existing equivalents,
   run via `cargo test --workspace`, no sockets, no environment
   mutation (all inputs injected as plain `Option<&str>` parameters).
3. **`status-bar::wire`** (Task 4) — six unit tests covering
   request serialization, full-shape response parsing (including the
   first client-side use of `OutputDto`), and the standard
   malformed-input rejection sweep (non-JSON, invalid UTF-8, unknown
   type, missing `type` field) — no sockets.
4. **`status-bar::bar_line`** (Task 5) — twelve unit tests: four
   covering `resolve_bar_line`'s full decision table (normal, no-tag-
   selected, unknown-output, and the defensive missing-tag-name
   fallback), five covering `format_waybar_line`'s JSON output for each
   render state plus the JSON-injection-safety guard (a tag name
   containing `"` and `\`, verified to still be valid, round-trippable
   JSON — the one correctness risk unique to this story relative to
   `tag-picker`'s tab-delimited, non-JSON wire format), and three
   covering `changed`'s equality logic — all pure, no sockets, no
   process spawning.
5. **`status-bar` CLI-argument parsing** (Task 6.1-6.2) — four unit
   tests covering `parse_output_id`'s full accept/reject table (valid
   numeric argument, zero arguments, more than one argument, a
   non-numeric argument) — pure, no sockets.
6. **`status-bar`'s poll loop and `main`** (Task 6.3) — **not** covered
   by automated tests, the same boundary class as every prior story's
   process-spawn/socket-I/O glue: the real `UnixStream::connect`/send/
   read round trip, the real stdout write/flush, and the real infinite
   loop with no exit path are unverified beyond Task 1's design
   rationale and structural review against 6.3's explicit description.
   Verified via `cargo build --workspace`/`cargo clippy --workspace
   --all-targets -- -D warnings` succeeding and the full workspace suite
   staying green (Task 8).
7. **Tooling gate** (Tasks 7-8) — `cargo fmt --all -- --check`, `cargo
   clippy --workspace --all-targets -- -D warnings`, `cargo test
   --workspace`, all in-container; `pre-commit run --all-files` against
   the newly-widened hook globs (Task 7's concrete fix — the prior
   two-member regex would otherwise silently never lint/fmt-check this
   story's own new files).

**Remaining, explicitly-flagged coverage gaps:** no `waybar` binary and
no live Wayland/`river` session exist anywhere in this sandbox (the same
disclosed-gap class `fuzzel` carried through Stories 2.2-2.4). This
story's own live-only unknowns are recorded in Task 8.3: whether the
real waybar process actually treats a no-`interval`/no-`signal` custom
module exactly as its own man page describes; the real per-output
multi-bar-block config producing one `status-bar` process per physical
output as designed; and the real visual result once a real `style.css`
targets the `normal`/`disconnected` CSS classes this binary emits. Every
*decision* behind these has full unit-test coverage; the live subprocess
integration and visual result remain unverified beyond structural review
and the direct man-page citations in Technical notes/Task 1. A live
smoke test is recommended before this is relied upon in daily use, the
same recommendation every prior Epic 2 story carried into this one — and,
since this is the last story in Epic 2 and in the entire currently
planned project, the natural point to finally close all of these
accumulated live-verification gaps in one real end-to-end session before
calling the daily-driver UX genuinely done.

## FR coverage
FR10

## Dev Agent Record

### Implementation Plan

Followed the story's own task breakdown exactly, in order, RED before
GREEN for every pure-logic task:

- **Task 1** (design decisions): all six decisions were already fully
  documented in the story's own Description/Technical notes, each verified
  against the actual codebase before being accepted as fact rather than
  recollection — `DESIGN.md`'s `bar-tag-normal`/`bar-tag-disconnected`/
  `accent-error` citations (`_bmad-output/planning-artifacts/ux-designs/
  ux-buoy-wm-2026-08-06/DESIGN.md`), `wm/src/wm_core/state.rs:271`'s
  `register_output` taking no arguments (confirming no startup-default
  tag), and `wm/src/ipc/protocol.rs`'s `OutputDto`/`Response::State` shape
  (confirming `get-state` already exposes everything needed). No RED/GREEN,
  per the story's own carve-out.
- **Task 2** (scaffolding): added `"status-bar"` to root `Cargo.toml`'s
  `members`, created `status-bar/Cargo.toml` (identical dependency shape
  to `tag-picker/Cargo.toml`: `serde`+`serde_json` only, no `fuzzel`
  anything) and a `fn main() {}` stub. Confirmed `cargo build --workspace`
  succeeded before proceeding.
- **Task 3** (`status-bar/src/socket_path.rs`): wrote all four
  `resolve_socket_path` tests first against a module with no
  implementation at all — confirmed `error[E0425]: cannot find function
  'resolve_socket_path'` (x4). Implemented `resolve_socket_path`/
  `default_socket_path` byte-for-byte identical to `tag-picker`'s own
  copy. All four tests passed (a transient `dead_code` warning on
  `default_socket_path` was expected and resolved once Task 6 wired it
  into `main`).
- **Task 4** (`status-bar/src/wire.rs`): wrote all six tests first
  (`serializes_get_state_request`,
  `parses_state_response_with_tags_and_outputs_ignoring_unmodeled_fields`,
  and the four `parse_response_rejects_*` negative tests) against a module
  with no `Request`/`Response`/`TagDto`/`OutputDto` at all — confirmed 10
  compile errors (`E0433`/`E0425`). Implemented `Request::GetState`-only
  enum, `TagDto`, `OutputDto` (this client's first-ever need for
  per-output data), a `Response::State{ tags, outputs }` narrower than
  `wm`'s own (no `views`/`focused_view`), plus `Ok`/`TagCreated`/`Error`
  kept only for negative-test/defensive-match purposes. Module doc comment
  records the Task 1.3 three-strike-DRY non-extraction decision in full.
  All six new tests passed.
- **Task 5** (`status-bar/src/bar_line.rs`): wrote all twelve tests first
  (four `resolve_bar_line`, five `format_waybar_line`, three `changed`)
  against a module with no `BarLine`/functions at all — confirmed 22
  compile errors. Implemented `BarLine{Normal,NoTagSelected,UnknownOutput}`,
  `resolve_bar_line` (output-not-found → `UnknownOutput`, `current_tag:
  None` → `NoTagSelected`, `current_tag: Some(id)` → `Normal(name)` with a
  defensive `format!("tag {id}")` fallback if the tag id is somehow
  absent), a private `WaybarLine` struct + `format_waybar_line` built
  exclusively through `serde_json::to_string` (verified against a tag name
  containing both `"` and `\`, round-tripped through `serde_json::Value`
  to confirm no corruption), and `changed`. All twelve new tests passed.
- **Task 6** (`status-bar/src/main.rs`): wrote all four `parse_output_id`
  tests first in a `#[cfg(test)] mod tests` inside `main.rs` — confirmed 4
  compile errors. Implemented `parse_output_id` exactly per spec; all four
  tests passed. Then implemented the untested I/O-glue poll loop
  (`try_get_state`, `main`'s argument-parse-then-loop-forever shape) with
  no RED/GREEN, per the story's own carve-out — every decision it composes
  is already unit-tested elsewhere.
- **Task 7** (`.pre-commit-config.yaml`): widened both hooks' `files:`
  regex from the hardcoded `^(wm|tag-picker)/...` two-name alternation to
  `^(wm|tag-picker|status-bar)/...`, with an expanded comment explaining
  both the concrete gap this closes and why a fully member-name-agnostic
  glob was deliberately not used (this is the last story in the entire
  currently planned project — no future fourth member to future-proof
  against).
- **Task 8**: full verification gate, in-container only. `devpod ssh
  buoy-wm` errored ("Error tunneling to container...") on the first
  attempt this session, consistent with every prior Epic 2 story's own
  note; fell back to `podman exec -u vscode -w /workspaces/buoy-wm
  bold_vaughan <cmd>` for every command in this story.

### Completion Notes

- `cargo fmt --all -- --check` found one real, pre-existing-style
  formatting violation in `status-bar/src/bar_line.rs` (a long test
  function signature line rustfmt wanted joined onto one line) — fixed via
  `cargo fmt --all`, then reconfirmed `--check` clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, zero
  warnings, on the first run after the fmt fix.
- `cargo test --workspace`: 149 `wm` + 63 `tag-picker` + 26 `status-bar`
  (4 `socket_path` + 6 `wire` + 12 `bar_line` + 4 `parse_output_id`) = 238
  total, all green — exactly matching the story's own Task 8.1 prediction.
- `cargo build --workspace`: clean.
- `pre-commit run --all-files`: both hooks (`cargo fmt --check`, `cargo
  clippy`) passed. Additionally ran `pre-commit run --files
  status-bar/src/main.rs status-bar/Cargo.toml` in isolation to positively
  confirm the Task 7 regex fix is what makes `status-bar` changes actually
  gated (both hooks fired and passed against those paths alone — under the
  pre-fix two-name regex they would have been silently skipped instead).
- Zero changes to `wm/src/ipc/`, `wm/src/wm_core/`, or `wm/src/main.rs` —
  confirmed via `git diff --stat`, matching the story's explicit
  constraint; `wm_core` remains exactly as protocol-agnostic as Story 2.1
  left it. No new WM-side IPC request/response types were added; `status-
  bar` consumes only the pre-existing `get-state`/`Response::State` shape.
- Live-only gaps carried forward unverified, per Task 8.3 and Technical
  notes: no real `waybar` binary and no live Wayland/`river` session exist
  in this sandbox, so the actual "script that loops itself" waybar
  behavior, the real per-output multi-bar-block config producing one
  `status-bar <id>` process per physical output, and the real visual
  result of the `disconnected`/`normal` CSS classes against a real
  `style.css` all remain unverified beyond unit tests, structural review,
  and the direct `waybar-custom(5)`/`waybar(5)` man-page citations already
  in the story's Description/Task 1. A live smoke test is recommended
  before this is relied upon in daily use — and, since this is the last
  story in Epic 2 and in the entire currently planned project, the natural
  point to finally close all such accumulated live-verification gaps
  (`fuzzel` included) in one real end-to-end session before calling the
  daily-driver UX genuinely done.

### Code Review Follow-up

Code review found a real bug in `try_get_state` (`status-bar/src/main.rs`):
the `UnixStream` it connects with had no read or write timeout set at all,
so a `wm` that accepted the connection but stalled or never wrote a
response (deadlocked, under heavy load, or simply never responding) hung
`reader.read_line()` forever — wedging the entire `status-bar` process with
no "disconnected" state shown and no automatic recovery, unlike every
other failure path (no socket file, connection refused, EOF, malformed
response), which all correctly degrade to the disconnected state and retry
every `POLL_INTERVAL` tick. The reviewer reproduced this live with a mock
server that accepts the connection but never writes back, confirming
`timeout 8 ./target/debug/status-bar 0` hung for the full 8 seconds with
zero stdout output (exit 124).

**Fix**: extracted a new `connect_with_timeout` helper (`status-bar/src/
main.rs`) that connects and immediately calls
`UnixStream::set_read_timeout`/`set_write_timeout` with a new
`SOCKET_IO_TIMEOUT` constant (`Duration::from_millis(100)`) before any
read/write happens; `try_get_state` now calls this helper instead of
`UnixStream::connect` directly. `100ms` was chosen deliberately well under
`POLL_INTERVAL` (250ms) so a timeout still resolves in time for the loop
to retry on its very next tick rather than blocking past it (documented
inline at `SOCKET_IO_TIMEOUT`'s definition). Both `set_read_timeout`/
`set_write_timeout` calls use the existing `.ok()?` short-circuit pattern
already used throughout `try_get_state`/`connect_with_timeout` — a failure
setting either timeout folds into the same "connection failed this tick,
show disconnected, retry next tick" path as every other failure, so no new
match arm or `BarLine` variant was needed (NFR2: no new panic path, no
`.unwrap()` introduced).

**Testing**: added one new fast, deterministic unit test,
`connect_with_timeout_sets_the_configured_read_and_write_timeouts`
(`status-bar/src/main.rs`'s `tests` module) — binds a real `UnixListener`
on a temp socket path, calls `connect_with_timeout` against it, and
asserts `stream.read_timeout()`/`write_timeout()` both return
`Some(SOCKET_IO_TIMEOUT)`. This is the preferred fast/deterministic
approach the follow-up instructions called for (over a slow live-hang
reproduction) since `UnixStream::read_timeout()`/`write_timeout()` make it
practical — no sleeping, no stalling peer required. Extracting
`connect_with_timeout` out of `try_get_state` was the minimal refactor
needed to make the timeout configuration itself unit-testable in
isolation.

**Re-verification, in-container** (`podman exec -u vscode -w
/workspaces/buoy-wm bold_vaughan <cmd>`, `devpod ssh buoy-wm` still
unreliable this session): `cargo test --workspace` — 149 `wm` + 27
`status-bar` (26 prior + 1 new) + 63 `tag-picker` = 239 total, all green.
`cargo build --workspace` clean. `cargo fmt --all -- --check` clean.
`cargo clippy --workspace --all-targets -- -D warnings` clean, zero
warnings. `pre-commit run --all-files` — both hooks (`cargo fmt --check`,
`cargo clippy`) passed. Additionally re-ran the reviewer's original live
hang reproduction against a Python mock server that accepts the connection
but never writes a response: previously zero stdout output for the full
timeout duration; now `status-bar` prints
`{"text":"⚠ disconnected","class":"disconnected"}` within ~108ms (matching
the 100ms `SOCKET_IO_TIMEOUT`) and continues running/polling normally
afterward (confirmed by observing it stayed alive and kept the connection
attempt cadence going until the outer `timeout` ended the test — it never
exits or crashes on its own). The hang is confirmed fixed.

## File List

- `Cargo.toml` — modified (Task 2.1: added `"status-bar"` to `[workspace]
  members`)
- `Cargo.lock` — modified (new `status-bar` package entry; `serde`/
  `serde_json` already present as dependencies via `tag-picker`)
- `status-bar/Cargo.toml` — new (Task 2.2: package manifest)
- `status-bar/src/main.rs` — new (Tasks 2.3, 6: `parse_output_id`,
  `try_get_state`, poll-loop `main`)
- `status-bar/src/socket_path.rs` — new (Task 3: `resolve_socket_path`,
  `default_socket_path`)
- `status-bar/src/wire.rs` — new (Task 4: `Request`, `TagDto`,
  `OutputDto`, `Response`, `ParseError`, `serialize_request`,
  `parse_response`)
- `status-bar/src/bar_line.rs` — new (Task 5: `BarLine`, `resolve_bar_line`,
  `WaybarLine`, `format_waybar_line`, `changed`)
- `.pre-commit-config.yaml` — modified (Task 7: widened both hooks'
  `files:` regex to include `status-bar/`)
- `docs/planning/epics/story-2-5.md` — this file (task checkboxes, Dev
  Agent Record, File List, Change Log, Status)

## Change Log

- 2026-08-08: Implemented Story 2.5 (Per-Output Status Bar via Waybar
  Module) per the bmad-dev-story workflow — all 8 tasks complete, a new
  third workspace member `status-bar` added with 26 new tests (149 `wm` +
  63 `tag-picker` + 26 `status-bar` = 238 total), zero regressions, zero
  changes to `wm/src/ipc/`/`wm/src/wm_core/`/`wm/src/main.rs`, the
  pre-commit hook-coverage gap closed. `cargo fmt`/`clippy`/`pre-commit`
  all clean. Status moved to `review`. This is the last story in Epic 2
  and in the entire currently planned project.

## Status

review
