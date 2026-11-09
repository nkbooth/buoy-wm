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
# Asserts scripts/check-installed.sh: its usage contract, that it passes
# three binaries that each print `<name> <version>` and nothing else,
# and that it names every binary that does not, for each way a binary
# can fail. It runs only in release.yml's `verify` job, on a tag, so this
# is the only place its contract runs on every push.
#
# The installed paths are derived from the spec's %files, the one owner
# of the layout: a binary's %{_bindir} line if it has one (the path a
# user runs), else its %{_libexecdir}/%{name} line, with the macros
# mapped as tests/deb-package.sh maps them. So a spec that moves a
# binary moves the fixture, and a script still running the old path
# fails here. The fixture binaries are stubs written at runtime.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly SCRIPT="$REPO_ROOT/scripts/check-installed.sh"
readonly SPEC="$REPO_ROOT/packaging/rpm/buoy-wm.spec"

if [ ! -f "$SCRIPT" ]; then
    echo "$SCRIPT: missing" >&2
    exit 1
fi
if [ ! -x "$SCRIPT" ]; then
    echo "$SCRIPT: not executable" >&2
    exit 1
fi

tmp="$(mktemp -d)"
readonly tmp
trap 'rm -rf "$tmp"' EXIT

readonly VERSION='1.2.3-rc.4'
readonly BINARIES=(buoy-wm buoy-tag-picker buoy-status-bar)

status=0
rc=0

fail() {
    echo "$1: $2" >&2
    status=1
}

# spec_path <path>: a %files path with its macros mapped to the installed
# layout, relative to the root; an unmapped macro is reported.
spec_path() {
    local p=$1
    p="${p//'%{name}'/buoy-wm}"
    p="${p//'%{_libexecdir}'//usr/libexec}"
    p="${p//'%{_bindir}'//usr/bin}"
    case $p in
        *%*) fail "packaging/rpm/buoy-wm.spec" "%files path '$1' uses a macro this test does not map" ;;
    esac
    printf '%s' "${p#/}"
}

# The %files section, from its header to the next section.
awk '/^%files$/ { on = 1; next } /^%[a-z]/ && !/^%(dir|doc|license|config|attr)/ { on = 0 } on' "$SPEC" >"$tmp/files"
declare -A installed=()
for name in "${BINARIES[@]}"; do
    line="$(grep -m1 -xE "%\{_bindir\}/$name" "$tmp/files" ||
        grep -m1 -xE "%\{_libexecdir\}/%\{name\}/$name" "$tmp/files" || true)"
    if [ -z "$line" ]; then
        fail "packaging/rpm/buoy-wm.spec" "%files has no %{_bindir} or %{_libexecdir}/%{name} line for $name"
        continue
    fi
    installed[$name]="$(spec_path "$line")"
done
if [ "$status" -ne 0 ]; then
    exit "$status"
fi

# Keeps stdout, stderr and the exit code apart: a failed check (1) and a
# usage error (2) must be told apart.
run_script() {
    set +e
    "$SCRIPT" "$@" >"$tmp/out" 2>"$tmp/err"
    rc=$?
    set -e
}

# write_stub <path> <body>: an executable sh script.
write_stub() {
    mkdir -p "$(dirname "$1")"
    printf '#!/bin/sh\n%s\n' "$2" >"$1"
    chmod 0755 "$1"
}

# make_root <dir>: the three binaries, each answering correctly.
make_root() {
    local name
    rm -rf "$1"
    for name in "${BINARIES[@]}"; do
        write_stub "$1/${installed[$name]}" "echo '$name $VERSION'"
    done
}

expected_out() {
    local name
    for name in "${BINARIES[@]}"; do
        echo "$name $VERSION"
    done
}

# --help: the usage on stdout, exit 0.
run_script --help
if [ "$rc" -ne 0 ] || [ ! -s "$tmp/out" ]; then
    fail "--help" "expected usage on stdout and exit 0, got exit $rc"
fi

# Usage errors: exit 2 with nothing on stdout.
make_root "$tmp/root"
for args in '' "--root $tmp/root $VERSION extra" "--bogus $VERSION" "--timeout x $VERSION" "--root"; do
    # shellcheck disable=SC2086 # word splitting is the point
    run_script $args
    if [ "$rc" -ne 2 ] || [ -s "$tmp/out" ]; then
        fail "usage '$args'" "expected exit 2 and empty stdout, got exit $rc and '$(cat "$tmp/out")'"
    fi
done

# All three good: exit 0 and the three lines, nothing else.
run_script --root "$tmp/root" "$VERSION"
expected_out >"$tmp/expected"
if [ "$rc" -ne 0 ]; then
    fail "all good" "expected exit 0, got $rc: $(cat "$tmp/err")"
fi
if ! cmp -s "$tmp/out" "$tmp/expected"; then
    fail "all good" "stdout is not the three '<name> $VERSION' lines: $(cat "$tmp/out")"
fi
if [ -s "$tmp/err" ]; then
    fail "all good" "stderr is not empty: $(cat "$tmp/err")"
fi

# break_one <case> <name>: a fresh root with one binary broken. The
# hanging one answers correctly after 30 s, so only the timeout fails it.
break_one() {
    local path="$tmp/root/${installed[$2]}"
    make_root "$tmp/root"
    case $1 in
        wrong-version) write_stub "$path" "echo '$2 0.0.1'" ;;
        exit-1) write_stub "$path" "echo '$2 $VERSION'; exit 1" ;;
        stderr) write_stub "$path" "echo '$2 $VERSION'; echo 'a warning' >&2" ;;
        two-lines) write_stub "$path" "echo '$2 $VERSION'; echo 'and more'" ;;
        missing) rm -f "$path" ;;
        not-executable) chmod 0644 "$path" ;;
        hanging) write_stub "$path" "sleep 30; echo '$2 $VERSION'" ;;
    esac
}

# names <name>: whether stderr names that binary's installed path.
names() {
    grep -qF -- "/${installed[$1]}:" "$tmp/err"
}

for case in wrong-version exit-1 stderr two-lines missing not-executable hanging; do
    for name in "${BINARIES[@]}"; do
        break_one "$case" "$name"
        run_script --root "$tmp/root" --timeout 1 "$VERSION"
        if [ "$rc" -ne 1 ]; then
            fail "$case $name" "expected exit 1, got $rc"
        fi
        if ! names "$name"; then
            fail "$case $name" "stderr does not name /${installed[$name]}: $(cat "$tmp/err")"
        fi
        for other in "${BINARIES[@]}"; do
            if [ "$other" != "$name" ] && names "$other"; then
                fail "$case $name" "stderr also names the good /${installed[$other]}: $(cat "$tmp/err")"
            fi
        done
    done
done

# Two broken binaries are both named in one run.
make_root "$tmp/root"
rm -f "$tmp/root/${installed[buoy-wm]}"
write_stub "$tmp/root/${installed[buoy-status-bar]}" "echo 'buoy-status-bar 0.0.1'"
run_script --root "$tmp/root" "$VERSION"
if [ "$rc" -ne 1 ] || ! names buoy-wm || ! names buoy-status-bar; then
    fail "two broken" "expected exit 1 naming both, got exit $rc: $(cat "$tmp/err")"
fi

exit "$status"
