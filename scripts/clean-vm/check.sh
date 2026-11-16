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
# check.sh: runs ON the VM, over ssh, while the buoy session is up. Prints
# the evidence summary.py reads, one `== ` section each: the process tree
# from GDM down, the session environment and units, the WM's journal
# identifier, the portals, what screen sharing offers and whether
# screenshots answer, the bar, buoy's own state, the pinned terminal,
# and every other process the test user has (a stray one could keep the
# user manager alive and hide a logout defect).
uid="$(id -u)"
export XDG_RUNTIME_DIR="/run/user/$uid"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"

echo "== (b) process tree"
# shellcheck disable=SC2009 # the --forest tree is the evidence; pgrep has no tree
ps -eo pid,ppid,user,args --forest | grep -E 'gdm-wayland-session|river|buoy|waybar|zellij|tmux|foot|portal' | grep -v grep
echo "== (b) environment"
systemctl --user show-environment | grep -E '^(XDG_CURRENT_DESKTOP|XDG_SESSION_TYPE|XDG_SESSION_DESKTOP|WAYLAND_DISPLAY|PATH)='
for unit in buoy-session.target graphical-session.target xdg-desktop-portal.service \
    xdg-desktop-portal-wlr.service xdg-desktop-portal-gtk.service waybar.service; do
    printf '%-34s %s\n' "$unit" "$(systemctl --user is-active "$unit")"
done
echo "== (b) failed units"
systemctl --user list-units --failed --no-legend
# The supervisor hands the WM's output to journald as `buoy-wm` with its
# severities (Story 6.2); on rc.3 and earlier this query is empty in the
# packaged session. A healthy WM may log nothing, so a request for an
# output that cannot exist is sent first: the WM refuses it, changes
# nothing, and logs the refusal as an error, which the query must find.
echo "== (b) journal identity"
python3 -c '
import os, socket
s = socket.socket(socket.AF_UNIX)
s.connect(os.path.join(os.environ["XDG_RUNTIME_DIR"], "buoy-wm.sock"))
s.sendall(b"{\"type\":\"switch-tag\",\"output_id\":4294967295,\"tag_id\":0}\n")
s.makefile().readline()'
sleep 1
journalctl --user -b -q --no-pager --identifier=buoy-wm -p err -o cat | grep -F 'switch-tag from' | tail -n 1 | grep . || echo "(none)"
echo "== (c) portal backend errors"
journalctl --user -b --no-pager | grep -E 'Requested backend|does not exist' || echo "(none)"
echo "== (c) AvailableSourceTypes"
busctl --user get-property org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop \
    org.freedesktop.portal.ScreenCast AvailableSourceTypes
echo "== (c) Screenshot portal"
busctl --user get-property org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop \
    org.freedesktop.portal.Screenshot version
# Story 5.2 saw this warning in a container before gtk's backend started;
# it is recorded here for a person to read, not judged.
echo "== (c) screenshot access warning"
journalctl --user -b --no-pager | grep -F 'requires an access impl' || echo "(none)"
echo "== (d) waybar children"
pgrep -x waybar | while read -r bar; do ps -o pid,args --ppid "$bar"; done
echo "== (e) get-state"
python3 -c '
import os, socket
s = socket.socket(socket.AF_UNIX)
s.connect(os.path.join(os.environ["XDG_RUNTIME_DIR"], "buoy-wm.sock"))
s.sendall(b"{\"type\":\"get-state\"}\n")
print(s.makefile().readline().strip())'
echo "== (e) pinned terminals"
pgrep -af pinned-term || echo "(no pinned-term process)"
command -v zellij >/dev/null && zellij list-sessions --no-formatting 2>&1
command -v tmux >/dev/null && tmux list-sessions 2>&1
echo "== other processes of $USER (contamination rule)"
# shellcheck disable=SC2009 # every process with its arguments, minus known noise
ps -u "$USER" -o pid,args | grep -vE 'systemd --user|\(sd-pam\)|sshd|ps -u|bash|grep' | head -40
echo "== journal: river, buoy, start-limit"
journalctl --user -b --no-pager | grep -E 'buoy-session:|start request repeated|failed to run init|EGL|exited unsuccessfully' | tail -20
