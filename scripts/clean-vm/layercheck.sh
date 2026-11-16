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
# layercheck.sh <vm> <ssh-port>: first boot of a finished desktop layer.
# Greeter screenshot, then the clean-machine evidence (nothing buoy needs
# is installed, the stock sessions only), logged, then power off. After
# this the layer is frozen: every run gets its own overlay (layer.sh).
set -uo pipefail
# shellcheck source=scripts/clean-vm/env.sh
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
vm=$1 port=$2
log="$BUOY_VM_DIR/log/$vm.layercheck.log"

sleep 20
bash "$CLEAN_VM_HOME/gdmwait.sh" "$vm" "$port" "$BUOY_VM_DIR/shots/$vm-layer-gdm.png" || exit 1
# The greeter's user (Fedora's is the dynamic gdm-greeter), then what is
# and is not installed. The query commands exit non-zero by design when a
# package is absent, so the evidence is read, not their status.
case $vm in
    f*) vm_ssh "$port" 'ps -o user:20,args -C gnome-shell; rpm -q river buoy-wm waybar zellij tmux; ls /usr/share/wayland-sessions/; command -v notify-send || echo "notify-send: absent"' ;;
    u*) vm_ssh "$port" 'ps -o user:20,args -C gnome-shell; dpkg -l river buoy-wm waybar tmux 2>&1 | grep -E "^(ii|un|dpkg)"; ls /usr/share/wayland-sessions/; dpkg-query -W gdm3 software-properties-common' ;;
esac | tee "$log"
status=0
if grep -qE '^(river|buoy-wm|waybar|zellij|tmux)-[0-9]|^ii +(river|buoy-wm|waybar|tmux) ' "$log"; then
    echo "$vm: the desktop layer already has something buoy needs installed; it is not clean" >&2
    status=1
fi
vm_ssh "$port" 'sudo systemctl poweroff' || true
exit "$status"
