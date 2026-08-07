# Story 1.4: Baseline Keybind Routing
Epic: 1 | Priority: H | Status: pending

## Description
Basic keybinds for spawning a terminal, closing the focused window, cycling
focus, and exiting the session — adapted from the `tinyrwm` skeleton's keybind
set. Makes the WM operable at all before any picker exists.

## Acceptance criteria
**Given** the WM is running with baseline keybinds active (adapted from `tinyrwm`)
**When** I press the spawn keybind
**Then** a new `foot` window opens and receives focus
**When** I press the close keybind on a non-pinned window
**Then** that window closes
**When** I press the close keybind while the pinned terminal is focused
**Then** nothing happens — the pinned terminal is never closed via any routed keybind (architectural constraint)
**When** I press the focus-cycle keybind
**Then** keyboard focus moves to the next window in stacking order
**When** I press the exit keybind
**Then** the Wayland session ends cleanly

- [ ] Tests pass (unit + integration where applicable)
- [ ] Code review: PASS

## Technical notes
Depends on Story 1.2 (state model).

## Test plan
TBD during dev loop.

## FR coverage
FR12
