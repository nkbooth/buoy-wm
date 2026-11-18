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

## A dropped built-in input setting is judged by the pattern, not the device

**The problem.** At login buoy-wm reports a built-in `[[input]]` setting
that the user's list no longer applies, such as the touchpad's `tap`. It
decides this at config load, before any device exists, by asking which of
the user's entries would apply to a device literally named after the
built-in pattern, `*Touchpad*`. That is wrong both ways. An entry that
names the real device more exactly (`name = "PIXA3854:00 093A:0274
Touchpad"`, copied from `libinput list-devices` as the man page suggests,
or `*Touchpad`) keeps tap working but does not match the text
`*Touchpad*`, so the user is told at every login that tap was dropped and
can silence it only by rewriting the entry. And an earlier entry that
matches the real touchpad but not that text (`name = "*093A*"` with only
`natural_scroll`) takes tap away, first match winning, with no report.
Both cases are documented in buoy-config(5) and pinned by
`wm/src/config/dropped.rs`'s tests.

**Direction.** Judge each real device when river announces it. For a
device name, compare the settings the built-in `[[input]]` list would
apply (`Config::default().input_for(name)`) with the settings the loaded
config applies (`input_for(name)`), by setting, as the load-time check
does now; report what is missing once per device per session through
`notify_user`, naming the device. Keep that comparison a pure function in
`wm/src/config/` so it stays tested and inside the coverage floor, and
call it from `compositor/libinput.rs` where devices arrive. The
`[[keybind]]` and `[[mousebind]]` half of the report stays at load, where
it is already exact, and the load-time `[[input]]` half goes.

**Open questions.** Whether a device that disappears and comes back (a
dock, a Bluetooth mouse) is reported again in the same session.

