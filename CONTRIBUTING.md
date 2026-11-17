# Contributing

`buoy` is a personal daily driver first and a shared project second. Bug
reports and patches are welcome; large feature proposals may well be turned
down as out of scope. The [README](README.md) lists what `buoy`
deliberately does not do — those are decisions, not gaps.

## Before you start

**The license matters here more than usual.** `buoy` is under the
[Reciprocal Public License 1.5](LICENSE.md). Under §6.0, anything you
contribute is governed by the same license, and you grant the licensor and
all third parties the rights described there. If that is not acceptable to
you, please open an issue instead of a pull request — a described bug is
still genuinely useful.

RPL 1.5 also requires (§6.4(b)) that every source file carry the license
notice. New files need the same header block as their neighbours; copy one.
The `licence-header` pre-commit hook and CI's `hygiene` job both fail if you
forget.

## Building

Rust 1.93 or newer and `libwayland-dev` are the only build dependencies; the
floor follows the oldest release the README supports, for the reasons
`docs/PACKAGING.md` "Toolchain floor" gives. `.devcontainer/` provides both
if you would rather not install them.

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

`pre-commit run --all-files` runs the last three, plus the licence-header
and gitleaks checks. The hooks are deliberately not `pre-commit install`ed —
they need a toolchain the host may not have, so the git shim would fail
closed on every commit.

Clippy also enforces size and complexity: `too_many_lines`,
`cognitive_complexity`, `too_many_arguments` and `type_complexity`, with the
thresholds in `clippy.toml`. None of these are on by clippy's defaults, so
before they were declared nothing in the pipeline could flag an oversized
function — one had reached 365 lines. If a change trips one, split the
function; an `#[allow]` needs a comment saying why splitting it would make
the code worse, like the three already in the tree.

Running CI locally with `act` needs a container engine, which the default
devcontainer deliberately does not expose. Select the opt-in variant for
that session only:

```sh
devpod up . --devcontainer-path .devcontainer/act/devcontainer.json
```

## What patches are expected to include

**A failing test first.** Every piece of logic in `wm_core`, `config`, and
`ipc` was written test-first, and that is the reason the tag model can be
refactored at all. Write the test, watch it fail, then make it pass. Pull
requests that add logic without tests will be asked for them.

The one standing exception is Wayland `Dispatch` glue and the socket setup
around it — code whose only behaviour is talking to something that does not
exist in a test harness. Keep those layers thin and push every decision they
make down into something testable; that is why `bar_line.rs`, `mode.rs` and
`session.rs` exist as separate modules from their `main.rs`, and why the
WM's own `compositor/` splits the drag arithmetic, the config-to-protocol
tables and the argv builders out of the `Dispatch` impls that use them.

The exception is narrower than it used to be. Every crate is a library plus a
thin `[[bin]]`, so there are three tiers, and a new test belongs in the
highest one that can hold it:

- **Unit tests** beside the code, in `#[cfg(test)] mod tests`. Where almost
  everything belongs.
- **Doctests** on public items — `cargo test` runs them, so a `///` example
  is compiled code and not prose.
- **`tests/`**, which can `use` the library and can run the real binaries via
  Cargo's `CARGO_BIN_EXE_<name>`. This is where anything crossing a process
  boundary goes: `buoy-status-bar`'s stdout contract with waybar, the
  picker's `fuzzel` argv and request sequence (stub the launcher by putting an
  executable named `fuzzel` first on `PATH`), and the wire compatibility
  between `protocol.rs` and the two satellites' `wire.rs`.

**Docstrings on public items.** Every public function, method, type, and
module gets a `///` or `//!` blurb. Private internals do not — the noise
costs more than it explains.

**Comments that say why.** Names and structure carry the *what*. Reserve
comments for the hidden constraint, the non-obvious invariant, or the
workaround for specific upstream behaviour. There are a lot of those in this
codebase already; they are the most valuable thing in it.

### The coverage floor

CI's `check` job fails when line coverage drops below **95%** across the
workspace minus a short list of files (the `--ignore-filename-regex` of the
step "cargo llvm-cov (floor below the composition roots)"). A file is on
that list when what is left in it is mostly glue a test cannot drive: code
that needs a live river, a real child process or the process's own exit.
Everything else is gated, including the parts of `compositor/` that hold
pure logic beside their proxies. Two exempt files still hold a little
tested code (see below); they stay exempt because gating them would take
the workspace below the floor, not because nothing in them can be tested.

The exempt files, and why:

- `wm/src/main.rs` and `buoy-tag-picker/src/main.rs`: composition roots.
  They connect, wire the pieces together and exit; the integration tier
  runs the binaries instead.
- `wm/src/compositor/dispatch.rs` and `wm/src/compositor/river.rs`: the
  `Dispatch` impls and the generated protocol bindings. Only river sends
  these events.
- `wm/src/compositor/manager.rs`, `wm/src/compositor/seat.rs` and
  `wm/src/compositor/libinput.rs`: the records that hold live protocol
  proxies, and the manage/render sequences that act on them. Their
  decisions are pushed down into `WmCore` and the pure helpers, which are
  gated. Splitting the manager's and the seat's records into proxy-free
  halves, together with the dispatch glue that feeds them, is on the
  roadmap ("The compositor glue has no unit tests") for when a feature
  next changes them.
- `wm/src/compositor/child.rs` and `wm/src/compositor/report.rs`: child
  reaping and the panic and notification paths. Both hold a little tested
  code, but the rest needs a real child or a real panic, and gating them
  takes the workspace below the floor.

When the list was last narrowed (November 2026, before v1.0.0), the gated
workspace measured
95.06%, with `buoy-status-bar/src/main.rs`, `compositor/launch.rs`,
`compositor/wire.rs` and `compositor/window.rs` newly inside the gate. That
leaves little headroom: a patch that adds untested lines to one of those
files is expected to add the test, not to put the file back on the list.

## Naming and readability

These are conventions the codebase already follows almost everywhere and
had never written down, which is exactly why review could not catch the
places it had drifted from them (audit finding J-10). Where a rule has an
exception in the tree, the exception is named — an unnamed one is drift.

**Casing is per boundary, not per language.** Each of these is a contract
with something outside this repo, so the case is not a style choice:

| Surface | Case | Example |
| --- | --- | --- |
| TOML keys | `snake_case` | `pinned_terminal_args` |
| TOML *values* — every serde enum | `snake_case` | `action = "cycle_tag"` |
| IPC JSON `type` | `kebab-case` | `{"type":"switch-tag"}` |
| Window `app_id` | `kebab-case` | `pinned-term-3` |
| waybar CSS classes | `kebab-case` | `disconnected` |
| Rust | rustfmt's defaults | — |

**Every serde enum gets `#[serde(rename_all = ...)]`.** Its absence is the
bug: two enums in one config table without it produced `mod = ["Super"]`
next to `action = "resize"`, PascalCase and snake_case values in a
four-line block. A `rename_all` change silently changes the spelling a
user's config has to use, so each one is pinned by a test.

**American English**, because the protocol, `libinput` and serde all are,
and one file switching halfway is worse than either choice.

**`e` is always an error.** Beyond that, a single-character binding is
fine for a closure whose whole scope is one expression and whose type is
obvious from the line — `|t| t.id`. What is not fine is binding a letter
that means something else in the same file: an `|o|` holding a `Window`,
in a module where `o` is an output everywhere else, reads as a different
type than it is. `clippy::min_ident_chars` exists in the toolchain but is
not enabled: it would fire on about forty idiomatic closures to catch the
one that misleads.

**Four parameters is where you look for a struct.** Clippy fails the build
at seven (`clippy.toml`), which is the outer limit rather than the target.
Two adjacent parameters of the same type are the real hazard, because
transposing them compiles: either the types have to differ, or the names
have to make the order obvious at *every* call site, or a test has to hold
the order down. `output_contains(position, dimensions, point)` takes three
`(i32, i32)`s and is the third case — the transposition it cannot prevent
is what its two tests are for.

**No `bool` parameter that a call site passes as a literal.** `f(x, true)`
tells the reader nothing; a two-variant enum does. The one `bool`
parameter in the tree, `WindowManager::manage_seats`'s `any_new_windows`,
is a private method whose single call site is four lines above it and
passes a named binding — a second `bool` on it is the signal to make the
enum.

**No tuple return past two elements**, and past one only when both halves
are the same kind of thing (`(width, height)`, `(x, y)`). Anything else
gets a named struct; a caller writing `let (_, _, thing) = f()` is the
symptom.

**An error is typed the moment a caller might branch on it.** Every error
that crosses out of `wm_core`, `config` or `ipc` is an enum implementing
`Display`, because those callers do match on the variant and because a log
line printing a Rust identifier at a user is not a diagnostic —
`ConfigError` and `WmCoreError` are the shape to copy. `Result<_, String>`
is for the other case, and only that one: a message whose sole consumer
formats it into a notification or a stderr line and stops. Both satellites
use it throughout for exactly that reason.

**Model the invariant in the type before reaching for `expect`.** The
`buoy-wm` binary contains exactly one non-test `expect`, and it carries
its own proof: `main` exits before the first dispatch if that global is
absent, and the alternative — returning without `manage_finish()` — would
freeze the desktop rather than end the session. A new `expect` needs an
argument of that shape or it needs a different design. The same standard
covers indexing: the two `windows[i]` expressions in `compositor::seat`
each sit directly below the `position()` that produced `i`, with no
mutation in between, and say so.

**Doc comments state the contract first, then a `# Rationale` heading for
why.** A reader looking up what a function does should not have to read
three paragraphs of history to find out.

**Comments say why, and cite rather than narrate.** A story number or an
audit finding id is a citation attached to a reason — *"handled gracefully
rather than panicking (NFR2)"* — never the reason itself. Roughly a
hundred `Story N.N Task N:` prefixes predate this rule and are being left
alone; do not add more. An audit finding id (`audit finding D-01`)
resolves to its row in [`docs/audit-2026-08.md`](docs/audit-2026-08.md),
which says how that finding was closed; the audit's full plan is private.

**A backticked identifier is a reference.** In `///` this is enforced —
`[`Foo::bar`]` is checked by CI's `cargo doc`, so a rename cannot leave it
pointing at nothing. In `//` nothing checks it, so prefer `///` whenever
the comment describes an item rather than a line inside one.

## Testing against a real compositor

The Wayland half of `buoy` cannot be verified without a live river session,
and the test suite is honest about that boundary rather than pretending to
cover it. waybar output and `fuzzel` invocation are no longer on the far side
of it — both are driven end to end in `tests/` against a stub — so what is
left is the compositor protocol itself. If your change touches Wayland glue,
say in the pull request what you actually ran it against.

Test from a TTY or a nested river before trusting a build as your login
session. A `buoy-wm` that fails to start means a black screen and a bounce
back to the display manager.

## Running a checkout as your session

Without the package there is no `buoy` login entry; a hand-written
`~/.config/river/init` starts the session instead. `buoy-wm` is the command
river runs at startup:

```sh
river -c ~/.local/lib/buoy-wm/buoy-wm
```

To start helpers alongside it, point river at a script instead and `exec` the
WM last, so its lifetime is the session's:

```sh
#!/bin/sh
# ~/.config/river/init
waybar &
exec ~/.local/lib/buoy-wm/buoy-wm
```

That gives you window management only: no portals, and no units bound to
the session. To get the packaged session's plumbing from a checkout, reuse
the shipped files unchanged. Copy the supervisor beside the binaries — it
resolves `buoy-wm` as its own sibling, which is why it must sit there and
why `install.sh` does not do it for you (the tarball does not carry it):

```sh
install -m 0755 packaging/buoy-session ~/.local/lib/buoy-wm/
install -Dm 0644 packaging/buoy-session.target ~/.config/systemd/user/buoy-session.target
systemctl --user daemon-reload
install -Dm 0644 packaging/buoy-portals.conf ~/.config/xdg-desktop-portal/buoy-portals.conf
```

`~/.config/xdg-desktop-portal` is the first directory xdg-desktop-portal
searches. Then make the init export the name both the portal file and the
session entry key on — a river session started from Fedora's own
`river.desktop` sets a different one — and `exec` the supervisor, with any
helpers started before it or, better, as the user units the README's
"First login" describes:

```sh
#!/bin/sh
# ~/.config/river/init
export XDG_CURRENT_DESKTOP=buoy
exec ~/.local/lib/buoy-wm/buoy-session
```

This recipe is written from the files and has not been exercised at a live
login; the packaged session, which installs the same files, is the form
that gets verified before a release.

## IPC

The WM listens on a Unix socket at `$XDG_RUNTIME_DIR/buoy-wm.sock`, speaking
newline-delimited JSON. `buoy-tag-picker` and `buoy-status-bar` are its only clients,
but the protocol is plain enough to script against:

```sh
echo '{"type":"get-state"}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/buoy-wm.sock
```

There is no fallback path. If `$XDG_RUNTIME_DIR` is unset, empty, relative,
or names a directory that is not yours or that other users can reach, the WM
logs the reason and runs with no IPC: `Super+A`, `Super+S` and every status
bar are inert for that session, and window management is unaffected. Every
connection is also checked with `SO_PEERCRED` at both ends and refused unless
both processes run as the same user.

## Wanted but not scheduled

[`docs/ROADMAP.md`](docs/ROADMAP.md) holds work that is wanted and not
scheduled. If you are looking for something worth doing, start there.

## Design history

[`docs/adrs.md`](docs/adrs.md) records the decisions that shaped `buoy` —
the tag model, the river-over-Hyprland choice and the fuzzel-driven picker
among them — and the code cites several of them by number. Beyond that, the
comments are the documentation — if you are wondering why something is the
way it is, the answer is usually sitting next to it.
