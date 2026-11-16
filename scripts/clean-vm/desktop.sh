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
# desktop.sh <vm> <ssh-port>: on a VM booted from its fresh desktop layer,
# wait for cloud-init, install the release's own desktop package set
# (Fedora Workstation's environment group, Ubuntu's ubuntu-desktop-minimal)
# and power off. About fifteen minutes each.
set -uo pipefail
# shellcheck source=scripts/clean-vm/env.sh
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
vm=$1 port=$2
log="$BUOY_VM_DIR/log/$vm.desktop.log"
python3 -I "$CLEAN_VM_HOME/vms.py" image "$vm" >/dev/null || exit 1

wait_for_ssh "$port" 600 || exit 1
if ! vm_ssh "$port" 'cloud-init status --wait'; then
    echo "$vm: cloud-init did not finish cleanly; the layer would not be clean" >&2
    exit 1
fi
vm_ssh "$port" 'grep -E "^(PRETTY_NAME|VERSION_ID)=" /etc/os-release; uname -r; df -h / | tail -n 1'
start=$(date +%s)
case $vm in
    f*) vm_ssh "$port" 'sudo dnf -y group install workstation-product-environment && sudo systemctl set-default graphical.target' ;;
    u*) vm_ssh "$port" 'sudo DEBIAN_FRONTEND=noninteractive apt-get update && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y ubuntu-desktop-minimal' ;;
esac >"$log" 2>&1
rc=$?
echo "$vm: desktop install exit $rc in $(($(date +%s) - start))s (log $log)"
vm_ssh "$port" 'systemctl get-default; sudo systemctl poweroff' || true
exit "$rc"
