# Story 2.2: Tag-Manager Picker — Toggle Existing Tags
Epic: 2 | Priority: H | Status: pending

## Description
A hotkey on the focused window opens a picker showing its current tags as
checkboxes, so tags can be added/removed without leaving the keyboard.

## Acceptance criteria
**Given** a window is focused
**When** I press the tag-manager hotkey
**Then** a `fuzzel`-driven checklist opens showing that window's current tags checked and other registry tags unchecked
**When** I toggle a checkbox
**Then** the `tag-picker` client sends `toggle-tag` over IPC and the WM applies it within the 50ms latency budget (NFR1)
**Given** fuzzel's native support for checkbox-toggle interaction is unconfirmed (ADR-004 open item)
**Then** this story includes a short spike to confirm the flow works in one `fuzzel` invocation, falling back to fuzzel's native multi-select (`--multi`) if not — see EXPERIENCE.md State Patterns for the fallback spec

- [ ] Tests pass (unit + integration where applicable)
- [ ] Code review: PASS

## Technical notes
Depends on Story 2.1 (IPC server). UX reference:
`_bmad-output/planning-artifacts/ux-designs/ux-buoy-wm-2026-08-06/EXPERIENCE.md`
Component Patterns → "Picker row (checkbox)" and the fuzzel-fallback note;
`DESIGN.md` Components → `picker-row`, `picker-checkbox-checked`. Visual
reference: `mockups/key-picker.html`.

## Test plan
TBD during dev loop.

## FR coverage
FR6, FR7, NFR1
