#!/usr/bin/env bash
# SPDX-FileCopyrightText: © 2026 Nick Booth
# SPDX-License-Identifier: RPL-1.5
#
# Unless explicitly acquired and licensed from Licensor under another
# license, the contents of this file are subject to the Reciprocal Public
# License ("RPL") Version 1.5, or subsequent versions as allowed by the
# RPL, and You may not copy or use this file in either source code or
# executable form, except in compliance with the terms and conditions of
# the RPL.
#
# All software distributed under the RPL is provided strictly on an "AS
# IS" basis, WITHOUT WARRANTY OF ANY KIND, EITHER EXPRESS OR IMPLIED, AND
# LICENSOR HEREBY DISCLAIMS ALL SUCH WARRANTIES, INCLUDING WITHOUT
# LIMITATION, ANY WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
# PURPOSE, QUIET ENJOYMENT, OR NON-INFRINGEMENT. See the RPL for specific
# language governing rights and limitations under the RPL.
#
# Asserts the file and dependency contract of packaging/rpm/buoy-wm.spec:
# the RPM installs the supervisor, the session entry, the session target
# and the portal routing at the paths the login path expects, and depends
# on every program that path calls (river, systemctl,
# dbus-update-activation-environment) plus the portal backends
# buoy-portals.conf routes to.
#
# The spec is only built on a tag, so a missing %files line or a dropped
# Requires would otherwise surface in the release build, not on the push
# that caused it. Two tiers: a text gate that always runs and reports every
# mismatch before exiting, and, where rpmspec exists, the real parser as a
# structural and dependency gate. rpmspec -q does not reject a misspelled
# section name, so the text tier stays the content gate.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly SPEC="$REPO_ROOT/packaging/rpm/buoy-wm.spec"

if [ ! -f "$SPEC" ]; then
    echo "$SPEC: missing" >&2
    exit 1
fi
if [ ! -r "$SPEC" ]; then
    echo "$SPEC: unreadable" >&2
    exit 1
fi

status=0

# `-Fx --`: the leading `%` and the braces are literal, and `--` because a
# pattern may start with `-`.
require_line() {
    grep -Fxq -- "$1" "$SPEC" || { echo "$SPEC: missing line: $1" >&2; status=1; }
}

# rpm ignores the whitespace after a tag's colon, so the match does too.
# The value is a regex, escaped by hand in the call.
require_tag() {
    grep -Eq "^$1:[[:space:]]+$2\$" "$SPEC" || { echo "$SPEC: missing tag: $1: $2" >&2; status=1; }
}

forbid() {
    ! grep -Eq -- "$1" "$SPEC" || { echo "$SPEC: forbidden: $1 ($2)" >&2; status=1; }
}

# %install: every packaging file comes from the unpacked source tree.
require_line 'install -Dpm 0755 packaging/buoy-session %{buildroot}%{_libexecdir}/%{name}/buoy-session'
require_line 'install -Dpm 0644 packaging/buoy.desktop %{buildroot}%{_datadir}/wayland-sessions/buoy.desktop'
require_line 'install -Dpm 0644 packaging/buoy-session.target %{buildroot}%{_userunitdir}/buoy-session.target'
require_line 'install -Dpm 0644 packaging/buoy-portals.conf %{buildroot}%{_datadir}/xdg-desktop-portal/buoy-portals.conf'

# %files: the installed paths, and the portal directory co-owned with
# xdg-desktop-portal because that package is only a Recommends away.
require_line '%{_libexecdir}/%{name}/buoy-session'
require_line '%{_datadir}/wayland-sessions/buoy.desktop'
require_line '%{_userunitdir}/buoy-session.target'
require_line '%dir %{_datadir}/xdg-desktop-portal'
require_line '%{_datadir}/xdg-desktop-portal/buoy-portals.conf'

# %{_userunitdir} is unexpanded without the macros package.
require_tag BuildRequires 'systemd-rpm-macros'

# The session entry runs river; the supervisor calls systemctl and
# dbus-update-activation-environment on every login.
require_tag Requires 'river >= 0\.4'
require_tag Requires 'systemd'
require_tag Requires 'dbus-tools'
require_tag Recommends 'foot'
require_tag Recommends 'zellij'
require_tag Recommends 'fuzzel'
require_tag Recommends 'xdg-desktop-portal-wlr'
require_tag Recommends 'xdg-desktop-portal-gtk'
require_tag Suggests 'waybar'

forbid '^Recommends:[[:space:]]+river' 'river is a hard dependency: the session entry runs it'
forbid '^Source2:' 'the session entry comes from the source tree, not a separate Source'
forbid '%\{SOURCE2\}' 'the session entry comes from the source tree, not a separate Source'
forbid 'not in Fedora' 'stale: Fedora 44 and 45 ship river 0.5.0'
# The comment that justified `Recommends: river` wrapped "not in Fedora"
# across two lines, so the forbid above never saw it; this phrase from the
# same paragraph sits on one line.
forbid 'river-classic' 'stale: the river in Fedora 44 and 45 is 0.5.0, not river-classic'

# Only buoy-wm is on PATH, and it must stay there: the supervisor and the
# helpers are libexec-only.
require_line '%{_bindir}/buoy-wm'
# `|| true`: the second grep exits 1 when nothing is left, which is the
# passing case.
extra_bindir="$(grep -E '^%\{_bindir\}/' "$SPEC" | grep -vx '%{_bindir}/buoy-wm' || true)"
if [ -n "$extra_bindir" ]; then
    echo "$SPEC: only %{_bindir}/buoy-wm may be on PATH; found:" >&2
    echo "$extra_bindir" >&2
    status=1
fi

[ "$status" -eq 0 ] || exit 1

# Tier (b): the real parser. Optional because the host and a bare runner
# have no rpmspec; CI installs the rpm package so this tier runs there.
if ! command -v rpmspec >/dev/null 2>&1; then
    echo "rpmspec not found; text gate passed, parser tier skipped"
    if [ -n "${GITHUB_ACTIONS:-}" ]; then
        echo "::warning file=tests/rpm-spec.sh::rpmspec not found; the spec was not parsed"
    fi
    exit 0
fi

# The define keeps the parse identical on a tree without
# systemd-rpm-macros, where %{_userunitdir} would otherwise stay
# unexpanded.
if ! rpmspec --define '_userunitdir /usr/lib/systemd/user' -P "$SPEC" >/dev/null; then
    echo "$SPEC: rpmspec -P failed" >&2
    exit 1
fi

# One prefixed line per dependency so each assertion below is unambiguous.
deps="$(rpmspec --define '_userunitdir /usr/lib/systemd/user' -q \
    --qf '[R %{REQUIRENEVRS}\n][W %{RECOMMENDNEVRS}\n][S %{SUGGESTNEVRS}\n]' "$SPEC")"

require_dep() {
    grep -Fxq -- "$1" <<<"$deps" || { echo "$SPEC: rpmspec does not report dependency: $1" >&2; status=1; }
}

require_dep 'R river >= 0.4'
require_dep 'R systemd'
require_dep 'R dbus-tools'
require_dep 'W foot'
require_dep 'W zellij'
require_dep 'W fuzzel'
require_dep 'W xdg-desktop-portal-wlr'
require_dep 'W xdg-desktop-portal-gtk'
require_dep 'S waybar'

if grep -Fxq 'W river >= 0.4' <<<"$deps"; then
    echo "$SPEC: rpmspec reports river as a Recommends; it must be a Requires" >&2
    status=1
fi

if [ "$status" -ne 0 ]; then
    echo "rpmspec dependencies:" >&2
    echo "$deps" >&2
    exit 1
fi
