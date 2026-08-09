---
baseline_commit: 6e32d5b
---

# Story 3.4: Named-tag and exec keybind actions

Epic: 3 | Priority: H | Status: done

**Depends on Story 3.1** (the config file, its schema, and the `Action`
enum that these two variants are added to) **and Story 3.3** (registering
config-declared bindings on each seat, including the change of
`Seat::pending_action` from a `Copy` enum to `Option<Action>` so a
parameterized action can be taken by value). Neither of this story's
actions is reachable without both: 3.1 is what lets a user *write*
`action = { switch_tag = "email" }` at all, and 3.3 is what turns that
written binding into a live `river_xkb_binding_v1` whose `Pressed` event
lands in `do_action`.

## Description
These two actions are the reason the keymap was made configurable in the
first place. The user's original ask was not "let me rebind `Mod4+Q`" —
it was **"hotkeys to create pre-named tags, and launching applications /
firing scripts."** Everything else in Epic 3 (the TOML file, the keysym
table, the replace-don't-merge rule, the generated cheat-sheet) is
infrastructure that exists so these two spellings can be written:

```toml
[[keybind]]
mod = ["Super"]
key = "1"
action = { switch_tag = "email" }

[[keybind]]
mod = ["Super"]
key = "P"
action = { exec = "grim -g \"$(slurp)\" ~/screenshot.png" }
```

Both are parameterized variants — `Action::SwitchTag(String)` and
`Action::Exec(String)` — carried as single-key TOML tables, in contrast
to every pre-existing action, which is a bare string (`action = "close"`).
Serde's default externally-tagged representation gives both spellings in
the same field for free (Story 3.1's design); this story is about what
happens in `Seat::do_action` when one of them actually fires.

**`switch_tag` is create-on-demand, deliberately.** A named-tag bind is
meant to be pressed *before* the tag exists. `Super+1` = "email" has to
work on a fresh session, on the very first press, with an empty-but-for-
`default` registry — that is the entire point of "pre-named tags." So a
missing tag is **created**, not treated as an error. The alternative the
user was offered and explicitly rejected was pre-declaring the tag set in
config (a `[tags]` table the WM would materialize at startup): it splits
one concept across two config sections, makes a typo in a keybind's tag
name a silent no-op instead of a self-healing create, and adds a second
place tags can come from for no gain.

**An existing name is looked up FIRST.** `wm_core.tag_id_by_name(&name)`
runs before any create attempt, so pressing `Super+1` a second, tenth and
hundredth time reuses the same `TagId` rather than piling up duplicate
"email" tags. This matters more here than it would in most systems:
**ADR-006 provides no tag-deletion operation** — the registry only ever
grows, ids are never reused, and there is no `delete_tag` anywhere in
`wm_core`. A duplicate tag created by a repeated keypress would therefore
be *permanent*, occupying one of the 64 available ids for the life of the
session with no way for the user to get rid of it. Lookup-before-create
is what keeps a keybind that is, by design, pressed constantly from
exhausting the registry.

**The arm returns `Some(tag_id)` on success, mirroring
`Action::CycleTag`.** `do_action`'s `Option<TagId>` return value is not
decoration — it is the signal `manage_seats` uses to decide whether to
push onto `pending_terminal_spawns` and, after dropping the `wm_core`
lock, call `ensure_pinned_terminal_spawned(tag_id)`. A named-tag switch
that returned `None` would land the user on a brand-new, completely empty
tag with no terminal backdrop; returning the id is what makes
`Super+1`-on-a-fresh-session produce a usable screen instead of a black
one. This is the same contract `Action::CycleTag` has had since Story
1.7, reused rather than reinvented.

**The no-output case logs and returns rather than fabricating an output
id.** If `active_output_id` is `None` (nothing registered yet — a startup
race Story 2.10's default-tag bootstrap makes rare but does not
eliminate), the arm prints `"Tag keybind for `<name>` pressed but no
output is registered yet"` and returns `None`, without touching the
registry at all. Message shape and placement deliberately match
`Action::CycleTag`'s existing `"Tag-cycle keybind pressed but no output
is registered yet"`. Critically, the guard runs *before* the
lookup-or-create, so a keypress in that window cannot leave a
permanently-undeletable orphan tag behind (the same ordering hazard
Story 2.10's own code review flagged in `tag-picker`, applied
pre-emptively here).

**`exec` runs through `sh -c`.** `Action::Exec(command_line)` spawns
`sh -c <command_line>`, not the string as a bare program name. That is
what lets a single bind carry a whole command line — arguments, quoting,
pipes, `$(...)`, `~` expansion — which is what "firing scripts" actually
requires in practice (`grim -g "$(slurp)" ~/screenshot.png` is useless
without it). See Technical notes for why this is not the injection
vector it superficially resembles.

Both arms use the existing `env_remove("WAYLAND_DEBUG")` fire-and-forget
spawn shape shared by every other process spawn in `main.rs` — spawn,
ignore the child, log on `Err`, never wait and never panic.

## Acceptance criteria
**Given** a keybind declared as `action = { switch_tag = "email" }`, an
output registered, and no tag named `"email"` in the registry
**When** the bind is pressed
**Then** a tag named `"email"` is created, the active output is switched
to it, and `do_action` returns `Some(tag_id)` so `manage_seats` spawns
that tag's pinned terminal on this first use

**Given** the same bind and a registry that already contains a tag named
`"email"`
**When** the bind is pressed again (second press, or hundredth)
**Then** the existing `TagId` is reused — the active output switches to
that same tag and **no** duplicate tag is created, the registry's tag
count is unchanged

**Given** the same bind and no output registered yet
**When** the bind is pressed
**Then** a message naming the tag is logged to stderr, `None` is
returned, and the tag registry is **not** mutated — no tag is created
that could never be switched to or deleted

**Given** an existing tag name and a `switch_tag` bind whose name differs
only in case or by surrounding whitespace (`"Email"`, `"email "`)
**When** the bind is pressed
**Then** it is treated as a distinct name — matching is exact, so the
near-miss tag is created rather than silently switching to the
similarly-named existing one

**Given** a keybind declared as `action = { exec = "<command line>" }`
**When** the bind is pressed
**Then** the command line is executed with full shell semantics
(arguments, quoting, pipes, command substitution, `~` expansion), with
`WAYLAND_DEBUG` removed from the child's environment, and `do_action`
returns `None` (no tag involved, nothing for `manage_seats` to spawn)

**Given** either action's spawn or `wm_core` call fails (`sh` missing,
`create_tag` hitting ADR-006's 64-tag ceiling, `switch_tag` rejecting an
unknown output)
**When** the failure occurs
**Then** it is reported on stderr and the session continues (NFR2) — no
panic, no `unwrap`, no half-applied state

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: `WmCore::tag_id_by_name` — a new pure query (AC 1, 2, 4) — TDD, genuine RED before GREEN**
  - [x] 1.1 RED — Add three tests to `wm/src/wm_core/state.rs`'s test module: `tag_id_by_name_finds_an_existing_tag` (create `"web"` and `"term"`, assert the query returns `"web"`'s id — proving it finds the *right* one, not just the first), `tag_id_by_name_returns_none_for_an_unknown_name` (asserts `None` for both `"nope"` and `""` — the empty-string case matters because a config could legally declare `switch_tag = ""`), and `tag_id_by_name_is_case_sensitive` (create `"web"`, assert `tag_id_by_name("Web")` is `None`). Confirm all three fail to compile in-container — `E0599: no method named tag_id_by_name`.
  - [x] 1.2 GREEN — Implement `pub fn tag_id_by_name(&self, name: &str) -> Option<TagId>` on `WmCore`, near `cycle_tag`/`topmost_visible_view`: `self.tags.ids().into_iter().find(|id| self.tags.get(*id).is_some_and(|tag| tag.name == name))`. Pure, never mutates `self`, no I/O. `///` doc comment stating the exact-match rule *and its reason* (a loosely-matching named-tag keybind could switch to a near-miss tag and then never be able to create the one actually asked for), not just the behavior. Confirm all three tests pass.
  - [x] 1.3 Replace the earlier in-development implementation that reached through `WmCore::snapshot()` to find the tag by name. `snapshot()` clones every tag name, every view's `app_id` and expanded tag list, and every output, then sorts two vectors — an entire copy of the WM's state allocated to answer one string lookup, on a keybind that is by design pressed constantly. The dedicated query is both cheaper (no allocation beyond `ids()`'s `Vec`) and directly unit-testable, whereas the `snapshot()` version could only be exercised through the snapshot type's own shape.

- [x] **Task 2: `Action::SwitchTag(name)` arm in `Seat::do_action` (AC 1, 2, 3, 4, 6)**
  - [x] 2.1 Add the arm to `do_action`'s `match pending_action`, taking `name: String` by value (possible only because Story 3.3 changed `pending_action` to `Option<Action>` and `do_action` opens with `self.pending_action.take()?`). Order of operations, which is load-bearing: (a) `let Some(output_id) = active_output_id else { eprintln!(...); return None; }` — the guard comes **first**, before any registry access, so the no-output case cannot create an undeletable orphan tag; (b) `wm_core.tag_id_by_name(&name)`, falling through to `wm_core.create_tag(name.clone())` only on `None`, log-and-`return None` on that call's `Err`; (c) `wm_core.switch_tag(output_id, tag_id)`, returning `Some(tag_id)` on `Ok(())` and log-and-`None` on `Err`.
  - [x] 2.2 Comment the arm with *why*, per project code style: the create-on-demand rationale (bind is meant to be pressed before the tag exists), the lookup-first rationale tied explicitly to ADR-006's missing delete, and — on the `Ok(())` arm specifically — that `Some(tag_id)` mirrors `Action::CycleTag` because that id is `manage_seats`' pinned-terminal-spawn signal. A reader who does not already know `do_action`'s return contract cannot otherwise tell why a "switch" returns anything at all.
  - [x] 2.3 No RED/GREEN. `do_action` takes `&mut VecDeque<Window>`, a `&RiverWindowManagerV1` proxy and a live `&mut WmCore`, and its observable effect here is a process spawn plus two `wm_core` calls whose own logic is already unit-tested (`create_tag`, `switch_tag`, and now `tag_id_by_name`). This is the project's standing build+clippy+manual-review carve-out for Wayland dispatch glue, same as every prior `main.rs` action-arm story (1.4, 1.7, 2.2, 2.4, 2.10). Verify by structural review that the no-output guard precedes the create, that `name` is cloned exactly once (for `create_tag`, which takes `impl Into<String>`) and remains available for both error messages afterward, and that no path can return `Some` without `switch_tag` having succeeded.

- [x] **Task 3: `Action::Exec(command_line)` arm in `Seat::do_action` (AC 5, 6)**
  - [x] 3.1 Add the arm: `std::process::Command::new("sh").arg("-c").arg(&command_line).env_remove("WAYLAND_DEBUG").spawn()`, `Ok(_) => {}`, `Err(e) => eprintln!("Failed to exec `{command_line}`: {e}")`, returns `None`. Identical fire-and-forget shape to `Action::Terminal`'s arm directly above it — same `env_remove`, same non-waiting spawn, same log-on-`Err` — so the two read as one pattern rather than two conventions.
  - [x] 3.2 Comment the `sh -c` choice and address its security question in the code itself, not only in this document: the string comes from the user's own config file, so shell interpretation is the *intent*, not an injection vector. Left uncommented, the next reader's correct instinct is that this is a bug.
  - [x] 3.3 No RED/GREEN, same carve-out and same verification method as 2.3.

- [x] **Task 4: Full in-container verification gate (AC: all)**
  - [x] 4.1 Inside the devcontainer (`podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>`): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pre-commit run --all-files`. Confirm Task 1's three new tests pass and that no existing test regresses.
  - [x] 4.2 Manual live-verification note (not automated, not a story blocker — same boundary as every prior story's live-compositor gap): on a real `river` session, confirm `Super+1` on a fresh login creates "email", switches to it and brings up its pinned terminal; that pressing it repeatedly does not grow the tag list; and that an `exec` bind with quoting and command substitution in it actually runs. Record whether this was confirmed live or deferred.

## Technical notes

**The duplicate-tag hazard is specific to this project's tag model.** In a
system with tag deletion, a stray duplicate is a minor annoyance a user
cleans up. Under ADR-006 it is not recoverable: `TagRegistry` has no
`delete_tag` (its absence is deliberate and documented as far back as
Story 1.2), ids are assigned as `TagId(self.tags.len())` and never
reused, and the ceiling is a hard 64. A named-tag keybind is the single
highest-frequency create path in the whole WM — a user with four named
tags bound to `Super+1..4` will press them thousands of times a session.
Without lookup-before-create, that user exhausts the registry in an
afternoon and cannot undo it without restarting.

Worth being precise about the layering here: `TagRegistry::create_tag` is
*already* idempotent by name (it returns the existing id for a known
name, before its own `Full` check — Story 1.2's inferred behavior), so
the explicit `tag_id_by_name` lookup is a second, local guarantee rather
than the only thing standing between this keybind and a full registry. It
is kept anyway for two reasons: it makes the create-on-demand intent
legible at the keybind level (a reader of this arm should not have to
trace two modules down to learn that repeated presses are safe), and it
keeps this arm correct on its own terms if `create_tag`'s dedup — which
Story 1.2 flagged as an *inference* from the PRD, not a locked decision —
is ever revisited.

**Why exact match.** `tag_id_by_name` compares `tag.name == name` with no
case folding, trimming, or prefix/substring fallback. The tempting
"helpful" behavior — match case-insensitively, or on a unique prefix — is
actively harmful for this specific caller. A `switch_tag = "Email"` bind
against an existing `"email"` tag would, under loose matching, switch to
`"email"` and then *never* be able to create `"Email"`: the lookup would
keep succeeding, so the create path would be permanently unreachable, and
the user would have no way to get the tag they actually asked for short
of renaming the existing one — which, like deletion, does not exist. Exact
matching makes the near-miss visible immediately (a new tag appears with
the name as written), which is both the correct behavior and the more
diagnosable one. The `tag_id_by_name_is_case_sensitive` test exists to
pin this down as a decision rather than an accident.

**Shell vs. argv, and why the two spawn styles in this file differ.**
`Action::Exec` runs `sh -c <string>`; `spawn_pinned_terminal` builds its
`Command` argument-by-argument (`.arg("-a").arg(PINNED_TERM_APP_ID)
.arg("zellij").arg("attach").arg("--create").arg(session_name)`) and
never involves a shell. This is not an inconsistency — it is the same
rule applied to two different trust situations:

- The `exec` string is a **command the user wrote in their own config
  file**. Shell interpretation is the entire feature; without it a bind
  could only name a bare executable, and `grim -g "$(slurp)"
  ~/screenshot.png` would be impossible to express. There is no privilege
  boundary being crossed: anyone who can edit `~/.config/buoy/config.toml`
  can already run anything as that user by a dozen easier routes (their
  shell rc, a systemd user unit, `~/bin`). Treating this as an injection
  vector would mean treating the user as an attacker against themselves,
  which buys nothing and costs the feature.
- A tag name in `spawn_pinned_terminal` is **data, not a command** — it
  arrives from a `fuzzel` text-input row (Story 2.3's create flow) or from
  a `switch_tag` bind, and ADR-006 explicitly permits arbitrary
  user-facing strings, which includes `;`, `$(...)`, backticks and
  newlines. Passing it individually to `Command` means those characters
  are a weird-looking `zellij` session name and nothing more. That
  function's doc comment already states this reasoning, and this story
  does not weaken it.

Stated as one rule: shell out when the string *is* the command; pass
argv when the string is a value that happens to end up near one.

**Scope boundary.** This story is the two `do_action` arms and the one
`wm_core` query they need. It does not cover the TOML schema or `Action`'s
serde representation (Story 3.1), binding registration or the
replace-don't-merge keymap rule (Story 3.3), keysym resolution for the
`key = "1"` / `key = "P"` names these binds use (the keysym-table story),
or the generated cheat-sheet's rendering of these two variants
(`Action::description`'s `"Run: {command}"` / `"Switch to tag \"{name}\""`
lines — the cheat-sheet story). It does not add a rename or delete
operation for tags, does not make `switch_tag` able to target a specific
output other than the active one, and does not add any `exec` variant that
waits on, reaps, or reports its child's exit status (YAGNI — every other
spawn in this file is fire-and-forget too).

## Test plan
1. **`wm_core` unit tests (TDD, real RED before GREEN)**: `tag_id_by_name`'s three cases — found, not-found (including the empty-name case), and case-sensitivity. Fully testable: pure query, no Wayland types, no I/O.
2. **Build/lint gate**: `cargo build --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` clean with both new `do_action` arms compiled in — including the by-value `String` binding in each pattern, which only compiles because Story 3.3 moved `pending_action` to `Option<Action>` and `take()`s it.
3. **Regression gate**: the whole existing suite green — in particular `state.rs`'s `create_tag`/`switch_tag`/`cycle_tag` tests, since this story calls all three from a new caller without changing any of them.
4. **Config-side coverage (Story 3.1's, exercised by this story's spellings)**: `parses_a_parameterized_switch_tag_action`, `parses_a_parameterized_exec_action` and `the_shipped_example_config_parses` (which asserts the shipped `docs/config.example.toml` still contains a parseable `switch_tag = "email"` bind and at least one `exec` bind) all confirm the config surface these arms consume stays valid.
5. **Live gate (manual, not automated)**: per Task 4.2 — fresh-session `Super+1` creates, switches and spawns; repeated presses do not grow the registry; an `exec` bind with quoting and `$(...)` runs correctly.

## FR coverage
No original PRD FR covers these. The PRD's tag model assumed tags were
created through the picker (FR-level: tag manager popup, tag switching)
and said nothing about keybinds that name a tag directly, nor anything
about arbitrary command execution — the launcher (`Mod4+R`, `fuzzel`'s
desktop-entry mode) was the only application-starting path envisioned.
This is **new scope beyond the original PRD**, requested by the user
during Epic 2's live testing ("hotkeys to create pre-named tags, and
launching applications / firing scripts") and the direct motivation for
Epic 3 existing at all. It builds on, and does not alter, the existing
tag FRs (FR1 tag tracking, FR2 one tag per output — `switch_tag`'s
existing reroute enforcement is reused untouched) and FR4's pinned
terminal lifecycle (reached via the returned `TagId`, not reimplemented).

## Dev Agent Record

**What was done**:

- `wm/src/wm_core/state.rs` (Task 1, real RED/GREEN TDD): added
  `tag_id_by_name_finds_an_existing_tag`,
  `tag_id_by_name_returns_none_for_an_unknown_name` and
  `tag_id_by_name_is_case_sensitive`; confirmed all three failed to
  compile in-container with `E0599: no method named tag_id_by_name found
  for struct WmCore`, then implemented
  `pub fn tag_id_by_name(&self, name: &str) -> Option<TagId>` as a
  `self.tags.ids().into_iter().find(...)` scan over `TagRegistry::get`,
  with a `///` comment stating the exact-match rule and its reason, and
  confirmed all three GREEN. The third test carries its own `///`
  explaining *why* case sensitivity is a requirement rather than an
  implementation detail, so a future "helpful" relaxation has to argue
  against a stated rationale rather than an unexplained assertion.
- **Replaced the earlier `snapshot()`-based lookup** (Task 1.3). The
  first working version of the `SwitchTag` arm found the tag by calling
  `wm_core.snapshot()` and scanning `snapshot.tags` for a matching
  `name`. That allocates the full `WmCoreSnapshot` — cloned name for
  every tag, cloned `app_id` and expanded per-view tag `Vec` for every
  view, every output, plus two sorts — to answer a single string lookup,
  on the highest-frequency keypress path in the WM. Swapped for the
  dedicated query, which also made the behavior directly unit-testable
  instead of only reachable through the snapshot type.
- `wm/src/main.rs` (Task 2): added the `Action::SwitchTag(name)` arm to
  `Seat::do_action`. Guards on `active_output_id` first (`let Some(...)
  else { eprintln!("Tag keybind for `{name}` pressed but no output is
  registered yet"); return None; }`), then `tag_id_by_name` →
  `create_tag(name.clone())` on miss → `switch_tag(output_id, tag_id)`,
  returning `Some(tag_id)` only on `Ok(())`. Both `wm_core` error paths
  log with the tag name and return `None`. Commented with the
  create-on-demand rationale, the ADR-006-no-delete justification for
  looking up first, and the `manage_seats`-spawn-signal reason for the
  `Some(tag_id)` return.
- `wm/src/main.rs` (Task 3): added the `Action::Exec(command_line)` arm,
  spawning `sh -c <command_line>` with `env_remove("WAYLAND_DEBUG")`,
  logging `"Failed to exec `{command_line}`: {e}"` on `Err`, returning
  `None`. Placed immediately after `Action::Terminal` so the shared
  fire-and-forget spawn shape is visible side by side. The `sh -c`
  security question is answered in an in-code comment (config-file
  origin ⇒ shell interpretation is the intent, not an injection vector),
  rather than left for a reader to re-derive.

**Deviations from the story's literal wording**: none identified. The
implementation matches Tasks 1-3 as written, including the guard-before-
create ordering, the single `name.clone()`, and the `Some(tag_id)`-only-
after-`Ok(())` return contract.

**Notes for reviewers** (flagged rather than left implicit, per this
project's established norm):

1. **The lookup-first guarantee is doubled, not sole.**
   `TagRegistry::create_tag` already returns the existing id for a known
   name, so `tag_id_by_name`'s miss path would not in fact create a
   duplicate even if it were skipped. Kept regardless — see Technical
   notes for the two reasons (intent legibility at the keybind level,
   and independence from an inference Story 1.2 itself marked as
   provisional). Flagging it so a reviewer does not read the lookup as
   the only thing preventing registry exhaustion, nor as redundant code
   to delete.
2. **`create_tag` can still fail here, and is handled.** With the
   registry at ADR-006's 64-tag ceiling, a `switch_tag` bind for a
   genuinely new name returns `Err(WmCoreError::TagLimitReached)`; the
   arm logs and returns `None` rather than panicking (NFR2). There is no
   user-facing surface for that message beyond stderr — consistent with
   every other failure in `do_action`, and out of scope to improve here.
3. **Whitespace and case near-misses create tags.** By design (AC 4), but
   it does mean a typo'd `switch_tag` value silently produces a new,
   permanent, empty tag rather than an error. Judged the better failure
   mode than loose matching (Technical notes), and visible to the user
   immediately in the status bar — but it is a real, accepted tradeoff,
   not an unconsidered one.

**Verification** (all inside the devcontainer via `podman exec -u vscode
-w /workspaces/buoy-wm bold_vaughan <cmd>`):
- `cargo fmt --all -- --check`: clean, no drift.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace`: `buoy-wm` (wm) 215, `status-bar` 27,
  `tag-picker` 69 = **311 total, 0 failures, 0 regressions**. Of the wm
  crate's 215, this story contributes exactly 3 (`wm_core::state`'s
  `tag_id_by_name` tests, taking that module from 120 to 123 and the
  crate from its 181-test baseline at `6e32d5b` toward the Epic 3 total);
  the remaining +31 belong to the sibling Epic 3 stories landed in the
  same commit (`config::mod` 21, `config::keysym` 10).
- `pre-commit run --all-files`: both configured hooks (`cargo fmt
  --check`, `cargo clippy`) passed.

**Task 4.2 (manual live-verification)**: **deferred** — no access to a
live `river` compositor in this sandbox, same boundary as every prior
story's live-compositor gap. The three things still needing a human on a
real session: `Super+1` on a fresh login actually creating "email",
switching to it, and bringing up its pinned terminal; repeated presses
leaving the tag list at one "email"; and an `exec` bind containing
quoting and command substitution actually running.

**File List**:
- `wm/src/wm_core/state.rs` — added `tag_id_by_name`; 3 new unit tests.
- `wm/src/main.rs` — added `Action::SwitchTag(name)` and
  `Action::Exec(command_line)` arms to `Seat::do_action`.
- `docs/config.example.toml` — the shipped example's `switch_tag` and
  `exec` bindings, which double as this story's documentation and are
  parse-asserted by `the_shipped_example_config_parses`.
- `docs/planning/epics/story-3-4.md` — this file.

## Code Review

HIGH-effort workflow-backed review (37 agents: per-angle finders, then an
independent verifier per finding location). Two findings against this
story, both fixed.

1. **`Action::Exec`'s children were never reaped.** Nothing ever reads a
   spawned child's exit status, so each finished `sh -c` lingered as a
   zombie holding a PID-table entry for the lifetime of the session — and
   this process is a long-lived session daemon, so they accumulate for as
   long as the user stays logged in. **Fixed** for every spawn site rather
   than just this one, since the same pattern predates this story: a
   `SPAWNED_CHILDREN` list plus `spawn_tracked`/`track_child`
   opportunistically reaps whatever has already exited each time a new
   child is spawned. The terminal, launcher, `exec`, both `tag-picker`
   spawns, the pinned terminal and the cheat-sheet all route through it. A
   poisoned lock is recovered the same way `ipc::lock_recovering` does —
   losing track of a child leaks a zombie, which is never worth taking
   down the session for (NFR2).
2. **`tag_id_by_name` re-implemented a lookup that already existed, and
   did it in O(n) with an allocation.** `TagRegistry` already maintains an
   `ids_by_name: HashMap<String, TagId>` for `create_tag`'s own idempotency
   check; the new query called `self.tags.ids()` — allocating a `Vec` of
   every registered tag id — and linearly scanned it on every keypress.
   **Fixed**: added `TagRegistry::id_by_name` over the existing index, with
   `WmCore::tag_id_by_name` delegating to it.
   The same finding corrected a claim in this story's own reasoning: the
   `SwitchTag` arm's comment credited the lookup-first with preventing
   duplicate tags, but `create_tag` is *already* idempotent by name and
   returns the existing id before any mutation, so duplicates were never
   possible either way. The lookup is kept, with the comment rewritten to
   the real justification — the overwhelmingly common case (a bind for a
   tag that already exists) then never calls a `&mut WmCore` mutator at
   all, which matters because this project's retrospective identifies new
   call sites onto shared-state mutators as its highest-risk change shape.

The `sh -c` exec action was scrutinised specifically and raised no
finding: the command string originates in the user's own config file, so
shell interpretation is the intent rather than an injection vector, and
the contrast with `spawn_pinned_terminal`'s argv-style call (where a tag
name is data, not a command) holds.

**Re-verification after fixes**: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `pre-commit run
--all-files` all clean; `cargo test --workspace` — 322 total (`buoy-wm`
226, `status-bar` 27, `tag-picker` 69), 0 failures.

**Code review: PASS**.
