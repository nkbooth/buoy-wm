# Components

## wm-core (state manager)
**Responsibility:** Owns all in-memory state — views, tags, outputs, focus —
and is the single source of truth for tag enforcement and placement decisions.
**Interface:** In-process function calls from the protocol client and the IPC
server; no external interface of its own.
**Key dependencies:** None (pure state + logic, no I/O).

## protocol-client (river-window-management-v1 binding)
**Responsibility:** Wayland protocol handshake, receives view map/unmap/config
events from the compositor, translates them into `wm-core` state updates, and
issues placement/rendering-order requests back to the compositor.
**Interface:** Wayland protocol socket to `river`.
**Key dependencies:** `wm-core`, `wayland-client`, `tinyrwm` skeleton.

## placement-engine
**Responsibility:** Decides floating-vs-tiled and render order per view —
pinned-term app-id forced tiled and bottom-of-stack, everything else floating
above it — and lazily spawns the pinned terminal on first switch to a tag.
**Interface:** Called by `protocol-client` on view-map events; reads/writes
`wm-core` state.
**Key dependencies:** `wm-core`, process-spawn (for `foot -a pinned-term` +
`zellij attach --create tag-<name>`).

## ipc-server
**Responsibility:** Unix domain socket exposing `wm-core` state (current tags,
per-view tag membership, per-output current tag) and accepting mutation
commands (toggle tag, create tag, switch tag) from external clients.
**Interface:** Unix socket, JSON Lines protocol (see ADR-007).
**Key dependencies:** `wm-core`.

## buoy-tag-picker (companion client, separate binary)
**Responsibility:** Hotkey-invoked; queries `ipc-server` for the focused
view's current tags, renders a `fuzzel` checkbox list plus free-text add-new
row, writes the toggled/created selection back over the socket. Reused
unmodified for tag-switching (different hotkey, different query).
**Interface:** Unix socket client to `ipc-server`; spawns/talks to `fuzzel`.
**Key dependencies:** `ipc-server`, `fuzzel` (external binary).

## buoy-status-bar (companion client, separate binary)
**Responsibility:** Does not render its own surface — `wlr-layer-shell` is
rejected as output-scoped, not tag-scoped (see technical-constraints.md), so
this client instead drives a waybar custom module: it subscribes to (or
polls) `ipc-server` for output→current-tag changes and feeds the current tag
(or a disconnected/stale indicator on IPC loss) to that module per output.
**Interface:** Unix socket client to `ipc-server`; output side feeds a
waybar custom module (script/polling or signal-driven — exact mechanism TBD
at implementation).
**Key dependencies:** `ipc-server`, `waybar` (external, user-configured).

See `_bmad-output/planning-artifacts/ux-designs/ux-buoy-wm-2026-08-06/EXPERIENCE.md`
for the resolved rendering-surface decision and both surfaces'
interaction/state design.
