# Story 1.5: Pinned Terminal Lifecycle
Epic: 1 | Priority: H | Status: pending

## Description
A persistent zellij-backed terminal automatically available on every tag,
lazily spawned on first switch to that tag.

## Acceptance criteria
**Given** a tag has no pinned terminal yet
**When** the output's current tag switches to it for the first time
**Then** the WM spawns `foot -a pinned-term` running `zellij attach --create tag-<name>`
**And** marks that tag's `terminal_spawned` as true
**Given** the pinned terminal is running
**Then** it is always tiled and always rendered at the bottom of the render order
**And** it is never respawned outside this lazy-spawn-once path, even if the process exits (architectural constraint)

> **Clarification (added 2026-08-06, implementation-readiness review):** the
> tag-switch trigger above is exercised via `wm-core`'s internal switch-tag
> function (established in Story 1.3), directly from a test harness — it does
> **not** require the Story 1.7 keybind wrapper to exist first. Not a forward
> dependency.

- [ ] Tests pass (unit + integration where applicable)
- [ ] Code review: PASS

## Technical notes
Depends on Story 1.3 (switch-tag operation exists in `wm-core`). Never route
a close keybind to the pinned terminal, and never respawn it outside the
lazy-spawn-once path.

## Test plan
TBD during dev loop.

## FR coverage
FR3, FR4
