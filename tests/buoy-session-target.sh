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

# BindsTo= is what lets a manually started buoy-session.target pull in
# graphical-session.target (RefuseManualStart=yes), and what lets stopping
# it tear every Requisite=/PartOf= unit down again via StopWhenUnneeded=yes.
expected='[Unit]
Description=buoy window manager session
Documentation=man:systemd.special(7)
BindsTo=graphical-session.target
Wants=graphical-session-pre.target
After=graphical-session-pre.target'

# Everything that is not a `#` comment or a blank line: the RPL header is
# comments, systemd ignores both, so this is what the manager actually reads.
actual="$(grep -v -e '^#' -e '^$' "$UNIT" || true)"

if ! diff <(printf '%s\n' "$expected") <(printf '%s\n' "$actual"); then
    echo "$UNIT: directives differ from the expected unit (expected < > actual)" >&2
    exit 1
fi

if ! command -v systemd-analyze >/dev/null 2>&1; then
    echo "systemd-analyze not found; line check passed, verify --user skipped"
    exit 0
fi

# verify refuses to run without a runtime directory ("Failed to lookup
# RuntimeDirectory path"), and GitHub runners do not set one. An empty
# temporary directory is enough: no user manager and no bus are needed.
if [ -z "${XDG_RUNTIME_DIR:-}" ]; then
    runtime_dir="$(mktemp -d)"
    trap 'rm -rf "$runtime_dir"' EXIT
    export XDG_RUNTIME_DIR="$runtime_dir"
fi

if ! output="$(systemd-analyze verify --user "$UNIT" 2>&1)" || [ -n "$output" ]; then
    printf '%s\n' "$output" >&2
    echo "$UNIT: systemd-analyze verify --user reported a problem" >&2
    exit 1
fi
