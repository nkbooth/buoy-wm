# Story 1.7: Raw-Keybind Tag Switching & Creation
Epic: 1 | Priority: H | Status: pending

## Description
Keybind-driven tag switching and creation, sufficient to validate the
multi-tag model end-to-end before the fuzzel-based picker exists.

## Acceptance criteria
**Given** at least one tag exists in the registry
**When** I invoke the tag-cycle keybind
**Then** the active output's current tag advances to the next tag in the registry, subject to one-tag-per-output enforcement (Story 1.3)
**And** the switch completes within the 50ms latency budget (NFR1)
**When** I invoke the tag-create keybind and supply a new name
**Then** a new `Tag` is added to the registry with a freshly assigned bitmask ID, never reusing a previously assigned ID (ADR-006)
**Given** 64 tags already exist in the registry
**When** I attempt to create another
**Then** creation fails and the failure is surfaced rather than silently ignored
**And** the `wm-core` functions this exercises (switch, create) are the same functions Epic 2's IPC handlers call — no duplicate logic path

- [ ] Tests pass (unit + integration where applicable)
- [ ] Code review: PASS

## Technical notes
Depends on Story 1.3 (switch-tag) and Story 1.2 (registry). Reused directly
by Epic 2's IPC handlers (Story 2.1) — not thrown away.

## Test plan
TBD during dev loop.

## FR coverage
FR13, NFR1
