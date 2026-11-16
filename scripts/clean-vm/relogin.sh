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
# relogin.sh: runs ON the VM after the second login. Both targets came back
# with no failed unit and no start-limit hit, the session is buoy's again,
# and the journal shows both targets reached.
uid="$(id -u)"
export XDG_RUNTIME_DIR="/run/user/$uid"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"

for unit in buoy-session.target graphical-session.target xdg-desktop-portal-wlr.service waybar.service; do
    printf '%-32s %s\n' "$unit" "$(systemctl --user is-active "$unit")"
done
echo "failed: $(systemctl --user list-units --failed --no-legend | wc -l)"
echo "start request repeated: $(journalctl --user -b --no-pager | grep -c 'start request repeated')"
pgrep -af 'buoy-wm|river -c|pinned-term'
journalctl --user -b --no-pager -o short-precise |
    grep -E 'Reached target (buoy-session|graphical-session)' | tail -n 4
