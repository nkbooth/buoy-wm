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
# The clean-VM harness (scripts/clean-vm/) builds each tier-1 VM's install
# script from README.md's own blocks, found by heading. It runs by hand,
# once before a final tag, so a README edit that renames a heading or
# reshapes a block would break it silently until the day it is needed:
# Story 5.7's review changed the zellij block and the harness's 26.04
# substitution stopped matching. tests/clean_vm_check.py, given the
# Supported platforms table as tests/lib.sh reads it, builds every tier-1
# script in both modes and holds it, and scripts/clean-vm/vms.py, to the
# README (its header says how).
set -euo pipefail

REPO_ROOT="$(CDPATH='' cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
# shellcheck source=tests/lib.sh
source "$REPO_ROOT/tests/lib.sh"

tmp="$(mktemp -d)"
readonly tmp
trap 'rm -rf "$tmp"' EXIT

platform_rows "$REPO_ROOT/README.md" >"$tmp/rows"
python3 -I "$REPO_ROOT/tests/clean_vm_check.py" "$REPO_ROOT/README.md" \
    "$REPO_ROOT/scripts/clean-vm/readme_blocks.py" "$tmp/rows"
