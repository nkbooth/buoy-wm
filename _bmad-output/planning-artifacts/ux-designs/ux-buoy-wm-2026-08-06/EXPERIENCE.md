---
name: buoy-wm
status: final
sources:
  - docs/planning/prd/
  - docs/planning/architecture/
  - _bmad-output/planning-artifacts/epics.md
  - docs/planning/brainstorm-2026-08-06.md
updated: 2026-08-06
---

# buoy-wm — Experience Spine

> Two companion clients around a headless WM daemon: a `fuzzel`-driven tag-manager picker (overlay, keyboard-only) and a waybar tag module (persistent, passive). Single user (Nick), hobby stakes, Fedora atomic host, `river` 0.4+. Paired with `DESIGN.md`. Spines win on conflict with any mock. Illustrated by `mockups/key-picker.html`, `mockups/key-bar.html`.

## Foundation

Desktop, Wayland (`river` compositor), keyboard-driven. No UI system — `fuzzel` renders the picker itself (buoy-wm's `tag-picker` client is a driver, not a renderer); the bar is a waybar custom module, not a self-rendered surface (see architecture note below). Laptop-only and docked-to-42"-widescreen are both first-class; occasional true multi-monitor. `DESIGN.md` is the visual identity reference for both surfaces.

**Architecture note carried from Discovery:** `wlr-layer-shell` is rejected in the PRD's technical constraints (output-scoped, not tag-scoped) with no replacement named. This UX pass resolved that gap by deciding the status-bar client does not render its own surface at all — it drives a waybar custom module over IPC instead. `docs/planning/architecture/external-integrations.md` and `components.md` have been updated accordingly (2026-08-06).

## Information Architecture

| Surface | Reached from | Purpose |
|---|---|---|
| Tag-picker (assign mode) | Assign-hotkey on a focused window | Toggle which tags the focused window belongs to; create a new tag |
| Tag-picker (switch mode) | Switch-hotkey, any focus | Change the active output's displayed tag |
| Status-bar tag module | Always visible (waybar, top of screen, per-output) | Passive: shows the current tag for that output; surfaces IPC disconnect |

One picker component, two modes, distinguished only by which hotkey opened it (FR9) — not two separate surfaces. No settings surface, no onboarding, no notification surface beyond the bar itself.

→ Composition reference: `mockups/key-picker.html`, `mockups/key-bar.html`. Spine wins on conflict.

## Voice and Tone

Minimal and literal — this is a systems tool, not a product with a voice. The only user-facing strings are: tag names (user-authored, verbatim), the free-text placeholder ("add tag…" or equivalent literal prompt), and the cap-out rejection message, which states the fact plainly: **"tag limit reached (64)"** — no apology, no suggested action, because there isn't one (registry is append-only in v1, ADR-006). The bar's disconnect state is a glyph + optional short label ("⚠ disconnected"), not a sentence.

## Component Patterns

- **Picker row (checkbox):** one row per registry tag, checkbox reflects the focused window's current membership (assign mode) or is not shown at all in switch mode (single-select-by-position — selecting *is* the action, no separate confirm). See `DESIGN.md` Components → `picker-row`, `picker-checkbox-checked`.
- **Picker free-text row:** always the last row in assign mode. Typing + Enter creates a new tag and checks it for the focused window in the same action (FR8). Not present in switch mode (switching only operates on existing tags).
- **Picker rejection state:** replaces/annotates the free-text row when the 64-tag cap is hit on creation attempt — literal message, not a toast, not a dialog (stays inside the fuzzel surface). See `DESIGN.md` → `picker-rejection-row`.
- **Bar tag module:** single text element per output, waybar-hosted. No click/hover interaction defined in v1 — display-only.

## State Patterns

| State | Trigger | Visual | Recovery |
|---|---|---|---|
| Picker — assign, at rest | Assign-hotkey pressed | Checkbox list, current membership pre-checked | Enter accepts, Esc cancels with no change |
| Picker — switch, at rest | Switch-hotkey pressed | Same list, no checkboxes; cursor-selected row = pending switch | Enter confirms switch, Esc cancels |
| Picker — cap-out rejection | New-tag text submitted at 64/64 registry | Rejection row appears, error-wash background (`DESIGN.md` accent-error) | Esc/Enter on existing text dismisses; tag is not created |
| Bar — normal | Output has a current tag and IPC is live | Tag text in `ink-primary` | — |
| Bar — disconnected/stale | IPC connection to `ipc-server` drops | Tag text dims to `ink-disabled`, disconnect glyph in `accent-error` appears | Bar returns to normal automatically once IPC reconnects (no user action) |

**Fuzzel capability fallback (open until Story 2.2's spike resolves):** primary interaction is native checkbox-toggle-in-one-invocation. If fuzzel can't do that, fall back to fuzzel's native multi-select (`--multi`) — user picks the full desired tag set in one pass, membership is replaced wholesale on accept, rather than a sequential toggle-and-reopen loop. `[ASSUMPTION]` — not yet verified against fuzzel's actual flag support.

## Interaction Primitives

- **Keyboard only.** No mouse interaction anywhere in either surface (concern-scan decision) — picker rows are keyboard-navigated (fuzzel default: type-to-filter, arrow/ctrl-n/p to move, Tab or equivalent to reach the free-text row).
- **Enter accepts, Esc cancels.** Universal across both picker modes — no destructive action is reachable without an explicit accept.
- **No animation.** Picker open/close and bar updates are instant — consistent with NFR1 (50ms action budget) and the concern-scan "no motion" decision.
- **Picker follows focus.** In multi-monitor, the picker always opens on the currently-focused output, never a fixed "home" output.

## Accessibility Floor

Hobby stakes, single sighted keyboard-only user — no screen-reader, motor-alternative-input, or i18n requirements captured. The one floor that *does* apply universally: color is never the sole carrier of state. Disconnect state pairs dimming with an explicit glyph (not color alone); cap-out rejection pairs the error wash with literal text (not color alone); checkbox-checked pairs a filled/outlined glyph change with the accent color (not color alone).

## Key Flows

**Flow A — Nick tags a window.** Nick is deep in a floating terminal window mid-task and realizes it belongs under a different context. He hits the assign-hotkey. Fuzzel opens on the focused output, showing the full tag registry with this window's current tags pre-checked. He arrows down, toggles on `deploy-watch`, hits Enter. The picker closes instantly; the window is now tagged, but since window-tags and the output's *displayed* tag are independent, nothing else on screen changes — the tag only becomes visible again when that output switches to `deploy-watch`.

**Flow B — Nick switches tags.** Nick wants to jump from his `work` context to `chat`. He hits the switch-hotkey from anywhere (no need to focus a specific window first). Fuzzel opens showing the full registry; he selects `chat`. The picker closes; the output's displayed tag updates immediately, the bar module at the top of that output re-renders to show `chat`, and if `chat`'s pinned zellij session hasn't been spawned yet this session, the WM lazily spawns it (`foot -a pinned-term` + `zellij attach --create tag-chat`) tiled beneath any floating windows already tagged `chat`.
