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
# Asserts that packaging/buoy-session.target says exactly what the session
# needs, then runs `systemd-analyze verify --user` over it where that tool
# exists (the CI runner; not the devcontainer).
#
# The line-by-line comparison is the gate, not verify. Measured on systemd
# 259: verify exits 0 on an unknown key (`BindTo=`), on `BindsTo=not a unit`
# and on a `[Service]` section in a `.target`, and for the last two only
# *prints* "ignoring". Its exit status therefore proves almost nothing on its
# own, so any output from it is treated as a failure here.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly UNIT="$REPO_ROOT/packaging/buoy-session.target"

if [ ! -f "$UNIT" ]; then
    echo "$UNIT: missing" >&2
    exit 1
fi
if [ ! -r "$UNIT" ]; then
    echo "$UNIT: unreadable" >&2
    exit 1
fi

# BindsTo= is what lets a manually started buoy-session.target pull in
# graphical-session.target (RefuseManualStart=yes), and what lets stopping
# it tear every Requisite=/PartOf= unit down again via StopWhenUnneeded=yes.
expected='[Unit]
Description=buoy window manager session
Documentation=man:systemd.special(7)
BindsTo=graphical-session.target
Wants=graphical-session-pre.target
After=graphical-session-pre.target'

# Deliberately stricter than systemd's parser: only `#` comment lines and
# blank lines are set aside (the RPL header is the former), and everything
# else has to match the expected body byte for byte. `;` comments, indented
# comments, trailing whitespace and line continuations are all legal to
# systemd and all rejected here, because the unit is ours and there is no
# reason for it to contain any of them. `|| true` because grep exits 1 when
# nothing is left, as on a comments-only file, and that case should fall
# through to the diff rather than die silently.
actual="$(grep -v -e '^#' -e '^$' "$UNIT" || true)"

if ! diff -u --label expected --label actual \
    <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") >&2; then
    echo "$UNIT: directives differ from the expected unit" >&2
    exit 1
fi

if ! command -v systemd-analyze >/dev/null 2>&1; then
    echo "systemd-analyze not found; line check passed, verify --user skipped"
    # Surfaced in the Actions summary so a runner image that drops systemd
    # does not switch this half of the test off silently.
    if [ -n "${GITHUB_ACTIONS:-}" ]; then
        echo "::warning file=tests/buoy-session-target.sh::systemd-analyze not found; verify --user skipped"
    fi
    exit 0
fi

# verify refuses to run without a usable runtime directory ("Failed to
# lookup RuntimeDirectory path" when unset, "Failed to create directory"
# when set to a path that does not exist), and GitHub runners do not set
# one. An empty temporary directory is enough: no user manager and no bus
# are needed.
if [ ! -d "${XDG_RUNTIME_DIR:-}" ]; then
    runtime_dir="$(mktemp -d)"
    trap 'rm -rf "$runtime_dir"' EXIT
    export XDG_RUNTIME_DIR="$runtime_dir"
fi

# Log level and colour are pinned because any output fails the test: an
# inherited SYSTEMD_LOG_LEVEL=debug prints hundreds of lines for a valid
# unit, while the "ignoring" lines this check relies on are warnings and
# still appear at this level.
if ! output="$(SYSTEMD_LOG_LEVEL=warning SYSTEMD_LOG_COLOR=false \
    systemd-analyze verify --user "$UNIT" 2>&1)" || [ -n "$output" ]; then
    printf '%s\n' "$output" >&2
    echo "$UNIT: systemd-analyze verify --user reported a problem" >&2
    exit 1
fi
