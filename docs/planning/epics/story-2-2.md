---
baseline_commit: c125257
---

# Story 2.2: Tag-Manager Picker — Toggle Existing Tags
Epic: 2 | Priority: H | Status: done

## Description
A hotkey on the focused window spawns a new companion binary, `tag-picker`,
which queries the WM over Story 2.1's IPC socket for the focused view's
current tags and drives `fuzzel` to show them as a checkbox-style list.
Selecting a row toggles that one tag on the focused window via `toggle-tag`
over IPC. This story covers **toggling existing tags only** — the picker's
free-text add-new-tag row is Story 2.3's job, and the second hotkey that
reuses this same picker for tag-*switching* is Story 2.4's job. Neither is
built here.

Per ADR-004, whether `fuzzel` can natively do "checkbox toggle in one
invocation" was an open spike item. **This story resolves it — see
Technical notes' "Spike finding."** The short version: real, current
`fuzzel` (1.14.1) supports neither native checkbox-toggle nor a `--multi`
flag. `EXPERIENCE.md`'s speculated fallback ("fuzzel's native multi-select
(`--multi`)") does not exist either — it was explicitly marked
`[ASSUMPTION] — not yet verified`, and this spike is that verification. The
actual, only-viable interaction given fuzzel's real flag surface is a
**sequential toggle-and-reopen loop**: one single-select `fuzzel --dmenu`
invocation per toggle, using real, documented flags (`--with-nth`,
`--accept-nth`, `--nth-delimiter`) to display a checkbox-glyph-prefixed
name per row and return a stable tag id on selection; `tag-picker` applies
the toggle over IPC and immediately reopens `fuzzel` with refreshed
glyphs, until Escape/cancel ends the loop. Full rationale and every flag
cross-checked against the man page in Technical notes.

This is also the first story to add a second Cargo target
(`tag-picker`, a "separate binary" per `components.md`), which requires
converting `wm/Cargo.toml`'s single-package layout into a two-member
workspace — a conversion Story 2.1's Technical notes explicitly deferred to
"whichever future story first adds `tag-picker`/`status-bar` as actual
separate Cargo targets," and which ADR-008 already anticipated
("`buoy-wm/wm/` (workspace subdirectory)"). See Technical notes'
"Workspace conversion."

**Five gaps neither the PRD, the ADRs, `components.md`, nor `EXPERIENCE.md`
fully resolve, closed here rather than silently invented or deferred** (see
Technical notes for full rationale on each):

1. **Fuzzel checkbox-toggle/multi-select capability** — the named spike.
   **Resolution:** neither exists; adopt the toggle-and-reopen loop (above).
2. **Cargo workspace structure for `tag-picker`.** **Resolution:** a new
   root-level `Cargo.toml` (`[workspace] members = ["wm", "tag-picker"]`);
   `wm/`'s existing layout is otherwise untouched.
3. **Wire-type duplication vs. a shared protocol crate.** **Resolution:**
   `tag-picker` defines its own minimal mirror of the two request/three
   response shapes it needs, rather than extracting a shared crate. Per
   this project's own three-strike DRY rule, two independent definitions of
   the same wire shapes (`wm`'s and `tag-picker`'s) is one occurrence below
   the extraction threshold; a shared crate is deferred to whichever story
   first gives `status-bar` (Story 2.5) the same need — a genuine third
   consumer.
4. **Exact hotkey/keysym for opening the picker.** No document names one.
   **Resolution:** `Mod4+A` ("Assign"), following the single-letter-mnemonic
   convention Story 1.7 already established (`T`=TagCreate, `N`=FocusNext,
   `Q`=Close).
5. **Fuzzel's exact exit-code/stdout behavior on Escape/cancel.** Not
   verified live (see Technical notes' "Spike method" — no live Wayland
   session or `fuzzel` binary available in this sandbox). **Resolution:**
   `tag-picker`'s response parser treats *any* non-success exit or
   unparseable stdout as "cancelled," rather than assuming a specific exit
   code — defensive by construction, not dependent on the unverified detail.

## Acceptance criteria
**Given** a window is focused (per `get-state`'s `focused_view` field)
**When** `Mod4+A` is pressed
**Then** the WM spawns the `tag-picker` binary fire-and-forget (same
`std::process::Command::spawn()` pattern as `Action::SpawnFoot`, no
argument passed — `tag-picker` resolves the focused view itself via
`get-state`)
**And** `tag-picker` connects to the IPC socket (same resolution logic as
`wm`'s `resolve_socket_path`), sends `{"type":"get-state"}`, and — if
`focused_view` is non-null — opens one `fuzzel --dmenu` invocation listing
every registry tag as one row: `"[✓] <name>"` for tags the focused view
currently has, `"[ ] <name>"` for the rest, in registry order

**Given** fuzzel's real flag surface has neither native checkbox-toggle nor
`--multi` (this story's spike — Technical notes' "Spike finding")
**When** the user selects one row
**Then** `tag-picker` sends `{"type":"toggle-tag","view_id":<id>,"tag_id":<id>}`
over the same already-open IPC connection, and on `{"type":"ok"}` flips that
tag's local checked state and immediately reopens `fuzzel` with the
refreshed glyphs (the toggle-and-reopen loop) — each individual toggle
lands on the WM within the 50ms WM-side handling budget (NFR1; `fuzzel`'s
own render/reopen time is compositor/UI time, outside NFR1's WM-side-only
scope per `non-functional-requirements.md`)
**And** on `{"type":"error","message":"..."}` (e.g. the window closed
mid-session and `view_id` is now unknown) the message is printed to stderr
and the loop ends without a further `fuzzel` invocation

**When** the user presses Escape/cancels, or `fuzzel` exits non-zero, or its
stdout is empty/unparseable as a valid selected tag id
**Then** the loop ends with no further IPC call, `tag-picker` exits 0, and
the WM's tag state is left exactly as of the last successfully applied
toggle (each toggle already committed individually — there is no
batched/all-or-nothing accept step)

**Given** no window is focused when `Mod4+A` is pressed
**Then** `tag-picker` sees `get-state`'s `focused_view: null`, never invokes
`fuzzel`, and exits 0

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: Spike — resolve fuzzel's checkbox-toggle/multi-select capability (ADR-004's open item; AC: "fuzzel's real flag surface has neither native checkbox-toggle nor `--multi`")**
  - [x] 1.1 Confirm `fuzzel` is not runnable interactively in this sandbox: neither the host nor the `bold_vaughan` devcontainer has the `fuzzel` binary installed (`which fuzzel` fails in both), and neither has a live Wayland compositor session for it to render into even if installed — same class of gap as this project's established Wayland-glue carve-out. This story's spike is therefore **documentation-based verification against fuzzel's own current, authoritative man page**, not a live interactive test, and is flagged as such rather than silently treated as equivalent to one.
  - [x] 1.2 Fetch and read `fuzzel(1)`'s complete, current option list (Arch Linux's package build, version 1.14.1, dated 2026-08-03 — i.e. essentially today's real current release, not a stale mirror). Enumerate every `--dmenu`-mode-relevant flag actually present: `--dmenu`/`-d`, `--dmenu0`, `--index`, `--with-nth`, `--accept-nth`, `--match-nth`, `--nth-delimiter`, `--only-match`, `--select`, `--select-index`, `--auto-select`, `--no-run-if-empty`, `--password`. Confirm, by their absence from this complete list: **no `--multi`/`--multiselect` flag exists**, and **no checkbox/toggle-state concept exists anywhere in dmenu mode** — dmenu mode is fundamentally single-select: one entry chosen per invocation, printed (or, with `--index`/`--accept-nth`, an index/column) to stdout.
  - [x] 1.3 Cross-check against `codeberg.org/dnkl/fuzzel`'s issue tracker for any open or historical multi-select feature request, to confirm this is a genuine, known gap in the tool rather than an undocumented flag the man page omitted. (No such feature exists as of this check.)
  - [x] 1.4 Record the finding in this story's Technical notes ("Spike finding," below) and in the Dev Agent Record once implementation starts: fuzzel supports neither of `EXPERIENCE.md`'s two speculated outcomes (native toggle, or native `--multi`). The only interaction achievable with fuzzel's real flag surface is the sequential **toggle-and-reopen loop** described there, built from real, existing flags (`--dmenu`, `--with-nth=1`, `--accept-nth=2`, `--nth-delimiter=<tab>`). Also record the visual-fidelity consequence: `DESIGN.md`'s `picker-checkbox-checked` accent-ok *color* token cannot be realized — `fuzzel --dmenu` has one global text/selection color for the whole list, no per-row color control from stdin — so checked/unchecked is carried by the `"[✓] "`/`"[ ] "` glyph prefix alone. This satisfies the Accessibility Floor's "color is never the sole carrier of state" rule but is visually plainer than the mockup's colored checkbox.
  - [x] **No RED/GREEN** — this task produces a documented finding, not code. It gates every later task's design (Tasks 4 and 6 are written directly against this finding, not against `EXPERIENCE.md`'s un-verified fallback text).

- [x] **Task 2: Cargo workspace conversion — add `tag-picker` as a genuine second binary target (AC: none directly; infrastructure this story's actual code needs)**
  - [x] 2.1 Add a new root-level `/Cargo.toml`: `[workspace]` with `resolver = "2"` and `members = ["wm", "tag-picker"]`. `wm/Cargo.toml` is otherwise untouched (no `[workspace]` table of its own — Cargo treats it as a normal member manifest once a root workspace manifest exists above it).
  - [x] 2.2 Create `tag-picker/Cargo.toml`: `[package] name = "tag-picker", version = "0.1.0", edition = "2024"`; `[dependencies] serde = { version = "1", features = ["derive"] }, serde_json = "1"` — deliberately **no** `wayland-client`/`wayland-backend`/`bitflags` (those are `wm`-only; this is the concrete reason a workspace split, not a same-package second `[[bin]]`, was chosen — `components.md`'s "separate binary" language means a genuinely separate dependency surface, not just a separate executable sharing one `Cargo.toml`). Create `tag-picker/src/main.rs` with a trivial `fn main() {}` placeholder for this task only (filled in by Tasks 3-6).
  - [x] 2.3 Regenerate the lockfile at the new workspace root (`cargo generate-lockfile` from repo root inside the devcontainer, or an equivalent full `cargo build`), producing a unified root-level `Cargo.lock` covering both members. Remove the now-stale `wm/Cargo.lock` (superseded by the root one — a workspace has exactly one lockfile). Add a root-level `.gitignore` containing `/target` (the build output directory moves from `wm/target` to the workspace root `target/` once builds run via the root manifest); `wm/.gitignore`'s existing `/target` line is now redundant but harmless — leave it, since `wm/target` could still appear if a command is mistakenly run with an explicit `--manifest-path wm/Cargo.toml` bypassing the workspace root.
  - [x] 2.4 Update `.github/workflows/ci.yml`: `cargo build --manifest-path wm/Cargo.toml` → `cargo build --workspace` (run from repo root, builds both members); same for the `cargo test` step. Update `.pre-commit-config.yaml`'s two hooks: `entry: cargo fmt --manifest-path wm/Cargo.toml --all -- --check` → `entry: cargo fmt --all -- --check` (workspace-wide, no `--manifest-path` needed once run from repo root); `entry: cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings` → `entry: cargo clippy --workspace --all-targets -- -D warnings`; both hooks' `files:` globs widen from `^wm/(src/.*\.rs|Cargo\.(toml|lock))$` to `^(wm|tag-picker)/(src/.*\.rs|Cargo\.toml)$|^Cargo\.(toml|lock)$` so changes to either member or the root manifest/lockfile trigger the gate.
  - [x] 2.5 Verify: `cargo build --workspace` and `cargo test --workspace`, run from the repo root inside the devcontainer, both succeed — the full pre-existing `wm` suite passes with **zero** regressions (144 tests — one more than the story's estimated 143, see Dev Agent Record), and the placeholder `tag-picker` binary builds cleanly with 0 tests (expected, same "0 tests run, exit 0 is correct" precedent `ci.yml`'s own comment already documents for `tinyrwm`).
  - [x] **No RED/GREEN** — pure build-graph/tooling restructuring, no decision logic of its own. Verified via the 2.5 build/test run plus a diff review confirming `wm/src/**` is byte-for-byte unchanged by this task.

- [x] **Task 3: `tag-picker/src/wire.rs` — the minimal request/response mirror `tag-picker` needs (AC: "connects to the IPC socket... sends `get-state`... sends `toggle-tag`")**
  - [x] 3.1 RED — In `tag-picker/src/wire.rs`'s `#[cfg(test)] mod tests`: `serializes_get_state_request` — `serialize_request(&Request::GetState)` equals `r#"{"type":"get-state"}"#`. `serializes_toggle_tag_request` — `serialize_request(&Request::ToggleTag{view_id:3,tag_id:2})` equals `r#"{"type":"toggle-tag","view_id":3,"tag_id":2}"#`. `parses_state_response_ignoring_unmodeled_fields` — feed the exact literal from `wm`'s own Story 2.1 Technical notes example, `r#"{"type":"state","tags":[{"id":0,"name":"web"}],"views":[{"id":3,"app_id":"foot","tags":[0,2]}],"outputs":[{"id":0,"current_tag":0}],"focused_view":3}"#`, assert `parse_response(...)` equals `Ok(Response::State{tags: vec![TagDto{id:0,name:"web".into()}], views: vec![ViewDto{id:3,tags:vec![0,2]}], focused_view: Some(3)})` — proving `app_id` and `outputs` (fields this client never declares) are silently ignored, not rejected (same no-`deny_unknown_fields` default `wm`'s own `Request` enum relies on). `parses_ok_response`, `parses_error_response`. `parse_response_rejects_non_json_garbage`, `parse_response_rejects_invalid_utf8_bytes` (`&[0xFF, 0xFE]`), `parse_response_rejects_unknown_type` (`r#"{"type":"tag-created","tag_id":5}"#` — a real `wm` response shape, just not one this client models, since `tag-picker` never sends `create-tag`), `parse_response_rejects_object_missing_type_field` (`"{}"`). Confirm all fail to compile.
  - [x] 3.2 GREEN — Define `#[derive(Debug, Clone, PartialEq, Serialize)] #[serde(tag = "type", rename_all = "kebab-case")] pub enum Request { GetState, ToggleTag { view_id: u64, tag_id: u8 } }`; `#[derive(Debug, Clone, PartialEq, Deserialize)] pub struct TagDto { pub id: u8, pub name: String }`; `#[derive(Debug, Clone, PartialEq, Deserialize)] pub struct ViewDto { pub id: u64, pub tags: Vec<u8> }` (no `app_id` field — unused by this client, and serde's default unknown-field tolerance means the server can keep sending it); `#[derive(Debug, Clone, PartialEq, Deserialize)] #[serde(tag = "type", rename_all = "kebab-case")] pub enum Response { State { tags: Vec<TagDto>, views: Vec<ViewDto>, focused_view: Option<u64> }, Ok, Error { message: String } }` (no `TagCreated` variant — not used by `toggle-tag`/`get-state`). `pub enum ParseError { InvalidUtf8, InvalidJson(String) }`. `pub fn serialize_request(request: &Request) -> String` (`serde_json::to_string(request).expect(...)`, same safe-by-construction precedent as `wm`'s `serialize_response`). `pub fn parse_response(bytes: &[u8]) -> Result<Response, ParseError>` (utf8-check-then-`serde_json::from_str`, mirroring `wm`'s `parse_request` exactly). `///` doc comment on the module explaining this is a deliberate, minimal, two-occurrence-tolerated mirror of `wm/src/ipc/protocol.rs`'s wire contract (Technical notes gap #3), not a shared crate. Confirm all new tests pass.

- [x] **Task 4: `tag-picker/src/checklist.rs` — the pure decision logic driving the toggle-and-reopen loop (AC: checkbox-glyph rendering, selection→toggle mapping, cancel detection, "no focused window" guard)**
  - [x] 4.1 RED — In `tag-picker/src/checklist.rs`'s `#[cfg(test)] mod tests`: `build_checklist_entries_marks_focused_views_tags_as_checked` — `build_checklist_entries(&[TagDto{id:0,name:"web".into()}, TagDto{id:1,name:"chat".into()}], &[0])` returns `vec![ChecklistEntry{tag_id:0,name:"web".into(),checked:true}, ChecklistEntry{tag_id:1,name:"chat".into(),checked:false}]`. `build_checklist_entries_preserves_registry_order` — entries come back in the same order as the input `tags` slice (registry/wire order — no re-sort). `build_checklist_entries_with_no_focused_tags_are_all_unchecked`. `render_fuzzel_input_formats_checked_row` — `render_fuzzel_input(&[ChecklistEntry{tag_id:0,name:"web".into(),checked:true}])` equals `"[✓] web\t0\n"`. `render_fuzzel_input_formats_unchecked_row` — equals `"[ ] chat\t1\n"`. `render_fuzzel_input_joins_multiple_rows_in_order`. `render_fuzzel_input_of_empty_entries_is_empty_string`. `parse_fuzzel_output_returns_toggled_for_successful_exit_and_valid_tag_id` — `parse_fuzzel_output(true, "3\n")` equals `PickerAction::Toggled(3)` (trailing newline trimmed). `parse_fuzzel_output_returns_cancelled_for_nonzero_exit` — `parse_fuzzel_output(false, "3\n")` equals `PickerAction::Cancelled` (exit status wins even if stdout looks parseable — this is the concrete case a real Escape-with-stale-buffer would hit). `parse_fuzzel_output_returns_cancelled_for_empty_stdout` — `parse_fuzzel_output(true, "")`. `parse_fuzzel_output_returns_cancelled_for_unparseable_stdout` — `parse_fuzzel_output(true, "not-a-number")`. `parse_fuzzel_output_returns_cancelled_for_tag_id_out_of_u8_range` — `parse_fuzzel_output(true, "999")` (defends against a hypothetical corrupted `--accept-nth` column). `toggle_local_membership_adds_tag_when_absent` — `let mut tags = vec![0]; toggle_local_membership(&mut tags, 1); assert_eq!(tags, vec![0,1]);`. `toggle_local_membership_removes_tag_when_present` — `let mut tags = vec![0,1]; toggle_local_membership(&mut tags, 1); assert_eq!(tags, vec![0]);`. `toggle_local_membership_is_idempotent_pairwise` — toggling the same id twice returns to the original vec. `should_open_picker_true_when_focused_view_is_some`, `should_open_picker_false_when_focused_view_is_none`. Confirm all fail to compile.
  - [x] 4.2 GREEN — `#[derive(Debug, Clone, PartialEq)] pub struct ChecklistEntry { pub tag_id: u8, pub name: String, pub checked: bool }`. `pub fn build_checklist_entries(tags: &[TagDto], focused_view_tags: &[u8]) -> Vec<ChecklistEntry>`: map each `TagDto` to `ChecklistEntry { tag_id: t.id, name: t.name.clone(), checked: focused_view_tags.contains(&t.id) }`, preserving input order. `pub fn render_fuzzel_input(entries: &[ChecklistEntry]) -> String`: for each entry, `format!("{} {}\t{}\n", if entry.checked { "[✓]" } else { "[ ]" }, entry.name, entry.tag_id)`, concatenated — column 1 (before the tab) is the checkbox+name shown via `--with-nth=1`, column 2 is the bare tag id returned via `--accept-nth=2` (Technical notes' "Spike finding" — these are real, existing fuzzel flags). `#[derive(Debug, Clone, Copy, PartialEq)] pub enum PickerAction { Toggled(u8), Cancelled }`. `pub fn parse_fuzzel_output(exit_success: bool, stdout: &str) -> PickerAction`: `if !exit_success { return PickerAction::Cancelled; } match stdout.trim().parse::<u8>() { Ok(id) => PickerAction::Toggled(id), Err(_) => PickerAction::Cancelled }`. `pub fn toggle_local_membership(tags: &mut Vec<u8>, tag_id: u8)`: `if let Some(pos) = tags.iter().position(|&t| t == tag_id) { tags.remove(pos); } else { tags.push(tag_id); }` (same add-if-absent/remove-if-present semantics as `wm_core`'s own `toggle_view_tag`/`TagSet`, kept independent per Technical notes gap #3's duplication rationale rather than calling into `wm_core` from a different binary/crate). `pub fn should_open_picker(focused_view: Option<u64>) -> bool { focused_view.is_some() }`. `///` doc comments throughout. Confirm all new tests pass.

- [x] **Task 5: `tag-picker/src/socket_path.rs` — mirrors `wm`'s `resolve_socket_path` (AC: "connects to the IPC socket")**
  - [x] 5.1 RED — Same four tests as Story 2.1 Task 5, against this crate's own copy: `resolve_socket_path_uses_xdg_runtime_dir_when_set`, `resolve_socket_path_falls_back_to_tmp_user_when_xdg_runtime_dir_unset`, `resolve_socket_path_falls_back_to_logname_when_user_also_unset`, `resolve_socket_path_falls_back_to_literal_unknown_when_nothing_is_set` — identical signatures and expected `PathBuf`s (`/run/user/1000/buoy-wm.sock`, `/tmp/buoy-wm-nick.sock`, `/tmp/buoy-wm-unknown.sock`) as `wm/src/ipc/server.rs`'s existing, already-passing tests, since both processes must resolve to the *same* socket path to talk to each other. Confirm all fail to compile.
  - [x] 5.2 GREEN — `pub fn resolve_socket_path(xdg_runtime_dir: Option<&str>, user: Option<&str>, logname: Option<&str>) -> PathBuf` and `pub fn default_socket_path() -> PathBuf` — byte-for-byte the same bodies as `wm/src/ipc/server.rs`'s versions (Technical notes gap #3's duplication rationale applies identically here: two occurrences, below the three-strike threshold). `///` doc comment noting the two copies must be kept in sync if the resolution rule ever changes, and that this is exactly the kind of drift risk a future shared crate (deferred to a genuine third consumer) would eliminate. Confirm all new tests pass.

- [x] **Task 6: `tag-picker/src/main.rs` — the toggle-and-reopen loop orchestration (AC: all — process spawn, socket I/O, the loop itself)**
  - [x] 6.1 Implement `fn main()`: resolve the socket path (`socket_path::default_socket_path()`), `UnixStream::connect` (on failure, `eprintln!` and `std::process::exit(1)` — no WM to talk to means nothing this binary can do). Send `wire::Request::GetState` (serialize + `\n` + flush), read one line back, `wire::parse_response`; on anything other than `Response::State{..}` (parse error, or an unexpected variant), `eprintln!` and exit(1). If `!checklist::should_open_picker(focused_view)`, `eprintln!("tag-picker: no window focused")` and exit(0) (AC: "no window focused... exits 0", not an error). Otherwise, look up the focused view's own `tags` from the `State` response's `views` list (default to empty if somehow absent) as the mutable `current_tags: Vec<u8>` the loop maintains locally.
  - [x] 6.2 The loop: `checklist::build_checklist_entries(&tags, &current_tags)` → `checklist::render_fuzzel_input(&entries)` → spawn `fuzzel --dmenu --with-nth=1 --accept-nth=2 --nth-delimiter=$'\t'` (`std::process::Command`, stdin piped with the rendered input written and the handle dropped/closed, stdout captured), collect `(exit_status.success(), stdout_as_utf8_lossy_string)` → `checklist::parse_fuzzel_output(..)`. On `PickerAction::Cancelled`, `break`. On `PickerAction::Toggled(tag_id)`: send `wire::Request::ToggleTag{view_id, tag_id}` on the **same** connection, read one response line, `parse_response`; on `Response::Ok`, `checklist::toggle_local_membership(&mut current_tags, tag_id)` and loop again (reopen `fuzzel` with refreshed glyphs); on `Response::Error{message}`, `eprintln!("tag-picker: {message}")` and `break`; on anything else (parse failure, unexpected variant), `eprintln!` and `break` (never panic — NFR2-style discipline extended to this client, even though NFR2 is formally a WM-side requirement).
  - [x] **No RED/GREEN** — process-spawn and live-socket I/O glue, the same carve-out class as `wm/src/main.rs`'s Wayland-`Dispatch` tasks and Story 2.1 Task 7: every decision this code path reaches (`should_open_picker`, `build_checklist_entries`, `render_fuzzel_input`, `parse_fuzzel_output`, `toggle_local_membership`, `parse_response`/`serialize_request`) is already unit-tested in Tasks 3-4; this task is pure composition. Verified via `cargo build --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` and structural code review against the 6.1/6.2 description above.

- [x] **Task 7: Wire the WM-side hotkey (`wm/src/main.rs`) — `Action::OpenTagPicker`, `Mod4+A` (AC: "the WM spawns the `tag-picker` binary fire-and-forget")**
  - [x] 7.1 Add `OpenTagPicker` to `enum Action` (`wm/src/main.rs:62-72`, alongside `SpawnFoot`/`TagCycle`/`TagCreate`).
  - [x] 7.2 In `init_new_seats` (`wm/src/main.rs:338-364`), add `const A: u32 = 0x61;` next to the existing `SPACE`/`N`/`Q`/`ESC`/`TAB`/`T` keysym constants (Technical notes gap #4 — `Mod4+A`, "Assign," following the established single-letter-mnemonic convention), and `seat.create_xkb_binding(river_xkb, qh, mods, A, Action::OpenTagPicker);` alongside the existing binding calls.
  - [x] 7.3 In `Seat::do_action` (`wm/src/main.rs:615-751`), add the `Action::OpenTagPicker` arm: byte-for-byte the same shape as the existing `Action::SpawnFoot` arm (`wm/src/main.rs:628-637`), spawning `"tag-picker"` instead of `"foot"`, same `env_remove("WAYLAND_DEBUG")`, same `Ok(_) => {}, Err(e) => eprintln!(...)` handling, returning `None` (never switches an output's active tag, so `manage_seats`' pinned-terminal-spawn signal is correctly never triggered by this action — same reasoning `TagCreate`'s own `None` return already documents). No `wm_core` access needed at all — `tag-picker` resolves the focused view itself via its own `get-state` call, so this arm is pure process-spawn, no state read.
  - [x] **No RED/GREEN** — same carve-out class and justification as Story 2.1 Task 7 (`main.rs` wiring: call-site composition, no new decision logic — `Action::SpawnFoot`'s existing arm is the direct, already-reviewed precedent this one copies). Verified via `cargo build --workspace` / `cargo clippy --workspace --all-targets -- -D warnings`, the full pre-existing 143-test `wm` suite staying green with zero regressions (this task adds one enum variant, one keysym constant, one binding call, and one `do_action` arm — no existing call site's behavior changes), and structural review confirming the new arm matches `SpawnFoot`'s pattern exactly.

- [x] **Task 8: Full in-container verification gate, across the new workspace (AC: all)**
  - [x] 8.1 Run, inside the devcontainer via `podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>` (`devpod ssh buoy-wm` was confirmed unreliable again at the start of this story — `Error tunneling to container: wait: remote command exited without exit status or exit signal` — so `podman exec` against the running `bold_vaughan` container was used throughout, per the story's own fallback instruction): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, all run from the repo root against the new root `Cargo.toml`. Confirmed: the pre-existing `wm` suite (144 tests — one more than this story's own 143 estimate, no discrepancy investigated further since it predates this story) all still pass, unchanged; `tag-picker` contributes 9 (`wire`, Task 3) + 17 (`checklist`, Task 4) + 4 (`socket_path`, Task 5) = 30 new tests, all green; zero clippy/fmt violations across both members.
  - [x] 8.2 Run `pre-commit run --all-files` (as the `vscode` user inside the container, per Story 2.1's own noted requirement) against the updated hook definitions (Task 2.4) — confirm both hooks now run workspace-wide and pass. Confirmed: both `cargo fmt --check` and `cargo clippy` hooks passed.
  - [x] 8.3 Manual smoke-test note — **this story's gap is materially larger than Story 2.1's own equivalent note**, and should be called out plainly rather than downplayed: unlike Story 2.1's socket I/O (which had full real-`UnixListener`/`UnixStream` integration-test coverage in-sandbox), **no part of the actual `fuzzel` invocation — the real subject of this story's spike — has been exercised live** in this sandbox, because neither the host nor the devcontainer has `fuzzel` installed nor a Wayland session to run it in. Tasks 3-5's unit tests cover every *decision* the code makes around that invocation (what to render, how to parse its output, how to detect cancel), but the actual `Command::new("fuzzel")` call, its real stdin/stdout framing, and its real Escape/cancel exit behavior are unverified beyond the man-page research in Task 1. Record in the Dev Agent Record that a live end-to-end smoke test (`river` running, a real focused window, `Mod4+A`, a real `fuzzel` on screen) was **not** performed and remains the single largest residual risk this story carries into Story 2.3/2.4, which build directly on this interaction pattern.

## Technical notes

**Spike finding (Task 1) — fuzzel has neither native checkbox-toggle nor
`--multi`.** Sourced from `fuzzel(1)`'s complete, current man page (Arch
Linux package build, version 1.14.1, dated 2026-08-03) — the authoritative,
up-to-date CLI contract, cross-checked against
`codeberg.org/dnkl/fuzzel`'s issue tracker for any open multi-select feature
request (none found). `--dmenu` mode is fundamentally single-select: stdin
lines in, one selected entry (or, via `--index`/`--accept-nth`, an
index/column of it) out on stdout, per invocation. There is no `--multi`
flag anywhere in the option list, and no concept of a per-row toggleable
checkbox state at all. This disproves *both* of `EXPERIENCE.md`'s
speculated outcomes ("primary interaction is native checkbox-toggle-in-one-
invocation... If fuzzel can't do that, fall back to fuzzel's native
multi-select (`--multi`)") — the second one doesn't exist either, and was
explicitly marked `[ASSUMPTION] — not yet verified` there, which is exactly
what this spike verifies. **Adopted interaction:** a sequential
toggle-and-reopen loop, built from real, existing flags:
- `--dmenu` — read tab-delimited lines from stdin.
- `--with-nth=1` — display only the checkbox-glyph+name column.
- `--accept-nth=2` — on selection, print only the tag-id column to stdout
  (not the displayed text), so `tag-picker` never has to parse a name back
  into an id.
- `--nth-delimiter` (default tab, used as-is) — the column separator.

Each row is `"[✓] <name>\t<tag_id>\n"` (checked) or `"[ ] <name>\t<tag_id>\n"`
(unchecked). Selecting a row toggles that one tag over IPC and the client
immediately relaunches `fuzzel` with refreshed glyphs — the interaction
`EXPERIENCE.md`'s fallback text explicitly said it wanted to avoid ("rather
than a sequential toggle-and-reopen loop"), but since neither of its two
hypothesized alternatives exists in real fuzzel, this loop is the only
interaction fuzzel's actual flag surface supports. **Visual-fidelity
consequence, flagged not hidden:** `fuzzel --dmenu` has exactly one global
text color and one global selection-background color for the whole list —
there is no per-row/per-glyph color control from stdin. `DESIGN.md`'s
`picker-checkbox-checked` component (accent-ok border/color/tinted
background) cannot be realized through this fuzzel-driven implementation;
the checked/unchecked distinction is carried entirely by the `"[✓] "`/
`"[ ] "` glyph text, which satisfies the UX spec's own Accessibility Floor
("color is never the sole carrier of state") but is visually plainer than
`mockups/key-picker.html`'s colored mockup. **Spike method, flagged not
hidden:** this is documentation-based verification, not a live interactive
test — `fuzzel` is installed on neither the host nor the `bold_vaughan`
devcontainer, and neither has a live Wayland compositor session, so no
sandbox in this project can actually run `fuzzel` interactively right now.
Task 8.3's manual-smoke-test note carries this gap forward explicitly.

**Workspace conversion (Task 2) — why now, and why a workspace rather than
a second `[[bin]]` in `wm/Cargo.toml`.** `components.md` describes
`tag-picker` as a "companion client, separate binary" with `ipc-server` and
`fuzzel` as its only key dependencies — notably *not* `wayland-client`.
Rust supports multiple binaries from one package (`src/bin/*.rs`), but they
all share that package's single `[dependencies]` table; `tag-picker` would
then unnecessarily link `wayland-client`/`wayland-backend`/`bitflags`,
misrepresenting its real dependency surface and adding build weight for no
reason. A genuine second Cargo package, added as a workspace member, is the
correct fit — and ADR-008 already named this destination
("`buoy-wm/wm/` (workspace subdirectory)"), so this converts an anticipated
layout into a real one rather than introducing a new architectural
direction. Story 2.1's own Technical notes explicitly deferred this exact
conversion to "whichever future story first adds `tag-picker`/`status-bar`
as actual separate Cargo targets" — this is that story. `wm/`'s existing
source tree, tests, and behavior are untouched by the conversion itself
(Task 2.5 verifies zero regressions in the pre-existing 143-test suite).

**Wire-type and `resolve_socket_path` duplication (gap #3) — why not a
shared crate yet.** This project's own DRY convention (three-strike rule:
extract after ~3 occurrences, not before) directly answers this. Right now
there are exactly two independent definitions of the relevant wire shapes
and of `resolve_socket_path`: `wm/src/ipc/protocol.rs`/`server.rs` (Story
2.1, already shipped and tested) and `tag-picker/src/wire.rs`/
`socket_path.rs` (this story). That's one occurrence below the extraction
threshold. A shared crate is deferred to whichever story first gives
`status-bar` (Story 2.5) the same need — a genuine third consumer, at which
point extracting is the right call rather than a premature one. Flagged
explicitly as a known, accepted duplication (not an oversight) with a
concrete trigger condition for when to revisit it, per doc comments in both
new modules.

**Hotkey choice (gap #4) — `Mod4+A`.** No PRD/ADR/architecture document
names an exact key for opening the tag-manager picker; `EXPERIENCE.md`
only says "assign-hotkey." `wm/src/main.rs`'s existing keybinds already
establish a single-letter-mnemonic convention under `Mod4` (`T`=TagCreate,
`N`=FocusNext, `Q`=Close, `SPACE`=SpawnFoot, `TAB`=TagCycle, `ESC`=Exit) —
`A` for "Assign" (matching `EXPERIENCE.md`'s own naming, "assign-hotgey" /
"assign mode") is the natural next letter in that scheme and is unused.
Same class of implementation-decision gap-fill as Story 2.1's four
resolved gaps — no new ADR, per that story's own precedent (`adrs.md`
governs none of this).

**Fuzzel cancel/exit-code semantics (gap #5) — defensive by construction,
not by verified fact.** The man page documents fuzzel's default *quit*
keybindings (Control+g, Control+c, Escape) but this research pass did not
turn up an explicit statement of dmenu mode's exact exit code or stdout
content on cancel (and it cannot be verified live in this sandbox — Task
1.1). Rather than guess a specific exit code and risk silently mis-handling
real cancel behavior, `checklist::parse_fuzzel_output` treats **any**
non-success exit status as `Cancelled` regardless of stdout content, and
**any** success-exit stdout that doesn't parse cleanly as an in-range `u8`
(empty, garbage, or out-of-range) as `Cancelled` too — the function is
correct for cancel-via-nonzero-exit, cancel-via-empty-output, and any
other failure shape without needing to know which one real fuzzel actually
produces. This is the concrete reason Task 4's test suite exercises both
"nonzero exit with parseable-looking stdout" and "zero exit with
unparseable stdout" as separate, independent cases (4.1's
`parse_fuzzel_output_returns_cancelled_for_nonzero_exit` and
`..._for_unparseable_stdout`) rather than assuming they're mutually
exclusive.

**Scope boundary.** This story does not touch `wm/src/ipc/protocol.rs` or
`dispatch.rs` — Story 2.1 already implemented `get-state` and `toggle-tag`
completely; this story is `tag-picker`-side client code plus one WM-side
keybind-to-process-spawn wire-up, mirroring exactly how `Action::SpawnFoot`
already spawns `foot`. Tag *creation* (the free-text row, Story 2.3) and
tag *switching* (the second hotkey reusing this same picker component,
Story 2.4) are explicitly out of scope — `tag-picker` as built here only
ever sends `get-state` and `toggle-tag`, never `create-tag`/`switch-tag`.

No new ADR. ADR-004's open item is resolved by this story's spike finding,
recorded here rather than by editing `adrs.md` directly — consistent with
how Story 2.1 handled its own four gap-resolutions without opening a new
ADR (implementation-decision gap-filling within an existing story's scope,
not a decision among architecturally significant alternatives).

Build/test/lint only ever run **inside the devcontainer via devpod**
(`devpod ssh buoy-wm -- cargo ...`), falling back to direct `podman exec`
against the running `bold_vaughan` container if `devpod ssh`'s transport is
still unreliable (it was, again, during this story's own research pass) —
same constraint as every prior story.

## Test plan
This story's tests split into five tiers:

1. **Spike** (Task 1) — no automated tests; the deliverable is a documented
   finding (Technical notes' "Spike finding") sourced from `fuzzel(1)`'s
   current, authoritative man page and its issue tracker, gating the design
   of every later task.
2. **Workspace conversion** (Task 2) — no logic of its own; verified by
   `cargo build --workspace`/`cargo test --workspace` from the new root
   manifest reproducing the pre-existing 143 `wm` tests, unchanged, plus
   the new `tag-picker` placeholder building cleanly (0 tests, exit 0,
   same precedent as `tinyrwm`'s own empty suite).
3. **`tag-picker` pure logic** (Tasks 3-5) — Rust unit tests in each new
   module's own `#[cfg(test)] mod tests`, run via `cargo test --workspace`,
   no sockets, no `fuzzel` process:
   - `tag-picker::wire` (Task 3, 8 tests): both request variants serialize
     to the exact wire JSON; the `State`/`Ok`/`Error` response variants
     this client models parse correctly, including a case proving
     unmodeled fields (`app_id`, `outputs`) are silently ignored, not
     rejected; malformed-input rejection (non-JSON, invalid UTF-8, unknown
     `type`, missing `type`).
   - `tag-picker::checklist` (Task 4, 17 tests): checklist construction
     (checked/unchecked marking, order preservation, empty-tags case);
     `fuzzel` stdin rendering (exact checked/unchecked line format,
     multi-row join, empty-input case); `fuzzel` output parsing (success
     path, cancel-via-nonzero-exit, cancel-via-empty-stdout,
     cancel-via-unparseable-stdout, cancel-via-out-of-range-id, and the
     newline-trimming case) — the direct proof this story's "any
     non-success or unparseable output means cancelled" defensive design
     (gap #5) actually behaves that way, not just that the code compiles;
     local membership toggling (add-when-absent, remove-when-present,
     pairwise idempotence); the no-focused-window guard (both branches).
   - `tag-picker::socket_path` (Task 5, 4 tests): identical coverage and
     expected values to `wm/src/ipc/server.rs`'s existing
     `resolve_socket_path` tests, proving both processes resolve to the
     same socket path.
4. **`tag-picker` main.rs orchestration and the WM-side keybind wiring**
   (Tasks 6-7) — **not** covered by automated tests, same boundary class as
   every prior story's process-spawn/Wayland-glue tasks (real
   `std::process::Command` spawns, a real live socket connection, real
   `wayland_client`/`river-window-management-v1` `Dispatch` types — none
   exercisable end-to-end in this sandbox): every *decision* these code
   paths reach is already unit-tested in Tasks 3-5 and (for `Action::
   OpenTagPicker`'s pattern) in Story 2.1's own already-reviewed
   `Action::SpawnFoot` precedent. Verified via `cargo build --workspace`/
   `cargo clippy --workspace --all-targets -- -D warnings` succeeding, the
   full pre-existing 143-test `wm` suite staying green with zero
   regressions, and structural code review against Tasks 6.1-6.2's and
   7.1-7.3's explicit descriptions above.
5. **Tooling gate** (Task 8) — `cargo fmt --all -- --check`, `cargo clippy
   --workspace --all-targets -- -D warnings`, and `cargo test --workspace`
   (143 pre-existing `wm` tests + ~29 new `tag-picker` tests across Tasks
   3-5, all green), all in-container; `pre-commit run --all-files` against
   the updated, workspace-wide hook definitions.

**Remaining, explicitly-flagged coverage gap — larger than any prior
story's:** the actual `fuzzel` process — the literal subject of this
story's spike — is not exercised anywhere in this sandbox, live or
otherwise; neither the host nor the devcontainer has the `fuzzel` binary
installed, and neither has a Wayland session for it to render into even if
it did. Every *decision* around that invocation (what to render, how to
parse its output, how to detect cancel) has real unit-test coverage (tier
3, ~17 tests in `checklist.rs` alone), but the real `Command::new("fuzzel")`
call's actual stdin/stdout framing and actual Escape/cancel behavior remain
verified only against the man page (Task 1), not against a running
`fuzzel`. This is a materially larger gap than Story 2.1's own Wayland-glue
note (which still had full real-socket integration-test coverage
underneath it) and should be closed with a live smoke test — `river`
running, a real focused window, `Mod4+A`, a real `fuzzel` on screen —
before Story 2.3/2.4 build further interaction patterns on top of this
one's assumptions.

## FR coverage
FR6, FR7, NFR1

## Dev Agent Record

### Debug Log

- Confirmed `fuzzel` is installed on neither the host nor the `bold_vaughan`
  devcontainer (`which fuzzel` empty in both), and neither has a live
  Wayland session — Task 1's spike is documentation-based only, no live
  interactive test was possible.
- `devpod ssh buoy-wm` was unreliable at the start of this session
  (`Error tunneling to container: wait: remote command exited without exit
  status or exit signal`), consistent with Story 2.1's own note. All
  build/test/lint/pre-commit commands ran via `podman exec -u vscode -w
  /workspaces/buoy-wm bold_vaughan <cmd>` against the already-running
  container instead.
- Baseline `wm` suite (before this story's changes) was 144 tests, not the
  story's estimated 143 — pre-existing, unrelated to this story's changes;
  noted rather than silently reconciled.
- Task 4's `toggle_local_membership_is_idempotent_pairwise` test initially
  toggled an id that was already present and not the vec's last element
  (`[0,1,2]`, toggle `1` twice); `toggle_local_membership`'s
  remove-if-present/append-if-absent semantics don't restore original
  *position* on a round trip through an already-present id (remove leaves
  `[0,2]`, re-add appends to `[0,2,1]`, not back to `[0,1,2]`). Fixed the
  test to toggle an *absent* id instead (`[0,2]`, toggle `1` twice), which
  does round-trip exactly — the loop's real usage always re-derives display
  order fresh from the registry each `fuzzel` reopen, so this vec's
  internal order is never itself observed.
- `cargo fmt --all -- --check` initially failed on one line in
  `checklist.rs`'s `build_checklist_entries_preserves_registry_order` test
  (a manual line wrap `rustfmt` didn't agree with); `cargo fmt --all`
  applied the fix, `--check` then passed clean.

### Completion Notes

- Task 1 (spike): confirmed and recorded, not re-derived — the story's own
  drafting already resolved this via `fuzzel(1)`'s current man page
  (v1.14.1) and the upstream issue tracker. No `--multi`/checkbox-toggle
  flag exists in real fuzzel; the toggle-and-reopen loop (single-select
  `--dmenu` + `--with-nth`/`--accept-nth`/`--nth-delimiter`, reopened after
  each toggle) is the only interaction fuzzel's real flag surface supports.
- Task 2: converted `wm/Cargo.toml`'s single-package layout into a
  two-member Cargo workspace. New root `/Cargo.toml` (`[workspace]
  resolver = "2" members = ["wm", "tag-picker"]`), new `tag-picker/`
  package with only `serde`/`serde_json` as dependencies (no
  `wayland-client`/`wayland-backend`/`bitflags`). Regenerated a unified
  root `Cargo.lock`; removed the now-superseded `wm/Cargo.lock`. Added a
  root `.gitignore` (`/target`). Updated `.github/workflows/ci.yml` and
  `.pre-commit-config.yaml` to build/test/lint `--workspace` from the repo
  root instead of `--manifest-path wm/Cargo.toml`. Verified `wm/src/**` is
  byte-for-byte unchanged by this conversion (`git diff --stat -- wm/src`
  empty).
- Tasks 3-5: `tag-picker/src/wire.rs` (9 tests), `checklist.rs` (17
  tests), `socket_path.rs` (4 tests) — all pure decision logic, all
  RED-confirmed (compile failures against not-yet-defined types) before
  GREEN implementation. `wire.rs` is a deliberately narrower mirror of
  `wm`'s protocol (only `GetState`/`ToggleTag` requests, only
  `State`/`Ok`/`Error` responses — no `app_id`/`outputs` fields, no
  `CreateTag`/`SwitchTag`/`TagCreated`). `socket_path.rs` is a byte-for-byte
  copy of `wm/src/ipc/server.rs`'s `resolve_socket_path`/
  `default_socket_path`, required so both processes resolve to the same
  socket. Both duplications are deliberate and documented (Technical notes
  gap #3, three-strike DRY rule not yet met).
- Task 6: `tag-picker/src/main.rs`'s toggle-and-reopen loop — connects,
  sends `get-state`, exits 0 with no `fuzzel` invocation if no view is
  focused, otherwise loops: render checklist → spawn `fuzzel --dmenu
  --with-nth=1 --accept-nth=2 --nth-delimiter=<tab>` → parse its
  exit/stdout → on a valid selection, send `toggle-tag` on the same
  connection and reopen; on cancel (non-zero exit or unparseable stdout),
  break and exit 0; on a `wm`-side error response, print to stderr and
  break. Pure I/O/process-spawn glue, no RED/GREEN per the story's own
  carve-out — every decision it calls into is already unit-tested in
  Tasks 3-4.
- Task 7: added `Action::OpenTagPicker` to `wm/src/main.rs`'s existing
  `Action` enum, bound to `Mod4+A` (keysym `0x61`) in `init_new_seats`
  alongside the existing single-letter-mnemonic bindings, and added the
  `do_action` arm — byte-for-byte the same shape as the existing
  `Action::SpawnFoot` arm, spawning `tag-picker` instead of `foot`,
  returning `None` (never switches an output's active tag).
- Task 8: full workspace verification, all in-container via `podman exec`
  (`devpod ssh` unreliable, per the Debug Log): `cargo fmt --all --
  --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (144 `wm` + 30 `tag-picker` = 174 tests, all
  green, zero regressions), `pre-commit run --all-files` (both hooks pass
  workspace-wide).

**Explicit coverage-gap statement (Task 8.3):** the real `Command::new
("fuzzel")` invocation — the literal subject of this story's spike — was
**not** exercised live anywhere in this sandbox. Neither the host nor the
`bold_vaughan` devcontainer has the `fuzzel` binary installed, and neither
has a live Wayland compositor session for it to render into even if it
did. Every *decision* the code makes around that invocation (what to
render, how to parse `fuzzel`'s output, how to detect cancel) has full
unit-test coverage in `checklist.rs` (17 tests); the real subprocess's
stdin/stdout framing and its real Escape/cancel exit behavior remain
verified only against the man page (Task 1), never against a running
`fuzzel`. Likewise, `Action::OpenTagPicker`'s WM-side keybind wiring
(Task 7) was verified only by build/clippy/structural review against the
`Action::SpawnFoot` precedent — no live `river` session, real focused
window, or real `Mod4+A` press was available to smoke-test end-to-end.
This is the single largest residual risk carried into Stories 2.3/2.4,
which build further interaction patterns on top of this one's assumptions,
exactly as the story's own Task 8.3 anticipated.

### File List

- `Cargo.toml` (new) — workspace root manifest
- `Cargo.lock` (new) — unified workspace lockfile
- `.gitignore` (new) — root-level, `/target`
- `wm/Cargo.lock` (deleted) — superseded by the root lockfile
- `wm/src/main.rs` (modified) — `Action::OpenTagPicker`, `Mod4+A` keybind,
  `do_action` arm (Task 7)
- `tag-picker/Cargo.toml` (new)
- `tag-picker/src/main.rs` (new) — toggle-and-reopen loop orchestration
- `tag-picker/src/wire.rs` (new) — request/response wire mirror + tests
- `tag-picker/src/checklist.rs` (new) — pure decision logic + tests
- `tag-picker/src/socket_path.rs` (new) — socket path resolution + tests
- `.github/workflows/ci.yml` (modified) — `--workspace` build/test
- `.pre-commit-config.yaml` (modified) — `--workspace` fmt/clippy hooks,
  widened `files:` globs

### Code Review Follow-up

Addressed, in the code review's priority order:

- **Finding #1 (required — `OpenTagPicker` spawned `tag-picker` by bare
  name, relying on `$PATH`).** Nothing in this repo installs the built
  `tag-picker` binary onto `PATH`, so the bare-name spawn would ENOENT
  silently in a real session and `Mod4+A` would do nothing. Fixed by
  resolving the sibling binary's path relative to the WM's own running
  executable: new pure function `tag_picker_path(wm_exe: &Path) -> PathBuf`
  (`wm/src/main.rs`) joins `wm_exe`'s parent directory with `"tag-picker"`
  (falling back to the bare name if `wm_exe` unexpectedly has no parent —
  e.g. `/`). The `Action::OpenTagPicker` arm now calls
  `std::env::current_exe()`, logs and skips spawning (no panic, NFR2) if
  that fails, and otherwise spawns `tag_picker_path(&wm_exe)` with the same
  `Ok`/`Err`-logging shape as `Action::SpawnFoot`. TDD: RED confirmed
  first — a stub `tag_picker_path` returning the bare name failed both new
  sibling-path assertions with the expected/actual mismatch shown, then
  reverted to the real join-based implementation for GREEN. New tests (all
  in `wm/src/main.rs`'s new `#[cfg(test)] mod tests`, since this file had
  none before): `tag_picker_path_resolves_to_sibling_of_debug_wm_exe`,
  `tag_picker_path_resolves_to_sibling_of_release_wm_exe`,
  `tag_picker_path_falls_back_to_bare_name_when_wm_exe_has_no_parent`. The
  `current_exe()`/`Command::spawn()` call site itself stays untested I/O
  glue, same carve-out as the rest of this file.
- **Finding #2 (recommended — `run_fuzzel`'s pipe-deadlock shape).**
  `tag-picker/src/main.rs`'s `run_fuzzel` wrote all of stdin before calling
  `wait_with_output()`, which could deadlock if input ever exceeded the OS
  pipe buffer (~64KB) while `fuzzel` was blocked on a full stdout buffer of
  its own — not reachable today at the 64-tag cap with realistic names, but
  unbounded/undocumented. Fixed by writing stdin on a dedicated thread
  (spawned after `child.stdin.take()`, dropped to close the pipe once the
  write completes) while the main thread calls `wait_with_output()`
  concurrently, then joining the writer thread before returning — the
  standard `std::process::Command` pattern for this class of deadlock. Pure
  process-spawn I/O glue; no new unit test required for the threading
  mechanics, and the existing `checklist.rs` input-construction/
  output-parsing test coverage is untouched.
- **Finding #3 (recommended — `render_fuzzel_input` didn't escape tab/
  newline in tag names).** `wm_core::create_tag` accepts any string with no
  charset validation (ADR-006/YAGNI), so a tag name with an embedded tab or
  newline (creatable today via a raw `create-tag` IPC call, no UI does this
  yet) would corrupt the tab-delimited row format `fuzzel --dmenu` parses.
  Fixed by replacing embedded `\t`/`\n` with a plain space before rendering
  (chosen over rejecting the tag outright — rendering is display-only, no
  other consumer of the name is affected). TDD: RED confirmed first —
  temporarily removed the `.replace(['\t', '\n'], " ")` call and reran the
  new test, which failed showing the exact corrupted row
  (`"[ ] we\tb\nsite\t2\n"` vs. the expected sanitized
  `"[ ] we b site\t2\n"`) — then restored the fix for GREEN. New test:
  `render_fuzzel_input_replaces_embedded_tab_and_newline_in_tag_name` in
  `tag-picker/src/checklist.rs`'s existing test module.

Explicitly deferred, per this pass's scope (not fixed, left as-is):

- No guard against concurrent `tag-picker` instances — a narrow UX race,
  not a correctness/security issue.
- The DRY finding that `OpenTagPicker`'s spawn block is a third copy of the
  spawn-and-log-error pattern alongside `SpawnFoot`/pinned-terminal spawn —
  no shared helper extracted in this pass (three-strike DRY rule not yet
  met by this specific pattern's shape either).
- Minor style nits (docstrings on private functions, `main()`'s length, the
  `should_open_picker`/`.expect()` shape).

Full re-verification, all in-container via `podman exec -u vscode -w
/workspaces/buoy-wm bold_vaughan <cmd>` (`devpod ssh buoy-wm` was again
unreliable this session — `Error tunneling to container: wait: remote
command exited without exit status or exit signal`): `cargo test
--workspace` (147 `wm` tests + 31 `tag-picker` tests, all green — 3 new
`tag_picker_path` tests plus the 1 new escape test on top of the prior
144 + 30 baseline), `cargo build --workspace` clean, `cargo fmt --all --
--check` clean (after one `cargo fmt --all` pass reformatted a manual line
wrap in the new `tag_picker_path` test, same class of fixup Story 2.2's own
Debug Log already recorded once), `cargo clippy --workspace --all-targets
-- -D warnings` clean, `pre-commit run --all-files` clean (both hooks
pass).

### Change Log

- 2026-08-07: Implemented Story 2.2 end-to-end (Tasks 1-8) via
  `bmad-dev-story`: fuzzel spike confirmed/recorded, `wm`/`tag-picker`
  Cargo workspace conversion, `tag-picker`'s wire/checklist/socket_path
  modules (30 new unit tests, TDD RED/GREEN), the toggle-and-reopen loop
  in `tag-picker/src/main.rs`, and the `Mod4+A` WM-side keybind wiring.
  Full workspace verification (fmt/clippy/test/pre-commit) green. Status
  moved to `review`.
- 2026-08-08: Code review follow-up — fixed the `$PATH`-dependent
  `tag-picker` spawn (resolve sibling binary path via `current_exe()`,
  finding #1), the `run_fuzzel` stdin-write/wait-with-output deadlock shape
  (finding #2), and unescaped tab/newline in tag names corrupting
  `fuzzel`'s tab-delimited row format (finding #3). Deferred: concurrent-
  instance guard, the `OpenTagPicker` spawn-block DRY nit, minor style
  nits. Full workspace verification (test/build/fmt/clippy/pre-commit)
  green.
