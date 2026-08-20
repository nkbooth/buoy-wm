# External integrations
| Integration | Purpose | Protocol |
|-------------|---------|----------|
| `river` compositor | Window management surface | `river-window-management-v1` (Wayland) |
| `foot` | Pinned terminal emulator | process exec, app-id tagging |
| `zellij` | Per-tag terminal session persistence | process exec (`zellij attach --create`) |
| `fuzzel` | Tag-manager picker UI | process exec, stdin/stdout |
| `buoy-tag-picker` / `buoy-status-bar` clients | IPC consumers | Unix domain socket, JSON Lines |
| `waybar` | Renders the buoy-status-bar's per-output tag element (resolves the `wlr-layer-shell` rejection — see technical-constraints.md and components.md) | `buoy-status-bar` feeds a waybar custom module (script/polling or signal-driven — exact mechanism TBD at implementation) |
