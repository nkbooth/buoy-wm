# Story 2.1: IPC Server Foundation
Epic: 2 | Priority: H | Status: pending

## Description
A Unix domain socket IPC server exposing WM state and accepting mutation
commands, giving the picker and bar a stable protocol to build against.

## Acceptance criteria
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

- [ ] Tests pass (unit + integration where applicable)
- [ ] Code review: PASS

## Technical notes
Depends on Story 1.7 (wm-core switch/create functions to call).

## Test plan
TBD during dev loop.

## FR coverage
FR11, NFR2
