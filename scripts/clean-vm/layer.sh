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
# layer.sh <vm> <label>: a copy-on-write overlay for one run.
#   label "desktop": <vm>.qcow2 on the VM's cloud image (30 GiB), the layer
#     desktop.sh installs the desktop into and that is never written again;
#   any other label: <vm>-<label>.qcow2 on <vm>.qcow2, so every run starts
#     from the same clean desktop.
# Refuses to replace an existing overlay.
set -euo pipefail
# shellcheck source=scripts/clean-vm/env.sh
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
vm=$1 label=$2

if [ "$label" = desktop ]; then
    image="$(python3 -I "$CLEAN_VM_HOME/vms.py" image "$vm")"
    backing="$BUOY_VM_DIR/img/$image" overlay="$BUOY_VM_DIR/$vm.qcow2" size=30G
else
    backing="$BUOY_VM_DIR/$vm.qcow2" overlay="$BUOY_VM_DIR/$vm-$label.qcow2" size=
fi
if [ -e "$overlay" ]; then
    echo "$overlay exists; remove it first to start this run over" >&2
    exit 1
fi
qemu-img create -q -f qcow2 -F qcow2 -b "$backing" "$overlay" ${size:+"$size"}
echo "$overlay on $backing"
