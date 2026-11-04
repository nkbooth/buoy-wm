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

## ADR-009: Shared behaviour is extracted; shared shapes are not
**Date:** 2026-08-20
**Status:** Accepted
**Decision:** A rule the three binaries have to *agree* on lives once, in
`buoy-common`. A *type* they happen to have in common stays per-crate:
`wm/src/ipc/protocol.rs`, `buoy-tag-picker/src/wire.rs` and
`buoy-status-bar/src/wire.rs` keep their own `Request`/`Response`
definitions.
**Rationale:** The two cases fail differently. If the binaries disagree
about where the socket lives, how a line is framed, or how a peer is
authenticated, they cannot find or trust each other and the symptom is
"the picker does nothing" — a silent runtime failure no compile step could
have caught, so it must not be duplicated. A wire *shape*, in contrast,
fails loudly: a message one end cannot parse shows up in the first test
that sends it. And the satellites' shapes are deliberately narrower than
`wm`'s, which is a property worth keeping rather than debt to consolidate —
`buoy-status-bar`'s `Request` has exactly one variant, so it *structurally
cannot* send a mutation. Merging to `wm`'s superset would trade that away
for consistency.
**Consequences:** `buoy-common` holds behaviour only, and its module docs
say so, so the next candidate for extraction has a test to meet rather than
a precedent to point at. Three copies of the wire types are maintained by
hand; the insurance is a byte-compatibility test per message shape, not an
extraction. Revisit on a fourth consumer, or on the first rename of a
`Response` field. Separately, the RPL-1.5 notice block is replicated in
every source file on purpose — RPL 1.5 §6.4(a) requires it there — and a
pre-commit hook checks for it rather than any attempt to factor it out.

## ADR-010: The session entry supervises buoy-wm; it does not exec it
**Date:** 2026-11-03
**Status:** Accepted
**Decision:** `wayland-sessions/buoy.desktop` runs
`river -c /usr/libexec/buoy-wm/buoy-session`. River is still what the
display manager starts; what changes is its init child, which becomes
`buoy-session`, a POSIX sh supervisor installed beside the three binaries
and resolving `buoy-wm` as its sibling, rather than `buoy-wm` itself. The
supervisor imports `WAYLAND_DISPLAY`, `DISPLAY`, `XDG_CURRENT_DESKTOP`,
`XDG_SESSION_TYPE` and `XDG_SESSION_DESKTOP` into the systemd user manager
and the D-Bus activation environment, starts `buoy-session.target` (bound to
`graphical-session.target`), runs the WM as a child, forwards termination
signals to it, stops the target when it exits, and exits with its status.
**Rationale:** Measured on the live host on 2026-11-03: an exec'd WM
imports nothing into systemd, so `xdg-desktop-portal`
(`Requisite=graphical-session.target`) never starts and screen sharing is
silently absent. And a WM that exits without stopping its target leaves
portals and user units running against a dead socket into the next login.
`graphical-session.target` refuses a manual start, which is why a session
needs its own target to bind to it. The hand-written `~/.config/river/init`
already did all of this with a `trap`; the packaged session has to do it
too, or it is worse than the thing it replaces.
**Consequences:** One shell script sits in the trusted login path and can
produce a black screen with no shell to recover from, so it is written
test-first against stub `systemctl`, `dbus-update-activation-environment`
and `buoy-wm` executables and shellchecked in CI. The `river/init` path
stays documented for source installs and remains the author's fallback login
until the packaged session is verified live.

## ADR-011: Releases ship distribution packages as GitHub release assets; archives and build services are optional mirrors
**Date:** 2026-11-03
**Status:** Accepted (supersedes the channel ranking in `docs/PACKAGING.md`
as of this date)
**Decision:** Every release attaches, next to the tarball, one RPM per
supported Fedora release and one `.deb` per supported Ubuntu release, each
built in that distribution's own container with its own `rpmbuild` or
`dpkg-buildpackage`, each installing the same layout: the three binaries in
`/usr/libexec/buoy-wm`, `buoy-wm` on PATH, the session entry, supervisor,
user target, portal configuration and example config. COPR, a Launchpad PPA
and the Homebrew tap are optional copies made after the tag, never the
source of truth.
**Rationale:** RPL 1.5 keeps `buoy` out of Fedora proper and out of the
Debian and Ubuntu archives, but nothing about it blocks a package on a
GitHub release or a personal build service; the earlier "Debian/Ubuntu
packaging — not viable" verdict conflated archive inclusion with
distribution. A release asset needs no second account, no vendored-crates
upload and no network-less build, and it is the one place every channel can
be derived from. The author's own machine installs the RPM by release URL.
**Consequences:** `release.yml` grows a per-distribution matrix and a Debian
packaging directory joins the RPM spec. The README's supported-platforms
table is what the matrix is derived from, so a release leaving support is a
one-line change in one place. The Homebrew formula keeps its placeholder
checksum until a post-tag step fills it, and that step is in the runbook
rather than implied.

## ADR-012: RC tags map to tilde versions
**Date:** 2026-11-03
**Status:** Accepted
**Decision:** A pre-release is tagged `vX.Y.Z-rc.N` and nothing else.
`Cargo.toml` carries `X.Y.Z-rc.N`, the RPM spec `X.Y.Z~rc.N`, the Debian
changelog `X.Y.Z~rc.N-1`, and the Homebrew formula URL the tag verbatim. The
release workflow derives all four from the tag and refuses a tag that
disagrees with any manifest, as it already does for final tags.
**Rationale:** `-` is illegal in an RPM `Version:` and in the upstream part
of a Debian version, and `~` is the one character both `rpm` and `dpkg`
sort *before* the empty string, so `1.0.0~rc.1 < 1.0.0` and an upgrade from
the RC to the final works. The first public tag is `v1.0.0-rc.1` rather than
`v0.1.0-rc.1`: nothing was ever tagged, the product has been a daily driver
since August, and the changelog already makes 1.0-style compatibility
promises for config spellings.
**Consequences:** The tag regex accepts exactly `rc.N` (not `rc1`, not
`beta`), so the mapping stays one-to-one and testable. The changelog's
phantom `[0.1.0]` entry is folded into the real first release.

## ADR-013: Supported distributions are the current releases of Fedora and Ubuntu, in two tiers
**Date:** 2026-11-03
**Status:** Accepted
**Decision:** Tier 1 — session-verified on a clean virtual machine before
every final release — is Fedora 44, Fedora 45, Ubuntu 26.04 LTS and Ubuntu
26.10. Tier 2 — builds, installs and passes tests in CI, no session
verified — is empty today and exists for a release that qualifies later. A
release in standard support with no `dnf`/`apt` source for river 0.4+
cannot be tier 1. The list lives in the README and drives the CI and
release matrices; a release leaves the list when it leaves standard support.
**Rationale:** Read from the package indexes on 2026-11-03. Fedora 44 and 45
ship river 0.5.0, zellij, rust 1.99 and `xdg-desktop-portal-wlr` 0.8.4 in
the archive. Ubuntu ships no river in any release; the only apt source is
one person's unofficial PPA, with 0.4.1 for 26.04 and 0.4.5 for 26.10, and
no zellij anywhere. Ubuntu 24.04 has no river from any source, a portal
stack two years behind 26.04's and rustc 1.75; a session there cannot be
verified without the user building river, so it is unsupported rather than
promised. Ubuntu 22.04 ships xdg-desktop-portal 1.14 and is not considered.
**Consequences:** The README tells Ubuntu users where river and a session
multiplexer come from — the PPA, with its caveats, and zellij from upstream
or `tmux` from the archive, since buoy only ever runs the configured
`pinned_terminal_args` and `zellij` is a default, not a dependency. The
`.deb` cannot `Depends` on `river` or `zellij`. The tier-2 definition is
kept so adding a release is a decision against a written rule, not a fresh
argument.

## ADR-014: MSRV is the oldest supported distribution's rustc
**Date:** 2026-11-03
**Status:** Accepted
**Decision:** `rust-version` in `Cargo.toml` is the rustc shipped by the
oldest tier-1 distribution — 1.93, from Ubuntu 26.04 LTS — and is
re-evaluated whenever the supported list changes. It is never raised to
"current stable" for its own sake.
**Rationale:** The 2026-10-03 bump from 1.88 to 1.99 was a policy choice,
not a feature need; the one feature that ever forced a bump (let chains)
landed in 1.88. No Ubuntu rustc meets 1.99 (24.04: 1.75, 26.04: 1.93,
26.10: 1.97), which made a Launchpad PPA impossible and a rebuild from any
Ubuntu toolchain fail. On 2026-11-03 the locked workspace was built and
every test suite passed on rust 1.93 in a one-shot container, so the drop
costs nothing today. Fedora 44 and 45 ship 1.99.1 and are unaffected.
**Consequences:** The `msrv` CI job already reads the value from
`Cargo.toml`, so the guard follows the manifest. A dependency bump that
raises the lockfile's effective MSRV above the floor fails that job, which
is the intended signal: either find a version that fits or make a conscious
decision to drop a distribution. Distribution packages — the RPM and the
`.deb` — are built inside their distribution's container with that
distribution's own rustc, which is exactly what this floor guarantees;
only the release tarball uses a pinned rustup toolchain, so that what ships
as a bare binary is reproducible independent of any distribution's
packaging cadence.

## ADR-015: The glibc floor is set by a container, never a runner label
**Date:** 2026-11-03
**Status:** Accepted
**Decision:** `release.yml` jobs run on the current Ubuntu LTS runner label
(`ubuntu-24.04` at the time of this decision; the move off `ubuntu-22.04`
is Story 5.4 of the release plan and had not landed when this was written)
purely as a host. The release tarball is built inside
the oldest-glibc container in the supported matrix (`ubuntu:26.04`, glibc
2.43), and `docs/PACKAGING.md` states that floor. Each distribution package
is built in its own distribution's container.
**Rationale:** Until now the floor was whatever glibc the `ubuntu-22.04`
runner happened to have (2.35), and that label began retiring on
2026-09-17, with brownouts from 2027-03-23 and removal on 2027-04-17
(actions/runner-images#14254). A floor that lives in a runner label moves
whenever GitHub retires one, silently, in the one job that mints the
provenance attestation users are told to trust. A container image is pinned
by the repository and changes only when the supported list does.
**Consequences:** The floor rises from 2.35 to 2.43, which every supported
release clears; older distributions may still run the tarball if they build
river themselves, and the README says so without promising it. The runner
label is a line that gets bumped on the LTS cadence with no effect on what
ships. This change has a hard deadline: it must land before the first
brownout on 2027-03-23.
