# Story 1.2: WM Core State Model
Epic: 1 | Priority: H | Status: pending

## Description
The single in-memory source of truth (`wm-core`) for window↔tag membership,
view geometry, focus, output→current-tag, the tag registry, and
terminal-spawned status.

## Acceptance criteria
**Given** the WM has connected to `river` via `river-window-management-v1`
**When** a view is mapped, unmapped, or its state changes
**Then** `wm-core` updates its in-memory `View` (id, app_id, tags, floating, geometry, focused), `Tag` (id, name, terminal_spawned), and `Output` (id, current_tag) records accordingly
**And** the registry starts empty on WM launch — no persisted state from a prior run (per data-model.md)
**When** the compositor sends malformed or unexpected client events
**Then** the WM logs and discards the event without crashing or corrupting state (NFR2)
**And** WM daemon idle RSS stays under 50MB with no views mapped (NFR3)

- [ ] Tests pass (unit + integration where applicable)
- [ ] Code review: PASS

## Technical notes
Depends on Story 1.1 (devcontainer/project scaffold).

## Test plan
TBD during dev loop.

## FR coverage
FR1, NFR2, NFR3
