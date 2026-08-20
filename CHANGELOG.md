# Changelog

Notable changes per release. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning is
[semantic](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
