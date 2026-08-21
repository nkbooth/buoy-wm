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

### Changed

- A second `buoy-wm` no longer unlinks a running instance's socket. A path
  that answers a connection means a live instance and the newcomer refuses
  to start; a non-socket inode at that path is refused rather than deleted.
  The socket is now removed when the WM's event loop ends.
- Both companion binaries bound how much they will read from the socket at
  the same 64 KiB the server has always applied to them, and
  `buoy-tag-picker` — which had no socket deadline at all — applies a
  five-second read and write timeout.

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
