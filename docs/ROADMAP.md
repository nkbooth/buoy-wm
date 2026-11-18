# Roadmap

Work that is wanted but not scheduled. Items leave this file by being done or
by being explicitly declined in [`docs/adrs.md`](adrs.md).

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
at load.

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
