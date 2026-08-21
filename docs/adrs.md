# Architecture decision records

The decisions that shaped `buoy`, recorded when they were made. The code
refers to several of them by number, which is why they are kept while the
rest of the planning material was not.

These are point-in-time records, amended only where a later finding settled
something they left open. Where one disagrees with the
[README](../README.md), the README is current.

## ADR-001: Target river 0.4+ (non-monolithic), not river-classic
**Date:** 2026-08-06
**Status:** Accepted
**Decision:** Build against `river` 0.4+ and `river-window-management-v1`,
not the frozen `river-classic` 0.3.x fork.
**Rationale:** `river-window-management-v1` landed in `main` 2025-12-10 and is
declared stable ("we do not break window managers"). `river-classic` is
deliberately frozen and gets no further protocol growth — picking it means
opting out of everything upstream does next.
**Consequences:** No compositor-native tags/float-filter/layout primitives to
lean on; the WM must own all of that state itself. Higher up-front build cost,
but aligned with where the ecosystem is headed.

## ADR-002: river over Hyprland
**Date:** 2026-08-06
**Status:** Accepted
**Decision:** Use `river` as the compositor foundation instead of Hyprland.
**Rationale:** Hyprland's "tags" are rule-matching labels, not workspace
membership — a window still belongs to exactly one workspace, and
multi-workspace membership is explicitly "plugin territory" upstream
(hyprwm/Hyprland#10358). Hyprland's `pin` dispatcher makes a floating window
omnipresent across *every* workspace on a monitor, not pinned per-workspace —
the wrong primitive for a per-tag terminal backdrop, and has open focus/
monitor-swap bugs (hyprwm/Hyprland#1826, #5460).
**Consequences:** `river` gives floating-above-tiled z-order and full
placement control natively via the protocol; a from-scratch WM there needs
less reinvention than fighting Hyprland's single-workspace model would.

## ADR-003: WM owns all tag state; no compositor-native tags
**Date:** 2026-08-06
**Status:** Accepted
**Decision:** Tag/workspace state (membership, registry, per-output current
tag) lives entirely in `wm-core`'s in-memory model. No compositor plugin or
protocol extension is used to represent tags.
**Rationale:** `river` 0.4+ has no knowledge of tags/workspaces/output focus
at all — that's the point of the protocol split ("rendering state" vs.
"management state"). There is nothing to integrate with; it has to be built.
**Consequences:** Full flexibility over the tag model (see ADR-006), but zero
compositor help — every enforcement rule (e.g. one-tag-per-output) is our own
state-machine invariant, not something the compositor can help guarantee.

## ADR-004: External fuzzel picker over custom-rendered popup
**Date:** 2026-08-06
**Status:** Accepted
**Decision:** The tag-manager UI is `fuzzel`, driven by a small companion
client over the WM's IPC socket — not a custom-rendered Wayland surface.
**Rationale:** Zero custom UI/rendering code to write and maintain; `fuzzel`
already exists, is fast, and the WM only needs to feed it a checklist and
read back a selection.
**Consequences:** Tag-manager UX is bounded by what `fuzzel` can do.
**Resolved 2026-08-08** (the spike this ADR called for, against `fuzzel(1)`
1.14.1): `fuzzel` has neither a native checkbox toggle nor a `--multi` flag.
Rather than fall back to a custom picker, the checklist is built from a
sequential toggle-and-reopen loop — one single-select `fuzzel --dmenu` per
toggle, using `--with-nth`/`--accept-nth`/`--nth-delimiter` to show a
checkbox glyph per row and return a stable tag id. See
`buoy-tag-picker/src/picker.rs`.

## ADR-005: Enforce one-tag-per-output maximum
**Date:** 2026-08-06
**Status:** Accepted
**Decision:** At most one tag is displayed per output at any time; the WM
rejects/reroutes any action that would violate this.
**Rationale:** ~95% of real usage is single-monitor; the only risk the
alternative (free multi-output tag display) would address is a rare
multi-monitor disorientation case, and that's fully covered instead by the
one-tag-per-output rule plus the always-visible status bar.
**Consequences:** Simpler state machine (`Output.current_tag: Option<TagId>`
rather than a set), no need to coordinate simultaneous multi-output tag
display anywhere in the design.

## ADR-006: Tag registry is named, with dynamically assigned bitmask IDs
**Date:** 2026-08-06
**Status:** Accepted
**Decision:** Tags are user-facing arbitrary strings (created on the fly via
the picker's text-input row), each assigned a small integer ID on first
creation. `View.tags` is stored as a bitset over those IDs (e.g. `u64`,
capping the registry at 64 concurrent tags — well above realistic usage).
IDs are never reused within a session; there is no tag-deletion feature in
v1, so no GC/reuse mechanism is built. If the registry is
exhausted (>64 distinct tag names created in a session), tag creation fails
and the failure is surfaced back through the picker rather than silently
allowed.
**Rationale:** Reconciles the original "bitmask, many-to-many" performance
assumption with the requirement for free-text, user-named tags — the name is just a label over an efficiently-
stored bitmask slot. No reuse/GC avoids building machinery for a
tag-deletion feature that doesn't exist yet (YAGNI) — revisit only if/when
deletion is scoped.
**Consequences:** Full flexibility over the tag model. A long-lived session
that creates more than 64 distinct tag names over its lifetime hits a hard
(if unlikely) ceiling — an accepted v1 edge case, not a silent failure.

## ADR-007: IPC protocol is JSON Lines over a Unix domain socket
**Date:** 2026-08-06
**Status:** Accepted
**Decision:** `ipc-server` speaks newline-delimited JSON messages over a Unix
domain socket (one JSON object per line, both directions).
**Rationale:** Simple to implement with `serde_json`, trivially debuggable by
hand with `nc`/`socat` during development, and avoids pulling in a binary
framing/schema dependency for a single-user local-only protocol.
**Consequences:** No schema versioning built in v1 — acceptable since both
ends (WM + companion clients) ship together from the same repo.

## ADR-008: WM source lives in a subdirectory of buoy-wm
**Date:** 2026-08-06
**Status:** Accepted; rationale partly superseded (see Consequences)
**Decision:** The WM binary's Rust source lives at `buoy-wm/wm/` (workspace
subdirectory), not a separate repository.
**Rationale:** Solo project — keeping planning docs and implementation in one
repo/devcontainer avoids cross-repo coordination overhead with no current
benefit (no separate versioning or sharing need).
**Consequences:** `buoy-wm` stops being planning-only; `wm/` becomes a real
Cargo project with its own build step inside the shared devcontainer. The
co-located planning documents this rationale rested on were removed before
the first public release; the workspace-subdirectory layout stands on its
own, and `wm/` is now one of three members alongside `buoy-tag-picker/` and
`buoy-status-bar/`.
