---
stepsCompleted: [document-discovery, prd-analysis, epic-coverage-validation, ux-alignment, epic-quality-review, final-assessment]
filesIncluded:
  prd: docs/planning/prd/ (sharded)
  architecture: docs/planning/architecture/ (sharded)
  epics: _bmad-output/planning-artifacts/epics.md (whole)
  ux: _bmad-output/planning-artifacts/ux-designs/ux-buoy-wm-2026-08-06/ (DESIGN.md + EXPERIENCE.md, status:final)
---

# Implementation Readiness Assessment Report

**Date:** 2026-08-06
**Project:** buoy-wm

## Document Inventory

### PRD
- Sharded: `docs/planning/prd/` (index.md + 9 section files)

### Architecture
- Sharded: `docs/planning/architecture/` (index.md + 10 section files + adrs.md)
- Reconciled 2026-08-06 against the new UX doc's status-bar/waybar decision (components.md, external-integrations.md, technical-constraints.md all updated)

### Epics & Stories
- Whole: `_bmad-output/planning-artifacts/epics.md` (18.5KB)
- No separate sharded stories folder found
- Line 58 (UX section) updated 2026-08-06 to point at the new UX doc rather than claiming none was needed

### UX Design
- `_bmad-output/planning-artifacts/ux-designs/ux-buoy-wm-2026-08-06/DESIGN.md` and `EXPERIENCE.md` — completed 2026-08-06 via `bmad-ux`, status: final. Covers the two visual/interactive surfaces (tag-picker, status-bar); no UI beyond those exists in this project.

## PRD Analysis

The PRD (`docs/planning/prd/`) does not itself number FRs/NFRs — its `features-and-acceptance-criteria.md` groups requirements by feature with checkbox acceptance criteria. The FR1–FR13/NFR1–NFR3 numbering used for traceability below is `epics.md`'s canonical mapping onto those same PRD acceptance criteria (no drift found between the two — every FR/NFR traces to a PRD acceptance-criteria line).

### Functional Requirements

FR1: The WM tracks window↔tags, window position, workspace ordering/layout, focus, output→current-tag, tag registry, and tags-with-spawned-terminal in memory.
FR2: The WM enforces at most one tag displayed per output at a time (rejects/reroutes any action that would violate this).
FR3: On first switch to a tag with no terminal yet, the WM lazily spawns `foot -a pinned-term` running `zellij attach --create tag-<name>`.
FR4: The pinned terminal is always tiled, always rendered at the bottom of render order, and cannot be closed via any routed keybind.
FR5: Any non-pinned-term window defaults to floating, rendered above the pinned terminal.
FR6: A hotkey on a focused window opens a fuzzel-based tag-manager picker showing checkboxes for that window's current tags.
FR7: Toggling a checkbox in the tag-manager picker adds/removes that tag from the focused window via WM IPC.
FR8: A text-input row in the tag-manager picker creates a new tag not already in the registry.
FR9: A second hotkey opens the same picker component, reused unmodified, for tag-switching — selecting a tag switches the active output's displayed tag.
FR10: A per-output status bar element always shows the currently selected tag for that output.
FR11: The WM exposes a Unix domain socket IPC server connecting the WM process to the tag-picker and status-bar companion client processes.
FR12: The WM provides keybind routing for baseline window operations — spawn terminal, close focused window (never routable to the pinned terminal), cycle keyboard focus between windows, exit the session.
FR13: The WM provides raw-keybind-driven tag switching (cycle/select among existing tags) and tag creation, sufficient to validate multi-tag behavior end-to-end before the fuzzel-based picker exists.

Total FRs: 13

### Non-Functional Requirements

NFR1: [CONFIRMED] Tag-switch and window-tagging actions complete within a 50ms internal handling budget (WM-side processing time, excluding compositor frame time).
NFR2: [CONFIRMED] The WM must not crash on malformed/unexpected client behavior — a WM crash strands the entire desktop session, making this the highest-priority reliability NFR.
NFR3: [CONFIRMED] WM daemon idle RSS stays under 50MB, in line with other lightweight wlroots-ecosystem window managers.

Total NFRs: 3

### Additional Requirements

- Language/starter: Rust, from `codeberg.org/river/tinyrwm`'s reference implementation.
- Protocol: `river-window-management-v1` only (no `river-layout-v3`, no `riverctl`, no river-classic path — explicit out-of-scope).
- No `wlr-layer-shell` — resolved 2026-08-06 via the UX pass: status-bar drives a waybar custom module instead (see architecture reconciliation above).
- IPC: Unix domain socket, JSON Lines protocol (ADR-007, resolves the open-questions.md item below).
- Tag registry: named/arbitrary strings, `u64` bitset, 64-tag cap, no delete-tag in v1 (ADR-006, resolves the open-questions.md bitmask-vs-named item below).
- Build/test only inside a devcontainer via devpod; WM binary source lives at `buoy-wm/wm/` as a Cargo workspace subdirectory (ADR-008, resolves the open-questions.md project-structure item below).
- Deployment: long-lived process from `~/.config/river/init` (chezmoi-managed launcher only — WM binary source itself is explicitly NOT chezmoi-managed).
- Async runtime (tokio vs smol) explicitly deferred to implementation time — not a v1 commitment.
- Out of scope (v1): session presets, picker-driven output/monitor assignment, tag-first reverse lookup, river-classic compatibility.

### PRD Completeness Assessment

The PRD is coherent and every acceptance-criteria line traces cleanly to an FR. However, **`docs/planning/prd/open-questions.md` is stale** — three of its four open items were resolved during architecture work (ADR-006, ADR-007, ADR-008) but the PRD file was never updated to reflect that:
- Bitmask vs. named tag registry → resolved (ADR-006, named + bitset)
- WM binary project structure → resolved (ADR-008, subdirectory of `buoy-wm`)
- IPC message format → resolved (ADR-007, JSON Lines)
- Fuzzel checkbox-toggle capability → **still genuinely open**, correctly tracked as a spike in epics.md Story 2.2 and now also carried in the UX doc's fallback pattern.

This is a documentation-hygiene gap, not a planning gap — the decisions exist and are consistent, they're just not reflected back in the PRD source of truth. Recommend closing out `open-questions.md`'s three resolved items with pointers to their ADRs before implementation starts, so the PRD doesn't mislead a future reader into thinking these are still undecided.

## Epic Coverage Validation

### Epic FR Coverage Extracted

FR1–FR5, FR12, FR13: Epic 1 | FR6–FR11: Epic 2 (per `epics.md`'s own FR Coverage Map, cross-checked against each story's acceptance criteria below)

**Bug found:** `epics.md` line 62 contains an unresolved template placeholder — `### FR Coverage Map` followed literally by `{{requirements_coverage_map}}`, never substituted. The real coverage map exists as a second, separately-written `### FR Coverage Map` section at lines 74–88. This is template-generation debris, not a content gap (the real map is complete and accurate), but it should be cleaned up before this doc is treated as final — a future reader hitting the unresolved token first may assume coverage was never mapped.

### FR Coverage Matrix

| FR | Requirement (short) | Epic/Story | Status |
|---|---|---|---|
| FR1 | In-memory tag/view/output state | Epic 1, Story 1.2 | ✓ Covered |
| FR2 | One-tag-per-output enforcement | Epic 1, Story 1.3 | ✓ Covered |
| FR3 | Lazy pinned-terminal spawn | Epic 1, Story 1.5 | ✓ Covered |
| FR4 | Pinned terminal tiled/bottom/uncloseable | Epic 1, Story 1.5 (+1.4 for keybind exclusion) | ✓ Covered |
| FR5 | Floating placement above pinned terminal | Epic 1, Story 1.6 | ✓ Covered |
| FR6 | Picker hotkey + checkbox UI | Epic 2, Story 2.2 | ✓ Covered |
| FR7 | Picker checkbox toggle via IPC | Epic 2, Story 2.2 | ✓ Covered |
| FR8 | Picker text-input tag creation | Epic 2, Story 2.3 | ✓ Covered |
| FR9 | Picker reused for tag-switching | Epic 2, Story 2.4 | ✓ Covered |
| FR10 | Per-output status bar | Epic 2, Story 2.5 | ✓ Covered |
| FR11 | IPC server for picker/bar | Epic 2, Story 2.1 | ✓ Covered |
| FR12 | Baseline keybind routing | Epic 1, Story 1.4 | ✓ Covered |
| FR13 | Raw-keybind tag switch/create | Epic 1, Story 1.7 | ✓ Covered |

### NFR Coverage

| NFR | Requirement | Referenced in | Status |
|---|---|---|---|
| NFR1 | 50ms action budget | Stories 1.3, 1.7, 2.2, 2.4 | ✓ Covered |
| NFR2 | No crash on malformed input | Stories 1.2, 2.1 | ✓ Covered |
| NFR3 | Idle RSS < 50MB | Story 1.2 | ✓ Covered |

### Missing Requirements

None. All 13 FRs and 3 NFRs have explicit story-level acceptance-criteria coverage.

### Coverage Statistics

- Total PRD FRs: 13 | Covered: 13 | Coverage: 100%
- Total PRD NFRs: 3 | Covered: 3 | Coverage: 100%

## UX Alignment Assessment

### UX Document Status

Found: `_bmad-output/planning-artifacts/ux-designs/ux-buoy-wm-2026-08-06/DESIGN.md` + `EXPERIENCE.md`, status: final, completed 2026-08-06 (same session as this readiness check).

### Alignment Issues

**UX ↔ PRD:** Clean. Both key flows (tag-a-window, switch-tags) trace directly to FR6–FR9 and their PRD acceptance criteria; no UX requirement contradicts a PRD statement.

**UX ↔ Architecture:** Reconciled during this session — the UX pass surfaced and resolved the `wlr-layer-shell` rejection gap (no replacement previously specified for status-bar rendering); `components.md`, `external-integrations.md`, and `technical-constraints.md` were updated to add `waybar` as the bar's rendering surface. No outstanding UX↔architecture conflicts remain.

Two minor gaps found where UX now specifies behavior the epics/stories don't yet have explicit acceptance criteria for:
- **Story 2.5 (status-bar):** UX's State Patterns table specifies the bar "returns to normal automatically once IPC reconnects (no user action)." The story's current AC covers detecting/displaying disconnect but not the reconnect-recovery behavior. Recommend adding an AC line for automatic recovery on reconnect.
- **Story 2.4 (tag-switching via picker):** UX resolved "picker always opens on the currently-focused output" for multi-monitor. The story's current AC doesn't state which output the switch-mode picker appears on. Recommend adding this as an explicit AC given the project's laptop+42"-dock multi-monitor context.

Neither gap blocks implementation (both are small, low-risk additions to existing stories, not new stories) but should be folded into the story files before those two stories are picked up.

### Warnings

None beyond the two AC-completeness notes above.

## Epic Quality Review

### Epic Structure Validation

**User value focus:** Both epics pass. This project is unusual for the checklist's default assumptions (no frontend/backend split — the WM daemon *is* the product), but under that lens: Epic 1 delivers a genuinely usable, if keybind-only, daily-driver WM (tag-aware placement, pinned terminal, baseline operations); Epic 2 delivers the full picker/bar UX on top of it. Neither is a disguised technical milestone — "Core WM Skeleton" sounds infrastructural but its acceptance criteria are all user-observable behavior (windows float/tile correctly, keybinds work, tags enforce).

**Epic independence:** Passes. Epic 1 stands alone (a complete, if bare, WM). Epic 2 depends only on Epic 1's output (`wm-core` functions, IPC groundwork), never the reverse. No forward references from Epic 1 into Epic 2.

### Story Quality Assessment

**Sizing/independence:** All 12 stories are appropriately scoped (single-session sized) with Given/When/Then ACs, testable and specific. No story requires a *later* story's functionality to be verifiable, with one clarity concern below.

**Forward-dependency check — Story 1.5 (Pinned Terminal Lifecycle):** Its trigger is "the output's current tag switches to it for the first time," but the user-facing tag-switch keybind isn't delivered until Story 1.7 (later in the same epic). This reads as a possible forward dependency. On inspection it's not a real violation — Story 1.3's one-tag-per-output enforcement already establishes an internal switch-tag operation in `wm-core` that Story 1.5 can exercise directly (e.g. via a test harness calling the internal function), independent of the Story 1.7 keybind wrapper. **Recommend:** add a one-line clarification to Story 1.5's AC noting the switch is exercised via the internal `wm-core` function, not the keybind, so a future implementer doesn't stall waiting for Story 1.7.

**Starter template check:** Architecture specifies `tinyrwm` as the starter template. Epic 1 Story 1 ("Devcontainer & CI Scaffolding") does include the starter-template seeding in its AC ("Cargo workspace... seeded from the `tinyrwm` Rust reference implementation") but its title foregrounds devcontainer/CI setup instead. **Minor** — the content is correct and complete, just the title undersells that this is also the project-bootstrap story.

### Findings by Severity

**🔴 Critical Violations:** None.

**🟠 Major Issues:** None.

**🟡 Minor Concerns:**
1. Story 1.5's tag-switch trigger should clarify it's exercised via the internal `wm-core` function, not the not-yet-built keybind (Story 1.7) — avoids a false read as a forward dependency.
2. Story 1.1's title doesn't reflect that it's also the starter-template bootstrap story.
3. (carried from UX Alignment) Story 2.4 and 2.5 are each missing one AC line for UX-specified behavior (picker output placement; bar reconnect recovery).

## Implementation Infrastructure Check (project-specific gate)

This project's readiness customization requires these checks beyond the standard BMAD checklist, with BLOCKED status if devcontainer, CI, `.env.template`, or gitleaks are missing. Repo state as of 2026-08-06: only a planning shell exists (`docs/`, `_bmad/`, `_bmad-output/` — no source tree, no dotfiles beyond git). Checked directly against the filesystem:

| Check | Status | Detail |
|---|---|---|
| Devcontainer (`.devcontainer/devcontainer.json`) | ❌ Missing | Not present anywhere in repo. Architecture (`technical-constraints.md`, `epics.md` Story 1.1) mandates devcontainer-via-devpod as the only build/test path — this is a hard blocker for Story 1.1 itself. |
| CI workflow (`.github/workflows/` or `.forgejo/workflows/`) | ❌ Missing | No workflow directory exists. Story 1.1's AC requires one that runs `cargo build`/`cargo test` on push — not yet created. |
| Pre-commit config (`.pre-commit-config.yaml`) | ❌ Missing | Story 1.1's AC requires `cargo fmt --check` and `cargo clippy` hooks — not yet created. |
| Test framework identified | 🟡 Partial | `cargo test` (Rust's built-in harness) is implied throughout (deployment.md, Story 1.1's AC) but never stated as an explicit architectural decision — no "Testing Strategy" section exists in `docs/planning/architecture/`. Adequate for a solo Rust project but worth a one-line explicit call-out. |
| TDD task structure (RED before GREEN per story) | ❌ Not present | Stories in `epics.md` are structured as user-story + Given/When/Then acceptance criteria only — none are broken into explicit RED-phase (failing test) → GREEN-phase (implementation) tasks. This conflicts with the project's own TDD mandate (write failing test first, never implement before a confirmed-failing test exists). Every story will need this task breakdown added before dev work starts, most likely during `bmad-dev-story`/`bmad-create-story` rather than retrofitted into `epics.md` now. |
| `.env.template` (op:// URIs) | ✅ Waived | User-confirmed 2026-08-06: buoy-wm's architecture has no secrets (single-user local session, filesystem-permission-only Unix socket, no auth, no external APIs — see `authentication-authorization.md`). Requirement waived project-wide; recorded in `_bmad/custom/bmad-check-implementation-readiness.toml` so future readiness runs don't re-flag it. |
| gitleaks in pre-commit | ✅ Waived | Same waiver as above. |
| CI secret wiring (`# TODO(1password):` or `1password/load-secrets-action@v2`) | ✅ N/A | Waived along with the rest of the secrets-infra requirement — nothing to wire. |

**Gate result: BLOCKED.** Two hard-blocker items remain — devcontainer and CI workflow, both still missing. The secrets-infra blockers (`.env.template`, gitleaks) are waived (see above) and no longer count against readiness. This is expected for a repo at "planning complete, zero source code" stage — none of this is a planning defect, it's simply Story 1.1's job, not yet done. Implementation cannot start at Story 1.2+ until Story 1.1 closes the devcontainer/CI/pre-commit gaps.

## Summary and Recommendations

### Overall Readiness Status

**BLOCKED** (project-specific infrastructure gate) / **NEEDS WORK** (planning-quality gate).

The planning artifacts themselves (PRD, architecture, epics/stories, UX) are in genuinely good shape — 100% FR/NFR traceability, no critical or major epic-quality violations, and the one real cross-doc gap found during this session (the `wlr-layer-shell` rendering question) was resolved and reconciled live rather than left open. But this project's own readiness customization hard-blocks on infrastructure scaffolding that doesn't exist yet in this repo, and that gate takes priority — a `BLOCKED` verdict stands regardless of planning quality until Story 1.1 is implemented.

### Critical Issues Requiring Immediate Action

1. **Story 1.1 (Devcontainer & CI Scaffolding) must be implemented before anything else** — it's the only story that unblocks the infrastructure gate (devcontainer, CI, pre-commit).
2. **TDD task structure is missing from every story** — `epics.md`'s stories are acceptance-criteria-only; none have RED/GREEN task breakdowns. This needs to be added (likely per-story during `bmad-create-story`/`bmad-dev-story`) before any story is picked up, per the project's TDD mandate.

*(Secrets infrastructure was flagged in the original pass but has since been waived — see the gate table above and `_bmad/custom/bmad-check-implementation-readiness.toml`.)*

### Recommended Next Steps

1. Run `bmad-create-story` (or equivalent) on Story 1.1 to produce a fully-specified story file with RED/GREEN task breakdown, then implement it — this closes the devcontainer/CI/pre-commit gaps directly.
2. Fold in the four small documentation fixes surfaced during this assessment: (a) resolve `epics.md`'s unresolved `{{requirements_coverage_map}}` template placeholder, (b) update `docs/planning/prd/open-questions.md` to close its three now-resolved items with ADR pointers, (c) add the missing AC lines to Story 2.4 (picker output placement) and Story 2.5 (bar reconnect recovery), (d) retitle or annotate Story 1.1 to reflect it's also the starter-template bootstrap story.
3. Once Story 1.1 is complete and the infrastructure gate clears, re-run this check (or just verify the gate table above) before starting Story 1.2.

### Final Note

This assessment found 0 critical planning defects, 0 major epic-quality violations, 3 minor epic-quality concerns, 2 minor UX↔story AC gaps, 2 documentation-hygiene issues (stale open-questions.md, unresolved template placeholder), and 2 hard-blocking infrastructure gaps (devcontainer, CI — both traceable to Story 1.1 not yet being implemented; the secrets-infra gaps are waived). The planning phase is essentially done and consistent; the project simply hasn't started Epic 1 yet. Address the infrastructure gate via Story 1.1 before any other implementation work begins — everything else here is a documentation cleanup, not a blocker.
