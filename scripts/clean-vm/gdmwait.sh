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
# gdmwait.sh <vm> <ssh-port> <shot.png>: wait until GDM's greeter runs,
# wake the screen (it blanks after about five idle minutes) and take a
# screenshot to read before the next click.
set -uo pipefail
# shellcheck source=scripts/clean-vm/env.sh
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
vm=$1 port=$2 shot=$3

start=$(date +%s)
# The bracket keeps pgrep -f from matching the remote shell running it.
until vm_ssh "$port" 'pgrep -f "[g]nome-shell --mode=gdm" >/dev/null' 2>/dev/null; do
    sleep 5
    if (($(date +%s) - start > 400)); then
        echo "$vm: no greeter after 400s" >&2
        exit 1
    fi
done
sleep 12
qmp "$vm" keys shift
sleep 3
qmp "$vm" shot "$shot"
echo "$vm: greeter after $(($(date +%s) - start))s, shot $shot"
