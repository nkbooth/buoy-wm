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
# logout.sh: runs ON the VM after Super+Esc. Both targets stopped, buoy's
# before the graphical one; nothing of the session survives; the user
# manager is still up (session.sh's ssh hold), which is the case the
# second stop exists for.
uid="$(id -u)"
export XDG_RUNTIME_DIR="/run/user/$uid"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"

echo "== stop order"
journalctl --user -b --no-pager -o short-precise |
    grep -E 'Stopped target (buoy-session|graphical-session)\.target|Stopping .*target' | tail -6
for target in buoy-session.target graphical-session.target; do
    printf '%-28s %s\n' "$target" "$(systemctl --user is-active "$target")"
done
echo "== survivors"
pgrep -af 'xdg-desktop-portal-wlr|waybar|buoy-status-bar|buoy-wm|river' || echo "(none)"
echo "== $USER processes"
# shellcheck disable=SC2009 # every process with its arguments, minus known noise
ps -u "$USER" -o pid,args | grep -vE 'ps -u|grep|bash$'
echo "== start limit"
journalctl --user -b --no-pager | grep -c 'start request repeated' || true
loginctl list-sessions --no-legend
