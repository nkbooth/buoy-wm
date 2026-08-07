# Story 1.3: One-Tag-Per-Output Enforcement
Epic: 1 | Priority: H | Status: pending

## Description
Enforces that at most one tag is displayed per output at any time, rejecting
or rerouting any action that would violate it. Establishes the internal
switch-tag operation in `wm-core` that later stories (1.5, 1.7, 2.4) build on.

## Acceptance criteria
**Given** an output currently displaying tag A
**When** any action would assign tag B to display on that same output
**Then** the WM rejects or reroutes the action so exactly one tag remains displayed per output
**And** the enforcement decision completes within the 50ms internal latency budget (NFR1)
**And** this holds under both single-output and multi-output configurations

- [ ] Tests pass (unit + integration where applicable)
- [ ] Code review: PASS

## Technical notes
Depends on Story 1.2 (state model). The internal switch-tag function this
story exercises is reused directly (not duplicated) by Story 1.5's lazy
terminal spawn, Story 1.7's keybind wrapper, and Story 2.4's IPC handler.

## Test plan
TBD during dev loop.

## FR coverage
FR2, NFR1
