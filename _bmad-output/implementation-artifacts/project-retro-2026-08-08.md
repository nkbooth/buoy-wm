# Project Retrospective: buoy-wm — Epic 1 + Epic 2 (Full Project)

**Date:** 2026-08-08
**Facilitator:** Amelia (Developer)
**Participants:** Nick (Project Lead), Amelia (Developer), Charlie (Senior Dev / reviewer perspective), Dana (QA)
**Scope:** Project-level synthesis across both epics — not a re-review of individual stories
**Status:** Project complete — 12/12 stories done across 2/2 epics, all committed to `main`. No Epic 3 exists; this is the final retrospective for the planned project.

**Inputs synthesized:**
- `_bmad-output/implementation-artifacts/epic-1-retro-2026-08-07.md` (Epic 1: Core WM Skeleton, 7 stories)
- `_bmad-output/implementation-artifacts/epic-2-retro-2026-08-08.md` (Epic 2: Tag Manager & Status Bar, 5 stories)
- `git log --oneline` (15 commits, 2026-08-06 → 2026-08-08)
- `docs/planning/epics/story-*.md`

This document does not re-litigate individual story bugs — those are documented in the two epic retros. It exists to answer a different question: **what does the whole project, taken together, teach about how this team and this codebase actually work?**

---

## Project Summary

buoy-wm is a Wayland window manager for the river compositor. Epic 1 built the WM skeleton — an in-memory tag/view/output state model (`wm_core`), one-tag-per-output enforcement, pinned-terminal lifecycle, floating placement, and full keybind operability. Epic 2 built the daily-driver UX on top of it — a Unix-socket IPC server, a fuzzel-driven tag-manager picker, and a per-output waybar status bar module. Both epics are complete, all 12 stories are `Status: done`, and every story that required a code-review fix has a documented follow-up.

---

## What Went Well (Project-Wide)

1. **TDD discipline held with zero exceptions across all 12 stories.** Every story with testable logic used strict RED-before-GREEN. Every story with genuinely untestable I/O — Wayland `Dispatch` glue in Epic 1, process-spawn/socket-I/O/waybar-runtime glue in Epic 2 — used an explicit, consistently-justified build+clippy+manual-review carve-out, never silently expanded to cover unit-testable work. Epic 2 in fact *tightened* the carve-out relative to Epic 1 by adding real Unix-socket integration tests where Epic 1 had zero testable surface for its glue layer. This is the single most consistent process win of the entire project, and it held under increasing IPC-surface complexity rather than eroding.

2. **The review→fix→focused-re-review pattern is the default for HIGH-severity or contract-changing fixes, not an exception.** It was used in Epic 1 (Stories 1.4, 1.5) and Epic 2 (Stories 2.3, 2.4) every time a fix touched shared state in a way a single review pass could plausibly miss — and in each case, the second pass caught something the first fix attempt didn't fully resolve (most sharply in 1.5, where the first fix introduced a new, undisclosed regression). This should be treated as standing practice for this codebase, not a situational escalation.

3. **User scope check-ins landed exactly when needed — never over-used, never under-used.** Genuine invalidated-planning-assumption moments were escalated for an explicit decision: Story 1.5's pinned-terminal render-order gap (surfaced twice — once pre-implementation, once when a fix's real behavior diverged from what was disclosed), and Story 2.2's fuzzel spike, which disproved the UX design doc's own assumed fallback (no native multi-select in fuzzel). Every other ambiguity across all 12 stories was resolved with documented evidence or reasoning inline, without escalation. **Zero `bmad-correct-course` invocations were needed across the entire project** — no ambiguity was large enough to require a formal sprint-change process; every one was resolvable at the story level.

4. **Two real process deviations were caught and corrected, both durably, with no repeat incidents.** A sub-agent spuriously self-invoked `taskplan` during a Story 1.4 fix-up — corrected immediately, never recurred through the rest of the project. An implementation agent self-committed Story 2.3 before review ran — corrected by establishing "orchestrator commits only after review passes," which held for every subsequent story with zero repeats. Both are evidence that this project's error-correction loop works: catch once, fix the standing instruction, verify it sticks.

5. **Infrastructure interruptions were never conflated with implementation quality.** Two Claude spend-limit stops and one transient connection-closed error were recognized as account/network-level issues, paused cleanly, reported to the user, and resumed without any quality impact once resolved. Across a 12-story project, the loop never once mistook an environment hiccup for a code problem, or vice versa.

## What to Change / Principles for Future Work on This Codebase

1. **Principle: any future work that reuses an existing stateful primitive via a new caller gets HIGH-effort review scrutiny by default, not as an escalation.** Epic 1's retro identified this pattern from partial evidence (`raise_view`, `cycle_focus`, `do_action` reused via new call paths produced real bugs in Stories 1.2, 1.4, 1.5, while purely fresh `wm_core` logic in 1.1, 1.3, 1.6 stayed clean) and predicted it would generalize to Epic 2's IPC layer. It did — at a materially higher rate: **5-for-5 Epic 2 stories found real bugs under this pattern, versus 3-of-7 in Epic 1**, because every single Epic 2 story routed through a `wm_core` mutator via a brand-new IPC/client call path by design. This is no longer a hypothesis or a retro footnote — it is a confirmed, twice-validated engineering property of this specific codebase: `wm_core` is a small, shared, mutex-guarded state object with a steadily growing number of callers, and every new caller is disproportionately likely to expose a real bug in the callee, not just the new code. Treat "clean on first review" as the surprising outcome, not the default, for any future story that adds a call site onto shared mutable state — regardless of which epic or feature it belongs to.

2. **Unresolved, flagged for separate follow-up — not resolved by this retro: the `devpod ssh` transport was unreliable for essentially the entire project.** Epic 1's retro raised it as an action item to "sanity-check" before Epic 2. It was nominally checked but never actually fixed, and stayed broken through the whole of Epic 2 as well — `podman exec -u vscode -w /workspaces/buoy-wm bold_vaughan <cmd>` was used as the de facto primary verification path for essentially every implementation and review step across both epics. This was raised twice across two epic retros and never resolved either time. **This needs the user's direct attention as a standalone environment/tooling task, not a silent fix folded into this retro** — the two viable resolutions are (a) actually debug and fix the `devpod ssh` transport, or (b) stop calling `podman exec` a "fallback" and formally document it as this project's primary verification path, since two epics of evidence say it's the reliable one.

3. **Two lower-severity items surfaced only in the Epic 2 retro are worth carrying forward as light process notes, not action items requiring rework:** review agents occasionally returned an incomplete first-pass summary requiring a follow-up nudge to get the full compiled findings (a latency/friction tax, never a correctness gap — every review eventually produced complete results); and mid-story self-caught bugs (Story 2.1's mutex deadlock, Story 2.4's missing pinned-terminal spawn) are exactly the kind of catch to want *more* of, since they never reached review at all.

## Principles for Future Work on `buoy-wm` (Consolidated)

These are the durable takeaways that should inform any future feature work on this codebase, beyond the scope of the two completed epics:

- **Shared mutable state (`wm_core`) is the highest-risk surface in this codebase.** Any new caller of an existing mutator — keybind, IPC handler, future client, anything — warrants default HIGH-effort review, not opt-in scrutiny.
- **The testability carve-out for Wayland/process/socket glue is legitimate and should stay narrow.** It has never been used to skip unit-testable work in 12 stories; keep it that way by requiring each story to justify the carve-out per-task, as both epics did.
- **Review→fix→focused-re-review is the standard response to a HIGH-severity or contract-changing fix**, not a special-case escalation.
- **Orchestrator commits only after review passes** is a permanent standing rule, established mid-project after the one incident where it wasn't followed, and never violated again since.
- **Escalate to the user only on genuine invalidated assumptions** (a planning-doc claim proven wrong, a behavior change from what was disclosed) — not on ordinary implementation ambiguity, which should be resolved inline with documented reasoning.

## Unresolved Item Requiring Separate Attention

> **`devpod ssh` transport reliability.** Flagged in the Epic 1 retro (2026-08-07), still broken through all of Epic 2 (2026-08-08), never fixed. `podman exec -u vscode -w /workspaces/buoy-wm <container> <cmd>` was the working substitute used throughout. This retrospective does not attempt to fix it — it is an environment/tooling decision for Nick to make outside the scope of story implementation: either debug the transport for real, or formally adopt `podman exec` as the project's primary verification path.

## Readiness Assessment (Project-Level)

- **Testing & quality:** All 12 stories closed with green fmt/clippy/test/pre-commit gates. `wm_core` and IPC decision logic carry full unit/integration coverage; Wayland/process-spawn/waybar-runtime glue is consistently build+clippy+manual-review-only, a disclosed and stable gap rather than a hidden one.
- **Deployment:** N/A for this project's scope — no live `river`/waybar compositor session was available in-sandbox for either epic; every story that needed live verification explicitly documented the gap rather than assuming coverage it didn't have.
- **Stakeholder acceptance:** Single-developer project; Nick is both stakeholder and reviewer of record. Both epics accepted via commit history and `Status: done` on all 12 stories.
- **Technical health:** Stable. No open technical debt items were carried out of Epic 2. Epic 1's one deferred item (no `wm_core::unregister_output`) remains explicitly scoped as YAGNI and is not a project blocker.
- **Unresolved blockers:** None for the completed project scope. The one open item is the `devpod ssh` tooling question above, which is an environment concern, not a code or scope blocker.

**Verdict: the planned project (Epic 1 + Epic 2, 12/12 stories) is complete, stable, and requires no further epic-level work.** The only carry-forward item is the `devpod ssh`/`podman exec` tooling decision, which is explicitly out of scope for this retrospective to resolve.

---

## Consolidated Action Items

| # | Action | Owner | Category |
|---|---|---|---|
| 1 | Treat "new call site onto an existing `wm_core` mutator" as a standing HIGH-review-scrutiny trigger for any future work on this codebase, not an epic-specific practice | Reviewer (Charlie/code-review) | Process principle |
| 2 | Resolve `devpod ssh`'s transport failure for real, or formally document `podman exec -u vscode -w /workspaces/buoy-wm <container> <cmd>` as this project's primary (not fallback) verification path | Nick (Project Lead) | Tooling — unresolved, needs decision |
| 3 | Keep "orchestrator commits only after review passes" as a permanent standing rule for any future work on this codebase | Dev (Amelia) | Process |
| 4 | Keep review→fix→focused-re-review as the default (not exceptional) response to any HIGH-severity or contract-changing fix | Reviewer | Process |
| 5 | If review agents are reused in future work, tighten their prompt to require the full compiled findings as the first response rather than a summary needing a follow-up nudge | Dev (Amelia) | Process hygiene |

No epic or PRD updates required. No significant discoveries invalidate any completed scope. No Epic 3 or further planned work exists in `docs/planning/epics/` as of this retrospective.
