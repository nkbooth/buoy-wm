# Changelog

Notable changes per release. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning is
[semantic](https://semver.org/spec/v2.0.0.html).

## [1.0.0-rc.1]

First release candidate of 1.0.0, and the project's first tag. Nothing was
released before it, so this entry describes what ships rather than what
changed.

### Window management

- Window manager for river 0.4+ over `river-window-management-v1`, owning
  all tag state itself — the compositor keeps none.
- Named tags, created on first use, up to 64 at once. One tag displayed
  per output; a window can carry several tags and appears wherever any of
  them is displayed.
- A persistent fullscreen terminal pinned behind every other window, one
  per tag, running `zellij` by default. It is spawned on the tag's first
  use as the configured terminal with a per-tag `app_id`,
  `pinned-term-<tag id>` (the `{app_id}` placeholder in
  `pinned_terminal_args` is required), so a mapped window says which tag
  it belongs to rather than the WM inferring it from the order spawns
  happen to start in; it is not closable with the ordinary close binding.
  A pinned terminal whose spawn fails releases the tag's claim, so the next
  switch to that tag tries again and the next pinned terminal that appears
  is not mis-tagged.
- Floating placement for everything else, plus pointer move and resize.
  Drags saturate instead of wrapping, so dragging far past a window's
  opposite edge cannot produce a nonsense size that the minimum-size floor
  could not detect.
- Active-output tracking follows the pointer; outputs being removed
  reroute rather than orphaning their tag.
- A click, drag or resize whose target window or seat has already gone
  away costs the one gesture, not the session: the compositor promises no
  ordering between an interaction event and the removal it races.
- A resize request naming edges this build's copy of the protocol does not
  recognise is dropped rather than fatal, so a future river that widens
  that bitfield costs a drag instead of a login session.

### Configuration

- Optional `~/.config/buoy/config.toml`. A file that is not valid TOML, or
  has a wrong value in `[defaults]`, is reported and the built-in defaults
  apply instead — the WM never refuses to start over its config, because
  it is the only thing that can put a screen in front of you to fix the
  file with. The report is a desktop notification and a journal line (see
  Diagnostics).
- A bad `[[keybind]]`, `[[mousebind]]` or `[[input]]` entry is skipped
  rather than discarding the whole file: everything else in the file
  applies, and every skipped entry is reported at once rather than one per
  restart.
- Fully rebindable and removable keybinds and mousebinds; declaring any
  replaces the whole built-in set. Keysym names resolved without
  libxkbcommon. Binding the same trigger twice is an error, not a silent
  race.
- `mod` and `button` values are lowercase like every other config value —
  `mod = ["super"]`, `button = "left"`. The PascalCase spellings
  (`"Super"`, `"Mod4"`, `"Control"`, `"Left"`, …) are accepted as well,
  and always will be.
- Actions `switch_tag` and `exec`, the latter through `sh -c`. The two
  pickers are `open_assign_picker` and `open_switch_picker`; `tag_picker`
  and `tag_switch` are accepted as aliases, and always will be.
- Configurable terminal, launcher, default tag, and pinned-terminal argv.
  `pinned_terminals = false` turns pinned terminals off entirely — the
  supported way out for a user whose terminal or multiplexer is broken, in
  the one subsystem of the process that *is* your session which launches
  another program. An empty `pinned_terminal_args` is not that switch: it
  is a `[defaults]` error, so the built-in defaults apply, and the report
  names the real one.
- `[[input]]` blocks configuring libinput devices by name, with
  tap-to-click enabled for touchpads by default because libinput ships it
  off.
- A hotkey cheat-sheet generated from the bindings actually loaded.
- A config file that is group- or world-**writable**, or owned by another
  user, is loaded *and* reported — the same warning bash gives a
  world-writable profile, for a file whose `exec` bindings run in your
  session. Readability is not checked: the usual `644` stays silent.
- The config file is read with a 1 MiB bound and must be a regular file,
  so an accidentally enormous file, or a FIFO at that path, is a reported
  problem rather than an out-of-memory or an unfinishable login.

### Companions and IPC

- `buoy-tag-picker`: `fuzzel`-driven tag assignment (a checklist of every
  tag; `Super+A` by default) and tag switching (`Super+S` by default),
  where typing an unknown name creates that tag and switches to it in
  one action. The assignment picker re-reads state before each pick
  rather than trusting a snapshot taken when it opened, so a membership
  changed meanwhile by a keybind or a second client does not invert the
  next toggle, and a tag created meanwhile appears. A refused connection
  is retried twice, 50 ms then 100 ms, which covers the login-time window
  before the WM has bound its socket; `fuzzel` failing to start is
  reported with a non-zero exit, distinct from the user pressing Escape.
- `buoy-status-bar`: feeds a waybar `custom/tag` module, one process per
  output, degrading to a `disconnected` state rather than freezing on a
  stale tag. A poll failure says which of the seven distinct failures it
  was, logged when it changes rather than four times a second forever.
- A Unix-socket JSON IPC server at `$XDG_RUNTIME_DIR/buoy-wm.sock` backing
  both. Every connection is authenticated at both ends with `SO_PEERCRED`:
  the WM refuses a connection from another user, and both companion
  binaries refuse to talk to a server that is not running as you.
- There is no `/tmp` fallback for the socket. `$XDG_RUNTIME_DIR` must be
  an absolute path to a directory you own that other users cannot reach;
  otherwise the WM logs the reason and runs with no IPC rather than
  binding a squattable path in a shared directory. `Super+A`, `Super+S`
  and the status bar are inert in that case; window management is
  unaffected.
- A second `buoy-wm` does not unlink a running instance's socket: a path
  that answers a connection means a live instance and the newcomer refuses
  to start, and a non-socket inode at that path is refused rather than
  deleted. The socket is removed when the WM's event loop ends.
- Both companion binaries bound how much they read from the socket at the
  same 64 KiB the server applies to them, and `buoy-tag-picker` applies a
  five-second read and write timeout.
- Tag names are validated where they enter the registry: non-empty and not
  whitespace-only, at most 64 bytes, not a bare `.` or `..`, and free of
  `/`, `\` and control characters. A rejected name is reported as
  `invalid tag name` rather than becoming a permanent registry entry, a
  multiplexer session path component and a waybar label.
- One IPC connection may create at most eight tags before it is closed, so
  the 64-slot registry — which by design is never reclaimed — cannot be
  spent in a single burst.
- A failed response write is logged unless it is just the peer having hung
  up.

### Diagnostics

- Failures the user can act on raise a desktop notification
  (`notify-send`, app name `buoy-wm`) as well as a log line: a rejected or
  partially-skipped config, an IPC server that never started, a tag
  keybind pressed before any output exists, and — from `buoy-tag-picker` —
  every reason it gives up, including `fuzzel` not being installed.
- Log lines carry journald severity, so `journalctl --user -b
  --identifier=buoy-wm -p err` shows exactly the failures, and the
  subsystem that logged each line is named automatically rather than by a
  hand-written prefix. Lines seen in a terminal rather than a journal
  carry a visible `<3>`/`<6>` severity marker.
- A panic anywhere in `buoy-wm` raises a notification and a greppable
  journal line naming the source location, rather than a stripped
  backtrace on a stderr nobody reads.
- Error log lines read as sentences rather than Rust identifiers: "the tag
  registry is already full at 64 tags", not `TagLimitReached`. Losing the
  Wayland connection prints a sentence rather than the `Debug` of a
  `DispatchError`.
- A child process this WM spawned that exits unsuccessfully says so, which
  is the difference between a misconfigured `terminal` and a working one.
  Finished children are reclaimed once per window-management pass rather
  than only on the next spawn.
- A keybind press discarded because a second binding fired before the
  first was acted on is reported rather than vanishing.

### Session

- A `buoy` entry for the display manager. It starts river with a
  supervisor (`/usr/libexec/buoy-wm/buoy-session`) that imports the
  session into systemd and D-Bus, keeps `buoy-session.target` up exactly
  as long as the window manager runs and exits with its status — so the
  portals, waybar and kanshi start and stop with the session instead of
  outliving a dead socket.
- `buoy-portals.conf`, which routes screen sharing and screenshots to the
  wlroots portal backend.

### Packaging

- GitHub release tarballs with build-provenance attestation, a Homebrew
  formula, and the RPM spec the release RPM is built from. See
  [docs/PACKAGING.md](docs/PACKAGING.md); COPR, a PPA and the tap are
  optional mirrors made after the tag.
- An RPM as a GitHub release asset, built with Fedora 44's own toolchain
  and installable by URL, with the vendored-crates tarball beside it for
  offline rebuilds; both are covered by the release's build-provenance
  attestation alongside the tarball. The RPM requires `river` (Fedora
  ships 0.5.0) and recommends the two portal backends.
- Release candidates, tagged `vX.Y.Z-rc.N` and published as pre-releases
  (`1.0.0~rc.1` in the RPM).

[1.0.0-rc.1]: https://github.com/nkbooth/buoy-wm/releases/tag/v1.0.0-rc.1
