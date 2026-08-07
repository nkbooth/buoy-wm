# Non-functional requirements
[CONFIRMED 2026-08-06]
- Tag-switch and window-tagging actions complete within a 50ms internal
  handling budget (WM-side processing time, excluding compositor frame
  time) — the standard perceptual threshold for "feels instantaneous"
- WM must not crash on malformed/unexpected client behavior — a WM crash
  strands the desktop session; highest-priority NFR
- WM daemon idle RSS stays under 50MB, in line with other lightweight
  wlroots-ecosystem window managers
