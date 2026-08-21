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
