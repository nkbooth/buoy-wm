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
# Sourced by every host-side script of the clean-VM harness (README.md in
# this directory). Everything under BUOY_VM_DIR (images, desktop layers,
# overlays, logs, screenshots, the ssh key) is rebuilt by these scripts, so
# losing it costs time, not the recipe. QMP sockets live apart because a
# unix socket path is capped at 108 bytes.

# Absolute, so podman reads it as a path and not a named volume.
BUOY_VM_DIR="$(realpath -m -- "${BUOY_VM_DIR:-/var/tmp/buoy-vm}")"
BUOY_VM_SOCKETS="${BUOY_VM_SOCKETS:-${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/buoy-vm}"
# CDPATH='' so a CDPATH match cannot print its path into the capture.
CLEAN_VM_HOME="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH='' cd -- "$CLEAN_VM_HOME/../.." && pwd)"
# The throwaway account cloud-init creates in each VM. GDM's password
# field is typed over QMP, so the login cannot use the ssh key.
VM_USER=tester
VM_PASSWORD=buoy-test
# shellcheck disable=SC2034 # read by the scripts that source this file
readonly BUOY_VM_DIR BUOY_VM_SOCKETS CLEAN_VM_HOME REPO_ROOT VM_USER VM_PASSWORD
# Every VM is new, so its host key is never worth remembering.
VM_SSH_OPTS=(-i "$BUOY_VM_DIR/id_vm" -o StrictHostKeyChecking=no
    -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=5)
readonly VM_SSH_OPTS

# vm_ssh <port> [command...]: run on the VM as the test user.
vm_ssh() {
    local port=$1
    shift
    ssh "${VM_SSH_OPTS[@]}" -p "$port" "$VM_USER@127.0.0.1" "$@"
}

# vm_scp <port> <remote-dir> <file>...: copy files into the test user's
# home on the VM.
vm_scp() {
    local port=$1 dest=$2
    shift 2
    scp -q "${VM_SSH_OPTS[@]}" -P "$port" "$@" "$VM_USER@127.0.0.1:$dest"
}

# qmp <vm> <op> [args...]: drive the VM's screen and keyboard (qmp.py).
qmp() {
    local vm=$1
    shift
    python3 -I "$CLEAN_VM_HOME/qmp.py" "$BUOY_VM_SOCKETS/$vm.qmp" "$@"
}

# wait_for_ssh <port> <seconds>: poll until the VM accepts ssh.
wait_for_ssh() {
    local port=$1 limit=$2 start
    start=$(date +%s)
    until vm_ssh "$port" true 2>/dev/null; do
        sleep 5
        if (($(date +%s) - start > limit)); then
            echo "no ssh on port $port after ${limit}s" >&2
            return 1
        fi
    done
}
