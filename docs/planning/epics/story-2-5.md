# Story 2.5: Per-Output Status Bar
Epic: 2 | Priority: H | Status: pending

## Description
An always-visible bar element per output showing the currently selected tag,
via a waybar custom module — resolves the `wlr-layer-shell` rejection gap
(no layer-shell surface; `status-bar` drives waybar instead of rendering its
own window).

## Acceptance criteria
**Given** the `status-bar` client is running
**When** an output's current tag changes (via keybind, picker, or WM startup default)
**Then** the corresponding bar element updates to show the new tag name
**And** this holds independently for each connected output
**Given** the IPC connection to the WM drops
**Then** the bar does not crash and indicates a disconnected/stale state (dimmed text + disconnect glyph, per `DESIGN.md` → `bar-tag-disconnected`) rather than showing stale data silently
**And** the bar returns to the normal state automatically once the IPC connection is restored — no user action required

- [ ] Tests pass (unit + integration where applicable)
- [ ] Code review: PASS

## Technical notes
Depends on Story 2.1 (IPC server). Architecture: `status-bar` feeds a waybar
custom module (script/polling or signal-driven — exact mechanism decided at
implementation) — see `docs/planning/architecture/components.md` and
`external-integrations.md` (updated 2026-08-06). UX reference:
`EXPERIENCE.md` State Patterns (bar rows), `DESIGN.md` Components →
`bar-module`, `bar-tag-normal`, `bar-tag-disconnected`. Visual reference:
`mockups/key-bar.html`. Bar position: top of screen.

> Reconnect-recovery AC added 2026-08-06 during implementation-readiness
> review — the original story text covered detecting disconnect but not
> automatic recovery on reconnect; UX specified it explicitly.

## Test plan
TBD during dev loop.

## FR coverage
FR10
