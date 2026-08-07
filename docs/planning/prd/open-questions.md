# Open questions
- [x] Bitmask vs. named/arbitrary tag registry — resolved: named/arbitrary
      strings, `u64` bitset, 64-tag cap (ADR-006).
- [x] WM binary project structure — resolved: subdirectory of `buoy-wm`
      (`buoy-wm/wm/`), not a separate repo (ADR-008).
- [x] IPC message format — resolved: JSON Lines over Unix domain socket
      (ADR-007).
- [ ] Whether fuzzel natively supports "checkbox toggle + add-new-item in one
      flow," or a custom picker is needed instead — needs a quick spike,
      tracked as part of Story 2.2 (`docs/planning/epics/story-2-2.md`). —
      owner: Nick
