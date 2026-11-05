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
# Asserts that packaging/buoy.desktop hands river the session supervisor,
# byte for byte.
#
# This entry is what GDM runs when the user picks "buoy". A wrong `Exec=`
# is a login that never imports the session into systemd, so the portals
# never start and screen sharing is silently absent (ADR-010). The body is
# diffed rather than grepped because every line matters: `DesktopNames=`
# is what selects buoy-portals.conf, and `Type=` is what makes the entry
# a session at all.
#
# There is no desktop-file-validate step: it rejects `DesktopNames=`,
# which session entries require, so this line check is the whole gate.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly ENTRY="$REPO_ROOT/packaging/buoy.desktop"

if [ ! -f "$ENTRY" ]; then
    echo "$ENTRY: missing" >&2
    exit 1
fi
if [ ! -r "$ENTRY" ]; then
    echo "$ENTRY: unreadable" >&2
    exit 1
fi

expected='[Desktop Entry]
Name=buoy
Comment=Keyboard-driven window manager on the river compositor
Exec=river -c /usr/libexec/buoy-wm/buoy-session
Type=Application
DesktopNames=buoy
Keywords=tiling;wayland;river;'

# Only `#` comment lines and blank lines are set aside (the RPL header is
# the former); everything else has to match the expected body byte for
# byte. `|| true` because grep exits 1 when nothing is left, as on a
# comments-only file, and that case should fall through to the diff.
actual="$(grep -v -e '^#' -e '^$' "$ENTRY" || true)"

# Checked before the diff so that the failure names the cause: the diff
# below would reject the old line too, but only as "entry differs". A
# here-string rather than a pipe: `grep -q` closes its input on the first
# match, and under `pipefail` the writer's SIGPIPE would turn a found line
# into "absent".
if grep -qx 'Exec=river -c /usr/libexec/buoy-wm/buoy-wm' <<<"$actual"; then
    echo "$ENTRY: Exec= runs the WM directly; the session must be supervised by buoy-session (ADR-010)" >&2
    exit 1
fi

if ! diff -u --label expected --label actual \
    <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") >&2; then
    echo "$ENTRY: entry differs from the expected body" >&2
    exit 1
fi
