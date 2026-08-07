---
stepsCompleted: [1, 2, 3, 4]
inputDocuments:
  - docs/planning/prd/index.md (sharded PRD)
  - docs/planning/architecture/index.md (sharded Architecture)
---

# buoy-wm - Epic Breakdown

## Overview

This document provides the complete epic and story breakdown for buoy-wm, decomposing the requirements from the PRD and Architecture into implementable stories.

## Requirements Inventory

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
FR12: The WM provides keybind routing for baseline window operations — spawn terminal, close focused window (never routable to the pinned terminal), cycle keyboard focus between windows, exit the session — adapted from the `tinyrwm` skeleton's keybind set.
FR13: The WM provides raw-keybind-driven tag switching (cycle/select among existing tags) and tag creation, sufficient to validate multi-tag behavior end-to-end before the fuzzel-based picker (FR6–FR9) exists. The underlying `wm-core` tag-switch/create logic this exercises is reused directly by the picker's IPC handlers in Epic 2 — this is not throwaway code.

### NonFunctional Requirements

NFR1: [CONFIRMED] Tag-switch and window-tagging actions complete within a 50ms internal handling budget (WM-side processing time, excluding compositor frame time).
NFR2: [CONFIRMED] The WM must not crash on malformed/unexpected client behavior — a WM crash strands the entire desktop session, making this the highest-priority reliability NFR.
NFR3: [CONFIRMED] WM daemon idle RSS stays under 50MB, in line with other lightweight wlroots-ecosystem window managers.

### Additional Requirements

- **Starter template**: The WM binary is built from `codeberg.org/river/tinyrwm`'s Rust reference implementation as the starting skeleton — this directly impacts Epic 1 Story 1 (project scaffolding).
- WM binary source lives at `buoy-wm/wm/` as a Cargo workspace subdirectory, not a separate repository (ADR-008).
- Target `river` 0.4+ (non-monolithic) and `river-window-management-v1` exclusively — no `river-classic`, no `riverctl`, no `river-layout-v3` (ADR-001).
- Never use `wlr-layer-shell` — output-scoped, cannot react to tag switches, already evaluated and rejected.
- IPC protocol: JSON Lines (newline-delimited JSON) over a Unix domain socket, both directions (ADR-007 — status: Accepted).
- Tag registry: user-facing arbitrary string names, each assigned a small dynamically-allocated integer ID on first creation; `View.tags` stored as a bitset (e.g. `u64`, capping the registry at 64 concurrent tags). IDs are never reused within a session (no delete-tag feature in v1); registry exhaustion (>64 distinct tags) fails tag creation with a surfaced error rather than silently allowing it (ADR-006 — status: Accepted).
- Terminal choice confirmed as `foot` (not ghostty/ptyxis) — its `unknown terminal type` SSH error on unprepared remote hosts is a `TERM=foot` terminfo-propagation issue common to any terminal with a distinct `$TERM` value (ghostty has the identical failure mode via `xterm-ghostty`, with sharper edges around SSH `RemoteCommand`). Fix is environment/dotfiles work (`infocmp foot | ssh <host> -- tic -x -`, or a `TERM=xterm-256color` fallback for unprepared hosts) — out of scope for this project's stories.
- `tinyrwm` confirmed as an appropriately minimal starter skeleton — a deliberately tiny reference implementation (4 keybinds, 2 pointer bindings) with ports in 6+ languages including Rust, not a heavy framework.
- No persistence layer in v1 — all state is process-memory only, rebuilt empty on WM start; zellij session content persists independently via `zellij attach`, but tag *assignments* on windows do not survive a WM restart.
- Authentication/authorization: N/A — single-user local session; Unix socket relies on filesystem permissions (user-only, default socket mode).
- Deployment: long-lived process launched from `~/.config/river/init` (chezmoi-managed dotfile) under a `river` 0.4+ session on the Fedora atomic host (`framework` image). The WM binary's own source is explicitly NOT chezmoi-managed — only the launcher script is; this boundary must stay intact.
- Build/test only inside a devcontainer via devpod — the Rust toolchain is never installed on the host.
- Never route a close keybind to the pinned terminal, and never respawn it on exit outside the lazy-spawn-once path.
- `tag-picker` and `status-bar` are separate companion binaries (not part of the WM daemon process) that talk to `ipc-server` over the Unix socket; `tag-picker` shells out to `fuzzel` for rendering (ADR-004).
- Async runtime (`tokio` vs `smol`) is explicitly deferred — not a v1 architectural commitment, decide at implementation time.

### UX Design Requirements

A UX design pass was completed 2026-08-06: `_bmad-output/planning-artifacts/ux-designs/ux-buoy-wm-2026-08-06/DESIGN.md` and `EXPERIENCE.md`. It confirms and extends the PRD/ADR-004 interaction patterns (checkbox picker, free-text add-row, Enter/Esc semantics) and resolves a prior gap: the status-bar renders via a `waybar` custom module rather than a self-rendered surface (`wlr-layer-shell` was rejected with no replacement previously specified — see technical-constraints.md and architecture/components.md). It also specifies the picker's cap-out rejection state, the bar's disconnect state, and a fuzzel-capability fallback pending Story 2.2's spike. Story 2.5 (status-bar) and Story 2.2/2.3 (picker) should be read against this doc, not just FR6–FR10.

## Epic List

### Epic 1: Core WM Skeleton — tag-aware placement, keybind-driven
A running river WM (built from the `tinyrwm` Rust skeleton) that tracks full tag/view/output state in memory, enforces one-tag-per-output, lazily spawns a persistent zellij-backed pinned terminal per tag, floats every other window above it, and is fully operable via keybinds — baseline window operations (spawn/close/focus-cycle/exit, adapted from `tinyrwm`) plus raw-keybind tag switching and creation. This validates the multi-tag state model end-to-end before the picker exists; the tag-switch/create logic exercised here is reused directly by Epic 2's IPC handlers, not thrown away.
**FRs covered:** FR1, FR2, FR3, FR4, FR5, FR12, FR13

### Epic 2: Tag Manager & Status Bar — full daily-driver UX
Two companion clients: a fuzzel-driven tag-manager picker (hotkey → checkboxes to toggle existing tags, text-input row to create new ones, second hotkey reused for tag-switching) and an always-visible per-output status bar showing the active tag — both driven over a new WM IPC socket. This turns Epic 1's keybind-only WM into the full multi-tag daily driver.
**FRs covered:** FR6, FR7, FR8, FR9, FR10, FR11

### FR Coverage Map

FR1: Epic 1 - In-memory tag/view/output state model
FR2: Epic 1 - One-tag-per-output enforcement
FR3: Epic 1 - Lazy pinned-terminal spawn per tag
FR4: Epic 1 - Pinned terminal tiled/bottom/uncloseable
FR5: Epic 1 - Floating placement above pinned terminal
FR6: Epic 2 - Tag-manager picker hotkey + checkbox UI
FR7: Epic 2 - Picker checkbox toggle via IPC
FR8: Epic 2 - Picker text-input tag creation
FR9: Epic 2 - Picker reused for tag-switching
FR10: Epic 2 - Per-output status bar
FR11: Epic 2 - IPC server for picker/bar clients
FR12: Epic 1 - Baseline keybind routing (spawn/close/focus-cycle/exit)
FR13: Epic 1 - Raw-keybind tag switching and creation

## Epic 1: Core WM Skeleton — tag-aware placement, keybind-driven

A running river WM (built from the `tinyrwm` Rust skeleton) that tracks full tag/view/output state in memory, enforces one-tag-per-output, lazily spawns a persistent zellij-backed pinned terminal per tag, floats every other window above it, and is fully operable via keybinds — baseline window operations (spawn/close/focus-cycle/exit, adapted from `tinyrwm`) plus raw-keybind tag switching and creation. This validates the multi-tag state model end-to-end before the picker exists; the tag-switch/create logic exercised here is reused directly by Epic 2's IPC handlers, not thrown away.

### Story 1.1: Devcontainer & CI Scaffolding

As a developer,
I want a working devcontainer with pre-commit hooks and a CI workflow stub,
So that all subsequent work happens in a reproducible environment without touching the host toolchain.

**Acceptance Criteria:**

**Given** a fresh clone of `buoy-wm`
**When** the devcontainer is opened via devpod
**Then** `cargo build` succeeds inside it with no host-installed Rust toolchain
**And** `.pre-commit-config.yaml` exists and runs `cargo fmt --check` and `cargo clippy` on commit
**And** a CI workflow file exists that runs `cargo build` and `cargo test` on push
**And** the Cargo workspace is rooted at `buoy-wm/wm/` per ADR-008, seeded from the `tinyrwm` Rust reference implementation

### Story 1.2: WM Core State Model

As the WM,
I want to track window↔tag membership, view geometry, focus, output→current-tag, the tag registry, and terminal-spawned status entirely in memory,
So that all placement and enforcement logic has a single source of truth.

**Acceptance Criteria:**

**Given** the WM has connected to `river` via `river-window-management-v1`
**When** a view is mapped, unmapped, or its state changes
**Then** `wm-core` updates its in-memory `View` (id, app_id, tags, floating, geometry, focused), `Tag` (id, name, terminal_spawned), and `Output` (id, current_tag) records accordingly
**And** the registry starts empty on WM launch — no persisted state from a prior run (per data-model.md)
**When** the compositor sends malformed or unexpected client events
**Then** the WM logs and discards the event without crashing or corrupting state (NFR2)
**And** WM daemon idle RSS stays under 50MB with no views mapped (NFR3)

### Story 1.3: One-Tag-Per-Output Enforcement

As a user,
I want at most one tag displayed per output at any time,
So that I never lose track of which tag is active where.

**Acceptance Criteria:**

**Given** an output currently displaying tag A
**When** any action would assign tag B to display on that same output
**Then** the WM rejects or reroutes the action so exactly one tag remains displayed per output
**And** the enforcement decision completes within the 50ms internal latency budget (NFR1)
**And** this holds under both single-output and multi-output configurations

### Story 1.4: Baseline Keybind Routing

As a user,
I want basic keybinds for spawning a terminal, closing the focused window, cycling focus, and exiting the session,
So that I can operate the WM at all before any picker exists.

**Acceptance Criteria:**

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

### Story 1.5: Pinned Terminal Lifecycle

As a user,
I want a persistent zellij-backed terminal automatically available on every tag,
So that I always have a terminal backdrop without manual setup.

**Acceptance Criteria:**

**Given** a tag has no pinned terminal yet
**When** the output's current tag switches to it for the first time
**Then** the WM spawns `foot -a pinned-term` running `zellij attach --create tag-<name>`
**And** marks that tag's `terminal_spawned` as true
**Given** the pinned terminal is running
**Then** it is always tiled and always rendered at the bottom of the render order
**And** it is never respawned outside this lazy-spawn-once path, even if the process exits (architectural constraint)

### Story 1.6: Floating Placement For Non-Pinned Windows

As a user,
I want every window except the pinned terminal to float above it,
So that my active work is always visible over the terminal backdrop.

**Acceptance Criteria:**

**Given** a newly mapped view whose app-id is not `pinned-term`
**When** the WM places it
**Then** it defaults to floating and renders above the pinned terminal
**And** this holds regardless of which tag or output it's mapped on

### Story 1.7: Raw-Keybind Tag Switching & Creation

As a user,
I want keybind-driven tag switching and creation,
So that I can exercise and validate the multi-tag model before the picker exists.

**Acceptance Criteria:**

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

## Epic 2: Tag Manager & Status Bar — full daily-driver UX

Two companion clients: a fuzzel-driven tag-manager picker (hotkey → checkboxes to toggle existing tags, text-input row to create new ones, second hotkey reused for tag-switching) and an always-visible per-output status bar showing the active tag — both driven over a new WM IPC socket. This turns Epic 1's keybind-only WM into the full multi-tag daily driver.

### Story 2.1: IPC Server Foundation

As a companion client developer,
I want a Unix domain socket IPC server exposing WM state and accepting mutation commands,
So that the picker and bar have a stable protocol to build against.

**Acceptance Criteria:**

**Given** the WM is running
**When** `ipc-server` starts
**Then** it listens on a Unix domain socket with default user-only permissions (no auth layer — filesystem permissions only, per architecture)
**And** it speaks newline-delimited JSON in both directions (ADR-007)
**When** a client sends `get-state`
**Then** it receives current tags, per-view tag membership, and per-output current tag
**When** a client sends `toggle-tag`, `create-tag`, or `switch-tag`
**Then** the WM applies the mutation via the same `wm-core` functions exercised by Story 1.7's raw keybinds
**When** a client sends a malformed or unparseable message
**Then** the connection is rejected/reset gracefully without crashing the WM (NFR2)

### Story 2.2: Tag-Manager Picker — Toggle Existing Tags

As a user,
I want a hotkey on my focused window to open a picker showing its current tags as checkboxes,
So that I can add or remove tags without leaving the keyboard.

**Acceptance Criteria:**

**Given** a window is focused
**When** I press the tag-manager hotkey
**Then** a `fuzzel`-driven checklist opens showing that window's current tags checked and other registry tags unchecked
**When** I toggle a checkbox
**Then** the `tag-picker` client sends `toggle-tag` over IPC and the WM applies it within the 50ms latency budget (NFR1)
**Given** fuzzel's native support for checkbox-toggle interaction is unconfirmed (ADR-004 open item)
**Then** this story includes a short spike to confirm the flow works in one `fuzzel` invocation, falling back to a documented alternative interaction if not

### Story 2.3: Tag-Manager Picker — Create New Tag

As a user,
I want a text-input row in the tag-manager picker,
So that I can create a new tag on the fly without a separate flow.

**Acceptance Criteria:**

**Given** the tag-manager picker is open
**When** I type a name not already in the registry into the input row and confirm
**Then** `tag-picker` sends `create-tag` over IPC, a new tag is registered (ADR-006 rules apply), and it's immediately applied to the focused window
**Given** the registry is at its 64-tag cap
**When** I attempt to create another
**Then** the picker surfaces the WM's rejection rather than silently failing

### Story 2.4: Tag-Switching via Picker

As a user,
I want a second hotkey that opens the same picker component for tag-switching,
So that I use one consistent UI for both tagging and switching.

**Acceptance Criteria:**

**Given** any window or none is focused
**When** I press the tag-switch hotkey
**Then** the same `tag-picker` binary opens in switch mode, listing all registry tags
**When** I select a tag
**Then** `tag-picker` sends `switch-tag` over IPC and the active output's displayed tag changes, subject to one-tag-per-output enforcement (Story 1.3), within the 50ms latency budget (NFR1)

### Story 2.5: Per-Output Status Bar

As a user,
I want an always-visible bar element per output showing the currently selected tag,
So that I always know which tag is active where, especially in multi-monitor sessions.

**Acceptance Criteria:**

**Given** the `status-bar` client is running
**When** an output's current tag changes (via keybind, picker, or WM startup default)
**Then** the corresponding bar element updates to show the new tag name
**And** this holds independently for each connected output
**Given** the IPC connection to the WM drops
**Then** the bar does not crash and indicates a disconnected/stale state rather than showing stale data silently
