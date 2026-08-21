# Changelog

Notable changes per release. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning is
[semantic](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security

- The IPC socket now authenticates both ends with `SO_PEERCRED`: the WM
  refuses a connection from another user, and both companion binaries refuse
  to talk to a server that is not running as you. Previously filesystem
  permissions were the only control on the whole surface.

### Removed

- **Breaking for sandbox use:** the `/tmp/buoy-wm-<user>.sock` socket-path
  fallback is gone. `$XDG_RUNTIME_DIR` must be an absolute path to a
  directory you own that other users cannot reach; otherwise the WM logs the
  reason and runs with no IPC rather than binding a squattable path in a
  shared directory. `Super+A`, `Super+S` and the status bar are inert in that
  case; window management is unaffected.

### Added

- Failures the user can act on now raise a desktop notification
  (`notify-send`, app name `buoy-wm`) as well as a log line: a rejected or
  partially-skipped config, an IPC server that never started, a tag keybind
  pressed before any output exists, and — from `buoy-tag-picker` — every
  reason it gives up, including `fuzzel` not being installed. Until now
  exactly one failure in the whole product was visible to the user.

### Changed

- **A bad `[[keybind]]`, `[[mousebind]]` or `[[input]]` entry is now skipped
  rather than discarding the entire config file.** Everything else in the
  file still applies, and every skipped entry is reported at once instead of
  one per restart. Invalid TOML and a wrong `[defaults]` value are still
  fatal to the whole file, because neither has a single entry to drop. This
  is what the project's own recorded acceptance criterion always specified.
- `buoy-tag-picker` retries a refused connection twice, 50 ms then 100 ms,
  which closes the login-time window where the WM had not yet bound its
  socket and `Super+A` was a dead key.
- `fuzzel` failing to start is no longer indistinguishable from the user
  pressing Escape: the picker reports it and exits non-zero.
- Log lines carry journald severity, so `journalctl --user -b
  --identifier=buoy-wm -p err` now shows exactly the failures, and the
  subsystem that logged each line is named automatically rather than by a
  hand-written prefix. Lines seen in a terminal rather than a journal carry
  a visible `<3>`/`<6>` severity marker.
- A `buoy-status-bar` poll failure says which of the seven distinct failures
  it was, logged when it changes rather than four times a second forever or
  never.
- Losing the Wayland connection prints a sentence instead of the `Debug` of a
  `DispatchError`.
- A second `buoy-wm` no longer unlinks a running instance's socket. A path
  that answers a connection means a live instance and the newcomer refuses
  to start; a non-socket inode at that path is refused rather than deleted.
  The socket is now removed when the WM's event loop ends.
- Both companion binaries bound how much they will read from the socket at
  the same 64 KiB the server has always applied to them, and
  `buoy-tag-picker` — which had no socket deadline at all — applies a
  five-second read and write timeout.

- `pinned_terminals = false` in `[defaults]` turns pinned terminals off
  entirely — the supported way out for a user whose terminal or `zellij` is
  broken, in the one subsystem of the process that *is* your session which
  launches another program. Setting `pinned_terminal_args = []` looked like
  that switch and was in fact a `[defaults]` error, which discards the whole
  file; it is still rejected, but now says so and names the real switch.
- A config file that is group- or world-**writable**, or owned by another
  user, is now loaded *and* reported — the same warning bash gives a
  world-writable profile, for a file whose `exec` bindings run in your
  session. Readability is not checked: the usual `644` stays silent.
- The config file is read with a 1 MiB bound and must be a regular file. An
  accidentally enormous file, or a FIFO at that path, used to be an
  out-of-memory or an unfinishable login rather than a reported problem.

- Tag names are validated where they enter the registry: non-empty and not
  whitespace-only, at most 64 bytes, not a bare `.` or `..`, and free of `/`,
  `\` and control characters. A rejected name is reported as
  `invalid tag name` instead of becoming a permanent registry entry, a zellij
  session path component and a waybar label.
- One IPC connection may create at most eight tags before it is closed, so
  the 64-slot registry — which by design is never reclaimed — cannot be spent
  in a single burst.
- A tag's pinned terminal is spawned with a per-tag `app_id`,
  `pinned-term-<tag id>`, instead of the shared `pinned-term` — a mapped
  window now says which tag it belongs to rather than the WM inferring it
  from the order spawns happen to start in. `pinned_terminal_args` is
  unchanged: `{app_id}` is still required, and still the placeholder the
  value is substituted into.

### Fixed

- A pinned terminal whose spawn failed no longer leaves its tag marked as
  spawned for the rest of the session, and no longer mis-tags the next
  pinned terminal that appears.
- A click, drag or resize whose target window or seat had already gone away
  no longer ends the session. The compositor promises no ordering between
  an interaction event and the removal it races, so this was reachable
  without doing anything unusual; it now costs the one gesture.
- A resize request naming edges this build's copy of the protocol does not
  recognise is dropped rather than fatal, so a future river that widens
  that bitfield costs a drag instead of a login session.
- Move and resize drags saturate instead of wrapping, so dragging far past
  a window's opposite edge can no longer produce a nonsense size that the
  minimum-size floor could not detect.
- The tag-assignment picker (`Super+A`) re-reads state before each pick
  instead of trusting a snapshot taken when it opened. Because the
  operation is a toggle, a membership changed meanwhile by a keybind or a
  second client used to invert the next pick rather than merely lose it; a
  tag created meanwhile also now appears.

### Changed

- A panic anywhere in `buoy-wm` now raises a notification and a greppable
  journal line naming the source location, rather than a stripped
  backtrace on a stderr nobody reads.
- Error log lines read as sentences rather than Rust identifiers: "the tag
  registry is already full at 64 tags", not `TagLimitReached`.
- A child process this WM spawned that exits unsuccessfully now says so,
  which is the difference between a misconfigured `terminal` and a working
  one. Finished children are also reclaimed once per window-management
  pass rather than only on the next spawn.
- A keybind press discarded because a second binding fired before the
  first was acted on is now reported instead of vanishing.
- A failed response write is logged unless it is just the peer having hung
  up.
- `mod` and `button` values are now spelled in lowercase like every other
  config value — `mod = ["super"]`, `button = "left"`. **Nothing breaks:**
  every previously documented spelling (`"Super"`, `"Mod4"`, `"Control"`,
  `"Left"`, …) is still accepted and always will be, so an existing config
  needs no edit.
- The two picker actions are now `open_assign_picker` and
  `open_switch_picker`. `tag_switch` opened a picker while `switch_tag`
  switched immediately, one transposition apart, so either spelling silently
  did the other one's job. **Nothing breaks:** `tag_picker` and `tag_switch`
  are still accepted and always will be.

## [0.1.0] — first public release

Initial release. Everything below is new.

### Window management

- Window manager for river 0.4+ over `river-window-management-v1`, owning all
  tag state itself — the compositor keeps none.
- Named tags, created on first use, up to 64 at once. One tag displayed per
  output; a window can carry several tags and appears wherever any of them is
  displayed.
- A persistent fullscreen `zellij` terminal pinned behind every other window,
  one per tag, spawned on the tag's first use and not closable with the
  ordinary close binding.
- Floating placement for everything else, plus pointer move and resize.
- Active-output tracking follows the pointer; outputs being removed reroute
  rather than orphaning their tag.

### Configuration

- Optional `~/.config/buoy/config.toml`. A malformed file reports on stderr
  and falls back to built-in defaults rather than refusing to start.
- Fully rebindable and removable keybinds and mousebinds; declaring any
  replaces the whole built-in set. Keysym names resolved without
  libxkbcommon.
- `switch_tag` and `exec` actions, the latter through `sh -c`.
- Configurable terminal, launcher, default tag, and pinned-terminal argv.
- `[[input]]` blocks configuring libinput devices by name, with tap-to-click
  enabled for touchpads by default because libinput ships it off.
- A hotkey cheat-sheet generated from the bindings actually loaded.

### Companions

- `buoy-tag-picker`: `fuzzel`-driven tag assignment (a checklist of every tag) and
  tag switching, where typing an unknown name creates that tag and switches
  to it in one action.
- `buoy-status-bar`: feeds a waybar `custom/tag` module, one process per output,
  degrading to a `disconnected` state rather than freezing on a stale tag.
- A Unix-socket JSON IPC server backing both.

### Packaging

- GitHub release tarballs with build-provenance attestation, a Homebrew
  formula, and a Fedora COPR spec. See [docs/PACKAGING.md](docs/PACKAGING.md).
