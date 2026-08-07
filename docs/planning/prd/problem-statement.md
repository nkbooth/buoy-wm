# Problem statement
No existing river companion WM implements dwm-style many-to-many bitmask/named
tags — a survey of the ~25 community WMs on codeberg.org/river/wiki found only
single-workspace tiling/stacking clones (dwm-, bspwm-, XMonad-, StumpWM-style).
Hyprland was evaluated and ruled out: its "tags" are rule-matching labels, not
workspace membership; multi-workspace membership is explicitly "plugin territory"
upstream (hyprwm/Hyprland#10358); and its `pin` dispatcher makes a window
omnipresent across *every* workspace rather than pinned per-workspace — the wrong
primitive for a per-tag persistent terminal backdrop. No off-the-shelf option
matches the desired interaction model, so it has to be built.
