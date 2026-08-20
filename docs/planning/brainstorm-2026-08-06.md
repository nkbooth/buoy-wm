# Brainstorm: river-config (custom terminal-first WM)
Date: 2026-08-06
Stance: Creative Partner
Techniques used: SCAMPER, First Principles Thinking

## Topic and goal
Resolve the open design questions left in DISCOVERY.md before implementation starts:
the tag-manager popup (UX + IPC shape), the per-output tag-enforcement open problem,
and the WM's internal state model. Goal: leave this session with enough concrete
design decided to start coding against the `tinyrwm` Rust skeleton.

## Idea log

### SCAMPER — tag-manager popup
- Substitute: keybind cycling — toggle tags via keybind, no dedicated UI (user)
- Substitute: floating widget with checkbox selectors (user)
- Yes-and: combine cycling-as-verb with an OSD-style transient widget as visual
  feedback while cycling (coach)
- Alternative: drive an external picker (fuzzel/wofi) over IPC instead of writing
  custom popup rendering (coach)
- Combine: reuse the same fuzzel picker for both tag-switching and tag-assignment,
  distinguished only by which hotkey opens it (user)
- Adapt: dwm's always-visible buoy-status-bar tag indicator (user)
- Modify/scope-down: status bar only needs to show the *currently selected* tag per
  output — not a full per-window multi-tag breakdown (user)
- Eliminate → converged concrete flow: focus window → hotkey → fuzzel selector opens
  → checkboxes toggle add/remove on existing tags → text-input row creates a new
  arbitrary tag name on the fly (user)
- Reverse (considered, rejected): tag-first reverse lookup ("show me all windows
  tagged X") — rejected as redundant, since visiting the tagged workspace already
  shows what's tagged there (user)
- Put to another use: same picker+IPC pattern for output/monitor assignment — which
  output an active tag binds to (user)
- Put to another use: session presets — saved bundles of which tags exist plus their
  pinned-terminal/app layout, invoked as a group (coach, elaborated on request).
  Refined by user: defined in a config file (not built through the picker),
  triggered by a dedicated "open session preset" hotkey.

### First Principles Thinking — per-output enforcement + state model
- Real usage pattern: ~95% single-monitor (laptop screen or 42" widescreen on dock).
  Comfortable enforcing one-tag-per-output-max; the actual worry is only about the
  rare multi-monitor sessions (user)
- Narrowed the "getting lost" fear to a single concrete cause: not knowing which tag
  is on which output *right now* (user) — resolved: the always-visible per-output
  status bar (from SCAMPER) plus the one-tag-per-output rule together close this.
  **This resolves DISCOVERY.md's "Open problem" section.**
- Gut state-model list: window↔tags, window position, workspace ordering/layout,
  focus (user)
- Confirmed additions after review: output→currently-displayed-tag map, tag registry
  independent of window membership (a tag can exist before any window carries it),
  tags-with-spawned-pinned-terminal tracking (user)

## Convergence
Method: MoSCoW (fits — the goal was scoping the next build phase, not choosing
between competing directions).

**Must** (blocks a working WM)
- State model: window↔tags, window position, workspace ordering/layout, focus,
  output→current-tag, tag registry, terminal-spawned-per-tag
- One-tag-per-output-max enforcement rule
- Pinned-terminal-per-tag lifecycle (lazy spawn, never close, zellij session per tag)
- Floating-above-tiled placement rule (app-id based)

**Should** (core day-to-day UX)
- Fuzzel-based tag-manager popup: checkbox toggle existing tags + free-text add-new,
  driven over WM IPC
- Same picker reused for tag-switching (different keybind, same component)
- Always-visible per-output status bar showing current tag

**Could** (real value, not blocking)
- Output/monitor assignment via the same picker pattern

**Won't this time**
- Session presets (config-file-defined, hotkey-invoked) — parked, real idea, out of
  scope for this build
- Tag-first reverse lookup — rejected, redundant with visiting the workspace

## Top directions

### Milestone 1: core WM skeleton (the Must list)
**Concept:** Extend `tinyrwm`'s Rust baseline with the seven-field state model, the
one-tag-per-output rule, pinned-terminal lifecycle, and floating/tiled placement —
no picker, no bar yet. A WM that tracks tags and renders correctly, driven only by
raw keybinds.
**Key assumption:** the state model (window↔tags, window position, workspace
ordering/layout, focus, output→current-tag, tag registry, terminal-spawned-per-tag)
is sufficient — nothing else surfaces once implementation starts.
**Fastest validation:** get `tinyrwm` running unmodified after reboot (per
DISCOVERY.md's install-status note), then add state fields one at a time, testing
each against real multi-tag window behavior on the single-monitor case first.
**Risks:** named/arbitrary tags (vs. a fixed bitmask) may complicate the
implementation DISCOVERY.md assumed ("Bitmask, many-to-many") — needs reconciling
before or during this milestone.

### Milestone 2: picker + bar (the Should list)
**Concept:** Fuzzel-driven tag-manager popup (checkbox + free-text add) reused for
both switching and tagging, plus an always-visible per-output status bar showing
current tag.
**Key assumption:** a Unix-socket IPC (as sketched in DISCOVERY.md) is enough to
drive fuzzel and receive its selection back without a custom rendering layer.
**Fastest validation:** build the IPC socket and a throwaway shell script that talks
to it via fuzzel before writing the bar; confirms the protocol shape independent of
bar-rendering work.
**Risks:** fuzzel's UI may not naturally support "toggle checkbox + add new item in
one flow" — worth a quick spike before committing to it over a custom picker.

## Recommended starting point
Milestone 1 first — the state model and enforcement rule are load-bearing for
everything else, and DISCOVERY.md's install-status plan already points at
`tinyrwm` as the concrete first step. Resolve the bitmask-vs-named-tags tension
early in that milestone, before the picker (Milestone 2) locks in an IPC shape
around one or the other.
