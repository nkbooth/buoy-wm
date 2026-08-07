# Scope

## In scope (v1)
- Core WM built on the `tinyrwm` Rust skeleton, targeting
  `river-window-management-v1`
- WM-owned tag state: window↔tag membership, tag registry (named, created on
  demand), per-output current-tag, tags-with-spawned-terminal
- One-tag-per-output-max enforcement rule
- Pinned-terminal-per-tag lifecycle: lazy spawn on first switch to a tag with
  none yet, forced-tiled via a distinct app-id (`foot -a pinned-term`), never
  closed, running `zellij attach --create tag-<name>`
- Floating-above-tiled placement: everything but the pinned terminal defaults
  to floating, rendered above it
- Tag-manager popup: fuzzel-driven, hotkey-triggered on the focused window;
  checkboxes toggle existing tags, a text-input row creates a new arbitrary tag
  name; the same picker component is reused (different hotkey) for tag-switching
- WM IPC: Unix socket connecting the WM process to the picker/bar clients
  (message format TBD — see open questions)
- Always-visible per-output status bar showing the currently selected tag

## Out of scope (v1)
- Session presets (config-file-defined tag/layout bundles, hotkey-invoked)
- Output/monitor assignment via the picker pattern (parked — real idea, later)
- Tag-first reverse lookup ("show all windows tagged X")
- Any river-classic compatibility path
