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

## Building

The Rust toolchain and `libwayland-dev` are the only build dependencies.
`.devcontainer/` provides both if you would rather not install them.

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

`pre-commit run --all-files` runs the last two. The hooks are deliberately
not `pre-commit install`ed — they need a toolchain the host may not have, so
the git shim would fail closed on every commit.

## What patches are expected to include

**A failing test first.** Every piece of logic in `wm_core`, `config`, and
`ipc` was written test-first, and that is the reason the tag model can be
refactored at all. Write the test, watch it fail, then make it pass. Pull
requests that add logic without tests will be asked for them.

The one standing exception is Wayland `Dispatch` glue, process spawning, and
socket I/O — code whose only behaviour is talking to something that does not
exist in a test harness. Keep those layers thin and push every decision they
make down into something testable; that is why `bar_line.rs` and `mode.rs`
exist as separate modules from their `main.rs`.

**Docstrings on public items.** Every public function, method, type, and
module gets a `///` or `//!` blurb. Private internals do not — the noise
costs more than it explains.

**Comments that say why.** Names and structure carry the *what*. Reserve
comments for the hidden constraint, the non-obvious invariant, or the
workaround for specific upstream behaviour. There are a lot of those in this
codebase already; they are the most valuable thing in it.

## Testing against a real compositor

Much of `buoy` cannot be verified without a live river session, and the test
suite is honest about that boundary rather than pretending to cover it. If
your change touches Wayland glue, waybar output, or `fuzzel` invocation, say
in the pull request what you actually ran it against.

Test from a TTY or a nested river before trusting a build as your login
session. A `buoy-wm` that fails to start means a black screen and a bounce
back to the display manager.

## Design history

[`docs/adrs.md`](docs/adrs.md) records the decisions that shaped `buoy`, and
the code cites several of them by number. Beyond that, the comments are the
documentation — if you are wondering why something is the way it is, the
answer is usually sitting next to it.
