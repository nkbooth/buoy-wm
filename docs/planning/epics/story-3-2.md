---
baseline_commit: 6e32d5b
---

# Story 3.2: Keysym name resolution without libxkbcommon

Epic: 3 | Priority: H | Status: done

## Description
A user-editable keymap needs a way to turn a *string* into a *keysym*. The
config file's whole point is that a user writes

```toml
[[keybind]]
mod = ["Super"]
key = "Return"
action = "terminal"
```

and gets the binding they asked for. But `river_xkb_bindings_v1`'s
`create_binding` request does not take a name — it takes an X11 keysym, a
bare `u32`. Something has to sit between `"Return"` and `0xff0d`, and until
this story nothing did.

Before the config module existed, that gap was closed by not having it:
every keysym this WM could bind was a hand-written hex constant inlined at
the single call site in `main.rs`:

```rust
const SPACE: u32 = 0x20;
const N: u32 = 0x6e;
const Q: u32 = 0x71;
const ESC: u32 = 0xff1b;
const TAB: u32 = 0xff09;
...
const QUESTION: u32 = 0x3f;
```

That is a perfectly reasonable shape for a fixed, compile-time keymap — the
set of bindings was closed, so the set of keysyms was closed too, and a
`const` per binding is about as simple as it gets. It stops working the
moment the keymap becomes *data*. A config file can name any key, so the
resolver has to accept names that were never enumerated at compile time,
and it has to reject the ones it genuinely cannot bind rather than guess.

This story is that resolver: `wm/src/config/keysym.rs`, a single public
function `from_name(&str) -> Option<u32>`, plus the two call sites that
make it load-bearing — `Keybind::keysym()` (the accessor `main.rs`'
binding-registration path actually calls) and `Config::parse`'s up-front
rejection of any `key = "..."` that does not resolve.

### Why not libxkbcommon
`xkb_keysym_from_name` is the canonical implementation of exactly this
function. It is the *right* answer in the abstract, and it was the first
thing considered. It was rejected for build-environment reasons, not
correctness ones:

- The `xkbcommon` Rust crate is a binding to the C library, not a
  reimplementation. Using it means `libxkbcommon` and its development
  headers must be present wherever this crate is compiled.
- They are not present in this project's devcontainer. Verified directly
  rather than assumed: `pkg-config --modversion xkbcommon` fails
  (`Package xkbcommon was not found in the pkg-config search path`), and
  `/usr/include/xkbcommon` does not exist (`ls: cannot access
  '/usr/include/xkbcommon': No such file or directory`).
- Making them present is not a per-project change. Per this project's
  host rules, the devcontainer image and the host are both built
  artifacts — installing a system package to satisfy one crate is an
  OS-image change made in the image definition, not something a story
  does on the side.

So the trade was: put a C library, a `pkg-config` probe, and a native link
step on this crate's build path — for *one string lookup* — or write the
lookup. The lookup is ~30 lines. The C dependency was rejected.

This is only a comfortable trade because the keysym space this WM actually
needs turns out to be almost entirely algorithmic. `libxkbcommon`'s table
is exhaustive across every keysym X11 has ever defined, including the
Unicode range, dead keys, and vendor-specific blocks. None of that is
reachable from a `key = "..."` in this config file — the resolver only has
to cover what a person plausibly binds a window-manager action to.

### The three cases
`from_name` tries three things, in order, and a table is only needed for
the third-of-a-case that is left over:

1. **A single ASCII graphic character IS its own keysym.** This is not a
   coincidence or an approximation — the X11 keysym values in `0x20..0x7e`
   are defined to be the Latin-1/ASCII code points. `a` is `0x61`, `A` is
   `0x41`, `1` is `0x31`, `?` is `0x3f`, `/` is `0x2f`. One `char as u32`
   handles the overwhelming bulk of real bindings, with no table at all.
2. **Function keys are contiguous.** `F1` is `0xffbe`, and `F2`..`F35`
   follow it one at a time, so `Fn` is `F1 + n - 1`. That is 35 entries
   replaced by one constant and an addition. The range genuinely stops at
   `F35` (`0xffe0`) — `F36` would be `0xffe1`, which is `Shift_L`. Running
   past the end of the block does not produce a nonexistent function key,
   it produces a *modifier*, which is why `MAX_FUNCTION_KEY` is a real
   bound and not defensive padding.
3. **Only genuinely irregular names need a table.** What survives cases 1
   and 2 is the short list of keys whose name has no arithmetic
   relationship to their value: `space`, `Return`, `Escape`, `Tab`,
   `BackSpace`, `Delete`, `Insert`, `Home`, `End`, `Page_Up`, `Page_Down`,
   the four arrows, `Print`, `Pause`, `Menu`. Eighteen entries, values
   taken from `xkbcommon/xkbcommon-keysyms.h` — the same header the old
   `main.rs` constants cited, so the numbers have a single provenance
   before and after this story.

### The order dependency
The function-key check runs **before** the single-character check, and the
comment saying so is load-bearing. `"F"` is a legitimate binding — it is
the letter F, keysym `0x46`. If the single-character case ran first it
would claim `"F"` and never reach `function_key`, which is harmless; the
real hazard is the reverse reading, that `function_key` might claim `"F"`
and return something wrong. It does not: `strip_prefix('F')` leaves `""`,
`"".parse::<u32>()` fails, `function_key` returns `None`, and `"F"` falls
through to `single_ascii` and binds the letter. The ordering is what makes
that fallthrough the *documented* behavior rather than an accident of
which arm happened to be written first — `"F1"` is a function key, `"F"`
is a letter, and both are correct under exactly this order.

### The case-sensitivity decision
This is the subtlest thing in the module, and it was not designed
up-front — it was forced by a real defect found mid-story.

The naive rule ("match names exactly, like X11 does") is wrong in
practice. X11 spells the space keysym `space`, lowercase. A person editing
a config file writes `Space`, because every other special key in the file
is capitalized (`Return`, `Escape`, `Tab`) and because `Space` is how the
key is spelled in English. The project's own `docs/config.example.toml`
does exactly this — its first example binding is `key = "Space"`. Under
exact matching, the example config shipped alongside the feature would be
rejected by the feature, with `UnknownKey("Space")`.

That is not hypothetical: it is the failure that was actually observed
(see Task 3), and the fix is that **`NAMED` is matched
case-insensitively** via `eq_ignore_ascii_case`. `Space`, `space`,
`RETURN`, `page_up` all resolve. Rejecting them would be pedantry — there
is no second key named `Space` that the user might have meant instead.

**Single characters stay case-SENSITIVE**, and this is not an
inconsistency. `a` (`0x61`) and `A` (`0x41`) are genuinely different
keysyms, corresponding to genuinely different physical inputs (`A` is what
the layout produces *with Shift held*). Case-folding them would silently
change which chord fires. The built-in `Mod4+Shift+?` binding depends on
this distinction directly — `?` is a shifted symbol, and the binding has
to match the shifted keysym the layout actually produces.

The two rules coexist safely for a specific structural reason, stated in
the doc comment so it survives future edits: **no `NAMED` entry is a
single character.** The shortest is `Tab`, at three. A single-character
name therefore can never match a `NAMED` entry, case-insensitively or
otherwise, so it never reaches the case-insensitive lookup at all and
always falls through to the case-sensitive `single_ascii` path. Adding a
one-character `NAMED` entry would break this invariant — hence the
comment, not just the behavior.

Function-key prefixes accept both `F` and `f` (`strip_prefix('F')
.or_else(|| name.strip_prefix('f'))`), for the same
don't-be-pedantic reason as `NAMED`. `f1` and `F1` are the same key.

### Deliberate strictness
`from_name` returns `Option`, and the `None` is meant to be acted on, not
swallowed. `Config::parse` walks every declared keybind and returns
`ConfigError::UnknownKey(name)` on the first one that will not resolve,
*before* building a `Config` at all. The reasoning is in the code: a typo
reported once at startup, naming the offending key, is strictly better
than a binding that registers cleanly and then silently never fires — the
latter is close to undiagnosable from the user's side, because nothing
distinguishes "the binding is broken" from "I pressed the wrong keys".

Non-ASCII input is rejected on the same principle rather than guessed at.
`é` and `→` have keysyms in X11's Unicode range (`0x01000000 + code
point`), but that range is deliberately out of scope for v1 — nothing in
this WM's binding set needs it, and half-supporting it (mapping some
characters and not others) would be worse than a clean refusal. `é` is
also a useful shape to reject explicitly: it is a single `char` but not a
single *byte*, so a naive "is this one character" check that forgot
`is_ascii_graphic` would happily return `0xe9`, which is a Latin-1 keysym
this WM has no business binding via this path.

## Acceptance criteria
**Given** a config `key` naming a single ASCII graphic character
**When** it is resolved
**Then** the keysym is that character's own code point — `a` → `0x61`,
`z` → `0x7a`, `A` → `0x41`, `1` → `0x31`, `?` → `0x3f`, `/` → `0x2f` — with
no table lookup involved

**Given** two config `key` names differing only in case, both single
characters
**When** both are resolved
**Then** they resolve to *different* keysyms — `a` and `A` are not the
same key, and the resolver must not fold them together

**Given** a config `key` naming a function key in `F1`..`F35`
**When** it is resolved
**Then** the keysym is computed as `0xffbe + n - 1` — `F1` → `0xffbe`,
`F12` → `0xffc9`, `F35` → `0xffe0` — without a 35-entry table

**Given** a config `key` naming a function key outside the real range
**When** it is resolved
**Then** it is rejected — `F0` (no such key), `F36` (would be `0xffe1`,
`Shift_L`), `F99` (far past the block) all return `None` rather than
producing a modifier or an out-of-range value

**Given** the bare config `key` name `"F"`
**When** it is resolved
**Then** it resolves to the *letter* F (`0x46`) via the single-character
case, because the function-key check declines a name with no number after
the prefix and the ordering lets it fall through

**Given** a config `key` naming an irregular special key
**When** it is resolved
**Then** the `NAMED` table supplies the value — `Return` → `0xff0d`,
`Escape` → `0xff1b`, `Tab` → `0xff09`, `space` → `0x20`, `BackSpace` →
`0xff08`, `Delete` → `0xffff`, and the arrows/navigation block `Left` →
`0xff51` through `Page_Down` → `0xff56`

**Given** a `NAMED` key written with the capitalization a config author
would naturally use rather than X11's own spelling
**When** it is resolved
**Then** it still resolves — `Space` → `0x20` (X11 spells it `space`),
`RETURN` → `0xff0d`, `page_up` → `0xff55` — because `NAMED` is matched
case-insensitively

**Given** a function-key name written with a lowercase prefix
**When** it is resolved
**Then** it still resolves — `f1` → `0xffbe`, same as `F1`

**Given** a config `key` name that matches none of the three cases
**When** it is resolved
**Then** it is rejected — a typo (`Retrun`), the empty string (`""`), and
a plausible-looking invention (`NotAKey`) all return `None`

**Given** a config `key` naming a non-ASCII character
**When** it is resolved
**Then** it is rejected — `é` and `→` return `None`, because the Unicode
keysym range is out of scope and a silently-never-firing binding is worse
than a refusal

**Given** a config file declaring a `[[keybind]]` whose `key` does not
resolve
**When** `Config::parse` runs
**Then** it returns `ConfigError::UnknownKey` carrying the offending name,
before constructing a `Config` — the failure is reported at load, naming
the typo, never deferred to a binding that never fires

**Given** every key name used by `Config::default()`'s built-in bindings
**When** each is resolved
**Then** all of them resolve — the built-in keymap can never ship a
binding its own resolver would reject

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: RED — the full resolution contract as tests, before any implementation (AC: all)**
  - [x] 1.1 Create `wm/src/config/keysym.rs` containing *only* a `#[cfg(test)] mod tests` and `use super::*;`. Write the case-by-case tests first: `single_ascii_characters_are_their_own_keysym` (the `a`/`z`/`A`/`1`/`?`/`/` set, deliberately including both a letter pair differing in case and two punctuation marks, since punctuation is where a naive "is this alphanumeric" check would fail), `named_special_keys_resolve` (`Return`, `Escape`, `Tab`, `space`, `BackSpace`, `Delete`), `arrow_and_navigation_keys_resolve` (all four arrows plus `Home`/`End`/`Page_Up`/`Page_Down`), `function_keys_are_computed_from_their_number` (`F1`, `F12`, `F35` — the two endpoints of the contiguous block plus one interior value, so an off-by-one in the arithmetic cannot pass), `function_keys_past_the_real_range_are_rejected` (`F0`, `F36`, `F99`), `unknown_names_are_rejected` (`Retrun`, `""`, `NotAKey`), `non_ascii_characters_are_rejected` (`é`, `→`).
  - [x] 1.2 Confirm RED, and confirm it is the *right* RED: `cargo test -p buoy-wm keysym` in-container must fail to compile with `cannot find function `from_name` in this scope` on every assertion — not a failing assertion against a stub, not a missing-module error. The tests must be exercising a function that genuinely does not exist yet.

- [x] **Task 2: GREEN — implement `from_name` and its two private helpers (AC: all except the case-insensitivity criteria, added in Task 3)**
  - [x] 2.1 Add `const NAMED: &[(&str, u32)]` with the eighteen irregular entries, values transcribed from `xkbcommon/xkbcommon-keysyms.h` — the same header `main.rs`' pre-config `const SPACE: u32 = 0x20;`-style constants cited, so the numbers keep one provenance across the change. `///` doc comment stating that these are the keysyms with *no algorithmic relationship to their name*, which is the entry criterion for the table.
  - [x] 2.2 Add `const F1: u32 = 0xffbe;` and `const MAX_FUNCTION_KEY: u32 = 35;`, each with a `///` explaining *why* it is a constant rather than a literal: `F1` because `F2`..`F35` follow contiguously (which is what makes the table unnecessary), `MAX_FUNCTION_KEY` because `F36` would collide with the modifier keysyms starting at `0xffe1`.
  - [x] 2.3 Implement `pub fn from_name(name: &str) -> Option<u32>` as three ordered attempts: `NAMED` lookup, `function_key(name)`, `single_ascii(name)`. `///` doc comment covering the three cases, the ordering rationale (a bare `"F"` must fall through to the letter), and the deliberate strictness (an unresolvable name is a config error the caller reports at load, never a silently-dead binding).
  - [x] 2.4 Implement `fn function_key(name: &str) -> Option<u32>` — strip the `F` prefix, `parse::<u32>()`, and return `F1 + number - 1` only if `(1..=MAX_FUNCTION_KEY).contains(&number)`. Private, so no docstring per this project's convention (public API gets intellisense blurbs; private internals do not).
  - [x] 2.5 Implement `fn single_ascii(name: &str) -> Option<u32>` — take the first `char`, reject if there is a second one *or* if the first is not `is_ascii_graphic()`, otherwise `first as u32`. The `is_ascii_graphic` check is what rejects `é` (one `char`, not one ASCII byte) and is not incidental.
  - [x] 2.6 Declare `pub mod keysym;` in `wm/src/config/mod.rs`. Confirm every Task 1 test is GREEN.

- [x] **Task 3: RED→GREEN again — case-insensitivity, driven by a real failure (AC: the `Space`/`RETURN`/`page_up` and `f1` criteria)**
  - [x] 3.1 Write `named_keys_are_case_insensitive_but_single_characters_are_not` and `function_key_names_are_case_insensitive` *after* Task 2 was already GREEN, prompted by `docs/config.example.toml`'s first binding being `key = "Space"` — the natural spelling, not X11's `space`.
  - [x] 3.2 Observe the genuine failure: `from_name("Space")` returned `None`, which `Config::parse` turns into `UnknownKey("Space")`. The example config shipped with the feature would have been rejected by the feature. This is a real mid-story RED, not a test written to describe code that already passed.
  - [x] 3.3 GREEN — widen the `NAMED` lookup from an exact match to `eq_ignore_ascii_case`, and accept both `F` and `f` as the function-key prefix. Deliberately do **not** touch `single_ascii`: `a` and `A` must stay distinct. Assert that distinction in the same test (`assert_ne!(from_name("a"), from_name("A"))`) so the two halves of the decision are locked together and cannot drift apart in a later edit.
  - [x] 3.4 Document the safety argument in `from_name`'s doc comment, not just the behavior: the mixed rule is sound *because no `NAMED` entry is a single character*, so single characters never reach the case-insensitive lookup. This is an invariant a future contributor could break by adding a one-character entry, so it is written down rather than left to be rediscovered.

- [x] **Task 4: Wire the resolver into the config type and its load-time validation (AC: the `UnknownKey` criterion)**
  - [x] 4.1 Add `Keybind::keysym(&self) -> Option<u32>` in `wm/src/config/mod.rs`, a thin delegation to `keysym::from_name(&self.key)`. `///` noting that `Config::parse` rejects the unresolvable case at load, so a `Keybind` that reaches the WM's binding-registration path always resolves — the `Option` is for the parser, not for every downstream caller to re-handle.
  - [x] 4.2 In `Config::parse`, before assembling the `Config`, iterate `raw.keybinds` and return `ConfigError::UnknownKey(keybind.key.clone())` on the first that fails to resolve. Carry the *name*, not just a flag, so the error message can point at the actual typo. Inline comment explaining the placement: rejected here rather than at binding-registration time so a typo is reported once, at startup, instead of becoming a binding that silently never fires.
  - [x] 4.3 Add the two `config::tests` cases that cover the seam rather than the resolver: `keybind_resolves_its_key_name_to_a_keysym` (a parsed `key = "Return"` yields `0xff0d` through `Keybind::keysym`) and `rejects_a_key_name_that_is_not_a_known_keysym` (a `key = "Retrun"` config is an error whose `Display` output contains the string `Retrun`, proving the name survives into the message).

- [x] **Task 5: Guard the built-in defaults against their own resolver (AC: the final criterion)**
  - [x] 5.1 Add `every_name_used_by_the_built_in_defaults_resolves`, iterating the exact names `Config::default()` binds — `["space", "q", "n", "Escape", "Tab", "a", "s", "r", "?"]` — and asserting each resolves, with the failing name interpolated into the assertion message so a regression names itself.
  - [x] 5.2 Rationale recorded as a `///` on the test: this is not redundant with the per-case tests. Those prove the resolver handles each *category*; this proves the specific set `Config::default()` actually ships is a subset of what it accepts. Without it, adding a built-in binding on a key the resolver rejects would produce a `Config::default()` that fails its own validation — the one config that must always be loadable.

- [x] **Task 6: Full in-container verification gate (AC: all)**
  - [x] 6.1 Inside the devcontainer (`podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>`): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pre-commit run --all-files`. All clean.
  - [x] 6.2 Verify the libxkbcommon claim empirically rather than assuming it — `pkg-config --modversion xkbcommon` and `ls /usr/include/xkbcommon` in the same container, both recorded in the Dev Agent Record. The "why not the C library" argument is only worth writing down if it was actually checked.

## Technical notes

**Why a hand-written resolver is acceptable here and would not be
elsewhere.** Reimplementing a well-tested upstream library is normally a
bad trade. It is acceptable in this specific case because the *input
domain* is not "every keysym X11 defines" — it is "every string a person
writes in `key = "..."` for a window-manager binding". That domain is
small, and two thirds of it is arithmetic rather than data. The eighteen
`NAMED` entries are the entire non-algorithmic remainder. If this module
ever needed dead keys, vendor keysyms, or the Unicode range, the
calculation flips and the C dependency becomes worth its build cost — but
those are not reachable from this config file, and building for them would
be speculative.

**Why `Option`, not `Result`, at the resolver boundary.** `from_name` has
exactly one failure mode — the name is not one this WM can bind — and the
caller already knows the name it passed in. A `Result<u32, E>` would carry
an error type whose only content is information `Config::parse` already
has, and `parse` would immediately discard it to build `UnknownKey` from
`keybind.key` anyway. The `Option` is converted to a named, reportable
error at exactly the layer that has the surrounding context (which
`[[keybind]]`, which file) to report it usefully.

**Why the `NAMED` values cite a header this crate does not include.** The
`xkbcommon/xkbcommon-keysyms.h` reference in the doc comment is a
provenance note, not a build dependency. It matters because the same
numbers previously lived in `main.rs` as `const ESC: u32 = 0xff1b;`-style
literals citing the same header — keeping the citation means the values
have one traceable origin across the refactor, and a future correction has
somewhere to check against.

**Why `F36` is rejected rather than clamped.** Clamping (`F36` → `F35`)
would be the friendlier-looking choice and is wrong. A config that says
`F36` is a config with a mistake in it; binding it to `F35` would hand the
user a working binding on a key they did not ask for, which is harder to
notice than an error at startup. Rejecting also protects the more serious
case: the value past the end of the block is `0xffe1` (`Shift_L`), so an
unbounded implementation would let a config bind an *action* to a bare
modifier keysym.

**Scope boundary.** This story is the name→keysym mapping only. It does not
map modifier names (`Super`, `Shift`) — those are a `Modifier` enum
deserialized by `serde`, a separate concern with a closed set. It does not
map pointer buttons (`Button::Left`/`Right` → `0x110`/`0x111`), also a
closed enum. It does not touch the `river_xkb_bindings_v1` registration
call itself; it only supplies the `u32` that call already took. It does
not support keysym-by-number (`key = "0xff0d"`) — nobody has asked, and
adding it would create two spellings for every key.

## Test plan
1. **`keysym` unit tests (TDD, RED before GREEN)**: ten tests covering all
   three resolution cases and every rejection — single ASCII (including a
   case pair and punctuation), `NAMED` specials, the arrow/navigation
   block, computed function keys at both endpoints and an interior value,
   out-of-range function keys, unknown names, non-ASCII, plus the two
   case-insensitivity tests added in the Task 3 cycle and the built-in
   defaults guard. Fully testable — a pure `&str` → `Option<u32>` function
   with no I/O, no Wayland types, and no environment dependence.
2. **`config` seam tests**: `keybind_resolves_its_key_name_to_a_keysym`
   and `rejects_a_key_name_that_is_not_a_known_keysym` cover the wiring
   (`Keybind::keysym`, `Config::parse`'s rejection and its error message
   carrying the offending name) rather than re-testing the resolver
   through a second layer.
3. **Defaults guard**: `every_name_used_by_the_built_in_defaults_resolves`
   pins the invariant that `Config::default()` is always loadable by its
   own validation.
4. **Build/lint gate**: `cargo fmt --all -- --check`, `cargo clippy
   --workspace --all-targets -- -D warnings` clean.
5. **Environment gate**: `pkg-config --modversion xkbcommon` and
   `ls /usr/include/xkbcommon` run in the devcontainer to substantiate the
   "no libxkbcommon on the build path" premise rather than assert it.
6. **Regression gate**: `status-bar` and `tag-picker` suites untouched —
   this module is entirely `wm`-local and adds no shared types.

## FR coverage
New scope beyond the original PRD. The PRD and Epic 1/2 assumed a fixed,
compile-time keymap — every FR describing a binding (`Mod4+Space` for a
terminal, `Mod4+A` for the tag picker, and so on) names the chord as a
constant of the design, and the implementation matched, with a hex `const`
per binding in `main.rs`. Epic 3 makes the keymap user-editable, which
turns "which keysym is this" from a compile-time fact into a runtime
parsing problem. This story supplies that missing piece; it does not
change the behavior of any existing FR's default binding, which is exactly
what `every_name_used_by_the_built_in_defaults_resolves` and
`Config::default()`'s "running with no config file changes nothing"
contract exist to guarantee.

## Dev Agent Record

**What was done**:

- Rejected `libxkbcommon` after verifying the premise in-container rather
  than assuming it: `pkg-config --modversion xkbcommon` →
  `Package xkbcommon was not found in the pkg-config search path` /
  `Package 'xkbcommon', required by 'virtual:world', not found` (exit 1),
  and `ls /usr/include/xkbcommon` → `No such file or directory` (exit 2).
  The `xkbcommon` Rust crate is a binding to the C library, so both would
  have had to be added to the image. Per the project's host rules that is
  an OS-image change, not a per-project one — and not a proportionate cost
  for a single string lookup.
- `wm/src/config/keysym.rs` (Tasks 1-3, real RED/GREEN TDD): wrote the
  test module first, with no implementation in the file at all, and
  confirmed RED as a compile failure — `cannot find function `from_name`
  in this scope` — rather than a stub returning `None`. Then implemented
  `NAMED` (18 entries, values from `xkbcommon/xkbcommon-keysyms.h`),
  `F1 = 0xffbe`, `MAX_FUNCTION_KEY = 35`, `from_name`, and the private
  `function_key` / `single_ascii` helpers, and confirmed GREEN.
- **Second, genuine RED→GREEN cycle mid-story** (Task 3): with the module
  already GREEN, cross-checking against `docs/config.example.toml` showed
  its first binding is `key = "Space"` — the natural spelling, not X11's
  `space`. Added `named_keys_are_case_insensitive_but_single_characters_
  are_not` and `function_key_names_are_case_insensitive`, observed the
  real failure (`from_name("Space")` → `None`, which `Config::parse`
  surfaces as `UnknownKey("Space")` — the shipped example config would
  have been rejected by the feature shipped alongside it), then widened
  the `NAMED` lookup to `eq_ignore_ascii_case` and accepted both `F` and
  `f` as the function-key prefix. This was a real defect caught by a real
  failing test, not a test retrofitted onto passing code.
- `single_ascii` was deliberately left case-**sensitive** in that same
  change. `assert_ne!(from_name("a"), from_name("A"))` lives inside the
  case-insensitivity test specifically so the two halves of the decision
  cannot drift apart — anyone relaxing single-character matching has to
  delete an assertion that says, in the same test, why they shouldn't.
  The safety argument (no `NAMED` entry is a single character, so single
  characters never reach the case-insensitive lookup) was written into
  `from_name`'s doc comment rather than left implicit, since it is an
  invariant a future one-character `NAMED` entry would silently break.
- `wm/src/config/mod.rs` (Task 4): `pub mod keysym;`; `Keybind::keysym()`
  delegating to `keysym::from_name`, documented as always-`Some` for any
  `Keybind` that reaches the WM because `Config::parse` already rejected
  the alternative; `Config::parse`'s pre-assembly loop returning
  `ConfigError::UnknownKey(keybind.key.clone())`, with the inline comment
  explaining why validation lives at load rather than at
  binding-registration time; and the two seam tests
  (`keybind_resolves_its_key_name_to_a_keysym`,
  `rejects_a_key_name_that_is_not_a_known_keysym`).
- `every_name_used_by_the_built_in_defaults_resolves` (Task 5) added as a
  standing guard over `Config::default()`'s own key names
  (`space`, `q`, `n`, `Escape`, `Tab`, `a`, `s`, `r`, `?`), with the
  failing name interpolated into the assertion message. Its `///` records
  why it is not redundant with the per-case tests: those prove each
  *category* resolves, this proves the specific set the built-ins ship is
  a subset of what the resolver accepts, so `Config::default()` can never
  fail its own validation.

**Notes on choices worth flagging**:

1. **`?` is in the built-in defaults and is a shifted symbol.** The
   `Mod4+Shift+?` cheat-sheet binding resolves through `single_ascii` to
   `0x3f`, which is the *shifted* keysym the layout actually produces —
   there is no unshifted `?` keysym to bind instead. This is the concrete
   case that makes single-character case-sensitivity non-negotiable, and
   it is called out in `Config::default()`'s own inline comment as the
   only built-in needing a second modifier.
2. **`F0` is rejected by the range check, not by the parse.** `"F0"`
   parses fine as the number `0`; it is `(1..=MAX_FUNCTION_KEY)
   .contains(&0)` that refuses it. Tested explicitly alongside `F36` and
   `F99` so the lower bound is covered as deliberately as the upper one.
3. **No `Result` at the resolver boundary.** `from_name` returns
   `Option<u32>` and `Config::parse` converts it to a named error; see
   the Technical notes for why an error type here would only carry
   information the caller already has.

**Verification** (all inside the devcontainer via
`podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>`):
- `cargo fmt --all -- --check`: clean, no drift.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace`: `buoy-wm` (wm) 215, `status-bar` 27,
  `tag-picker` 69 = **311 total, 0 failures**. This story's contribution
  is the 10 `config::keysym::tests` cases plus the 2 `config::tests` seam
  cases (`keybind_resolves_its_key_name_to_a_keysym`,
  `rejects_a_key_name_that_is_not_a_known_keysym`); `status-bar` and
  `tag-picker` are untouched, confirming the change is `wm`-local.
- `pre-commit run --all-files`: both configured hooks (`cargo fmt
  --check`, `cargo clippy`) passed.

**Live confirmation**: not separately required for this story. The
resolver is a pure function with no compositor, socket, or process
dependency, and the one behavior a live session could add — that
`Config::default()`'s bindings still register — is covered by
`every_name_used_by_the_built_in_defaults_resolves` plus the unchanged
built-in keymap. Live verification of the *config feature as a whole*
(editing `~/.config/buoy/config.toml` and seeing a rebound key take
effect) belongs to Epic 3's config-loading story, not to this one.

**File List**:
- `wm/src/config/keysym.rs` (new)
- `wm/src/config/mod.rs`

## Code Review

HIGH-effort workflow-backed review (37 agents: per-angle finders, then an
independent verifier per finding location). No findings against this
module's own resolution logic — the ordered three-case strategy, the
`F`-before-single-char precedence, and the mixed case rule all verified as
described.

One finding lands adjacent to it and is recorded in full under Story 3.1,
since the fix is in the parser and the shipped example rather than here:
the example config bound six actions to bare **uppercase** letters with
`Super` alone. An uppercase letter *is* the shifted keysym, so those
bindings could never fire. This module resolved those names entirely
correctly — `"Q"` really is 0x51 — which is precisely why the defect was
invisible: `from_name` is deliberately strict about names it cannot
resolve, but a name that resolves to the *wrong keysym for the modifier
set it is paired with* is not a keysym question. The validation added for
it (`ConfigError::ShiftedKeyWithoutShift`) therefore lives in
`Config::parse`, where both halves of a binding are visible at once.

Worth noting for future work on this file: the case rule documented here —
`NAMED` case-insensitive, single characters case-sensitive — is load-
bearing for that validation. It is what makes "single ASCII uppercase
letter" a reliable signal that Shift is required, and it is why the rule
was scoped to letters rather than extended to `?`/`!`, whose shifted-ness
is layout-dependent.

**Code review: PASS**.
