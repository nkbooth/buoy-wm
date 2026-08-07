# Features and acceptance criteria

## Core WM state & tag enforcement
**Acceptance criteria:**
- [ ] WM tracks window↔tags, window position, workspace ordering/layout, focus,
      output→current-tag, tag registry, and tags-with-spawned-terminal in memory
- [ ] At most one tag is displayed per output at a time; the WM enforces this
      rather than relying on user discipline

## Pinned terminal lifecycle
**Acceptance criteria:**
- [ ] First switch to a tag with no terminal yet spawns `foot -a pinned-term`
      running `zellij attach --create tag-<name>`
- [ ] Pinned terminal is always tiled, always bottom of render order, and
      cannot be closed via any routed keybind

## Floating placement
**Acceptance criteria:**
- [ ] Any non-pinned-term window defaults to floating, rendered above the
      pinned terminal

## Tag-manager popup
**Acceptance criteria:**
- [ ] Hotkey on a focused window opens a fuzzel picker showing checkboxes for
      its current tags
- [ ] Toggling a checkbox adds/removes that tag via WM IPC
- [ ] A text-input row creates a new tag not already in the registry
- [ ] A second hotkey opens the same picker for tag-switching (selecting a tag
      switches the active output's displayed tag)

## Status bar
**Acceptance criteria:**
- [ ] Per-output bar element always shows the currently selected tag for that
      output
