# Story 1.6: Floating Placement For Non-Pinned Windows
Epic: 1 | Priority: H | Status: pending

## Description
Every window except the pinned terminal floats above it, so active work is
always visible over the terminal backdrop.

## Acceptance criteria
**Given** a newly mapped view whose app-id is not `pinned-term`
**When** the WM places it
**Then** it defaults to floating and renders above the pinned terminal
**And** this holds regardless of which tag or output it's mapped on

- [ ] Tests pass (unit + integration where applicable)
- [ ] Code review: PASS

## Technical notes
Depends on Story 1.2 (state model).

## Test plan
TBD during dev loop.

## FR coverage
FR5
