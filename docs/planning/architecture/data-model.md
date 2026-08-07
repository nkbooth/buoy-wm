# Data model

## Entities
| Entity | Fields | Storage |
|--------|--------|---------|
| View | id, app_id, tags: TagSet, floating: bool, geometry, focused: bool | in-memory |
| Tag | id (small int, dynamically assigned), name (String), terminal_spawned: bool | in-memory |
| Output | id, current_tag: Option\<TagId\> | in-memory |

No persistence layer in v1 — all state is process-memory only and rebuilt on
WM start (empty tag registry, no views). Zellij session state persists
independently of the WM (survives WM restarts via `zellij attach`), which is
what makes losing in-memory tag state on restart acceptable for the terminal
content itself, though tag *assignments* on windows do not survive a restart.

## Data flow
Compositor view-map event → `protocol-client` → `wm-core` (register view,
default floating unless `pinned-term` app-id) → `placement-engine` (spawn
pinned terminal if first-for-tag; decide render order) → protocol requests
back to compositor. Picker/bar commands flow: external client → `ipc-server`
→ `wm-core` mutation → protocol requests back to compositor (e.g. re-render on
tag switch).
