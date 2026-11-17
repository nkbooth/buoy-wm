# Roadmap

Work that is wanted but not scheduled. Items leave this file by being done or
by being explicitly declined in [`docs/adrs.md`](adrs.md).

## Defaults should live where the user can see them

**The problem.** Every default in `buoy` is a Rust value in
`Config::default()`, and every list in the config file is
replace-not-merge: declaring one `[[keybind]]`, `[[mousebind]]` or
`[[input]]` discards the entire built-in list for that section. Both halves
are deliberate — replace-not-merge is what makes a default *removable* as
well as rebindable — but together they mean a user adding one setting
silently loses everything they did not restate, and nothing warns them.

This is not hypothetical. Enabling natural scrolling on one touchpad
requires restating `tap = true`, or tap-to-click disappears with no error at
all — the session simply stops responding to taps, at next login, with the
cause several days behind the symptom. The same trap swallows a whole
keymap: one `[[keybind]]` block means `Super+Q` no longer closes a window.

A related failure already happened. `docs/config.example.toml` claimed to be
"exactly the defaults" and was not — it carried four bindings
`Config::default()` has never had, including one running a script that does
not exist on most machines. A hand-maintained copy of a code value drifts,
and this one drifted undetected until an audit read both.

**Direction.** Two changes, either useful alone, better together.

1. **Generate the template from the code.** A `--print-default-config` flag
   that renders `Config::default()` as valid, commented TOML. That makes the
   defaults something a user can read, copy and edit as a complete starting
   file, and makes drift between code and template structurally impossible
   rather than merely tested. `docs/config.example.toml` becomes generated
   output, checked in CI against the binary rather than proofread.

2. **Warn when a declared list drops a built-in.** At load, compare each
   declared list against the built-in one and report what was discarded, via
   the same `notify_user` channel the config-trust and skipped-entry
   warnings already use. Not an error — discarding a default is a legitimate
   thing to want — but never silent.

**Deliberately not proposed:** making the lists additive with an explicit
removal syntax. That trades a silent drop for a config language nobody can
read, and it removes the property that makes the current design defensible:
what the file says is what you get.

**Open questions.** Whether the generated template should include the
illustrative extras that make an example readable (named-tag and `exec`
binds) or only real defaults — and if both, how to mark the difference so
the "exactly the defaults" claim cannot be made again. Whether the
drop-warning belongs at load or behind a `--check` subcommand a user runs
deliberately.

## A pinned terminal that exits early is not retried

**The problem.** The first switch to a tag claims that tag's pinned
terminal and spawns it. If the spawn itself fails, the claim is released
and the next switch tries again. If the terminal *starts* and then exits —
the multiplexer is not installed, its arguments are wrong, it crashes, or
the user quits zellij — the claim stays. The tag then has no backdrop for
the rest of the session, and nothing says why. The reaper in
`compositor/child.rs` sees the exit but does not know which tag the child
belonged to.

**Direction.** Record the tag with the child when the pinned terminal is
spawned. When the reaper collects a pinned terminal, release that tag's
claim through `WmCore::release_pinned_terminal_claim` and tell the user
through `notify_user`, so the next switch retries and the cause is on
screen. A terminal that exits at once on every switch should be reported
once, not on every switch.

**Open questions.** Whether a terminal the user quits on purpose should be
respawned on the next switch (as now after a failed spawn) or left closed
until the next login.

## Tag names in the config are checked only when used

**The problem.** Every tag is checked by `validate_tag_name` when it is
created, but a tag name from the config is created only when it is first
used. At load, `default_tag` is checked only for being empty and the
`switch_tag` names in keybinds not at all. So a name the rules refuse
fails at the first output for `default_tag`, or at the keypress for a
`switch_tag` bind, and the only trace is a journal line. The config loaded
cleanly, so the user has no reason to look.

**Direction.** Call `validate_tag_name` from config validation for
`default_tag` and for every `switch_tag` name, and report a bad one
through the skipped-entry path that already reports a bad keybind: the
entry is dropped, the rest of the file applies, and the user is notified
at load. Story 6.4 changes config loading too, so the two can share a
review.

## The compositor glue has no unit tests

**The problem.** `compositor/manager.rs` and `compositor/seat.rs` hold
the records that carry live river proxies, and `compositor/dispatch.rs`
receives their events. No unit test can build those records, so these
files are among the ones outside the coverage floor (CONTRIBUTING.md, "The
coverage floor"). Their decisions are already pushed into `WmCore` and the
pure helpers, which are gated, but what is left — the order of requests in
a manage or render sequence, which proxy a binding press reaches — is
checked only by using the session.

**Direction.** Split the Window, Output and Seat records into a proxy-free
half that holds the state and the decisions, and a thin half that holds
the proxies and sends requests. The proxy-free half can then be tested and
gated. Do this when a feature next changes `compositor/`, not
speculatively: the split costs about as much as the decomposition of the
old `main.rs` did, and it should be paid for by a change that needs it.
