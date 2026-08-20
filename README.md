# buoy-wm

A Wayland window manager for `river` 0.4+, speaking
`river-window-management-v1`. It owns all tag/workspace state itself (the
compositor has none), gives every tag a persistent pinned `zellij` terminal
backdrop, floats everything else above it, and exposes an IPC socket that
drives a `fuzzel`-based tag-manager picker and a status bar.

## Configuration

Optional, at `~/.config/buoy/config.toml` (or `$XDG_CONFIG_HOME/buoy/`).
With no config file, the defaults below apply unchanged. See
[`docs/config.example.toml`](docs/config.example.toml) for a fully
commented example.

```toml
[defaults]
terminal = "foot"      # also used for each tag's pinned terminal
launcher = "fuzzel"    # the `launcher` action only — see note below
default_tag = "default"

[[keybind]]
mod = ["Super"]
key = "1"
action = { switch_tag = "email" }   # created on first press

[[keybind]]
mod = ["Super"]
key = "P"
action = { exec = "grim -g \"$(slurp)\" ~/shot.png" }
```

Declaring any `[[keybind]]` replaces the entire built-in set, so list every
binding you want — that is what makes a default rebindable *and* removable.
`[[mousebind]]` is a separate list with the same rule. A malformed config is
reported on stderr and the built-in defaults are used, rather than refusing
to start a session you would then have no way to fix the file from.

Key names are keysyms: a single character (`a`, `1`, `?`), a named key
(`Return`, `Space`, `Tab`, `Escape`, `Left`, `Page_Up`), or `F1`–`F35`.
Named keys are case-insensitive; single characters are **not**. An
uppercase letter is the *shifted* symbol, so `Super`+`Q` can never fire —
write `key = "q"` for Super+q, or add `"Shift"`. The config is rejected at
load if you get this wrong, rather than the binding silently doing nothing.
`mod` is optional; omit it to bind an unmodified key.

Actions: `terminal`, `launcher`, `close`, `focus_next`, `exit`,
`cycle_tag`, `tag_picker`, `tag_switch`, `hotkeys`,
`{ switch_tag = "<name>" }`, `{ exec = "<command>" }`. `exec` runs through
`sh -c`, so pipes and arguments work. `move` and `resize` drive a pointer
drag and are mousebind-only. Binding the same trigger twice is an error.

`launcher` sets the program the `launcher` action spawns, and nothing else.
`fuzzel` remains hardcoded where it is driven as a menu rather than as a
launcher — the tag pickers (which spawn it from the separate `tag-picker`
binary) and the `hotkeys` cheat-sheet — because those pass fuzzel-specific
flags (`--dmenu`, `--layer`, `--prompt`).

A tag's pinned terminal is spawned as `terminal` plus
`pinned_terminal_args`, which defaults to foot's
`["-a", "{app_id}", "zellij", "attach", "--create", "{session}"]`. Set it
alongside `terminal` if your terminal spells the app-id flag differently
(most use `--class`). `{app_id}` is required — it is how the WM recognises
that window — and its absence is rejected at load.

## Keybindings

These are the defaults, used when no config file overrides them. All use
`Mod4` (the "Super"/"Windows" key) as the modifier.

| Keybind | Action |
| --- | --- |
| `Super+Space` | Open a new floating terminal (`foot`) |
| `Super+N` | Rotate focus to the next window |
| `Super+Q` | Close the focused window (the pinned terminal can't be closed this way) |
| `Super+Tab` | Cycle the active output to the next tag |
| `Super+R` | Open the application launcher (`fuzzel`) |
| `Super+A` | Open the tag-assignment picker for the focused window |
| `Super+S` | Open the tag-switch picker for the active output (also creates tags) |
| `Super+Shift+?` | Show the hotkey cheat-sheet |
| `Super+Esc` | Exit the session |
| `Super+Left-click` | Move a window (drag) |
| `Super+Right-click` | Resize a window (drag) |

The cheat-sheet is generated from the bindings actually in effect, so it
reflects your config rather than this table.

"The active output" is whichever monitor the pointer is currently over.

### Tag-assignment picker (`Super+A`)

Opens a `fuzzel` checklist of every tag, checked for the tags the focused
window currently has. Type to filter the list; selecting a row toggles that
tag on the window and reopens the list, so several tags can be toggled in
one visit. Only existing tags — new tags are created from the switcher
below.

### Tag-switch picker (`Super+S`)

Opens a `fuzzel` list of every tag. Selecting one switches the active
output to display that tag (spawning its pinned terminal on first use).

Typing a name that matches no existing tag and confirming it creates that
tag and switches to it in the same action — creating a tag is how you start
working somewhere new, so it lives here rather than in the assignment
picker. Typing the exact name of a tag that already exists switches to it
rather than duplicating it. At the 64-tag registry cap the picker reopens
with the rejection shown and your typed name restored, and nothing is
created or switched.

## Input devices

Touchpads, keyboards and mice are configured with `[[input]]` blocks, each
matching devices by libinput name:

```toml
[[input]]
name = "*Touchpad*"           # `*` is the only wildcard
tap = true                    # tap-to-click: 1/2/3 fingers = left/right/middle
tap_button_map = "lrm"        # or "lmr" (1/2/3 = left/middle/right)
click_method = "button_areas" # or "clickfinger", "none"
natural_scroll = true
disable_while_typing = true
accel_speed = 0.3             # -1.0 (slowest) .. 1.0 (fastest)
```

The only built-in entry is `name = "*Touchpad*"` with `tap = true`, because
libinput defaults tap-to-click to *off* on any device with physical buttons
— so without it a laptop touchpad accepts motion and scrolling but ignores
taps entirely. Matching is on the name rather than the protocol's device
*type*, which reports only `pointer` for touchpads, mice and trackballs
alike; forcing tap on an external mouse is not wanted.

Every setting is optional, and an omitted one is **not** the same as
`false`: an omitted setting sends no request at all and leaves libinput's
own default alone, while `tap = false` actively turns tap off. As with the
binding lists, declaring any `[[input]]` replaces the built-in list
entirely, so restate `tap = true` for your touchpad if you add entries for
other devices. Entries are matched top to bottom and the first match wins,
which is what lets a specific device sit above a catch-all `name = "*"`.

A pattern matching nothing, an entry setting nothing, a duplicate pattern,
and an `accel_speed` outside `-1.0..=1.0` are all rejected at load —
otherwise the section would look configured and silently do nothing.

Requires river's `river_input_manager_v1` and `river_libinput_config_v1`
globals. Both are optional: on a river without them the WM logs once and
every device keeps libinput's defaults. For a device you have configured,
its tap state and click method are logged at startup and again after each
setting is applied, and any setting the device rejects is named on stderr.

## Development

Build and test the full workspace (`wm`, `tag-picker`, `status-bar`):

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Installing

The session must not run out of `target/`, which `cargo clean` wipes. Build a
release inside the devcontainer, then install from the host:

```sh
cargo build --workspace --release   # in the devcontainer
./scripts/install.sh                # on the host
```

All three binaries go to `~/.local/lib/buoy-wm` (override with
`BUOY_INSTALL_DIR`), and they must stay in one directory — the WM finds
`tag-picker` as a sibling of its own executable. Only `buoy-wm` is symlinked
onto `PATH`; nothing resolves the other two through it.

River `exec`s the WM as the session leader, so a broken install means a black
screen and a bounce back to GDM with no shell to recover from. Verify from a
TTY or a nested river before logging out.

Story specs and Dev Agent Records live under `docs/planning/epics/`.
