---
baseline_commit: 6e32d5b
---

# Story 3.1: Config file foundation — TOML schema and loader

Epic: 3 | Priority: H | Status: done

## Description
Through the end of Epic 2, **`buoy-wm` had no configuration of any kind** —
no config file, no CLI flags, no environment variables. Every keybind was a
`const` keysym plus a hardcoded `Modifiers::Mod4` inside `main.rs`'s
`init_new_seats`; every program name was a string literal at its spawn site
(`Command::new("foot")`, `Command::new("fuzzel")`); the bootstrap tag name
was the literal `"default"` in the `Event::Output` handler (Story 2.10);
and the hotkey cheat-sheet was a hand-maintained `const HOTKEY_HELP: &[&str]`
whose own doc comment asked the next developer to "keep this list in sync
when adding a binding." Changing which key opened the tag picker, or which
terminal the pinned terminal used, meant editing `main.rs` and rebuilding
the window manager.

This is genuinely new ground for the project, not a deferred item finally
picked up. `docs/planning/prd/scope.md` lists config-file work under **"Out
of scope (v1)"** — "Session presets (config-file-defined tag/layout
bundles, hotkey-invoked)" — so nothing in the PRD, the epics, or Epic 1/2's
twenty stories ever specified a configuration surface, a file format, a
search path, or an error policy for one. Epic 3 opens that ground, and this
story is its foundation: **the schema and the loader, and nothing that
consumes them.**

**The chosen shape:** a single optional TOML file at
`$XDG_CONFIG_HOME/buoy/config.toml`, falling back to
`~/.config/buoy/config.toml` when `XDG_CONFIG_HOME` is unset. TOML rather
than a bespoke or scripting format because the binding list is fundamentally
an array of tables (`[[keybind]]`), which TOML expresses natively and
`serde` derives for free — no parser to write, no evaluation semantics to
define, and a format the target user (a CLI-first Linux desktop user) already
reads daily.

**Scope boundary.** This story owns the *data*: `wm/src/config/mod.rs`'s
types (`Config`, `Defaults`, `Keybind`, `Mousebind`, `Action`, `Modifier`,
`Button`, `ConfigError`), the `RawConfig`/`RawDefaults` wire shape,
`Config::parse`, `Config::load`, `config_path`, the shipped
`docs/config.example.toml`, and the `toml` dependency. It deliberately owns
none of the consumption:

- **Story 3.2** provides the key-name → keysym resolution table
  (`wm/src/config/keysym.rs`) that `Keybind::keysym` delegates to. This
  story defines only the *boundary* — that a key name which fails to
  resolve is rejected inside `parse`, by name — not the resolution itself.
- **Story 3.5** is the counterpart that actually wires the loaded `Config`
  into startup: `Config::load`'s call site in `main`, `init_new_seats`
  registering bindings from the config instead of constants, and threading
  `defaults.terminal` through to `spawn_pinned_terminal` and
  `ipc::server::spawn`.
- The parameterized actions' *behavior* (`Action::Exec` running through
  `sh -c`, `Action::SwitchTag`'s create-on-demand lookup and
  `WmCore::tag_id_by_name`) and the generated cheat-sheet
  (`Config::hotkey_help`, `Action::description`) are separate Epic 3
  stories. Both appear in `config/mod.rs` because that is where the data
  they act on lives, but neither is this story's acceptance surface.

## Acceptance criteria
**Given** no `config.toml` exists at `config_path()`
**When** `Config::load` runs
**Then** it returns `Ok(Config::default())` — the built-in keybinds,
mousebinds and program names, byte-for-byte the behaviour that was
hardcoded before this module existed — because running without a config
file is the expected case, not an error

**Given** a `config.toml` that exists but is completely empty
**When** `Config::parse` runs on its contents
**Then** it returns `Ok`, and the result equals `Config::default()` — an
empty file is a valid file, and the absence of a `[[keybind]]` is not a
request to lose every binding

**Given** a config file whose `[defaults]` names only `terminal`
**When** it is parsed
**Then** `defaults.terminal` takes the file's value and `defaults.launcher`
/ `defaults.default_tag` keep their built-in values — `[defaults]` merges
per-key, so changing one program does not require restating the others

**Given** a config file declaring one `[[keybind]]`
**When** it is parsed
**Then** `config.keybinds` contains exactly that one binding — declaring
any keybind **replaces** the built-in set outright rather than merging into
it — and `config.mousebinds` still contains both built-in mousebinds,
because `[[mousebind]]` is an independent list governed by the same
replace rule applied separately

**Given** a keybind whose `action` is a single-key table
(`action = { switch_tag = "email" }` or `action = { exec = "grim -g slurp" }`)
**When** it is parsed
**Then** it yields `Action::SwitchTag("email")` / `Action::Exec("grim -g
slurp")` respectively, from the same `action` field that accepts bare
strings (`action = "close"`) for the unit variants

**Given** a keybind naming a modifier that does not exist (`mod = ["Hyper"]`)
**When** it is parsed
**Then** parsing fails, and the error message contains the offending
spelling `Hyper` — so the user is pointed at the actual typo, not told
"invalid config"

**Given** a keybind naming an action that does not exist
(`action = "self_destruct"`)
**When** it is parsed
**Then** parsing fails, and the error message contains `self_destruct`

**Given** a keybind naming a key that resolves to no keysym
(`key = "Retrun"`)
**When** it is parsed
**Then** parsing fails with `ConfigError::UnknownKey("Retrun")`, whose
`Display` names the key — the rejection happens at load, not at
binding-registration time, so the typo can never become a binding that
silently never fires

**Given** a config file that is not well-formed TOML at all
(`[[keybind]\nkey =`)
**When** it is parsed
**Then** parsing fails with `ConfigError::Toml`, never a partially-applied
config — a half-loaded keymap is harder to diagnose than a refusal

**Given** the shipped `docs/config.example.toml`
**When** the test suite runs
**Then** it parses successfully, and every action spelling it advertises
(including `{ switch_tag = "..." }` and `{ exec = "..." }`) round-trips to
the variant it documents — the documentation is compiled into the test
binary via `include_str!`, so it cannot drift from the parser

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: RED — write the whole schema's test module first (AC: all)**
  - [x] 1.1 Before any implementation existed, write `#[cfg(test)] mod
        tests` at the bottom of the new `wm/src/config/mod.rs`, covering
        every AC above: `default_config_reproduces_the_previously_hardcoded_programs`,
        `default_config_keeps_every_built_in_keybind`,
        `empty_config_file_is_valid_and_yields_the_defaults`,
        `defaults_section_overrides_only_the_keys_it_names`,
        `declaring_keybinds_replaces_the_built_in_set`,
        `parses_multiple_modifiers`, `parses_every_modifier_spelling`,
        `parses_a_parameterized_switch_tag_action`,
        `parses_a_parameterized_exec_action`, `parses_mousebinds`,
        `rejects_an_unknown_modifier`, `rejects_an_unknown_action`,
        `rejects_a_key_name_that_is_not_a_known_keysym`,
        `rejects_malformed_toml`, `the_shipped_example_config_parses`,
        `keybind_resolves_its_key_name_to_a_keysym`.
  - [x] 1.2 Confirm RED **in the devcontainer**, not by inspection: with
        the test module present and no types defined,
        `cargo test -p buoy-wm config::` fails to compile with **70
        errors**, dominated by "cannot find type `Config` in this scope",
        "cannot find type `Action` in this scope", "cannot find type
        `Button` in this scope" and "cannot find type `Modifier` in this
        scope" — i.e. every assertion is genuinely exercising a surface
        that does not yet exist, rather than a test that would have passed
        against an accidental stub.
  - [x] 1.3 The three rejection tests assert on `err.to_string()`
        *containing the offending value*, not on an error variant — the
        thing the user actually needs is the typo echoed back, and
        asserting on the message is what keeps that property from being
        refactored away.

- [x] **Task 2: GREEN — the value types (AC: modifier/action/button parsing)**
  - [x] 2.1 `Modifier` — `Super`/`Ctrl`/`Alt`/`Shift`, each with `#[serde(alias
        = ...)]` for the spellings the X11/xkb world uses interchangeably:
        `Mod4`/`Logo`/`Win` → `Super`, `Control` → `Ctrl`, `Mod1` → `Alt`.
        Aliases rather than a normalizing pre-pass so the rejection message
        for a genuinely unknown modifier still comes from serde and still
        names the offender.
  - [x] 2.2 `Button` — `Left`/`Right`/`Middle`. No aliases; there is no
        competing conventional spelling to accommodate.
  - [x] 2.3 `Action` — eleven unit variants (`Terminal`, `Launcher`,
        `Close`, `FocusNext`, `Exit`, `CycleTag`, `TagPicker`, `TagSwitch`,
        `Hotkeys`, `Move`, `Resize`) plus two newtype variants
        (`Exec(String)`, `SwitchTag(String)`), under
        `#[serde(rename_all = "snake_case")]`. Left on serde's **default
        externally-tagged** representation deliberately: that is precisely
        what lets a unit variant be written as a bare string
        (`action = "close"`) and a parameterized one as a single-key table
        (`action = { exec = "..." }`) in the *same* field, with no custom
        `Deserialize` impl and no separate `action`/`arg` key pair.
  - [x] 2.4 `Keybind { mods: Vec<Modifier>, key: String, action: Action }`
        and `Mousebind { mods: Vec<Modifier>, button: Button, action:
        Action }`, both `#[serde(rename = "mod")]` on `mods` (`mod` is a
        Rust keyword; `mod = [...]` is what reads naturally in the file)
        and both `#[serde(deny_unknown_fields)]`, so a typo'd key is an
        error rather than being silently ignored — silently-ignored is the
        failure mode where the user edits the file, sees no change, and has
        nothing to debug.
  - [x] 2.5 `Keybind::keysym() -> Option<u32>`, delegating to
        `keysym::from_name` (Story 3.2). Its doc comment records the
        invariant this story establishes: `Config::parse` rejects
        unresolvable names at load, so a `Keybind` that reaches the WM
        always resolves.

- [x] **Task 3: GREEN — `Defaults` and `Config::default` (AC: no-config-file, empty-file)**
  - [x] 3.1 `Defaults { terminal, launcher, default_tag }` with a hand-written
        `impl Default` returning `"foot"`, `"fuzzel"`, `"default"` — the
        three literals that were previously scattered across
        `Seat::do_action`, `spawn_pinned_terminal` and the `Event::Output`
        bootstrap.
  - [x] 3.2 `impl Default for Config` reproduces the previously-hardcoded
        behaviour *exactly*: the nine `Super`-modified keybinds
        (`space`→`Terminal`, `q`→`Close`, `n`→`FocusNext`,
        `Escape`→`Exit`, `Tab`→`CycleTag`, `a`→`TagPicker`,
        `s`→`TagSwitch`, `r`→`Launcher`) plus the one two-modifier binding
        (`Super+Shift+?`→`Hotkeys`, which needs `Shift` in the modifier set
        because xkbcommon has no unshifted `?` keysym and the binding must
        match the shifted symbol the layout actually produces), and the two
        `Super`-modified mousebinds (`Left`→`Move`, `Right`→`Resize`).
        This is the whole reason a missing config file is a no-op rather
        than a downgrade.
  - [x] 3.3 A local `keybind` closure builds the eight single-modifier
        entries so the list reads as data rather than as ten near-identical
        struct literals; the `Super+Shift+?` entry is spelled out in full
        precisely because it is the exception, and carries the comment
        explaining why.

- [x] **Task 4: GREEN — the `RawConfig` wire shape (AC: `[defaults]` partial override)**
  - [x] 4.1 `RawConfig { defaults: RawDefaults, keybinds: Vec<Keybind>,
        mousebinds: Vec<Mousebind> }` with `#[serde(default)]` on every
        field and `#[serde(rename = "keybind"/"mousebind")]` so the file
        reads `[[keybind]]` (singular, as an array-of-tables entry should)
        while the field stays plural.
  - [x] 4.2 `RawDefaults { terminal: Option<String>, launcher:
        Option<String>, default_tag: Option<String> }` — a **separate
        type from `Defaults`, existing precisely so absence is
        representable per key.** `Defaults` itself has no `Option`s because
        by the time it reaches the WM every value is resolved; `RawDefaults`
        is the only place "the user didn't say" is a distinct state from
        "the user said something."
  - [x] 4.3 Both raw types are `#[serde(deny_unknown_fields)]` and private
        to the module — the wire shape is an implementation detail of
        `parse`, never part of the public API.

- [x] **Task 5: GREEN — `Config::parse` (AC: replace-vs-merge, unknown key)**
  - [x] 5.1 `toml::from_str::<RawConfig>` first, mapping the error to
        `ConfigError::Toml` — malformed TOML fails before any semantic
        check, so the message the user sees is the parser's own
        line/column report.
  - [x] 5.2 Validate every declared keybind's key name **before** building
        the `Config`: `keybind.keysym().is_none()` →
        `Err(ConfigError::UnknownKey(keybind.key.clone()))`. Carried by
        name, not as a bare "invalid key", so `Display` can point at the
        actual typo. Rejected here — once, at startup — rather than at
        binding-registration time, where the alternative outcome is a
        binding that registers and then silently never fires.
  - [x] 5.3 `defaults` merges per key via `Option::unwrap_or` against
        `Config::default()`'s values.
  - [x] 5.4 `keybinds` / `mousebinds` **replace**: `if raw.keybinds.is_empty()
        { built_in.keybinds } else { raw.keybinds }`, and the same
        independently for mousebinds. Declaring one binding therefore
        yields exactly one binding, and declaring only keybinds leaves both
        built-in mousebinds intact.
  - [x] 5.5 The replace rule is documented on `parse` itself, in the doc
        comment, with the *reason* — not just the behaviour — because the
        behaviour is surprising the first time and the reason is the only
        thing that makes it obviously right.

- [x] **Task 6: GREEN — `config_path` and `Config::load` (AC: no-config-file)**
  - [x] 6.1 `config_path()` → `$XDG_CONFIG_HOME/buoy/config.toml`, or
        `$HOME/.config/buoy/config.toml` when `XDG_CONFIG_HOME` is unset,
        or a bare relative `buoy/config.toml` when neither variable exists
        (which `load` then simply doesn't find, taking the missing-file
        path — no panic, no special case).
  - [x] 6.2 `Config::load()` matches on `read_to_string`'s error kind:
        `Ok(contents)` → `Config::parse`; `ErrorKind::NotFound` →
        `Ok(Config::default())`; **any other IO error** →
        `Err(ConfigError::Io(e))`. The asymmetry is the point — a file
        that isn't there is the expected case, while a file that is there
        and can't be read (permissions, a directory in its place, an IO
        fault) is a real problem, and collapsing the two would let a typo
        or a `chmod 000` degrade silently into "the defaults, apparently."
  - [x] 6.3 `ConfigError` gets `Display` and `std::error::Error`, so the
        caller (Story 3.5's startup path) can report it with `{e}` and get
        a message naming the file, the line or the key.

- [x] **Task 7: Ship `docs/config.example.toml`, and pin it with a test (AC: shipped example parses)**
  - [x] 7.1 Write a fully-commented example covering every section: the
        three `[defaults]` keys, the full built-in keybind set restated
        (so a user copying it gets exactly today's behaviour and can edit
        from there — the replace rule makes "list every binding you want"
        mandatory, and the file says so at the top), two
        `{ switch_tag = "..." }` binds, two `{ exec = "..." }` binds, and
        both mousebinds. Document the accepted modifier spellings, the
        key-name grammar, and the full action list inline.
  - [x] 7.2 `the_shipped_example_config_parses` pulls it in with
        `include_str!("../../../docs/config.example.toml")` and asserts it
        parses **and** that the specific spellings it advertises resolve
        (a `SwitchTag("email")`, at least one `Exec`, both mousebinds).
        Compiling the documentation into the test binary is what makes
        drift impossible: an action rename that forgets the example file
        breaks the build, not a user's session.

- [x] **Task 8: Add the `toml` dependency (AC: all parsing)**
  - [x] 8.1 `toml = { version = "1.1.4", default-features = false, features
        = ["parse", "serde"] }` in `wm/Cargo.toml`. Default features off
        and only `parse` + `serde` on: this crate only ever *reads* TOML —
        nothing writes a config file back — so the serializer and the
        `Display`/format machinery are dead weight in a WM whose NFRs
        include a 50MB idle RSS ceiling. Pulls in `serde_spanned`,
        `toml_datetime`, `toml_parser` and `winnow`.

- [x] **Task 9: Full in-container verification gate (AC: all)**
  - [x] 9.1 Via `podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan
        <cmd>`: `cargo fmt --all -- --check`, `cargo clippy --workspace
        --all-targets -- -D warnings`, `cargo test --workspace`,
        `pre-commit run --all-files`. All clean; counts recorded in the Dev
        Agent Record.

## Technical notes

**Why declaring a keybind replaces the set instead of merging into it.**
This is the single most consequential decision in the story, and the one a
user is most likely to be surprised by, so it is documented on
`Config::parse`, in `docs/config.example.toml`'s header, and in the commit
message. Merging has two failure modes that replacing does not:

1. **A built-in binding becomes impossible to *remove*.** Under a merge
   rule the config file is a set of additions and overrides; there is no
   syntax for "unbind `Super+Q`" short of inventing one (a `remove = true`
   flag, a null action, a separate `[[unbind]]` table). Every one of those
   is a second concept the user has to learn in order to express something
   the replace rule expresses by simply not writing the line.
2. **A rebound default gets silently reintroduced.** If a user moves
   `close` from `Super+Q` to `Super+Shift+Q` and a merge re-adds the
   built-in `Super+Q → close`, they now have two bindings for close, one
   of which they explicitly tried to get rid of — and nothing in the file
   they wrote hints at why.

The cost of replacing is that a user who wants one extra binding must
restate the built-in set. That cost is paid once, is fully mechanical, and
is exactly what `docs/config.example.toml` exists to make trivial: it *is*
the built-in set, spelled out, ready to copy and edit.

`[[mousebind]]` follows the same rule but as an **independent list** — the
two are separate arrays with separate emptiness checks, so declaring
keybinds does not disturb mousebinds and vice versa. `declaring_keybinds_
replaces_the_built_in_set` asserts both halves of that in one test, because
"my mouse stopped working when I rebound a key" is exactly the coupling
worth pinning down.

**Why `[defaults]` merges per key while the binding lists don't.** The
asymmetry is deliberate and reflects what each section *is*. A binding list
is a complete statement of intent — "these are my bindings" — where partial
statements have no obvious meaning. `[defaults]` is a bag of unrelated
scalars, where "I use `alacritty`" says nothing whatsoever about which
launcher the user wants, and forcing them to restate `launcher` and
`default_tag` to change `terminal` would be pure ceremony. The
`RawConfig`/`RawDefaults` split exists precisely to make this per-key
fallback expressible: `RawDefaults`'s `Option<String>` fields are the only
place in the module where "absent" is distinguishable from "set", and
`Defaults` itself carries no `Option`s because by the time it leaves
`parse` every value is resolved.

**Why a broken config is reported and then ignored, rather than fatal.**
`Config::load` returns `Err` on a file that exists and fails — it does not
paper over the failure. But the *startup* handling of that `Err` (Story
3.5's `main`) prints the path and the error to stderr and then continues
with `Config::default()`, rather than aborting. This looks like the
error-swallowing this project's conventions forbid, and it is the one place
it is justified: **this window manager is the only thing that can put a
screen in front of the user to fix the typo with.** Refusing to start over
a misspelled keysym means no terminal, no editor, and no session in which
to correct the file — the user is dropped back to a display manager or a
TTY by a program that already knew exactly what was wrong. Degrading to the
built-in bindings leaves a working desktop and a loud message. The
error/no-error split still lives entirely in `load`, so the policy is the
caller's and could be changed (a `--strict` flag, say) without touching the
loader. NFR2's "a WM crash strands the desktop session" is the same
principle applied one layer down.

**Why the unknown-key check lives in `parse` rather than at registration.**
`Config::parse` walks every declared keybind and rejects the first
unresolvable name before returning. The alternative — letting the binding
through and discovering the problem in `init_new_seats` — produces a
binding that registers successfully with nothing behind it, or one that is
skipped with a message buried in the WM's stderr long after startup. Either
way the user presses the key and nothing happens, which is the worst
diagnostic outcome available. Checking at parse time means one message, at
one moment, naming one key. (Story 3.5's `init_new_seats` still handles
`keysym()` returning `None` defensively, skip-and-log rather than panic,
because `Config::default` can reach it without passing through `parse` —
but that path is covered by its own test and cannot be reached by a user's
file.)

**Why serde's default enum representation was left alone.** Externally
tagged is the representation that maps a unit variant to a bare string and
a newtype variant to a single-key table. That is exactly the ergonomic
shape wanted — `action = "close"` for the common case, `action = { exec =
"..." }` only where an argument is genuinely needed — and it falls out of
`#[derive(Deserialize)]` with no custom impl. An internally- or
adjacently-tagged representation would have forced `action = { type =
"exec", value = "..." }`-style noise onto all thirteen variants to
accommodate the two that need a payload.

**Why modifier aliases.** `Mod4`, `Logo` and `Win` all name the same
physical key depending on which of sway, i3, X11 or the keycap the user
learned it from; `Control` and `Ctrl` likewise; `Mod1` and `Alt` likewise.
Accepting all of them costs three `#[serde(alias)]` attributes and avoids
telling a user that the spelling they have typed in every window-manager
config they have ever written is wrong here. `parses_every_modifier_
spelling` pins the canonical four; the aliases ride on the same
`Deserialize` derive, so an unknown modifier still produces serde's own
message naming the offending string (`rejects_an_unknown_modifier` asserts
on that).

**Why the example config is a test fixture.** `the_shipped_example_config_
parses` reads `docs/config.example.toml` through `include_str!`, so the
documentation is compiled into the test binary. Renaming an action variant,
tightening `deny_unknown_fields`, or changing the modifier spellings breaks
`cargo test` immediately rather than silently invalidating the file every
user copies as their starting point. The assertions go past "it parses" to
name the specific advertised spellings (`{ switch_tag = "email" }`, an
`{ exec = ... }`, both mousebinds), because a file that parses while
documenting an action that no longer exists is still wrong.

**Not in scope.** No config reloading (the `config` field on
`WindowManager` is loaded once at startup; re-reading on change would mean
tearing down and recreating every live binding object, which nothing needs
yet — YAGNI). No config *writing* — hence `toml`'s serializer feature
staying off. No per-output, per-tag or per-application configuration. No
include/import mechanism. No validation beyond "every key name resolves"
— in particular, duplicate bindings for the same modifier+key combination
are not detected here; the protocol's own binding registration is what
would surface that, and no live case has called for it.

## Test plan
1. **Schema/loader unit tests (TDD, RED before GREEN)** — 16 tests in
   `wm/src/config/mod.rs`'s `mod tests`, one per acceptance criterion plus
   the modifier-spelling coverage: defaults reproduction (2), empty file
   (1), per-key `[defaults]` merge (1), replace-not-merge including the
   mousebind independence assertion (1), modifier parsing (2), the two
   parameterized actions (2), mousebind parsing (1), the four rejection
   paths (unknown modifier, unknown action, unknown key name, malformed
   TOML), the shipped-example fixture (1), and the keysym boundary (1).
   Entirely pure — no Wayland types, no filesystem, no compositor — so
   unlike most of this project's stories there is **no untestable-glue
   carve-out at all** for the surface this story owns.
2. **Documentation-drift gate**: `the_shipped_example_config_parses`, which
   fails the build rather than the user if `docs/config.example.toml` and
   the parser disagree.
3. **Regression gate**: the pre-existing 277-test workspace suite unaffected
   — this story adds a new module and a new dependency and changes no
   existing behaviour, so every prior test must pass untouched.
4. **Build/lint gate**: `cargo fmt --all -- --check`, `cargo clippy
   --workspace --all-targets -- -D warnings`, `pre-commit run --all-files`.
5. **Live gate (manual, not automated)**: not applicable to this story —
   nothing here reaches a compositor. The observable behaviour of a config
   file (bindings actually firing, `foot` actually being replaced) is
   Story 3.5's live gate.

## FR coverage
**None — this is new scope beyond the original PRD, stated plainly.**
`docs/planning/prd/features-and-acceptance-criteria.md` defines no
configuration feature, and `docs/planning/prd/scope.md` explicitly lists
config-file work ("Session presets (config-file-defined tag/layout bundles,
hotkey-invoked)") under **Out of scope (v1)**. Epic 3 is a post-v1
expansion driven by live use: after Epic 2 the WM was usable, and the first
thing usage produced was the want to change a keybind without a rebuild.

The one PRD requirement this story does touch is **NFR2** ("WM must not
crash on malformed/unexpected client behavior — a WM crash strands the
desktop session; highest-priority NFR"), extended here from *client*
behaviour to *user configuration*: a malformed config file is an `Err`
value, never a panic, and — per the Technical notes and Story 3.5 — never
an aborted startup.

Note also that this story quietly *narrows* a PRD statement: the pinned
terminal's acceptance criterion names `foot -a pinned-term` literally. The
terminal is now `defaults.terminal`; the `-a pinned-term` app-id is not
configurable, because `PINNED_TERM_APP_ID` is how every other part of the
WM recognizes the pinned terminal. A terminal emulator that cannot be given
an app-id cannot fill the role at all, and that constraint is documented on
`spawn_pinned_terminal` rather than left to be discovered.

## Dev Agent Record

**What was done**:

- `wm/src/config/mod.rs` — new module, **written test-module-first**. The
  `#[cfg(test)] mod tests` block was authored in full before any type in
  the file existed, and RED was confirmed in the devcontainer rather than
  assumed: `cargo test -p buoy-wm config::` failed with **70 compile
  errors**, overwhelmingly "cannot find type `Config` in this scope",
  "cannot find type `Action` in this scope", "cannot find type `Button` in
  this scope" and "cannot find type `Modifier` in this scope" — every
  assertion demonstrably exercising a surface that did not yet exist. The
  implementation was then added to GREEN: `Modifier` (with the `Mod4`/
  `Logo`/`Win`, `Control`, `Mod1` aliases), `Button`, `Action` (11 unit +
  2 newtype variants under `rename_all = "snake_case"`, on serde's default
  externally-tagged representation), `Keybind`/`Mousebind` (both
  `deny_unknown_fields`, both `rename = "mod"`), `Defaults` +
  `impl Default`, `Config` + `impl Default` reproducing the previously
  hardcoded nine keybinds and two mousebinds exactly, `ConfigError`
  (`Toml`/`UnknownKey(String)`/`Io`) with `Display` + `std::error::Error`,
  the private `RawConfig`/`RawDefaults` wire types, `Config::parse`,
  `Config::load`, `config_path`, and `Keybind::keysym`.
- `docs/config.example.toml` — new, 115 lines, fully commented: the three
  `[defaults]` keys, the complete built-in keybind set restated so a copy
  reproduces today's behaviour, two `{ switch_tag = "..." }` binds, two
  `{ exec = "..." }` binds, both mousebinds, plus inline documentation of
  the accepted modifier spellings, the key-name grammar and the full action
  list. Pinned by `the_shipped_example_config_parses` via `include_str!`.
- `wm/Cargo.toml` / `Cargo.lock` — added `toml = { version = "1.1.4",
  default-features = false, features = ["parse", "serde"] }`. Serializer
  features deliberately off (this crate never writes TOML). Transitively
  adds `serde_spanned`, `toml_datetime`, `toml_parser`, `winnow`.
- `README.md` — new "Configuration" section documenting the file location,
  the optional-file/defaults contract, the replace-not-merge rule for
  `[[keybind]]`/`[[mousebind]]`, and the report-and-fall-back behaviour on
  a malformed config, pointing at `docs/config.example.toml` for the full
  reference.

**Scope notes** (flagged explicitly, per this project's established norm —
see Stories 2.7/2.9/2.10's own Dev Agent Records):

1. **`Config::hotkey_help` and `Action::description` live in this file but
   are not this story's surface.** They were implemented in the same commit
   (`cf0d405`) as the cheat-sheet counterpart story, and account for 5 of
   the 21 tests in `config::tests` (`hotkey_help_covers_every_binding`,
   `_names_the_modifiers_key_and_action`, `_shows_parameterized_action_
   payloads`, `_reflects_the_configured_programs`,
   `_labels_mouse_buttons_as_clicks`). This story's 16 tests are the
   schema/loader ones listed in the Test plan.
2. **`wm/src/config/keysym.rs` (10 tests) is Story 3.2's**, not this
   story's, even though `Keybind::keysym` and
   `rejects_a_key_name_that_is_not_a_known_keysym` sit on the boundary
   between them. This story specifies only that an unresolvable name is
   rejected inside `parse`, by name.
3. **`WmCore::tag_id_by_name` (3 tests) and every `main.rs` /
   `ipc/server.rs` change in `cf0d405` belong to the action-behaviour and
   startup-wiring stories**, not here. `cf0d405` is a single commit
   spanning several Epic 3 stories; this document covers the config
   schema and loader portion of it.

**Verification** (all inside the devcontainer via `podman exec -u vscode -w
/workspaces/buoy-wm bold_vaughan <cmd>`):
- `cargo fmt --all -- --check`: clean, no drift.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace`: `buoy-wm` 215 (181 baseline at `6e32d5b` + 34
  added by `cf0d405`, of which **21 are `config::tests`** — this story's 16
  schema/loader tests plus the cheat-sheet story's 5 — 10 are
  `config::keysym::tests` (Story 3.2) and 3 are `wm_core::state`'s
  `tag_id_by_name` tests), `status-bar` 27 (unaffected), `tag-picker` 69
  (unaffected) = **311 total, 0 failures, 0 regressions** against the
  277-test baseline.
- `cargo test -p buoy-wm config::`: 31 passed, 0 failed, 184 filtered out.
- `pre-commit run --all-files`: both configured hooks (`cargo fmt --check`,
  `cargo clippy`) passed.

**Live verification**: not applicable to this story. Nothing in
`wm/src/config/mod.rs` touches a compositor, a socket or a spawned process,
so there is no live-`river` gap of the kind every Epic 1/2 story had to
defer. Confirming that an edited `~/.config/buoy/config.toml` actually
changes the running session's bindings is Story 3.5's live gate.

**File List**:
- `wm/src/config/mod.rs` — new module: `Config`, `Defaults`, `Keybind`,
  `Mousebind`, `Action`, `Modifier`, `Button`, `ConfigError`,
  `RawConfig`/`RawDefaults`, `Config::parse`, `Config::load`,
  `config_path`, `Keybind::keysym`, and the 21-test `mod tests`.
- `docs/config.example.toml` — new; the shipped reference config, pinned by
  `the_shipped_example_config_parses`.
- `wm/Cargo.toml` — added the `toml` dependency (`--no-default-features`,
  `parse` + `serde` only).
- `Cargo.lock` — `toml`, `serde_spanned`, `toml_datetime`, `toml_parser`,
  `winnow`.
- `README.md` — new "Configuration" section.
- `docs/planning/epics/story-3-1.md` — this file.

## Code Review

HIGH-effort workflow-backed review (37 agents: per-angle finders, then an
independent verifier per finding location). Four findings against this
story, all fixed.

1. **HIGH — the shipped example config bound six actions to keysyms that
   can never fire.** `key = "Q"` with `mod = ["Super"]` resolves to 0x51,
   the *shifted* symbol. Pressing Super+q reports 0x71 with `Mod4`;
   Super+Shift+q reports 0x51 with `Mod4|Shift`. Neither matches, so a user
   who copied the example exactly as its own header instructs silently lost
   `close`, `focus_next`, `launcher`, `tag_picker`, `tag_switch` and the
   `exec` example — with nothing logged. `Config::default` had it right
   (lowercase), and `main.rs`'s retained comment on the `?` binding states
   the rule explicitly, so this was drift between the built-in table and
   its own documentation. `the_shipped_example_config_parses` did not catch
   it because parsing was never the failing step. **Fixed** on both sides:
   the example's keys are lowercased, *and* `Config::parse` now rejects an
   ASCII uppercase letter bound without `Shift`
   (`ConfigError::ShiftedKeyWithoutShift`), naming the key and suggesting
   the unshifted form. The rule is deliberately narrow — only single ASCII
   *letters*, since uppercase requires Shift on every Latin layout, while
   other shifted symbols (`?`, `!`) are layout-dependent and rejecting them
   would break legitimate configs. Guarded by tests over the built-in
   defaults *and* the shipped example, which is the guard that would have
   caught the original defect. Two pre-existing tests in this file were
   themselves using `key = "P"` and had to be corrected — they were
   encoding the bug.
2. **`config_path` fell back to a CWD-relative path.** With neither
   `XDG_CONFIG_HOME` nor `HOME` set, `unwrap_or_default()` produced an
   empty base and therefore the relative `buoy/config.toml`, which `load`
   resolves against the WM's working directory — so a `buoy/config.toml` in
   whatever directory the session launched from would be read, and its
   `exec` actions run through `sh -c`. The docstring's claim that `load`
   "simply won't find" it was wrong. **Fixed**: `config_path` returns
   `Option<PathBuf>` and yields `None` when there is no home to look in;
   `load` treats that as "no config" and returns the defaults.
3. **`mod` was a required field.** Omitting it — the natural way to bind a
   bare `F1` — made serde reject the entire file for a missing field, so
   the user lost their whole keymap over one binding, and nothing
   documented that `mod = []` was required. **Fixed**: `#[serde(default)]`
   on both `Keybind::mods` and `Mousebind::mods`, documented in the README
   and the example.
4. **No validation of `[defaults]` values or duplicate bindings.**
   `terminal = ""` parsed cleanly and then failed at `Command::new("")`
   once per keypress; two bindings on the same modifier set and trigger
   both registered, leaving the winner to the compositor. **Fixed**: both
   are rejected at load, naming the offending field or binding label.

**Re-verification after fixes**: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `pre-commit run
--all-files` all clean; `cargo test --workspace` — 322 total (`buoy-wm`
226, `status-bar` 27, `tag-picker` 69), 0 failures.

**Code review: PASS**.
