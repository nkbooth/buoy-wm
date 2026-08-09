---
baseline_commit: 6e32d5b
---

# Story 3.5: Configurable program defaults and startup config loading

Epic: 3 | Priority: H | Status: done

**Depends on Story 3.1** (`wm/src/config/mod.rs` — the `Config`/`Defaults`
types, the TOML grammar, `Config::parse`, `Config::load`, and
`config_path()`). Story 3.1 built the parser and proved it correct in
isolation; nothing in the running WM read it. This story is where that
config is actually loaded at startup, handed to the live `WindowManager`,
and consumed by the last remaining hardcoded program names.

## Description
Story 3.1 introduced `[defaults]` with three keys — `terminal`, `launcher`,
`default_tag` — and a `Config::default()` that reproduces the previously
hardcoded values exactly. But at the end of 3.1 the WM still never called
`Config::load()`: `main()` built an `AppData::default()` and went straight
to the first roundtrip, and every spawn site still named its program with a
string literal. This story closes that loop. It is deliberately the *last*
of the Epic 3 config stories to touch `main.rs`'s runtime behavior, because
it is the one that makes a malformed config file observable to a user
mid-login, which is what forces the error-posture decision below.

**The literals this story replaces:**

- `"foot"` — appeared in **three** places: `Seat::do_action`'s spawn arm
  (then `Action::SpawnFoot`, now `Action::Terminal`), the same arm's error
  message (`"Failed to spawn foot: {e}"`), and `spawn_pinned_terminal`'s
  own `Command::new("foot")`. All three now read
  `config.defaults.terminal`, and both error messages now name the program
  that actually failed (`"Failed to spawn terminal `{}`: {e}"` /
  ``"Failed to spawn pinned terminal `{terminal}`: {e}"``) — with a
  configurable program name, "failed to spawn foot" would be a lie on any
  machine that had rebound it.
- `"fuzzel"` — `Seat::do_action`'s `Action::SpawnLauncher` arm (now
  `Action::Launcher`), the bare-`fuzzel` desktop-entry launcher spawn.
  Now `config.defaults.launcher`. Note the `Action::Hotkeys` arm still
  spawns `fuzzel` explicitly and is **not** covered by
  `defaults.launcher` — see Technical notes.
- `"default"` — the tag name in the login bootstrap that Story 2.10 added
  to the `Dispatch<RiverWindowManagerV1, ()>` `Event::Output` handler
  (`wm_core_guard.create_tag("default")`). Now
  `state.wm.config.defaults.default_tag.clone()`.

**The startup wiring.** `main()` gained a `Config::load()` block, placed
immediately after `AppData::default()` and *before* `event_queue
.roundtrip(&mut app_data)?`. `WindowManager` gained a `config: Config`
field to hold the result; its `#[derive(Debug, Default)]` still works
untouched because `Config` implements `Default` (Story 3.1), so the
pre-load window — between `AppData::default()` and the assignment — holds
the built-in configuration rather than an invalid or `Option`-wrapped
placeholder, and nothing needs an unwrap.

**The IPC plumbing.** `wm/src/ipc/server.rs` also spawns pinned terminals
(the picker-driven `switch-tag` path, Story 2.4 — `handle_connection_inner`
calls `crate::spawn_pinned_terminal` directly on its own per-connection
thread). That module holds an `Arc<Mutex<WmCore>>` and nothing else; it has
no route to the config. The terminal name is therefore threaded through it
as a plain `String` parameter rather than read there — see Technical notes
for why that direction was chosen.

## Acceptance criteria
**Given** no `~/.config/buoy/config.toml` exists at all
**When** the WM starts and the user spawns a terminal, opens the launcher,
and logs in fresh
**Then** the programs launched are `foot` and `fuzzel` and the bootstrap
tag is named `"default"` — byte-for-byte the pre-Epic-3 behavior, with no
config file required to get it

**Given** a config file declaring `[defaults] terminal = "alacritty"`
**When** the terminal keybind is pressed
**Then** `alacritty` is spawned, not `foot`

**Given** that same config file
**When** a tag is shown for the first time and its pinned terminal is
spawned — via the keybind path (`ensure_pinned_terminal_spawned`) *or* via
the picker-driven IPC path (`handle_connection_inner`'s `pending_spawn`)
**Then** both paths spawn `alacritty -a pinned-term zellij attach --create
<session>`; the IPC path is not left on the old hardcoded program

**Given** a config file declaring `[defaults] launcher = "wofi"`
**When** the launcher keybind is pressed
**Then** `wofi` is spawned with the same `--layer=overlay` /
`--output=<name>` argument wiring the `fuzzel` call already used

**Given** a config file declaring `[defaults] default_tag = "home"`
**When** a completely fresh session registers its first output with an
empty tag registry
**Then** the bootstrap tag created and switched to is named `"home"`, and
its pinned terminal spawns exactly as Story 2.10 specified

**Given** a config file that exists but is malformed (a TOML syntax error,
an unknown key name, or an unreadable file)
**When** the WM starts
**Then** the failure is printed to stderr naming the full path it tried to
load and the specific error, followed by `"Falling back to built-in
defaults."`, and the session **starts normally on `Config::default()`** —
it does not exit, and the user is not locked out

- [x] Tests pass (unit + integration where applicable)
- [ ] Code review: pending (separate review pass)

## Tasks / Subtasks

- [x] **Task 1: `WindowManager` holds the loaded config (AC: all)**
  - [x] 1.1 Add `config: Config` to `struct WindowManager` in
    `wm/src/main.rs`, with a `///` comment stating what it is (the user's
    `~/.config/buoy/config.toml`, or `Config::default`'s built-in
    equivalent when there is no such file) and that it is loaded once at
    startup. Confirm the existing `#[derive(Debug, Default)]` still
    compiles unchanged — it does, because `Config` implements `Default`;
    no `Option` wrapper, no manual `Default` impl, no unwrap at any use
    site.
  - [x] 1.2 No RED/GREEN — a struct field with no behavior of its own.
    Covered by the build gate.

- [x] **Task 2: Load the config in `main()` with a fall-back-not-abort
  posture (AC 1, 6)**
  - [x] 2.1 In `wm/src/main.rs`'s `main()`, immediately after
    `let mut app_data = AppData::default();` and **before**
    `event_queue.roundtrip(&mut app_data)?`, assign
    `app_data.wm.config = match Config::load() { Ok(config) => config,
    Err(e) => { eprintln!(...); Config::default() } }`.
  - [x] 2.2 The error branch must print the path (`config::config_path()
    .display()`) alongside the error, then `"Falling back to built-in
    defaults."` on its own line — a user staring at a session that came up
    with unexpected keybinds needs to know *which file* was rejected and
    why, and stderr is the only channel available at that point in
    startup.
  - [x] 2.3 The ordering is load-order-critical, not cosmetic:
    `init_new_seats` registers every binding the config declares, and it
    runs off seat-registration events delivered by the first roundtrip. A
    config loaded *after* the roundtrip would race the first seat, which
    would come up with an empty or stale binding set. Document this in a
    comment at the call site.
  - [x] 2.4 No RED/GREEN — `Config::load()`'s own success/not-found/error
    branches are Story 3.1's tested territory; what this task adds is
    `main()`-level startup sequencing, which needs a live Wayland
    connection to exercise (`Connection::connect_to_env()` is the first
    line of `main`). Same standing carve-out as every prior `main.rs`
    startup/registration story (1.7, 2.6, 2.7, 2.8, 2.9, 2.10). Verify by
    build, clippy, and structural review that the assignment precedes the
    roundtrip and that the `Err` arm returns a `Config` rather than
    exiting.

- [x] **Task 3: Replace `"foot"` at all three sites (AC 2, 3)**
  - [x] 3.1 `Seat::do_action` gains a `config: &Config` parameter (threaded
    from `manage_seats`, which already holds `&self.config` at that point).
    The `Action::Terminal` arm's `Command::new("foot")` becomes
    `Command::new(&config.defaults.terminal)`, and its error message names
    the configured program.
  - [x] 3.2 `spawn_pinned_terminal` gains a `terminal: &str` parameter;
    `Command::new("foot")` becomes `Command::new(terminal)`, error message
    likewise. Its doc comment is updated to
    `<terminal> -a pinned-term zellij attach --create <session_name>` and
    gains an explicit paragraph on the `-a` flag *not* being configurable
    (Task 5).
  - [x] 3.3 `WindowManager::ensure_pinned_terminal_spawned` passes
    `&self.config.defaults.terminal` at its `Ok(Some(session_name))` arm.
  - [x] 3.4 No RED/GREEN — process-spawn glue, standing carve-out (Task
    2.4). Verify by build/clippy plus a grep confirming no `"foot"` string
    literal survives anywhere in `wm/src/` outside `config/mod.rs`'s
    `Defaults::default()` and its tests.

- [x] **Task 4: Replace `"fuzzel"` in the launcher arm and `"default"` in
  the login bootstrap (AC 4, 5)**
  - [x] 4.1 `Action::Launcher`'s `Command::new("fuzzel")` becomes
    `Command::new(&config.defaults.launcher)`. The existing
    `--layer=overlay` and conditional `--output=<name>` argument wiring
    (Stories 2.9 / the fuzzel-behind-fullscreen fix) is unchanged — those
    are passed to whatever launcher is configured, which is a documented
    limitation of the same shape as Task 5's.
  - [x] 4.2 In the `Dispatch<RiverWindowManagerV1, ()>` `Event::Output`
    handler, `wm_core_guard.create_tag("default")` becomes
    `wm_core_guard.create_tag(state.wm.config.defaults.default_tag
    .clone())`. `.clone()` because `create_tag` takes an owned name and the
    config outlives the call; the guard/lock/drop sequencing Story 2.10
    established around this block is otherwise untouched.
  - [x] 4.3 No RED/GREEN — same carve-out.

- [x] **Task 5: Thread the terminal name through the IPC server (AC 3)**
  - [x] 5.1 `ipc::server::spawn` gains a third parameter,
    `terminal: String` (owned — it is moved into the accept loop's thread
    and must outlive `main`'s stack frame). Its doc comment states that the
    value is passed in rather than read here, and why.
  - [x] 5.2 The accept loop clones `terminal` per accepted connection,
    alongside the existing `Arc::clone(&wm_core)`, and passes it to
    `handle_connection`.
  - [x] 5.3 `handle_connection` and `handle_connection_inner` each gain a
    `terminal: &str` parameter; `handle_connection_inner`'s
    `crate::spawn_pinned_terminal(&session_name)` becomes
    `crate::spawn_pinned_terminal(&session_name, terminal)`. The
    `catch_unwind` boundary in `handle_connection` is unchanged — a `&str`
    is `UnwindSafe`-compatible under the existing
    `AssertUnwindSafe` wrapper already in place.
  - [x] 5.4 `main()`'s `ipc::server::spawn` call passes
    `app_data.wm.config.defaults.terminal.clone()`. This is *after* Task
    2's load, so the value is the user's, not the built-in default.
  - [x] 5.5 No RED/GREEN for the threading itself — it is parameter
    plumbing with no branching. Verify by build/clippy and by confirming
    the IPC server's existing test suite still passes with the new
    signature (Task 6).

- [x] **Task 6: Stop the IPC server's own tests from launching a real
  terminal (test hygiene, no AC)**
  - [x] 6.1 `ipc/server.rs`'s test helper `spawn_test_server_with_core`
    passes `"/bin/true".to_string()` as the terminal, and the
    `stale non-socket file` test passes the same. Comment at the call site
    explaining *why*: a request that claims a pinned-terminal spawn reaches
    a genuine `crate::spawn_pinned_terminal` → `Command::spawn` from these
    tests, so before this change they would have tried to launch `foot` on
    whatever machine ran the suite. `/bin/true` is an inert no-op that
    exits 0 immediately, so the code path is still fully exercised and
    nothing appears on screen.
  - [x] 6.2 No new test cases — this changes the fixture, not the
    assertions. The existing tests' behavior is unchanged; what changes is
    what they do to the host running them.

- [x] **Task 7: Full in-container verification gate (AC: all)**
  - [x] 7.1 Inside the devcontainer (`podman exec -u vscode -w
    /workspaces/buoy-wm bold_vaughan <cmd>`): `cargo fmt --all -- --check`,
    `cargo clippy --workspace --all-targets -- -D warnings`,
    `cargo test --workspace`, `pre-commit run --all-files`. Record the
    total in the Dev Agent Record.
  - [ ] 7.2 Manual live-verification note (not automated, not a story
    blocker — same boundary as every prior story's live-compositor gap):
    with a real `[defaults]` block in place, confirm the configured
    terminal is what actually opens for both a new terminal and a pinned
    terminal (including via the picker), the configured launcher opens on
    `Mod4+R`, and a fresh login lands on the configured `default_tag` name.
    Then confirm a deliberately-broken config file still yields a usable
    session with the built-in bindings and a legible stderr message.

## Technical notes

**Fall back, don't abort — the most important decision in this story.**
`Config::load()`'s `Err` is *not* propagated out of `main()`. A window
manager is the only process that can put a screen in front of the user to
fix the typo with; if a stray character in a keybind name aborts startup,
the user is left at a blank TTY or bounced back to a display manager with
no editor, no terminal, and no obvious cause. Refusing to start is the
strictly worse failure mode even though it is the more "correct-looking"
one. So the error is reported loudly (path + error + an explicit
"falling back" line, so it is never a silent degrade) and the session comes
up on `Config::default()` — which is, by Story 3.1's construction, exactly
the pre-Epic-3 built-in behavior. The user gets a working session with
familiar bindings and a message telling them what to fix. Note that this
posture lives *only* here, at the `main()` call site: `Config::load` /
`Config::parse` themselves are strict, and never silently half-apply a bad
file (Story 3.1's own documented stance). Strict parse, lenient startup —
the leniency is a deliberate, single, documented policy decision at one
place, not a property of the parser.

**Why the load happens before the first roundtrip.** `init_new_seats`
registers a binding object with the compositor for every entry in
`config.keybinds` and `config.mousebinds`, and it runs in response to seat
registration — which the first `event_queue.roundtrip` triggers. Loading
after the roundtrip would mean the first seat registers whatever
`Config::default()` happened to contain and then never re-registers, so a
user's rebinds would silently not apply on the primary seat while applying
on any hotplugged one. Placing the load between `AppData::default()` and
the roundtrip removes the race entirely rather than adding re-registration
logic.

**Why the IPC server takes the terminal name as a parameter instead of
reading the config.** `ipc/server.rs` currently depends on the rest of the
WM through exactly one thing: the `Arc<Mutex<WmCore>>` handle. That is a
deliberate, narrow seam — the server thread and the Wayland dispatch thread
share state, and only that state. Giving the server its own `Config` (or a
second `Arc<Config>`, or a `config` field on `WmCore`) would widen that
seam for one string. Passing a `String` in at `spawn` and cloning it per
connection costs one small allocation per accepted connection — on a
human-paced desktop where connections are opened by a user pressing a
picker keybind — and keeps the module's dependency surface exactly where
it was. The parameter is owned (`String`) at `spawn` because it is moved
into the detached accept-loop thread, and borrowed (`&str`) below that
because `handle_connection`/`handle_connection_inner` are synchronous and
never outlive it.

**The `-a <PINNED_TERM_APP_ID>` flag is deliberately NOT configurable — a
documented limitation.** `spawn_pinned_terminal` still passes
`-a pinned-term` unconditionally, regardless of what `defaults.terminal`
is set to. This is not an oversight. That app-id is the *only* way every
other part of the WM recognises a pinned terminal: `init_new_windows`
branches on it to associate the window with its tag and give it
output-sized geometry, `Action::Close` refuses to close a window carrying
it, `lower_view`/`raise_view` use it to pin the window to the bottom of
stacking order, and `recompute_pinned_terminal_*` re-resolves it every
sequence. A terminal emulator that does not accept an `-a`/app-id flag
therefore cannot fill the pinned-terminal role at all — it would map as an
ordinary window and none of that machinery would engage. Stated plainly:
**`defaults.terminal` is only usable with terminals that accept
`-a <app_id>`** (foot, alacritty via `--class`… which is a *different*
flag, so alacritty would need a follow-up story to work as a pinned
terminal). Making the flag itself configurable is a real, separable piece
of work; it is not in this story's scope and no story needs it yet.

The same shape applies, less severely, to `defaults.launcher`: the
launcher spawn still passes `--layer=overlay` and `--output=<name>`, which
are `fuzzel`-specific spellings. And `Action::Hotkeys` still spawns
`fuzzel` by name outright — it uses `fuzzel --dmenu` as a *pager* for the
generated cheat-sheet, which is a distinct role from "the launcher the user
prefers", so pointing it at `defaults.launcher` would break the cheat-sheet
for anyone whose launcher has no dmenu mode. Left hardcoded on purpose.

**Loaded once, no live reload.** The config is read exactly once, at
startup. Re-reading it on change would mean destroying and recreating every
live `river_xkb_binding_v1` and `river_pointer_binding_v1` object on every
seat, plus reconciling `defaults` changes against already-spawned
processes — meaningful work with a real risk of leaving a seat with a
half-applied binding set. No story needs it (YAGNI); restarting the session
applies a config change today. The `config` field's own doc comment records
this so the next reader does not assume reload was forgotten.

**Scope boundary.** This story does not add, remove, or rename any
`[defaults]` key — the three keys and their built-in values are Story 3.1's
work, unchanged here. It does not touch `tag-picker` or `status-bar`, which
have their own program invocations and their own (non-)configuration story.
It does not make the pinned terminal's `zellij attach --create` multiplexer
configurable.

## Test plan
1. **No new unit tests in this story** — see Tasks/Subtasks. Every change
   here is either a process-spawn call site, a `main()` startup sequencing
   step, or parameter plumbing, all of which fall under this project's
   standing build+clippy+manual-review carve-out for `main.rs` glue.
2. **Coverage that already exists**: Story 3.1's `config/mod.rs` tests
   cover the *values* this story consumes —
   `default_config_reproduces_the_previously_hardcoded_programs` asserts
   `terminal == "foot"`, `launcher == "fuzzel"`, `default_tag ==
   "default"` (AC 1 in data form), and
   `defaults_section_overrides_only_the_keys_it_names` asserts a
   partially-specified `[defaults]` block overrides exactly the keys it
   names and leaves the rest at built-in values (ACs 2/4/5 in data form).
   What this story adds is the wiring from those values to
   `Command::new`, which is precisely the untestable-without-a-compositor
   part.
3. **Regression gate**: `ipc/server.rs`'s full existing test suite passes
   unchanged against `spawn`'s new three-argument signature, including the
   tests that drive a `SwitchTag` request all the way to a real
   `Command::spawn` — now against `/bin/true` rather than a real terminal.
4. **Build/lint gate**: `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`,
   `pre-commit run --all-files` clean with every new parameter and the new
   `WindowManager::config` field compiled in.
5. **Live gate (manual, not automated)**: per Task 7.2 — configured
   terminal for both spawn paths including the IPC one, configured
   launcher, configured `default_tag` on a fresh login, and a deliberately
   malformed config still producing a usable session.

## FR coverage
New scope beyond the original PRD. The PRD and Epic 1/2 specified a
window manager with a fixed, opinionated set of programs — FR-level
requirements name behaviors ("spawn a terminal", "the pinned terminal runs
a multiplexer session"), never which binaries implement them, and the
original acceptance criteria were all satisfied by the hardcoded literals.
Epic 3 adds user configurability as a new capability on top; this story
is the part of it that reaches the program-spawn sites and the startup
path. No pre-existing FR changes meaning: with no config file present,
every FR's observable behavior is byte-for-byte what it was before
(AC 1, and Story 3.1's
`default_config_reproduces_the_previously_hardcoded_programs` test is the
standing guard on that).

## Dev Agent Record

**What was done**:

- `wm/src/main.rs` — `struct WindowManager` gained `config: Config`, with a
  doc comment recording that it is the user's `~/.config/buoy/config.toml`
  (or `Config::default`'s built-in equivalent when absent) and that it is
  loaded once at startup, re-reading on change being explicitly out of
  scope. The struct's `#[derive(Debug, Default)]` was left untouched and
  still compiles, since `Config` implements `Default` — no `Option`
  wrapper and no unwrap anywhere.
- `wm/src/main.rs` (`main()`) — added the `Config::load()` block between
  `AppData::default()` and `event_queue.roundtrip(&mut app_data)?`. The
  `Err` arm prints `"Failed to load {}: {e}\nFalling back to built-in
  defaults."` with `config::config_path().display()` and evaluates to
  `Config::default()`; startup continues. A comment at the site records
  both the fall-back-not-abort rationale and the before-the-roundtrip
  ordering requirement (`init_new_seats` registers whatever this
  produces).
- `wm/src/main.rs` (`Seat::do_action`) — gained a `config: &Config`
  parameter, threaded from `manage_seats`'s existing call. `Action::
  Terminal` spawns `&config.defaults.terminal`; `Action::Launcher` spawns
  `&config.defaults.launcher` with its `--layer=overlay` /
  conditional-`--output=` wiring unchanged. Both error messages now
  interpolate the configured program name instead of naming `foot`/
  `fuzzel` literally.
- `wm/src/main.rs` (`spawn_pinned_terminal`) — signature became
  `fn spawn_pinned_terminal(session_name: &str, terminal: &str)`;
  `Command::new(terminal)`. Doc comment updated to the generic
  `<terminal> -a pinned-term zellij attach --create <session_name>` form
  and given a new paragraph stating that the `-a <app_id>` flag is
  deliberately not configurable, because `PINNED_TERM_APP_ID` is how the
  rest of the WM recognises the pinned terminal at all.
  `ensure_pinned_terminal_spawned` passes
  `&self.config.defaults.terminal`.
- `wm/src/main.rs` (`Dispatch<RiverWindowManagerV1, ()>`'s `Event::Output`
  arm) — Story 2.10's bootstrap now calls
  `wm_core_guard.create_tag(state.wm.config.defaults.default_tag.clone())`
  instead of `create_tag("default")`. The surrounding lock/mutate/drop
  sequencing and the `tag_count() == 0` gate are unchanged.
- `wm/src/ipc/server.rs` — `spawn` gained `terminal: String` (owned; moved
  into the detached accept-loop thread), documented at the signature as
  "passed in rather than read here so this module keeps its only
  dependency on the WM being the shared `wm_core` handle". The accept loop
  clones it per connection alongside the existing `Arc::clone(&wm_core)`.
  `handle_connection` and `handle_connection_inner` each gained
  `terminal: &str`, and the latter's `crate::spawn_pinned_terminal(
  &session_name)` became `crate::spawn_pinned_terminal(&session_name,
  terminal)`. `main()`'s `ipc::server::spawn` call passes
  `app_data.wm.config.defaults.terminal.clone()`, after the config load.
- `wm/src/ipc/server.rs` (tests) — `spawn_test_server_with_core` and the
  stale-socket test now pass `"/bin/true".to_string()` as the terminal,
  with a comment explaining that a request claiming a pinned-terminal
  spawn reaches a real `Command::spawn` from these tests, so before this
  they would have tried to launch `foot` on whatever machine ran the
  suite. No assertions changed — this is a fixture fix, not new coverage.

**Testing posture — stated explicitly rather than glossed over**: **no new
unit tests were added by this story.** Every line it touches is one of
three things: a `std::process::Command` spawn call site, `main()`'s
startup sequencing (which begins with `Connection::connect_to_env()` and
cannot run without a live Wayland compositor), or parameter plumbing with
no branching of its own. None of that is reachable from `cargo test` in
this sandbox, and this project has a standing carve-out for exactly that
category — build + clippy + structural review in place of RED/GREEN — used
by every prior `main.rs` story (1.7, 2.6, 2.7, 2.8, 2.9, 2.10). The
*behavior* this story makes configurable is covered on the data side by
Story 3.1's parser tests (see Test plan item 2); what is genuinely
unverified by automation is the wiring between the two, which Task 7.2's
live gate exists to cover. The one test-suite change here is a hygiene fix
(Task 6), not added coverage.

**Verification** (all inside the devcontainer via `podman exec -u vscode -w
/workspaces/buoy-wm bold_vaughan <cmd>`):
- `cargo fmt --all -- --check`: clean, no drift.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace`: `buoy-wm` (wm) 215, `status-bar` 27,
  `tag-picker` 69 = **311 total, 0 failures, 0 regressions**. The
  `buoy-wm` count is up from the 176 baseline entirely on Story 3.1's
  `config::` parser and `config::keysym::` tests; this story contributes
  0 of them, and `ipc::server`'s own tests are unchanged in count (only
  their terminal fixture changed).
- `pre-commit run --all-files`: both configured hooks (`cargo fmt
  --check`, `cargo clippy`) passed.

**Task 7.2 (manual live-verification)**: **deferred**, not performed by
this agent — no `river` session, no live compositor, and no physical output
in this sandbox, the same boundary as every prior story's live gate (most
recently Story 2.10's own Task 6.2). Still needing a human on a real boot:
(a) a configured `terminal` actually opening for both a keybind-spawned
terminal and a pinned terminal, including the picker-driven IPC path;
(b) a configured `launcher` opening on the launcher keybind; (c) a
configured `default_tag` naming the tag a fresh login lands on; and
(d) a deliberately-malformed `config.toml` producing a usable session on
the built-in bindings plus a legible stderr message rather than a failed
login.

**File List**:
- `wm/src/main.rs` — `WindowManager::config` field; `Config::load()` block
  in `main()`; `Seat::do_action`'s `config: &Config` parameter and the
  `Action::Terminal`/`Action::Launcher` spawn sites;
  `spawn_pinned_terminal`'s `terminal` parameter and updated doc comment;
  `ensure_pinned_terminal_spawned`'s call site; the `Event::Output`
  bootstrap's `default_tag`; `ipc::server::spawn`'s new argument.
- `wm/src/ipc/server.rs` — `spawn`/`handle_connection`/
  `handle_connection_inner` terminal parameter and its doc comment; the
  `spawn_pinned_terminal` call site; `/bin/true` in the two test call
  sites.
- `docs/planning/epics/story-3-5.md` — this file.

## Code Review

HIGH-effort workflow-backed review (37 agents: per-angle finders, then an
independent verifier per finding location). One finding against this
story, fixed — and it overturns a limitation this story had documented as
acceptable.

1. **HIGH — making `terminal` configurable while leaving its argv
   hardcoded broke every pinned terminal, silently and permanently.** This
   story documented the fixed `-a <PINNED_TERM_APP_ID>` flag as a
   deliberate limitation on the grounds that the app-id is how the WM
   recognises the pinned terminal. That reasoning is sound for the *app-id*
   but was wrongly extended to the *flag*: `-a` is foot's spelling, and
   most other terminals use `--class`. The failure mode is what makes this
   more than a documentation matter. `claim_pinned_terminal_spawn` marks
   the tag `terminal_spawned = true` and queues it *before* the spawn;
   `Command::spawn` then succeeds, because the binary exists, so nothing is
   logged; the terminal exits on the unrecognised flag or maps a window
   whose app-id is not `PINNED_TERM_APP_ID`; the queued tag is never
   popped; and because the claim is idempotent it is never retried. Every
   tag permanently loses its FR4 backdrop, and Story 2.12's last-resort
   focus fallback then has nothing to fall back to, so switching onto an
   empty tag clears focus entirely. **Fixed**: `[defaults]
   pinned_terminal_args` makes the invocation configurable, defaulting to
   foot's exact existing form, with `{app_id}` and `{session}` substituted
   per-argument (whole-token, never through a shell, so a tag name can
   contain anything). `Config::parse` rejects an argv missing `{app_id}`
   (`ConfigError::MissingAppIdPlaceholder`), which converts the
   silent-and-permanent runtime failure into a named error at load —
   preserving the invariant the original limitation was protecting while
   dropping the coupling to one terminal. `Defaults` gained
   `pinned_terminal_argv`, and the IPC server is now handed the whole
   `Defaults` rather than a bare terminal `String`, since it needs both
   halves of the recipe for the picker-driven spawn path.

The startup posture (report, then fall back to defaults rather than
aborting) and the `/bin/true` test-fixture change both verified as
described and raised no findings.

**Re-verification after fixes**: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `pre-commit run
--all-files` all clean; `cargo test --workspace` — 322 total (`buoy-wm`
226, `status-bar` 27, `tag-picker` 69), 0 failures.

**Code review: PASS**.
