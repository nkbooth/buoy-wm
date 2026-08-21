# buoy

A keyboard-driven window manager for the [river](https://codeberg.org/river/river)
Wayland compositor (0.4+), built around a terminal you never have to open.

`buoy` speaks `river-window-management-v1`, which means river handles pixels
and input while `buoy` decides window policy. It owns **all** tag state
itself — the compositor keeps none — so the model below is entirely `buoy`'s
to define.

It has been the author's daily driver since August 2026.

## The idea

Two convictions shape everything here.

**Terminal first.** Every tag has a permanent, fullscreen terminal running a
[`zellij`](https://zellij.dev) session pinned behind everything else. It is
not a window you launch, focus, or close; it is the tag's floor. Switch to
the `email` tag and its zellij session is simply *there*, exactly as you left
it — across tag switches, across days, across `buoy` restarts. Graphical
applications float on top of that floor when you need them.

**Keyboard primary.** Every operation has a binding, all of them rebindable
and removable. The mouse can move and resize floating windows, and that is
the extent of what it is required for. There is no panel to click, no menu
bar, and no tray. `Super+Shift+?` prints the bindings actually in effect.

What `buoy` deliberately does not do: automatic tiling, window decorations,
gaps, animations, or a session/state file. Floating windows are placed once
and then left alone.

## Requirements

| | |
| --- | --- |
| **river** | 0.4 or newer, built with `river-window-management-v1` |
| **foot** | default terminal — swappable, see [Configuration](#configuration) |
| **zellij** | runs inside each tag's pinned terminal |
| **fuzzel** | drives the tag pickers and the hotkey cheat-sheet |
| **waybar** | *optional* — renders the current-tag indicator |

`river`'s `river_input_manager_v1` and `river_libinput_config_v1` globals are
also optional: without them, `[[input]]` blocks are skipped with one log line
and every device keeps libinput's own defaults.

## Install

### From a release

```sh
version=0.1.0
archive="buoy-wm-${version}-x86_64-unknown-linux-gnu.tar.gz"
base="https://github.com/nkbooth/buoy-wm/releases/download/v${version}"

workdir="$(mktemp -d)"
curl --proto '=https' --tlsv1.2 -fsSL -o "$workdir/$archive" "$base/$archive"
curl --proto '=https' --tlsv1.2 -fsSL -o "$workdir/$archive.sha256" "$base/$archive.sha256"
(cd "$workdir" && sha256sum -c "$archive.sha256")

# Optional and stronger: proves this exact archive came out of this repo's
# release workflow. See SECURITY.md.
gh attestation verify "$workdir/$archive" --repo nkbooth/buoy-wm

tar -xzf "$workdir/$archive" -C "$workdir"
"$workdir/buoy-wm-${version}-x86_64-unknown-linux-gnu/install.sh"
rm -rf "$workdir"
```

Longer than a one-line pipe on purpose. `mktemp -d` rather than `/tmp`,
because `/tmp` is world-writable and sticky; an exact path rather than a
`/tmp/buoy-wm-*/install.sh` glob, because another local user can pre-create
a directory that sorts first and have the shell run *their* `install.sh` as
you; a pinned version rather than `releases/latest/download/...`, because
that URL is a moving target; and the checksum is verified because CI
publishes it and nothing was checking it.

### From source

The Rust toolchain and `libwayland-dev` are the only build dependencies.

```sh
cargo build --workspace --release
./scripts/install.sh
```

Both paths put all three binaries — `buoy-wm`, `buoy-tag-picker`, `buoy-status-bar` —
into `~/.local/lib/buoy-wm` (override with `BUOY_INSTALL_DIR`) and symlink
only `buoy-wm` onto your `PATH`. **They must stay in one directory**: the WM
locates `buoy-tag-picker` as a sibling of its own executable.

Installing outside the build tree is deliberate. A `river` session `exec`s
the WM as its session leader, so a `cargo clean` that deleted the running
binaries would cost you the session. For the same reason: **verify a new
build from a TTY or a nested river before logging out.** A broken WM binary
means a black screen and a bounce back to your display manager, with no shell
to fix it from.

## Running it

`buoy-wm` is the command river runs at startup:

```sh
river -c ~/.local/lib/buoy-wm/buoy-wm
```

To start helpers alongside it, point river at a script instead and `exec` the
WM last, so its lifetime is the session's:

```sh
#!/bin/sh
# ~/.config/river/init
waybar &
exec ~/.local/lib/buoy-wm/buoy-wm
```

## Tags

A tag is a named workspace. It is created the moment you first name it and
lives until the session ends; there is no fixed set to configure up front and
no numbered grid to memorise. Up to 64 tags exist at once.

Three rules cover the whole model:

- **One tag is displayed per output.** Each monitor shows exactly one tag,
  and each tag can be displayed on at most one monitor at a time.
- **A window can carry several tags.** It is visible whenever any tag it
  carries is the one being displayed. This is how a chat window follows you
  between `work` and `personal` without being duplicated.
- **Every tag owns one pinned terminal.** Spawned on the tag's first use,
  held fullscreen behind every other window, and not closable with the
  ordinary close binding.

"The active output" — the target of tag switches — is whichever monitor the
pointer is currently over.

## Keybindings

Defaults, in effect when no config file overrides them. `Super` is `Mod4`.

| Keybind | Action |
| --- | --- |
| `Super+Space` | Open a new floating terminal |
| `Super+N` | Rotate focus to the next window |
| `Super+Q` | Close the focused window (never the pinned terminal) |
| `Super+Tab` | Cycle the active output to the next tag |
| `Super+R` | Open the application launcher |
| `Super+A` | Open the tag-assignment picker for the focused window |
| `Super+S` | Open the tag-switch picker for the active output |
| `Super+Shift+?` | Show the hotkey cheat-sheet |
| `Super+Esc` | Exit the session |
| `Super+Left-drag` | Move a window |
| `Super+Right-drag` | Resize a window |

The cheat-sheet is generated from the bindings actually loaded, so it always
reflects your config rather than this table.

### Switching and creating tags — `Super+S`

Opens a `fuzzel` list of every tag; picking one displays it on the active
output, spawning its pinned terminal if this is its first use.

Typing a name that matches nothing and confirming **creates that tag and
switches to it in one action**. Creating a tag is how you start working
somewhere new, so it lives here rather than buried in the assignment picker.
Typing the exact name of an existing tag switches to it instead of
duplicating it. At the 64-tag cap the picker reopens with the rejection shown
and your typed name restored — nothing is created, nothing is switched.

### Assigning tags to a window — `Super+A`

Opens a `fuzzel` checklist of every tag, pre-checked for the tags the focused
window already carries. Selecting a row toggles that tag and reopens the
list, so you can toggle several in one visit. Existing tags only.

## Status bar

`buoy-status-bar` is a companion binary that feeds one waybar `custom/tag` module
per output. It takes the output's numeric id as its only argument and loops
itself, so waybar needs neither `interval` nor `signal`:

```jsonc
// ~/.config/waybar/config.jsonc
[
  {
    "layer": "top",
    "output": "eDP-1",
    "modules-right": ["custom/tag"],
    "custom/tag": {
      "exec": "~/.local/lib/buoy-wm/buoy-status-bar 0",
      "return-type": "json",
      "escape": true
    }
  },
  {
    "layer": "top",
    "output": "DP-2",
    "modules-right": ["custom/tag"],
    "custom/tag": {
      "exec": "~/.local/lib/buoy-wm/buoy-status-bar 1",
      "return-type": "json",
      "escape": true
    }
  }
]
```

`"escape": true` is not optional. Tag names are arbitrary strings and waybar
renders module text as Pango markup with `escape` defaulting to `false`, so a
tag called `<3` produces invalid markup and blanks the label. `buoy-status-bar`
escapes its output for *JSON*, which is a different layer and does not help
here.

Output ids are `buoy`'s own, assigned in the order river announces monitors —
they are not connector names, and nothing translates between the two. Find
which is which by switching tags and watching which bar reacts. The module
emits a `normal` or `disconnected` CSS class for your stylesheet to target;
if the WM stops responding the bar degrades to the disconnected state rather
than freezing on a stale tag name.

## Configuration

Optional, at `~/.config/buoy/config.toml` (or under `$XDG_CONFIG_HOME`). With
no config file the built-in defaults apply unchanged.
[`docs/config.example.toml`](docs/config.example.toml) is a fully commented
copy of those defaults plus four clearly-marked illustrative extras — start
there, and delete what you do not want. Remember that declaring any
`[[keybind]]` replaces the whole built-in set.

```toml
[defaults]
terminal = "foot"      # also used for each tag's pinned terminal
launcher = "fuzzel"    # the `launcher` action only — see below
default_tag = "default"

[[keybind]]
mod = ["super"]
key = "1"
action = { switch_tag = "email" }   # created on first press

[[keybind]]
mod = ["super"]
key = "p"
action = { exec = "grim -g \"$(slurp)\" ~/shot.png" }
```

### When something in the file is wrong

A bad `[[keybind]]`, `[[mousebind]]` or `[[input]]` entry is **skipped**;
everything else in the file still applies. Every skipped entry is reported at
once, so a file with four independent mistakes takes one edit-and-restart
cycle rather than four.

Two problems are not skippable, because there is no single entry to drop: a
file that is not valid TOML, and a wrong value in `[defaults]`. Either one
falls back to the built-in defaults *entirely* — including the built-in
keybinds, which may be a keymap you have not used in months. Refusing to
start would leave you in a session with no way to reach the file and fix it,
so the WM starts either way.

Two more refusals are about the file rather than its contents: buoy reads at
most 1 MiB of it, and it must be a regular file — a directory or a FIFO at
that path is reported instead of read, because a read that never returns is a
login that never completes.

This file is as trusted as `~/.bashrc`: an `{ exec = "..." }` binding runs
through `sh -c` in your session. So if it is group- or world-**writable**, or
owned by another user, buoy loads it anyway and tells you — the same warning
bash gives a world-writable profile. `chmod 600 ~/.config/buoy/config.toml`
clears it. The default `644` is fine; who can *read* the file is not the
concern.

Both cases raise a desktop notification naming the file and the problem, and
log it. To read the log:

```sh
journalctl --user -b --identifier=buoy-wm
journalctl --user -b --identifier=buoy-wm -p err   # failures only
```

### Bindings

**Declaring any `[[keybind]]` replaces the entire built-in set**, so list
every binding you want. That is what makes a default rebindable *and*
removable. `[[mousebind]]` is a separate list with the same rule. Binding the
same trigger twice is an error, not a silent race.

Keys are keysym names: a single character (`a`, `1`, `?`), a named key
(`Return`, `Space`, `Tab`, `Escape`, `Left`, `Page_Up`), or `F1`–`F35`. Named
keys are case-insensitive; **single characters are not**. An uppercase letter
is the *shifted* symbol, so `Super`+`Q` can never fire — write `key = "q"`,
or add `"Shift"`. That binding is skipped at load, and reported, rather than
silently doing nothing. `mod` is optional; omit it to bind an unmodified key.

`mod` values are `super` (or `mod4`), `ctrl` (or `control`), `alt` (or
`mod1`) and `shift`; `button` values are `left`, `right` and `middle`. The
PascalCase spellings this file used to document (`"Super"`, `"Left"`) are
still accepted, so an existing config needs no edit.

Actions: `terminal`, `launcher`, `close`, `focus_next`, `exit`, `cycle_tag`,
`tag_picker`, `tag_switch`, `hotkeys`, `{ switch_tag = "<name>" }`, and
`{ exec = "<command>" }`. `exec` runs through `sh -c`, so pipes, arguments
and `~` work as they would in a shell. `move` and `resize` drive a pointer
drag and are mousebind-only.

### Programs

`launcher` sets what the `launcher` action spawns, and nothing else. `fuzzel`
stays hardcoded where it is driven as a *menu* rather than a launcher — the
two tag pickers and the cheat-sheet — because those pass fuzzel-specific
flags (`--dmenu`, `--layer`, `--prompt`).

A tag's pinned terminal is spawned as `terminal` plus `pinned_terminal_args`,
defaulting to foot's spelling:

```toml
pinned_terminal_args = ["-a", "{app_id}", "zellij", "attach", "--create", "{session}"]
```

`pinned_terminals = false` turns the feature off entirely — the supported
escape hatch if your terminal or `zellij` is broken, or if you just do not
want one. Tags, the pickers and the status bar work exactly as before; only
the per-tag terminal and its backdrop go away. Emptying `pinned_terminal_args`
is not the off switch and is rejected as a mistake, since it would spawn a
terminal carrying no app id that nothing could recognise.

Set it alongside `terminal` if your terminal spells the app-id flag
differently — most use `--class`. `{app_id}` is **required**: it is how the
WM recognises the pinned window, and its absence is rejected at load. It is
substituted per tag, as `pinned-term-<tag id>` (for example
`pinned-term-3`), which is how a mapped window says which tag it belongs to
rather than the WM having to guess from the order spawns happen to start in.
`{session}` becomes the tag's zellij session name.

### Input devices

Touchpads, keyboards and mice are configured with `[[input]]` blocks matched
against libinput device names:

```toml
[[input]]
name = "*Touchpad*"           # `*` is the only wildcard
tap = true                    # 1/2/3 fingers = left/right/middle
tap_button_map = "lrm"        # or "lmr" (1/2/3 = left/middle/right)
click_method = "button_areas" # or "clickfinger", "none"
natural_scroll = true
disable_while_typing = true
accel_speed = 0.3             # -1.0 (slowest) .. 1.0 (fastest)
```

The only built-in entry is `name = "*Touchpad*"` with `tap = true`, because
libinput ships tap-to-click *disabled* on any device with physical buttons —
so without it a laptop touchpad accepts motion and scrolling but ignores taps
entirely. Matching is on name rather than the protocol's device *type*, which
reports plain `pointer` for touchpads, mice and trackballs alike; forcing tap
on an external mouse is not wanted.

An omitted setting is **not** the same as `false`. Omitting one sends no
request and leaves libinput's default alone; `tap = false` actively turns tap
off. As with bindings, declaring any `[[input]]` replaces the built-in list,
so restate `tap = true` for your touchpad if you add entries for other
devices. Entries match top to bottom, first match wins — which is what lets a
specific device sit above a catch-all `name = "*"`.

Skipped at load and reported, rather than looking configured and doing
nothing: a pattern matching no device, an entry setting nothing, a duplicate
pattern, and an `accel_speed` outside `-1.0..=1.0`. Configured devices log
their tap state and click method at startup and after each setting is
applied; anything the device rejects is named in the log.

Find device names with `libinput list-devices` (needs root) or read them out
of `/proc/bus/input/devices`.

## IPC

The WM listens on a Unix socket at `$XDG_RUNTIME_DIR/buoy-wm.sock`, speaking
newline-delimited JSON. `buoy-tag-picker` and `buoy-status-bar` are its only clients,
but the protocol is plain enough to script against:

```sh
echo '{"type":"get-state"}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/buoy-wm.sock
```

There is no fallback path. If `$XDG_RUNTIME_DIR` is unset, empty, relative,
or names a directory that is not yours or that other users can reach, the WM
logs the reason and runs with no IPC: `Super+A`, `Super+S` and every status
bar are inert for that session, and window management is unaffected. Every
connection is also checked with `SO_PEERCRED` at both ends and refused unless
both processes run as the same user.

## Development

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

`.devcontainer/` carries a ready toolchain if you would rather not install
one. `pre-commit run --all-files` runs the fmt and clippy gates; it is
deliberately not `pre-commit install`ed, since the hooks need a toolchain the
host may not have.

The decisions behind the tag model, the river-over-Hyprland choice and the
fuzzel-driven picker are recorded in [`docs/adrs.md`](docs/adrs.md). Several
are cited by number from the code.

## License

[Reciprocal Public License 1.5](LICENSE.md). RPL 1.5 is a strong reciprocal
license: if you deploy a modified `buoy`, including internally, you are
required to publish your changes. Personal and research use carry no such
obligation.

Third-party material redistributed here — river's protocol XML (MIT) and the
[`tinyrwm`](https://codeberg.org/river/tinyrwm) scaffolding this started from
(0BSD) — is itemised in [NOTICE.md](NOTICE.md).
