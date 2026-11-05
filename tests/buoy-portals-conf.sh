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
# Asserts that packaging/buoy-portals.conf routes ScreenCast and Screenshot
# to the wlroots portal backend and everything else to gtk, byte for byte.
#
# This file is the only thing that gives the packaged session screen
# sharing: `DesktopNames=buoy` matches no shipped portals.conf and no
# backend's `UseIn=`, and the gtk fallback implements neither interface.
# A typo in a key is not an error to the portal, just an ignored key and a
# silent fall-through, which is why the body is diffed rather than grepped.
#
# `none` is forbidden anywhere in a value: xdg-desktop-portal (1.22.1)
# checks every list for `none` before trying any backend named in it, so
# `wlr;none` means "disabled", and the portal only says so at debug level.
# There is no validator for portals.conf on the CI runner, so this line
# check is the whole gate.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly CONF="$REPO_ROOT/packaging/buoy-portals.conf"

if [ ! -f "$CONF" ]; then
    echo "$CONF: missing" >&2
    exit 1
fi
if [ ! -r "$CONF" ]; then
    echo "$CONF: unreadable" >&2
    exit 1
fi

expected='[preferred]
default=gtk
org.freedesktop.impl.portal.ScreenCast=wlr
org.freedesktop.impl.portal.Screenshot=wlr'

# Deliberately stricter than the parser: only `#` comment lines and blank
# lines are set aside (the RPL header is the former), and everything else
# has to match the expected body byte for byte. `|| true` because grep
# exits 1 when nothing is left, as on a comments-only file, and that case
# should fall through to the diff rather than die silently.
actual="$(grep -v -e '^#' -e '^$' "$CONF" || true)"

# Checked before the diff so that the failure names the cause: the diff
# below would reject `none` too, but only as "routing differs". `-w` so
# that `;` and `=` count as word boundaries. A here-string rather than a
# pipe: `grep -q` closes its input on the first match, and under
# `pipefail` the writer's SIGPIPE would turn a found `none` into "absent".
if grep -qw none <<<"$actual"; then
    echo "$CONF: 'none' disables an interface in xdg-desktop-portal >= 1.22; remove it" >&2
    exit 1
fi

if ! diff -u --label expected --label actual \
    <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") >&2; then
    echo "$CONF: routing differs from the expected body" >&2
    exit 1
fi
