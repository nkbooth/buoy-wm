---
name: buoy-wm
description: Terminal-native dark UI for a personal river/Wayland window manager — a fuzzel-driven tag picker and a waybar tag module, inheriting the same palette and font already used across foot/zellij.
status: final
sources:
  - docs/planning/prd/
  - docs/planning/architecture/
  - _bmad-output/planning-artifacts/epics.md
  - docs/planning/brainstorm-2026-08-06.md
  - ~/.local/share/chezmoi/dot_config/foot/foot.ini (colors-dark palette, inherited)
updated: 2026-08-06
colors:
  surface-base: '#101216'
  surface-base-elevated: 'rgba(16, 18, 22, 0.84)'
  surface-backdrop: '#06070a'
  surface-raised: '#0a0b0d'
  ink-primary: '#d6d9df'
  ink-bright: '#f3f4f6'
  ink-secondary: '#748394'
  ink-disabled: '#5a6270'
  selection-bg: '#2b3038'
  selection-fg: '#f3f4f6'
  accent-ok: '#90a482'
  accent-error: '#a46a6a'
  accent-error-wash: 'rgba(164, 106, 106, 0.1)'
  accent-secondary: '#8b7a8f'
typography:
  body:
    fontFamily: "'JetBrainsMono Nerd Font', 'JetBrains Mono', monospace"
    fontSize: 14px
  row:
    fontFamily: "'JetBrainsMono Nerd Font', 'JetBrains Mono', monospace"
    fontSize: 12px
  meta:
    fontFamily: "'JetBrainsMono Nerd Font', 'JetBrains Mono', monospace"
    fontSize: 11px
  micro:
    fontFamily: "'JetBrainsMono Nerd Font', 'JetBrains Mono', monospace"
    fontSize: 10px
rounded:
  sm: 3px
  md: 4px
  lg: 8px
spacing:
  '1': 4px
  '2': 6px
  '3': 8px
  '4': 12px
  gutter: 48px
  window-pad: 10px
components:
  picker-window:
    background: '{colors.surface-base-elevated}'
    blur: true
    rounded: '{rounded.lg}'
    padding: '{spacing.window-pad}'
  picker-row:
    fontFamily: '{typography.row.fontFamily}'
    fontSize: '{typography.row.fontSize}'
    rounded: '{rounded.md}'
    selected:
      background: '{colors.selection-bg}'
      color: '{colors.selection-fg}'
  picker-checkbox-checked:
    borderColor: '{colors.accent-ok}'
    color: '{colors.accent-ok}'
    background: 'rgba(144,164,130,0.12)'
  picker-rejection-row:
    color: '{colors.accent-error}'
    background: '{colors.accent-error-wash}'
    rounded: '{rounded.sm}'
  picker-newrow-placeholder:
    color: '{colors.ink-disabled}'
  bar-module:
    background: '{colors.surface-raised}'
    fontFamily: '{typography.meta.fontFamily}'
    fontSize: '{typography.meta.fontSize}'
  bar-tag-normal:
    color: '{colors.ink-primary}'
  bar-tag-disconnected:
    color: '{colors.ink-disabled}'
    glyphColor: '{colors.accent-error}'
---

# buoy-wm — Design Spine

> Two surfaces, one palette: a `fuzzel`-driven tag-manager picker (the only overlay in the system) and a small waybar tag module. Both inherit the dark, blurred, low-chroma terminal aesthetic already established in `foot.ini` — this is not a new brand, it's the same desk extended to two more surfaces. Illustrated by `mockups/key-picker.html` and `mockups/key-bar.html`.

## Brand & Style

Terminal-native, not app-native. No logo, no marketing surface, no onboarding — this is desk furniture for a single daily driver (Nick). The posture is quiet: nothing here should compete for attention with the actual work happening in the pinned zellij session or floating windows. Chrome exists only to answer two questions at a glance — *what tag am I on* and *did anything just go wrong* — and to accept two kinds of input — *toggle a tag* and *type a new one*.

## Colors

- `{colors.surface-base}` / `{colors.surface-base-elevated}` — the picker window body, always at 0.84 alpha with blur, matching `foot`'s `colors-dark` exactly so the picker never looks like a foreign surface floating over a terminal-heavy desktop.
- `{colors.surface-backdrop}` — used only in mocks to represent the desktop behind the picker; not a token buoy-wm itself renders.
- `{colors.ink-primary}` / `{colors.ink-bright}` — primary text and the brighter selected-row text. Never used for error states.
- `{colors.ink-secondary}` / `{colors.ink-disabled}` — secondary/meta text and placeholder/disabled text (also doubles as the bar's disconnected-state tag color — dimming *is* the disconnect signal, not a separate hue).
- `{colors.accent-ok}` — checked-checkbox state only. Never used elsewhere; it must stay legible as "this tag is active on this window."
- `{colors.accent-error}` / `{colors.accent-error-wash}` — reserved exclusively for the two error states: picker cap-out rejection and bar disconnect glyph. Not used for hover, focus, or any non-error emphasis — if it's red-adjacent, something is wrong.

## Typography

Single family throughout (`{typography.body.fontFamily}`) — same Nerd Font already used in `foot`, so glyphs (the disconnect warning icon) render without a fallback-font mismatch. Four sizes only, all in the 10–14px range appropriate to a compact overlay and a slim bar: `{typography.body}` for the picker's row text baseline, `{typography.row}` for compact list rows, `{typography.meta}` for the bar module, `{typography.micro}` for the smallest inline labels (e.g. the rejection message). No weight ramp — regular weight throughout; state is carried by color and background, not boldness.

## Layout & Spacing

Spacing scale is small and flat (`{spacing.1}`–`{spacing.4}`, plus `{spacing.gutter}` for the mock's illustrative side-by-side state comparison — not a real on-screen gutter). `{spacing.window-pad}` is the picker window's outer padding. Rows stack with `{spacing.2}` internal padding and `{spacing.3}` between elements. No grid system — both surfaces are single-column stacks (picker: list + one input row; bar: one text element).

## Elevation & Depth

The picker window is the only elevated surface in the system — it sits above the floating-window layer via alpha + blur (`{colors.surface-base-elevated}`, blur: true), not a drop shadow (matches `foot`'s treatment; shadows are absent everywhere else in this palette). The bar module has no elevation of its own — it inherits waybar's own bar-level compositing.

## Shapes

Two radii: `{rounded.lg}` for the picker window itself (the one "object" a user perceives), `{rounded.md}` for its internal rows, `{rounded.sm}` for the smallest inline element (the rejection message chip). Nothing pill-shaped, nothing sharp — soft-cornered rectangles throughout, consistent with `foot`'s own rendering (which has no radius concept but sets the tonal register this extends).

## Components

### `picker-window`
The fuzzel-rendered overlay. `{components.picker-window}` — elevated dark surface, blurred, `{rounded.lg}`, `{spacing.window-pad}` padding. Appears on the currently-focused output only.

### `picker-row`
One row per tag in the registry. Unchecked: `{colors.ink-primary}` on transparent. Selected/focused row: `{components.picker-row.selected}` (`{colors.selection-bg}` background, `{colors.selection-fg}` text) — this is keyboard-cursor focus, distinct from checkbox-checked state.

### `picker-checkbox-checked`
`{components.picker-checkbox-checked}` — border and glyph in `{colors.accent-ok}`, faint tinted background. Unchecked checkboxes render as an empty outline in `{colors.ink-secondary}`.

### `picker-newrow`
Free-text add-new-tag input, bottom row. Placeholder text in `{components.picker-newrow-placeholder}` before typing.

### `picker-rejection-row`
`{components.picker-rejection-row}` — appears in place of/beneath the new-row input when the 64-tag cap is hit. Literal message text ("tag limit reached (64)"), `{typography.micro}`, error-wash background.

### `bar-module`
`{components.bar-module}` — waybar custom module, top of screen, one per output. Normal state: `{components.bar-tag-normal}`. Disconnected state: `{components.bar-tag-disconnected}` — dimmed text plus a small warning glyph rendered in `{colors.accent-error}`.

## Do's and Don'ts

- **Do** keep the picker and bar on exactly one shared palette — no per-surface color drift.
- **Do** reserve `{colors.accent-error}` for genuine error/disconnect states only.
- **Don't** add elevation/shadow effects anywhere — this palette communicates depth through alpha+blur, not shadow.
- **Don't** introduce a second font family or a bold weight — state is color/background-only.
- **Don't** animate transitions — NFR1's 50ms action budget and the "no motion" concern-scan decision rule out fades/slides on picker open/close or bar updates.
