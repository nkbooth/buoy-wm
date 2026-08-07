# Story 2.4: Tag-Switching via Picker
Epic: 2 | Priority: H | Status: pending

## Description
A second hotkey opens the same picker component for tag-switching, so one
consistent UI handles both tagging and switching.

## Acceptance criteria
**Given** any window or none is focused
**When** I press the tag-switch hotkey
**Then** the same `tag-picker` binary opens in switch mode, listing all registry tags
**And** it opens on the currently-focused output — never a fixed "home" output — per `EXPERIENCE.md` Interaction Primitives ("Picker follows focus")
**When** I select a tag
**Then** `tag-picker` sends `switch-tag` over IPC and the active output's displayed tag changes, subject to one-tag-per-output enforcement (Story 1.3), within the 50ms latency budget (NFR1)

- [ ] Tests pass (unit + integration where applicable)
- [ ] Code review: PASS

## Technical notes
Depends on Story 2.1–2.3. Multi-monitor context: laptop-only and
docked-to-42"-widescreen are both first-class; occasional true multi-monitor
(per `EXPERIENCE.md` Foundation). UX reference: `EXPERIENCE.md` Interaction
Primitives, Key Flows → Flow B.

> Output-placement AC added 2026-08-06 during implementation-readiness
> review — the original story text didn't specify which output the
> switch-mode picker appears on in multi-monitor; UX resolved it as
> focused-output.

## Test plan
TBD during dev loop.

## FR coverage
FR9, NFR1
