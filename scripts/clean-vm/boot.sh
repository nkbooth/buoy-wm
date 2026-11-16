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
# boot.sh <vm> <label> <ssh-port> <seed-port>: run one VM headless until it
# powers off. The disk is layer.sh's overlay for <label>; ssh is forwarded
# to 127.0.0.1:<ssh-port>; cloud-init's NoCloud seed is served over HTTP on
# <seed-port> and named through SMBIOS, so no seed ISO is needed; QMP (the
# screen and keyboard qmp.py drives) is a socket in BUOY_VM_SOCKETS. The
# screen is 1280x800, the size the GDM click coordinates assume.
set -euo pipefail
# shellcheck source=scripts/clean-vm/env.sh
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
vm=$1 label=$2 ssh_port=$3 seed_port=$4

if [ "$label" = desktop ]; then
    disk="$BUOY_VM_DIR/$vm.qcow2"
    # Overlays record the layer's state when they were made; writing the
    # layer under them corrupts every one.
    shopt -s nullglob
    overlays=("$BUOY_VM_DIR/$vm"-*.qcow2)
    shopt -u nullglob
    if [ "${#overlays[@]}" -gt 0 ]; then
        echo "$disk is frozen under ${overlays[*]}: boot an overlay (layer.sh $vm <label>) instead" >&2
        exit 1
    fi
else
    disk="$BUOY_VM_DIR/$vm-$label.qcow2"
fi
[ -f "$disk" ] || { echo "$disk missing: run layer.sh $vm $label" >&2; exit 1; }
memory="$(python3 -I "$CLEAN_VM_HOME/vms.py" memory "$vm")"
mkdir -p "$BUOY_VM_SOCKETS" "$BUOY_VM_DIR/log"

python3 -m http.server -b 127.0.0.1 -d "$BUOY_VM_DIR/seed-$vm" "$seed_port" \
    >"$BUOY_VM_DIR/log/$vm.http.log" 2>&1 &
seed_server=$!
trap 'kill "$seed_server" 2>/dev/null || true' EXIT
sleep 1
if ! kill -0 "$seed_server" 2>/dev/null; then
    echo "seed server on 127.0.0.1:$seed_port did not start (port taken?): $BUOY_VM_DIR/log/$vm.http.log" >&2
    exit 1
fi
qemu-system-x86_64 -name "$vm" -enable-kvm -cpu host -smp 4 -m "$memory" \
    -machine q35 -drive "file=$disk,if=virtio" \
    -nic "user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:$ssh_port-:22" \
    -smbios "type=1,serial=ds=nocloud;s=http://10.0.2.2:$seed_port/" \
    -device virtio-vga -device virtio-tablet-pci -display none \
    -qmp "unix:$BUOY_VM_SOCKETS/$vm.qmp,server,nowait" \
    -serial "file:$BUOY_VM_DIR/log/$vm-$label.serial.log"
