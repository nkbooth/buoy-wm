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
# run_readme.sh <vm> <ssh-port> <pre|post> <label>: on a VM booted from a
# fresh overlay, prove it clean, build its script from this checkout's
# README.md (readme_blocks.py), copy in the candidate packages (pre) and
# the evidence scripts, run the README script under a pty that answers
# dnf's and apt's prompts as a person would (answer.py), log the run to
# runs/<vm>-<label>.log (replacing an earlier run's), and reboot to GDM
# for session.sh. Exits 0 only when the README script exited 0; pre-tag,
# only when the candidates were built from this checkout's HEAD.
set -euo pipefail
# shellcheck source=scripts/clean-vm/env.sh
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
vm=$1 port=$2 mode=$3 label=$4
log="$BUOY_VM_DIR/runs/$vm-$label.log"
script="$BUOY_VM_DIR/runs/$vm-$label.sh"

case $mode in
    pre) substitute=() ;;
    post) substitute=(--no-substitute) ;;
    *) echo "mode must be pre or post, not '$mode'" >&2; exit 2 ;;
esac
mkdir -p "$BUOY_VM_DIR/runs"
: >"$log"
candidates=()
if [ "$mode" = pre ]; then
    target="$(python3 -I "$CLEAN_VM_HOME/vms.py" target "$vm")"
    built="$(cat "$BUOY_VM_DIR/cand/out/COMMIT" 2>/dev/null || true)"
    head="$(git -C "$REPO_ROOT" rev-parse HEAD)"
    if [ "$built" != "$head" ]; then
        echo "candidates are from '${built:-nothing}', not HEAD $head: run candidates.sh" >&2
        exit 1
    fi
    shopt -s nullglob
    candidates=("$BUOY_VM_DIR/cand/out/$target"/*)
    shopt -u nullglob
    if [ "${#candidates[@]}" -eq 0 ]; then
        echo "no candidate packages in $BUOY_VM_DIR/cand/out/$target" >&2
        exit 1
    fi
fi
python3 -I "$CLEAN_VM_HOME/readme_blocks.py" "$REPO_ROOT/README.md" "$vm" "$script" "${substitute[@]}" >>"$log"
wait_for_ssh "$port" 300

echo "== clean check ($(date -Is))" >>"$log"
case $vm in
    f*) vm_ssh "$port" 'rpm -q river buoy-wm waybar zellij tmux; command -v notify-send || echo "notify-send: absent"' >>"$log" 2>&1 || true ;;
    u*) vm_ssh "$port" 'dpkg -l river buoy-wm waybar tmux 2>&1; ls /usr/share/wayland-sessions/' >>"$log" 2>&1 || true ;;
esac
vm_ssh "$port" 'mkdir -p ~/cand'
if [ "${#candidates[@]}" -gt 0 ]; then
    vm_scp "$port" cand/ "${candidates[@]}"
fi
vm_scp "$port" . "$script" "$CLEAN_VM_HOME/check.sh" "$CLEAN_VM_HOME/logout.sh" "$CLEAN_VM_HOME/relogin.sh"

echo "== README script ($(date -Is))" >>"$log"
start=$(date +%s)
# answer.py appends the script's own output to the same log, between
# these headings; ssh's status is the echo's, so the script's own exit
# status is read back from the README-EXIT line.
python3 -I "$CLEAN_VM_HOME/answer.py" "$log" -- \
    ssh "${VM_SSH_OPTS[@]}" -p "$port" -tt "$VM_USER@127.0.0.1" \
    "bash -x ~/$vm-$label.sh 2>&1; echo README-EXIT=\$?" || true
echo "== README script took $(($(date +%s) - start))s" >>"$log"
status="$(sed -n 's/^README-EXIT=\([0-9]*\).*/\1/p' "$log" | tail -n 1)"
echo "$vm $label: README-EXIT=${status:-missing}, log $log"
vm_ssh "$port" 'sudo systemctl reboot' || true
[ "$status" = 0 ]
