---
baseline_commit: dd7f428
---

# Story 2.11: Fix fuzzel `--accept-nth` silently corrupting created tag names

Epic: 2 | Priority: H | Status: done

## Description
Live testing found a tag literally named `"{2}"` in a running session. The
user confirmed they had typed `"test"`, not `"{2}"` — this was a live,
reproducible bug, not a stray/accidental input.

Direct evidence, queried from the live `buoy-wm` IPC socket while the bug
was present:
```
{"id":0,"name":"default"},{"id":1,"name":"{2}"}
```

**Root cause: a confirmed upstream `fuzzel` bug**, cross-checked against
`fuzzel`'s own issue tracker (Codeberg `dnkl/fuzzel` #670, fixed by #671 for
one specific case but not this project's case): `--accept-nth=N` prints the
*literal string* `"{N}"` instead of the real value whenever there is no
actual N:th column to extract from the accepted line. This project has used
`--accept-nth=2` since Story 2.2 (`run_fuzzel`) to have `fuzzel` return just
the bare tag-id column on selection. That works fine for a real, rendered
row (`"[ ] name\tid\n"`, which genuinely has 2 tab-delimited columns) — but
the *create-tag* path is exactly the case where the user types free text
that **matches no row**, which `fuzzel`'s own dmenu-mode contract says gets
printed back verbatim, with **no columns at all**. `--accept-nth=2` cannot
extract "column 2" from a single, tab-less string — and instead of falling
back to the raw text, `fuzzel` prints the broken literal `"{2}"`.

This means **every tag ever created by typing a new name, in this entire
project's history, was silently misnamed to the literal text `"{2}"`**,
never the name actually typed. This was never caught by the extensive
existing unit test suite because every `parse_fuzzel_output`/
`parse_switch_selection` test simulated `stdout` as an *already-extracted*
bare id or free-text string — exactly what `--accept-nth` was assumed to
correctly produce — never the real `fuzzel` process's actual (buggy)
output for the create-tag case. No amount of unit testing here could have
caught this; it required a real `fuzzel` process and a real typed name to
surface at all.

## Fix
Stop relying on `--accept-nth` (broken for this use case) entirely.
`--with-nth=1` (display-only, unaffected by this bug) is kept. Without
`--accept-nth`, `fuzzel(1)` itself documents the real behavior: "the full
input line is printed on stdout" for a matched row, and the typed text
verbatim for a non-matching one. `tag-picker` now parses this full raw
line itself (`checklist::parse_fuzzel_output`/`parse_switch_selection`,
`str::rsplit_once('\t')`): a line with a tab and a known id after it is a
real selection (`Toggled`/`Selected`); a line with no tab at all is a
typed, non-matching custom entry (`CreateTag` in assign mode, always
`Cancelled` in switch mode per `EXPERIENCE.md`'s no-create-in-switch-mode
rule); a line with a tab but an empty or unknown id after it (the
rejection/apply-failed synthetic rows, or a defensively-unreachable stale
row) safely cancels rather than creating a tag out of row furniture.

**Non-obvious fix detail**: only the trailing newline is stripped
(`str::trim_end_matches('\n')`), not a blanket `.trim()` — tab is ASCII
whitespace, and a blanket trim would eat the rejection/apply-failed rows'
significant trailing tab, breaking their entire dismissal mechanism (an
empty id after the tab is what makes selecting them equivalent to Escape).

## Acceptance criteria
**Given** the user types a new tag name in assign mode's create-tag box
**When** the invocation is accepted
**Then** the tag is created with **exactly** the typed name — not `"{2}"`
or any other literal placeholder text

**Given** an existing tag row is selected in assign mode or switch mode
**When** the invocation is accepted
**Then** the correct tag id is extracted and the correct action taken
(toggle or switch), unchanged from previous behavior

**Given** the 64-tag-cap rejection row or the create-apply-failed row is
selected, or the picker is dismissed with Escape
**When** the invocation is accepted or cancelled
**Then** the result is `Cancelled` — no tag is created from the message
text itself

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Technical notes

**Why this couldn't have been caught by any existing test.** Every
`parse_fuzzel_output`/`parse_switch_selection` unit test, since Story 2.2,
passed a pre-extracted bare id or free-text string as `stdout` — simulating
what `--accept-nth` was *assumed* to correctly return. The bug lives
entirely in the gap between that assumption and `fuzzel`'s real behavior, a
gap no amount of testing `tag-picker`'s own parsing logic in isolation
could ever expose. This is a direct argument for periodic real
live-testing against the actual `fuzzel` binary, not just this project's
extensive but `fuzzel`-behavior-assuming unit test suite.

**Scope boundary.** This does not fix `fuzzel` itself (upstream, tracked
separately at Codeberg `dnkl/fuzzel`) — it removes this project's
dependency on the broken flag combination entirely, which is strictly
better regardless of whether/when upstream fixes the underlying issue.

**Live session caveat.** The already-running `buoy-wm` process (and its
in-memory `wm_core` state) still has the misnamed `"{2}"` tag from before
this fix — `wm_core` has no delete/rename-tag API (ADR-006/v1 scope,
deliberate, unrelated to this bug). This tag disappears on the next full
`buoy-wm` restart along with all other session-local tag state; no code
change fixes already-corrupted in-memory state retroactively.

## Test plan
1. **`tag-picker` unit tests (TDD)**: `parse_fuzzel_output`/
   `parse_switch_selection` fully rewritten around the new full-raw-line
   parsing contract — toggled/selected-from-a-full-row, create-tag-from-
   no-tab-freeform-text, cancelled-for-a-row-with-an-unknown-id,
   cancelled-for-the-rejection-row's-actual-round-trip (a new,
   directly-meaningful test this bug class specifically needed),
   trailing-newline-only-stripped (not a blanket trim). Fully testable —
   pure string parsing, no live `fuzzel` process involved.
2. **Build/lint gate**: `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` clean.
3. **Regression gate**: `wm`/`status-bar` test suites unaffected (this bug and fix are entirely `tag-picker`-local).
4. **Live gate (manual, not automated, but this is exactly the gate that found the bug in the first place)**: confirm a newly-created tag's name matches exactly what was typed.

## FR coverage
Tag-manager popup's create-a-new-tag path (existing FR, Story 2.2/2.3) —
fixes a silent data-corruption defect present since that FR's original
implementation, not a new FR.

## Dev Agent Record

**What was done**: Diagnosed live via direct IPC query against a running
session (`socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/buoy-wm.sock` with a raw
`{"type":"get-state"}` request), confirming the corrupted tag name
server-side before writing any fix — ruled out a `tag-picker`/`wm`
wire-protocol bug (round-trip already covered by existing tests) before
researching `fuzzel` itself. Confirmed via `fuzzel --version` (1.14.0) and
`fuzzel --help` that `--with-nth`/`--accept-nth`/`--nth-delimiter` are all
present and current, then found the exact matching upstream bug via
`fuzzel`'s own Codeberg issue tracker (#670/#671) — the maintainer's own
words on the *fixed* case ("this is not the same as `--with-nth={0}`,
which is just an invalid formatter, that will result in `'{0}'` in the
output string") directly describes the mechanism still affecting this
project's *unfixed*, non-zero `--accept-nth=2` case for non-matching
custom entries.

- `tag-picker/src/main.rs`: removed `--accept-nth=2` from `run_fuzzel`;
  rewrote its doc comment to explain why.
- `tag-picker/src/checklist.rs`: rewrote `parse_fuzzel_output` and
  `parse_switch_selection` to parse the full raw line via
  `rsplit_once('\t')`; changed `.trim()` to `.trim_end_matches('\n')` in
  both (a blanket trim would eat the rejection/apply-failed rows'
  significant trailing tab); updated every doc comment referencing the
  old `--accept-nth`-based contract; rewrote the full `parse_fuzzel_output`/
  `parse_switch_selection` test suites around the new full-row input
  shape, including new tests with no prior equivalent (the rejection-row
  round-trip, the unknown-id-with-a-tab-present branch).

**Verification** (all inside the devcontainer via `devpod ssh buoy-wm`):
`cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets
-- -D warnings` clean; `cargo test --workspace` — `buoy-wm` 176,
`status-bar` 27, `tag-picker` 69 (63 baseline + 6 net: several renamed/
restructured, `parse_fuzzel_output`/`parse_switch_selection` each gained
one genuinely new test) = **272 total, 0 failures**; `pre-commit run
--all-files` clean.

**Live confirmation**: deferred to the user's next real `Mod4+A` create-tag
attempt after rebuild — this bug was found via live testing and the fix's
correctness is best confirmed the same way, but the code fix itself,
`fuzzel`'s own documented behavior without `--accept-nth`, and the full
rewritten unit test suite all agree independently.

**File List**:
- `tag-picker/src/main.rs`
- `tag-picker/src/checklist.rs`
