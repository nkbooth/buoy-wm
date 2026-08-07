# Story 2.3: Tag-Manager Picker — Create New Tag
Epic: 2 | Priority: H | Status: pending

## Description
A text-input row in the tag-manager picker to create a new tag on the fly,
without a separate flow.

## Acceptance criteria
**Given** the tag-manager picker is open
**When** I type a name not already in the registry into the input row and confirm
**Then** `tag-picker` sends `create-tag` over IPC, a new tag is registered (ADR-006 rules apply), and it's immediately applied to the focused window
**Given** the registry is at its 64-tag cap
**When** I attempt to create another
**Then** the picker surfaces the WM's rejection rather than silently failing — render as a literal message row ("tag limit reached (64)"), not a toast/dialog

- [ ] Tests pass (unit + integration where applicable)
- [ ] Code review: PASS

## Technical notes
Depends on Story 2.2 (picker open, checkbox rows) and Story 2.1 (IPC). UX
reference: `EXPERIENCE.md` Component Patterns → "Picker rejection state";
`DESIGN.md` Components → `picker-rejection-row`, `picker-newrow-placeholder`.
Visual reference: `mockups/key-picker.html` (cap-out section).

## Test plan
TBD during dev loop.

## FR coverage
FR8
