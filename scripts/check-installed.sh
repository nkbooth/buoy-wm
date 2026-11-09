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
# Runs the three installed binaries' `--version` and checks each answer.
# This is the release's last proof, before `publish` runs, that each
# package's binaries load on their target: their shared libraries
# resolve in a clean container of that release, they sit at the installed
# paths (docs/PACKAGING.md "Release model", constraints 1 and 2: one
# directory, only `buoy-wm` on PATH; `/usr/bin/buoy-wm` is the symlink a
# user runs) and they are the version the tag claims (NFR6).
#
# <version> is the cargo form (`1.0.0-rc.2`): the binaries print it
# whatever the package header says (`1.0.0~rc.2`).
#
# Each binary runs under `timeout`, so one that starts the WM instead of
# answering fails rather than hangs the job. Each must be present and
# executable, exit 0, print exactly one line `<basename> <version>` and
# nothing on stderr. Every failing binary is named, with why, in one run.
#
# Callers: release.yml (the `verify` job, one leg per package in a fresh
# container of its release) and tests/check-installed.sh with stub
# binaries under a fixture root.
#
#   check-installed.sh [--root <dir>] [--timeout <seconds>] <version>
#       Defaults: root `/`, timeout 10. On success prints the three lines
#       the binaries printed, one per binary, and nothing else.
#
# Exit codes: 0 all three answered; 1 any binary failed (each named on
# stderr, nothing on stdout); 2 usage.
set -euo pipefail

readonly BINARIES=(
    usr/bin/buoy-wm
    usr/libexec/buoy-wm/buoy-tag-picker
    usr/libexec/buoy-wm/buoy-status-bar
)

usage_text() {
    cat <<'EOF'
usage: check-installed.sh [--root <dir>] [--timeout <seconds>] <version>

Runs `--version` on <root>/usr/bin/buoy-wm and on buoy-tag-picker and
buoy-status-bar under <root>/usr/libexec/buoy-wm (default root: /), each
under `timeout <seconds>` (default 10), and requires each to exit 0
printing exactly `<name> <version>` and nothing on stderr. Prints the
three lines on success; names every failing binary on stderr otherwise.
EOF
}

usage() {
    usage_text >&2
    exit 2
}

root=/
timeout_seconds=10
positional=()
while [ $# -gt 0 ]; do
    case $1 in
        --root)
            [ $# -ge 2 ] || usage
            root=$2
            shift
            ;;
        --timeout)
            [ $# -ge 2 ] || usage
            timeout_seconds=$2
            shift
            ;;
        --help | -h)
            usage_text
            exit 0
            ;;
        --)
            shift
            positional+=("$@")
            break
            ;;
        -?*) usage ;;
        *) positional+=("$1") ;;
    esac
    shift
done
if [ "${#positional[@]}" -ne 1 ] || [ -z "${positional[0]}" ]; then
    usage
fi
if [[ ! "$timeout_seconds" =~ ^[1-9][0-9]*$ ]]; then
    usage
fi
readonly version="${positional[0]}"
root="${root%/}"
readonly root

tmp="$(mktemp -d)"
readonly tmp
trap 'rm -rf "$tmp"' EXIT

: >"$tmp/report"
: >"$tmp/lines"

# check <path>: runs one binary and appends every problem with it to the
# report, or its line to the lines on success.
check() {
    local path=$1 name expected rc=0 lines
    name="${path##*/}"
    expected="$name $version"
    if [ ! -e "$path" ]; then
        echo "$path: missing" >>"$tmp/report"
        return
    fi
    if [ ! -x "$path" ]; then
        echo "$path: not executable" >>"$tmp/report"
        return
    fi
    # `-k`: a binary that ignores TERM (a compositor's signal handler)
    # would otherwise outlive the timeout; KILL follows 2 s later and
    # timeout then exits 137 instead of 124.
    timeout -k 2 "$timeout_seconds" "$path" --version >"$tmp/out" 2>"$tmp/err" </dev/null || rc=$?
    if [ "$rc" -eq 124 ] || [ "$rc" -eq 137 ]; then
        echo "$path: did not answer within ${timeout_seconds}s (killed by timeout)" >>"$tmp/report"
        return
    fi
    local before
    before="$(wc -l <"$tmp/report")"
    if [ "$rc" -ne 0 ]; then
        echo "$path: exited $rc" >>"$tmp/report"
    fi
    if [ -s "$tmp/err" ]; then
        echo "$path: wrote to stderr: $(head -n1 "$tmp/err")" >>"$tmp/report"
    fi
    lines="$(wc -l <"$tmp/out")"
    if [ "$lines" -ne 1 ] || [ "$(cat "$tmp/out")" != "$expected" ]; then
        echo "$path: printed '$(head -c 200 "$tmp/out" | tr '\n' '|')' ($lines lines), not the one line '$expected'" >>"$tmp/report"
    fi
    if [ "$(wc -l <"$tmp/report")" -eq "$before" ]; then
        cat "$tmp/out" >>"$tmp/lines"
    fi
}

for binary in "${BINARIES[@]}"; do
    check "$root/$binary"
done

if [ -s "$tmp/report" ]; then
    cat "$tmp/report" >&2
    exit 1
fi
cat "$tmp/lines"
