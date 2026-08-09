---
baseline_commit: 6e32d5b
---

# Story 3.3: Data-driven keybind registration and generated hotkey help

Epic: 3 | Priority: H | Status: done

**Depends on Story 3.1** (the `config` module's schema — `Config`,
`Defaults`, `Keybind`, `Mousebind`, `Modifier`, `Button`, `Action`,
`Config::parse`/`load`/`default`) **and Story 3.2** (`config::keysym`'s
`from_name`, which is what makes a `key = "Escape"` string in a TOML file
into a `u32` the protocol can bind). Neither of those stories changes any
behavior on its own: 3.1 builds a config nothing reads, 3.2 builds a name
resolver nothing calls. This story is where both become load-bearing —
it is the one that deletes the hardcoded bindings and points `river` at
the loaded config instead.

## Description
Two hand-maintained tables in `wm/src/main.rs` were the last thing
standing between the config module (Stories 3.1/3.2) and a config that
actually does anything.

**Table 1 — `init_new_seats`'s hardcoded binding list.** Every one of the
eleven bindings this WM has ever had was registered from a block of
`const` hex keysyms declared inline in the function body (`const SPACE:
u32 = 0x20;`, `const ESC: u32 = 0xff1b;`, `const A: u32 = 0x61;`, …,
plus `BTN_LEFT`/`BTN_RIGHT`), each followed by a literal
`seat.create_xkb_binding(river_xkb, qh, mods, SPACE, Action::SpawnFoot)`
call with a single shared `let mods = Modifiers::Mod4;`. Changing a
keybind meant editing this function and rebuilding the compositor.

**Table 2 — `HOTKEY_HELP`.** A `const HOTKEY_HELP: &[&str]` of eleven
pre-formatted, pre-padded strings (`"Mod4+Space       Spawn terminal
(foot)"`), carrying a doc comment that explicitly admitted the problem:

> Hand-maintained alongside `init_new_seats`'s bindings below — there are
> few enough of these that a shared declarative table isn't worth the
> indirection (YAGNI); keep this list in sync when adding a binding.

That reasoning was defensible while the binding set was a compile-time
constant only this repo's author could change. It stops being defensible
the moment a *user* can add a binding: a cheat-sheet that is manually
kept in sync with a table the user cannot edit will be wrong for every
user who edits the table they *can*. The binding table itself had already
proven it drifts — `8a151ee`, the very commit that introduced
`HOTKEY_HELP`, is also the commit that had to hand-remove a `Mod4+T`
binding that had gone redundant several stories earlier and simply sat
there until someone noticed.

This story replaces both tables with the loaded `Config`: `init_new_seats`
iterates `config.keybinds`/`config.mousebinds`, and `Action::Hotkeys`
writes `config.hotkey_help()` — a list computed from the very bindings
that were registered — to `fuzzel`'s stdin.

**Type unification.** `main.rs` had its *own* `Action` enum, entirely
separate from `config::Action` and predating it by two epics. Keeping
both would have meant a translation `match` between two enums that must
be kept in sync — the exact failure mode this story exists to delete. So
`main.rs`'s `Action` was **removed outright** and `config::Action`
imported in its place (`use config::{Action, Config};`), with two
consequences:

- **`Copy` is gone.** `config::Action` carries `String` payloads
  (`Exec(String)`, `SwitchTag(String)`), so it is `Clone`, not `Copy`.
  Every place that used to move an `Action` out by copy now clones
  explicitly at registration (`keybind.action.clone()`) and at the
  press event (`seat.pending_action = Some(binding.action.clone())`) —
  the binding tables own the canonical copy and must outlive each press.
- **`Action::None` is gone, replaced by `Option<Action>`.**
  `Seat.pending_action` was `Action` with a `None` sentinel variant,
  cleared by assignment (`self.pending_action = Action::None`) and read
  by copy. It is now `Option<Action>`, consumed with
  `self.pending_action.take()?` at the top of `do_action`. This is not
  cosmetic: a parameterized action must be taken **by value** so its
  `String` payload can be moved into the arm that uses it
  (`Action::Exec(command_line) => …`, `Action::SwitchTag(name) => …`)
  without a clone. `take()` on an `Option` does exactly that — swap in
  `None`, hand back the owned value — where a sentinel variant would
  have required either a clone-then-reset or a `std::mem::replace` with
  a hand-written sentinel. It also deletes `do_action`'s
  `Action::None => None` arm entirely: the `?` on `take()` handles "no
  action fired" before the `match` is reached, so the `match` now covers
  only real actions and the compiler enforces exhaustiveness over the
  config's action set.

The old variant names were renamed to the config spellings so there is
one vocabulary, not two: `SpawnFoot`→`Terminal`, `SpawnLauncher`→
`Launcher`, `ShowHotkeys`→`Hotkeys`, `TagCycle`→`CycleTag`,
`OpenTagPicker`→`TagPicker`. `Close`, `FocusNext`, `Exit`, `TagSwitch`,
`Move` and `Resize` kept their names. The renames are mechanical — no
arm's body changed except where it reads a config value — but they are
why this story's diff touches `do_action` at all.

## Acceptance criteria
**Given** no `~/.config/buoy/config.toml` exists
**When** a seat is initialized
**Then** exactly the same eleven bindings are registered as before this
story — `Super+Space`, `Super+Q`, `Super+N`, `Super+Escape`,
`Super+Tab`, `Super+A`, `Super+S`, `Super+R`, `Super+Shift+?`,
`Super+LeftClick`, `Super+RightClick` — with the same keysyms, the same
modifier bitfields and the same actions, because `Config::default`
carries them

**Given** a config file that declares `[[keybind]]` entries
**When** a seat is initialized
**Then** the bindings registered on that seat are exactly the config's
keybinds and only those — no built-in binding survives alongside them
(Story 3.1's replace-don't-merge rule, now observable)

**Given** a config file that declares `[[mousebind]]` entries
**When** a seat is initialized
**Then** the pointer bindings registered are exactly the config's
mousebinds, translated to Linux input event codes, and the keybind list
is independently whatever `[[keybind]]` (or its absence) resolved to

**Given** any loaded config
**When** `Action::Hotkeys` fires
**Then** the cheat-sheet lists one line per binding actually in effect —
every keybind *and* every mousebind, including bindings the user added
that this codebase has never heard of — so the list cannot describe a
binding that does not exist, or omit one that does

**Given** a config with parameterized actions and non-default programs
**When** the cheat-sheet is generated
**Then** each line shows the payload that distinguishes the binding
(`Switch to tag "email"`, `Run: grim -g slurp`) and the spawn actions
name the program that will actually launch (`Spawn terminal
(alacritty)`), not a hardcoded `foot`/`fuzzel`

**Given** a binding whose key name does not resolve to a keysym, in a
config that reached `init_new_seats` without passing `Config::parse`
**When** that seat is initialized
**Then** that one binding is skipped with a message naming the offending
key, every other binding on the seat is still registered, and the session
does not panic (NFR2)

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: `Config::hotkey_help` and its supporting formatters (AC 4, 5) — TDD, RED before GREEN**
  - [x] 1.1 RED — Add tests to `wm/src/config/mod.rs`'s `tests` module before any implementation exists: `hotkey_help_covers_every_binding` (asserts `help.len() == keybinds.len() + mousebinds.len()` against `Config::default` — the structural property that makes drift impossible, not a golden-string comparison that would itself need hand-maintaining), `hotkey_help_names_the_modifiers_key_and_action` (a `mod = ["Super", "Shift"], key = "Q", action = "close"` bind produces a line containing both `"Super+Shift+Q"` and `"Close focused window"`), `hotkey_help_shows_parameterized_action_payloads` (`switch_tag = "email"` and `exec = "grim -g slurp"` binds show `email` and `grim -g slurp` respectively — the payload is the *only* thing distinguishing two otherwise-identical binds, so omitting it would make the sheet ambiguous), `hotkey_help_reflects_the_configured_programs` (`terminal = "alacritty"` + an `action = "terminal"` bind produces a line containing `alacritty`), `hotkey_help_labels_mouse_buttons_as_clicks` (`Config::default`'s help contains `"Super+LeftClick"`). Confirm all five fail to compile.
  - [x] 1.2 GREEN — Implement three private formatters plus the one public method:
    - `Modifier::label(self) -> &'static str` — `Super`/`Ctrl`/`Alt`/`Shift`. Deliberately the config's own spellings, not the protocol's (`Mod4`/`Mod1`): the sheet exists to tell the user what to type in their config file, and `Modifier`'s serde aliases mean `Mod4` is accepted input but `Super` is the canonical name.
    - `binding_label(mods: &[Modifier], trigger: &str) -> String` — joins modifier labels and the trigger with `+`, yielding `Mod+Mod+Key`. A free function rather than a method because its two call sites pass different trigger sources (a keybind's `key` field, a mousebind's rendered button name) and neither owns the other.
    - `Action::description(&self, defaults: &Defaults) -> String` — one arm per action. Takes `&Defaults` specifically because `Terminal`/`Launcher` must name the program they will actually spawn (`format!("Spawn terminal ({})", defaults.terminal)`), which is a config value now, not a constant; every other arm ignores the parameter but takes it for a uniform signature.
    - `Config::hotkey_help(&self) -> Vec<String>` — builds `(label, description)` pairs from `self.keybinds`, `chain`s the same from `self.mousebinds` (with `Left`/`Right`/`Middle` rendered as `LeftClick`/`RightClick`/`MiddleClick` — "Click" reads as a physical action where a bare `Left` would read as a direction), then pads every label to the widest label's width and joins with two spaces.
  - [x] 1.3 Column width is computed (`.map(|(label, _)| label.chars().count()).max().unwrap_or(0)`), not a fixed constant. The old `HOTKEY_HELP` hardcoded its padding to fit `"Mod4+RightClick"`; a user binding `Super+Ctrl+Alt+Shift+BackSpace` would have blown straight past it and produced a ragged sheet. `chars().count()` rather than `len()` so a multi-byte key name counts as one column per character. `unwrap_or(0)` covers the empty-binding-set case (`format!("{label:<0$}")` on an empty list is never reached, but `max()` on an empty iterator returns `None` and must not panic — NFR2).

- [x] **Task 2: Delete `main.rs`'s `Action` enum and adopt `config::Action` (AC: all)**
  - [x] 2.1 Remove the local `#[derive(Debug, Clone, Copy)] enum Action { None, SpawnFoot, … }` and add `use config::{Action, Config};`. `XkbBinding.action` and `PointerBinding.action` need no declaration change — they were already typed `Action` and now resolve to the imported one.
  - [x] 2.2 `Seat.pending_action: Action` → `Option<Action>`; `Seat::new`'s initializer `Action::None` → `None`; both `Dispatch` `Event::Pressed` arms (`RiverXkbBindingV1`, `RiverPointerBindingV1`) `seat.pending_action = binding.action` → `= Some(binding.action.clone())`. `///` doc comment on the field recording *why* it is an `Option` and not a `None` variant: so a parameterized action can be taken by value with `.take()` without cloning its payload.
  - [x] 2.3 `do_action`: replace the two-line copy-then-clear preamble with `let pending_action = self.pending_action.take()?;` and delete the now-unreachable `Action::None => None` arm. Rename every remaining arm to the config spelling (`SpawnFoot`→`Terminal`, `SpawnLauncher`→`Launcher`, `ShowHotkeys`→`Hotkeys`, `TagCycle`→`CycleTag`, `OpenTagPicker`→`TagPicker`) and update the comments that referred to the old names by name.
  - [x] 2.4 No RED/GREEN — `do_action`'s arms spawn real processes and drive real Wayland proxies, the same untestable-in-this-sandbox glue every prior `main.rs` story has carved out. Verified by build, `clippy -D warnings`, and structural review that each renamed arm's body is otherwise unchanged.

- [x] **Task 3: The two protocol-translation helpers (AC 1, 2, 3)**
  - [x] 3.1 `river_modifiers(mods: &[config::Modifier]) -> Modifiers` — folds a config modifier slice into the protocol's bitfield with `Modifiers::empty()` as the identity. The mapping is not identity-named and that is the whole point of the function existing: `Super` → `Modifiers::Mod4` and `Alt` → `Modifiers::Mod1`, per `river-window-management-v1.xml`'s `modifiers` enum, which names the X11 modifier *slots* rather than the keys conventionally bound to them. `Ctrl` and `Shift` map to their like-named bits. Folding (rather than a fixed two-field struct) is what lets a config declare any combination, including the four-modifier case a user could write.
  - [x] 3.2 `input_event_code(button: config::Button) -> u32` — `Left` → `0x110`, `Right` → `0x111`, `Middle` → `0x112`, per `linux/input-event-codes.h` (`BTN_LEFT`/`BTN_RIGHT`/`BTN_MIDDLE`). These were previously two inline `const`s inside `init_new_seats`; `Middle` is new, because the config schema offers it and `create_pointer_binding` has always been able to take any code.
  - [x] 3.3 Both are free functions at module scope, not `WindowManager` methods — they touch no state, and `init_new_seats` calls them from inside a `self.seats.values_mut()` loop that already holds a mutable borrow of `self` (the same borrow-checker constraint `output_proxy_for_id` documents from Story 2.7).
  - [x] 3.4 No RED/GREEN — each is a total `match` over a closed enum with no branches to get wrong beyond the constant values themselves, which are verified against the protocol XML and the kernel header by review. Wayland-proxy-adjacent glue under the standing carve-out.

- [x] **Task 4: Rewrite `init_new_seats` as a config walk (AC 1, 2, 3, 6)**
  - [x] 4.1 Invert the `if seat.new { … }` body to an early `if !seat.new { continue; }` so the two binding loops sit at one indent level rather than three, then iterate `&self.config.keybinds` and `&self.config.mousebinds`, calling the existing `create_xkb_binding`/`create_pointer_binding` with `river_modifiers(&bind.mods)`, the resolved trigger, and `bind.action.clone()`. `seat.new = false` moves to the end of the (now un-nested) per-seat body — unconditionally, so a seat whose every binding was skipped is still not retried forever.
  - [x] 4.2 Error posture for an unresolvable key: `let Some(keysym) = keybind.keysym() else { eprintln!("Skipping keybind with unresolvable key `{}`", keybind.key); continue; };`. Document in the function's `///` why this is a log-and-skip rather than a panic *or* a hard error: `Config::parse` already rejects an unresolvable key name at load time (Story 3.1's `ConfigError::UnknownKey`, with `rejects_a_key_name_that_is_not_a_known_keysym` covering it), so a `None` here means the config never went through `parse` at all. The only thing in the codebase that can do that is `Config::default`, whose own key names are covered by `keybind_resolves_its_key_name_to_a_keysym` and by the shipped-example test. So this branch is structurally unreachable today and exists purely so a future regression costs the user one binding instead of the whole session (NFR2).
  - [x] 4.3 No RED/GREEN — seat/binding registration against live `river_xkb_bindings_v1` and `river_seat_v1` proxies, the same carve-out as every prior registration-path story (1.7, 2.6, 2.7, 2.8, 2.9, 2.10). Verify by build + `clippy --all-targets -D warnings` + structural review that the default path registers the same eleven bindings with the same keysyms/modifiers as the deleted constant block, one binding at a time.

- [x] **Task 5: Point `Action::Hotkeys` at the generated sheet (AC 4, 5)**
  - [x] 5.1 `HOTKEY_HELP.join("\n")` → `config.hotkey_help().join("\n")` in the `writeln!` to `fuzzel`'s stdin; delete the `HOTKEY_HELP` const and its stale in-sync-by-hand doc comment. `do_action` already takes `config: &Config` for the spawn actions (Story 3.1's plumbing), so no new parameter is needed here. The surrounding "small and fixed, well under the ~64KiB default pipe buffer, so writing it synchronously can't block" comment stays accurate — a generated sheet is bounded by the config's own binding count, which is not a realistic route to 64KiB.

- [x] **Task 6: Full in-container verification gate (AC: all)**
  - [x] 6.1 `podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>`: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pre-commit run --all-files`. Record the net test delta in the Dev Agent Record.
  - [x] 6.2 Manual live-verification note (not automated, not a story blocker — same boundary as every prior story's live-compositor gap): once rebuilt, confirm `Super+Shift+?` renders a correctly-aligned sheet reflecting the running config, and that a config-declared bind actually fires. Record whether this was confirmed live or deferred.

## Technical notes

**Why the built-in binding table moved into `Config::default()` instead of
staying in `main.rs`.** The alternative — keep the eleven hardcoded
`create_xkb_binding` calls as a fallback and only walk the config when
one was loaded — would have left two independent definitions of "the
default keymap," one of which (`main.rs`'s) no test can reach, since
`main.rs`'s seat loop needs a live compositor. Putting the whole table in
`Config::default()` makes the built-in keymap ordinary, inspectable data:
`default_config_keeps_every_built_in_keybind` and
`empty_config_file_is_valid_and_yields_the_defaults` turn "no config file
changes nothing" from a claim in a commit message into an asserted
property. `init_new_seats` is then a single code path with no
default-vs-configured branch to diverge — the case it can't test is the
case it doesn't have.

**Why the protocol translation lives in `main.rs` and not in `config`.**
`river_modifiers` and `input_event_code` are the only two places the
config vocabulary meets protocol types. Putting them in `config` would
mean the config module importing `Modifiers` from the generated
`river-window-management-v1` bindings, coupling a plain-data, fully
unit-tested module to the one part of this crate that can't be tested
without a compositor. Keeping the translation on the `main.rs` side keeps
`config` protocol-agnostic in exactly the way `wm_core` already is.

**`Move` and `Resize` remain bindable to keys, not just buttons.**
`Action` is one flat enum shared by `[[keybind]]` and `[[mousebind]]`, so
nothing stops a user writing `action = "move"` on a keybind (or `action =
"terminal"` on a mousebind). This is harmless and deliberately not
validated against: `do_action`'s `Move`/`Resize` arms act on the seat's
*hovered* window and start a pointer-driven drag, so a key-triggered
`Move` begins the same operation with the pointer wherever it already is
— an odd way to use it, but well-defined, and `SeatOp` already handles a
drag that never receives motion. Rejecting the combination would mean
splitting `Action` into two enums (or adding a per-variant "is this a
pointer action" predicate) to prevent something that costs nothing when
it happens.

**The `?` in `do_action` changes its early-return shape, not its
contract.** `let pending_action = self.pending_action.take()?;` returns
`None` from `do_action` when no binding fired — which is exactly what the
deleted `Action::None => None` arm did, and `manage_seats` already treats
`None` as "this seat produced no tag to spawn a terminal for." No caller
needed changing.

**Scope boundary.** This story does not reload the config on change (the
`WindowManager.config` field's own doc comment records why: re-reading
would mean destroying and recreating every live `river_xkb_binding_v1`
object, which nothing needs yet). It does not sort, group or paginate the
cheat-sheet — lines come out in config order, which is the order the user
wrote them and therefore the order they expect. It does not add a
`Modifiers` bit beyond the four the config schema offers.

## Test plan
1. **`config` unit tests (TDD, RED before GREEN)**: `hotkey_help`'s five cases (Task 1.1) — total line count equals the total binding count, labels name modifiers and trigger, parameterized payloads appear, configured program names appear, mouse buttons render as `…Click`. All pure string formatting over plain data; no Wayland types, no I/O.
2. **Regression gate on Story 3.1's own tests**: `default_config_keeps_every_built_in_keybind`, `empty_config_file_is_valid_and_yields_the_defaults`, `declaring_keybinds_replaces_the_built_in_set` and `the_shipped_example_config_parses` are what actually cover AC 1/2/3 — this story's contribution to those ACs is making `init_new_seats` consume the list those tests assert on, which is the untestable half.
3. **Build/lint gate**: `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` clean with the `Copy`→`Clone` change compiled in — clippy is doing real work here, since every site that relied on `Action` being `Copy` becomes a compile error and must be resolved deliberately rather than silently.
4. **Regression gate**: the rest of the suite (`wm_core`, `ipc`, `status-bar`, `tag-picker`) untouched by this story stays green.
5. **Live gate (manual, not automated)**: per Task 6.2 — the cheat-sheet renders aligned and matches the running config; a config-declared bind fires.

## FR coverage
New scope beyond the original PRD. The PRD's FRs describe *what* the WM
does (tags, pinned terminal, visibility, status bar), never *how a
binding is chosen* — every FR was fully satisfiable with a hardcoded
keymap, which is why the first two epics shipped one. Epic 3 adds
user-configurable input as a new capability rather than closing a gap in
an existing FR. NFR2 (panic-free) is directly exercised: the skip-and-log
path in `init_new_seats` and `hotkey_help`'s `max().unwrap_or(0)` are
both there so an unexpected input costs a line, not the session.

## Dev Agent Record

**What was done**:

- `wm/src/config/mod.rs` (Task 1, real RED/GREEN TDD): the five
  `hotkey_help` tests were written and run first, confirming RED
  in-container with `E0599: no method named hotkey_help found for struct
  config::Config` on every one of them before any implementation existed.
  Then added `Modifier::label`, the free function `binding_label`,
  `Action::description(&self, defaults: &Defaults)` and
  `Config::hotkey_help(&self) -> Vec<String>` in a second `impl Config`
  block placed next to them (rather than folded into the existing
  `parse`/`load` block, keeping the presentation concern visibly separate
  from the loading concern), and confirmed all five GREEN.
- `wm/src/main.rs` (Task 2): deleted the local `Action` enum entirely —
  including its `None` variant and its `Copy` derive — and imported
  `config::Action` via `use config::{Action, Config};`. `Seat.pending_action`
  became `Option<Action>` with a `///` recording that the `Option` exists
  so `.take()` can move a parameterized action's `String` payload out
  without cloning it. Both `Event::Pressed` arms now store
  `Some(binding.action.clone())`; `do_action` opens with
  `self.pending_action.take()?` and its `Action::None => None` arm is
  gone. All five variant renames applied (`SpawnFoot`→`Terminal`,
  `SpawnLauncher`→`Launcher`, `ShowHotkeys`→`Hotkeys`,
  `TagCycle`→`CycleTag`, `OpenTagPicker`→`TagPicker`), including in the
  prose of the comments that named them.
- `wm/src/main.rs` (Task 3): added `river_modifiers` (fold over
  `Modifiers::empty()`, `Super`→`Mod4`, `Alt`→`Mod1`, `Ctrl`/`Shift`
  like-named, per the `modifiers` enum in
  `protocol/river-window-management-v1.xml`) and `input_event_code`
  (`Left`/`Right`/`Middle` → `0x110`/`0x111`/`0x112`, per
  `linux/input-event-codes.h`), both free functions at module scope.
- `wm/src/main.rs` (Task 4): `init_new_seats` lost its entire inline
  `const` block (`SPACE`, `N`, `Q`, `ESC`, `TAB`, `A`, `S`, `R`,
  `QUESTION`, `BTN_LEFT`, `BTN_RIGHT`, and the shared `let mods =
  Modifiers::Mod4`) and its eleven literal registration calls, and now
  walks `self.config.keybinds` then `self.config.mousebinds`. The
  `if seat.new` block was inverted to `if !seat.new { continue; }`. A
  `let … else` guard skips-and-logs an unresolvable key name; the
  function's `///` records why that is unreachable-but-handled.
- `wm/src/main.rs` (Task 5): deleted `const HOTKEY_HELP: &[&str]` and the
  doc comment conceding it was hand-maintained; `Action::Hotkeys`'s
  `writeln!` to fuzzel's stdin now takes `config.hotkey_help().join("\n")`.

**Deviations from the story's literal wording** (flagged explicitly, per
this project's established norm — see Stories 2.7/2.10's own Dev Agent
Records):

1. **The `Middle` button has no built-in binding and therefore no
   registration-path coverage.** Task 3.2 adds `Button::Middle` →
   `0x112` because the schema offers the variant, but `Config::default`
   binds only `Left` and `Right`, so nothing in the default path ever
   evaluates that arm. It is covered indirectly —
   `hotkey_help_labels_mouse_buttons_as_clicks` only checks
   `Super+LeftClick` — meaning a wrong constant for `Middle` would not be
   caught by any test, only by a user binding it. Judged acceptable
   (three constants copied from a kernel header, verified by review), but
   recorded rather than left implicit.
2. **`Action::description` takes `&Defaults` that eleven of its thirteen
   arms ignore.** The alternative — pass only the two strings the spawn
   arms need, or split description into "needs defaults" and "doesn't" —
   would leak which actions happen to be spawn actions into the
   signature, and would need changing again the first time another action
   grows a configurable dependency. Taking the whole `&Defaults`
   uniformly was chosen deliberately; clippy raised no objection.
3. **Task 4.1's `seat.new = false` placement.** The story says
   "unconditionally, so a seat whose every binding was skipped is still
   not retried forever." Implemented exactly so, but worth noting the
   consequence: a seat that fails to register anything is silently left
   with no bindings rather than retried on the next manage sequence. That
   matches the pre-existing behavior (the old code also set `new = false`
   once, inside the same block) and is the correct choice — a retry loop
   would re-log the same skip message every sequence forever.

No other deviations identified; the implementation otherwise matches
Tasks 1-6 as written, including exact function names, signatures and
match-arm shapes.

**Verification** (all inside the devcontainer via `podman exec -u vscode
-w /workspaces/buoy-wm bold_vaughan <cmd>`):
- `cargo fmt --all -- --check`: clean, no drift.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace`: `buoy-wm` (wm) 215, `status-bar` 27,
  `tag-picker` 69 = **311 total, 0 failures, 0 regressions**. Against the
  `6e32d5b` baseline of 277 (`wm` 181 + 27 + 69), Epic 3 adds 34 wm
  tests, of which **this story owns the 5 `hotkey_help` tests** in
  `wm/src/config/mod.rs`; the other 29 belong to Story 3.1 (16 config
  schema/parse tests + 3 `wm_core::tag_id_by_name` tests) and Story 3.2
  (10 `config::keysym` tests).
- `pre-commit run --all-files`: both configured hooks (`cargo fmt
  --check`, `cargo clippy`) passed.

**Task 6.2 (manual live-verification)**: **deferred**, not performed by
this agent — no access to a live `river` compositor, a real seat, or
`fuzzel` in this sandbox, the same boundary as every prior story's
live-compositor gap. Confirming that `Super+Shift+?` renders a correctly
aligned, config-accurate sheet, and that a user-declared bind actually
fires against a real session, still needs a human on the next real boot.

**File List**:
- `wm/src/config/mod.rs` — added `Modifier::label`, `binding_label`,
  `Action::description`, `Config::hotkey_help`; 5 new unit tests.
- `wm/src/main.rs` — deleted the local `Action` enum and `HOTKEY_HELP`;
  added `river_modifiers` and `input_event_code`; rewrote
  `init_new_seats` as a config walk; `Seat.pending_action` →
  `Option<Action>` with `take()?` in `do_action`; `Action::Hotkeys` now
  writes `config.hotkey_help()`.
- `docs/planning/epics/story-3-3.md` — this file.

## Code Review

HIGH-effort workflow-backed review (37 agents: per-angle finders, then an
independent verifier per finding location). Two findings against this
story, both fixed.

1. **`move`/`resize` became bindable to keys, where they start a pointer
   drag with no button held.** The deleted hardcoded table enforced this
   structurally: `Action::Move`/`Resize` were only ever passed to
   `create_pointer_binding`, so the invariant needed no check. Collapsing
   `main.rs`'s `Action` and the config's into one shared enum removed that
   structure without replacing it, and the README listed `move`/`resize`
   among plain actions with no restriction. Bound to a key,
   `Action::Move` calls `pointer_move` → `op_start_pointer` with no button
   down, and the protocol only ends an op when all buttons are *released* —
   so the hovered window latches to the cursor and drags with every motion
   until the user happens to click. **Fixed**: `Action::is_pointer_only`
   plus a `Config::parse` rejection (`ConfigError::PointerOnlyAction`)
   naming the action and pointing at `[[mousebind]]`; README and example
   updated. This is the general lesson of the story — an invariant that
   used to hold by construction has to become an explicit check once the
   table is user-supplied.
2. **The generated cheat-sheet kept a synchronous stdin write whose
   justification it had deleted.** The comment above the write asserted
   `HOTKEY_HELP` is "small and fixed (well under the ~64KiB default pipe
   buffer)", which is exactly why writing it inline could not block — and
   that constant no longer exists. `hotkey_help()` generates one line per
   binding from user data, with no bound, so a large enough config could
   fill the pipe and block the WM's only thread until fuzzel drained it,
   freezing all window management. **Fixed**: the write moved to a thread,
   matching what `tag-picker`'s own `run_fuzzel` already does for its
   arbitrarily-long checklist, and the stale comment replaced with the real
   reasoning. Also documented there why `fuzzel` is deliberately *not*
   `defaults.launcher` at that call site: it is driven as a dmenu-style
   pager with fuzzel-specific flags, not as the user's chosen launcher.

**Re-verification after fixes**: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `pre-commit run
--all-files` all clean; `cargo test --workspace` — 322 total (`buoy-wm`
226, `status-bar` 27, `tag-picker` 69), 0 failures.

**Code review: PASS**.
