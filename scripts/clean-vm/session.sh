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
# session.sh <vm> <ssh-port> <label> [greeter]: the session check on a VM
# run_readme.sh installed and rebooted to GDM.
#   1. Greeter: hold an ssh session open first, so the user manager
#      outlives the login (the Story 4.13 logout scenario), click the user
#      tile and open the session menu, screenshot each step. With
#      `greeter`, stop here: read shots/<vm>-<label>-gear.png to confirm
#      vms.py's coordinates before a full pass.
#   2. Login: click `buoy` and the password field where vms.py says this
#      release's greeter draws them (1280x800), type the password, then
#      collect check.sh's evidence.
#   3. Logout: Super+Esc, then logout.sh's evidence.
#   4. Re-login (GDM remembers buoy), then relogin.sh's evidence.
# Evidence lands in runs/<vm>-<label>-{check,logout,relogin}.log for
# summary.py, which judges it; a failed click or keypress stops the run.
set -euo pipefail
# shellcheck source=scripts/clean-vm/env.sh
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
vm=$1 port=$2 label=$3 stop=${4:-}
read -r buoy_x buoy_y pw_x pw_y <<<"$(python3 -I "$CLEAN_VM_HOME/vms.py" gdm "$vm")"
runs="$BUOY_VM_DIR/runs" shots="$BUOY_VM_DIR/shots"
mkdir -p "$runs" "$shots"

hold_ssh() {
    setsid ssh "${VM_SSH_OPTS[@]}" -o ServerAliveInterval=30 -p "$port" "$VM_USER@127.0.0.1" \
        'sleep 3000' </dev/null >/dev/null 2>&1 &
    hold=$!
    trap 'kill "$hold" 2>/dev/null || true' EXIT
    local tries
    for tries in 1 2 3 4 5 6; do
        # shellcheck disable=SC2016 # $USER is the VM's, expanded there
        if vm_ssh "$port" 'pgrep -u "$USER" -fx "sleep 3000" >/dev/null'; then
            return 0
        fi
        sleep "$tries"
    done
    echo "$vm: the ssh hold never reached the VM" >&2
    return 1
}

greeter_menu() {
    bash "$CLEAN_VM_HOME/gdmwait.sh" "$vm" "$port" "$shots/$vm-$label-greeter.png"
    hold_ssh
    # The user tile and the session gear, where both stock greeters put them.
    qmp "$vm" click 640 349 1280 800
    sleep 2
    qmp "$vm" shot "$shots/$vm-$label-pw.png"
    qmp "$vm" click 1160 744 1280 800
    sleep 2
    qmp "$vm" shot "$shots/$vm-$label-gear.png"
}

log_in() {
    qmp "$vm" click "$buoy_x" "$buoy_y" 1280 800
    sleep 1
    qmp "$vm" click "$pw_x" "$pw_y" 1280 800
    sleep 0.5
    qmp "$vm" type "$VM_PASSWORD"
    qmp "$vm" keys ret
    sleep 15
    qmp "$vm" shot "$shots/$vm-$label-session.png"
    # The evidence scripts' own status is not the verdict; summary.py is.
    vm_ssh "$port" 'bash ~/check.sh' >"$runs/$vm-$label-check.log" 2>&1 || true
}

log_out() {
    qmp "$vm" keys meta_l-esc
    sleep 10
    qmp "$vm" shot "$shots/$vm-$label-logout.png"
    vm_ssh "$port" 'bash ~/logout.sh' >"$runs/$vm-$label-logout.log" 2>&1 || true
}

log_in_again() {
    qmp "$vm" keys shift
    sleep 1
    qmp "$vm" click 640 349 1280 800
    sleep 2
    qmp "$vm" shot "$shots/$vm-$label-relogin-pw.png"
    qmp "$vm" click "$pw_x" "$pw_y" 1280 800
    sleep 0.5
    qmp "$vm" type "$VM_PASSWORD"
    qmp "$vm" keys ret
    sleep 15
    qmp "$vm" shot "$shots/$vm-$label-relogin.png"
    vm_ssh "$port" 'bash ~/relogin.sh' >"$runs/$vm-$label-relogin.log" 2>&1 || true
}

greeter_menu
if [ "$stop" = greeter ]; then
    echo "$vm: greeter screenshots in $shots/$vm-$label-{greeter,pw,gear}.png"
    exit 0
fi
log_in
sleep 3
log_out
log_in_again
echo "$vm: evidence in $runs/$vm-$label-{check,logout,relogin}.log, screenshots in $shots"
