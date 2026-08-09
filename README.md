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
launcher = "fuzzel"    # also used for the tag pickers
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
Named keys are case-insensitive; single characters are not.

Actions: `terminal`, `launcher`, `close`, `focus_next`, `exit`,
`cycle_tag`, `tag_picker`, `tag_switch`, `hotkeys`, `move`, `resize`,
`{ switch_tag = "<name>" }`, `{ exec = "<command>" }`. `exec` runs through
`sh -c`, so pipes and arguments work.

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
| `Super+S` | Open the tag-switch picker for the active output |
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
tag on the window. Typing a name that doesn't match an existing tag and
confirming it creates a new tag and applies it.

### Tag-switch picker (`Super+S`)

Opens a `fuzzel` list of every tag. Selecting one switches the active
output to display that tag (spawning its pinned terminal on first use).

## Development

Build and test the full workspace (`wm`, `tag-picker`, `status-bar`):

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Story specs and Dev Agent Records live under `docs/planning/epics/`.
