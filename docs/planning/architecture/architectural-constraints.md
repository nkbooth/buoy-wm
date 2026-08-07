# Architectural constraints
- Never use `wlr-layer-shell` — output-scoped, cannot react to tag switches,
  already explored and rejected
- Never route a close keybind to the pinned terminal, and never respawn it on
  exit outside the lazy-spawn-once path
- The WM binary's source is not chezmoi-managed — only the `river/init`
  launcher script is; keep that boundary intact
- No `river-classic`/`riverctl` command usage anywhere — those primitives
  (tags, float-filter, `river-layout-v3`) don't exist under `river` 0.4+
