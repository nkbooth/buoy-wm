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
# Asserts scripts/build-rpm.sh: the preflight that names every missing
# input before exiting, the rpmbuild invocation (a full -bb, nothing that
# would make dnf refuse the result), the debuginfo filter and the
# published name with the tag's hyphen in place of the spec's tilde.
#
# The RPM is built only on a tag (release.yml), so this is the only place
# the script's contract runs on every push. A stub `rpmbuild` first on
# PATH records its argv and writes fake packages under the _topdir the
# script hands it; a fixture spec carries the two version lines the
# script reads. Nothing here needs rpm installed.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly SCRIPT="$REPO_ROOT/scripts/build-rpm.sh"

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

# The stub reads these; the script under test inherits them and never
# looks at them.
export STUB_LOG="$tmp/argv.log"

readonly FIXTURE_VERSION='1.0.0~rc.1'
readonly FIXTURE_UPSTREAM='1.0.0-rc.1'
readonly SOURCE0="buoy-wm-$FIXTURE_UPSTREAM.tar.gz"
readonly SOURCE1="buoy-wm-$FIXTURE_UPSTREAM-vendor.tar.gz"
readonly BUILT_RPM="buoy-wm-$FIXTURE_VERSION-1.fc44.x86_64.rpm"
readonly PUBLISHED_RPM="buoy-wm-$FIXTURE_UPSTREAM-1.fc44.x86_64.rpm"
readonly RPM_BYTES='fake rpm payload'

status=0
rc=0

fail() {
    echo "$1: $2" >&2
    status=1
}

# Keeps stdout, stderr and the exit code apart: a preflight failure (1)
# and a usage error (2) must be told apart, which `|| true` would lose.
run_script() {
    set +e
    "$SCRIPT" "$@" >"$tmp/out" 2>"$tmp/err"
    rc=$?
    set -e
}

# write_spec <path> <version> <upstream>: the two lines the script reads
# plus enough around them that a human reading the fixture sees a spec.
# Written at runtime, so it carries no licence header.
write_spec() {
    local path=$1 version=$2 upstream=$3
    {
        echo "%global upstream_version $upstream"
        echo
        echo 'Name:           buoy-wm'
        echo "Version:        $version"
        echo 'Release:        1%{?dist}'
        echo 'Summary:        Fixture for tests/build-rpm.sh'
        echo
        echo '%description'
        echo 'Fixture spec; only the version lines matter.'
    } >"$path"
}

# write_stub <bin-dir> <exit-code>: an `rpmbuild` that logs its argv one
# per line, finds _topdir in its own --define argument, refuses to run
# unless SOURCES/ holds both archives (the message must reach the
# caller's stderr), writes the main package and a debuginfo package the
# way a real build on Fedora does, and exits <exit-code>.
write_stub() {
    local bin=$1 code=$2
    mkdir -p "$bin"
    cat >"$bin/rpmbuild" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" >> "$STUB_LOG"
echo 'stub rpmbuild: build log line that must not reach the caller stdout'
topdir=
prev=
for arg in "$@"; do
    if [ "$prev" = --define ]; then
        case $arg in
            '_topdir '*) topdir=${arg#_topdir } ;;
        esac
    fi
    prev=$arg
done
if [ -z "$topdir" ]; then
    echo 'stub rpmbuild: no --define "_topdir <dir>" in argv' >&2
    exit 3
fi
for f in STUB_SOURCE0 STUB_SOURCE1; do
    if [ ! -f "$topdir/SOURCES/$f" ]; then
        echo "stub rpmbuild: SOURCES/$f missing under $topdir" >&2
        exit 3
    fi
done
mkdir -p "$topdir/RPMS/x86_64"
printf '%s\n' 'STUB_RPM_BYTES' > "$topdir/RPMS/x86_64/STUB_BUILT_RPM"
printf '%s\n' 'fake debuginfo payload' > "$topdir/RPMS/x86_64/buoy-wm-debuginfo-STUB_VERSION-1.fc44.x86_64.rpm"
EOF
    # `~` in the version is literal to sed; the names hold no `|`.
    sed -i \
        -e "s|STUB_SOURCE0|$SOURCE0|" -e "s|STUB_SOURCE1|$SOURCE1|" \
        -e "s|STUB_RPM_BYTES|$RPM_BYTES|" -e "s|STUB_BUILT_RPM|$BUILT_RPM|" \
        -e "s|STUB_VERSION|$FIXTURE_VERSION|" "$bin/rpmbuild"
    printf 'exit %s\n' "$code" >>"$bin/rpmbuild"
    chmod 0755 "$bin/rpmbuild"
}

# A PATH holding every tool the script needs except rpmbuild, so the
# missing-tool case cannot find a real one (Ubuntu's `rpm` package, which
# CI installs for rpmspec, ships rpmbuild).
make_tools_bin() {
    local dir=$1 tool
    mkdir -p "$dir"
    for tool in bash sh sed grep awk head mktemp mkdir cp find sha256sum rm dirname cat basename; do
        ln -s "$(command -v "$tool")" "$dir/$tool"
    done
}

expect_rc() {
    local label=$1 expected=$2
    if [ "$rc" -ne "$expected" ]; then
        fail "$label" "expected exit $expected, got $rc"
        cat "$tmp/out" "$tmp/err" >&2
    fi
}

expect_stderr_mentions() {
    local label=$1
    shift
    local want
    for want in "$@"; do
        if ! grep -Fq -- "$want" "$tmp/err"; then
            fail "$label" "stderr does not mention '$want':"
            cat "$tmp/err" >&2
        fi
    done
}

expect_stderr_lines() {
    local label=$1 n=$2 lines
    lines="$(wc -l <"$tmp/err")"
    if [ "$lines" -ne "$n" ]; then
        fail "$label" "expected $n stderr line(s), got $lines:"
        cat "$tmp/err" >&2
    fi
}

expect_empty_stdout() {
    local label=$1
    if [ -s "$tmp/out" ]; then
        fail "$label" "stdout must be empty, got:"
        cat "$tmp/out" >&2
    fi
}

write_spec "$tmp/fixture.spec" "$FIXTURE_VERSION" "$FIXTURE_UPSTREAM"
mkdir -p "$tmp/sources" "$tmp/empty-sources" "$tmp/only-source1"
echo 'tag archive' >"$tmp/sources/$SOURCE0"
echo 'vendored crates' >"$tmp/sources/$SOURCE1"
cp "$tmp/sources/$SOURCE1" "$tmp/only-source1/"
write_stub "$tmp/bin" 0
make_tools_bin "$tmp/tools-bin"

# Usage: asked for, on stdout, exit 0; mistaken, on stderr, exit 2.
run_script --help
expect_rc --help 0
if ! grep -q '^usage:' "$tmp/out"; then
    fail --help "expected usage on stdout, got:"
    cat "$tmp/out" "$tmp/err" >&2
fi
run_script
expect_rc '(no arguments)' 2
expect_empty_stdout '(no arguments)'
run_script "$tmp/sources"
expect_rc '(one argument)' 2
run_script --bogus "$tmp/sources" "$tmp/out-dir"
expect_rc --bogus 2
expect_empty_stdout --bogus

# No rpmbuild anywhere on PATH: named, exit 1, and no bash error text
# from a tool the preflight itself could not find.
set +e
PATH="$tmp/tools-bin" "$SCRIPT" --spec "$tmp/fixture.spec" "$tmp/sources" "$tmp/out-no-rpmbuild" >"$tmp/out" 2>"$tmp/err"
rc=$?
set -e
expect_rc 'no rpmbuild' 1
expect_stderr_mentions 'no rpmbuild' rpmbuild
expect_stderr_lines 'no rpmbuild' 1
if grep -q 'command not found' "$tmp/err"; then
    fail 'no rpmbuild' "the preflight needs a tool the tools-bin does not hold:"
    cat "$tmp/err" >&2
fi
if [ -e "$tmp/out-no-rpmbuild" ]; then
    fail 'no rpmbuild' "out-dir must not be created when the preflight fails"
fi

# Every missing source is one line, and all of them are reported at once.
export PATH="$tmp/bin:$PATH"
run_script --spec "$tmp/fixture.spec" "$tmp/empty-sources" "$tmp/out-empty"
expect_rc 'empty sources' 1
expect_stderr_mentions 'empty sources' "$tmp/empty-sources/$SOURCE0" "$tmp/empty-sources/$SOURCE1"
expect_stderr_lines 'empty sources' 2
expect_empty_stdout 'empty sources'
if [ -s "$STUB_LOG" ]; then
    fail 'empty sources' "rpmbuild must not run when a source is missing"
fi

run_script --spec "$tmp/fixture.spec" "$tmp/only-source1" "$tmp/out-only1"
expect_rc 'only Source1' 1
expect_stderr_mentions 'only Source1' "$tmp/only-source1/$SOURCE0"
expect_stderr_lines 'only Source1' 1

# A spec the script cannot read a version from names the spec.
write_spec "$tmp/no-version.spec" "$FIXTURE_VERSION" "$FIXTURE_UPSTREAM"
sed -i -e '/^Version:/d' -e '/^%global upstream_version/d' "$tmp/no-version.spec"
run_script --spec "$tmp/no-version.spec" "$tmp/sources" "$tmp/out-no-version"
expect_rc 'spec without versions' 1
expect_stderr_mentions 'spec without versions' "$tmp/no-version.spec"
expect_empty_stdout 'spec without versions'

# The build: both sources staged, a full -bb, the one main package
# published under the hyphen name with its checksum, the debuginfo left
# behind, and nothing but the published path on stdout.
rm -f "$STUB_LOG"
run_script --spec "$tmp/fixture.spec" "$tmp/sources" "$tmp/out-dir"
expect_rc build 0
if [ "$(cat "$tmp/out")" != "$tmp/out-dir/$PUBLISHED_RPM" ]; then
    fail build "stdout must be exactly the published path, got:"
    cat "$tmp/out" >&2
fi
if [ "$(wc -l <"$tmp/out")" -ne 1 ]; then
    fail build "stdout must be one line"
fi
if [ ! -f "$tmp/out-dir/$PUBLISHED_RPM" ]; then
    fail build "$PUBLISHED_RPM not in out-dir:"
    ls -A "$tmp/out-dir" >&2 2>/dev/null || true
elif [ "$(cat "$tmp/out-dir/$PUBLISHED_RPM")" != "$RPM_BYTES" ]; then
    fail build "published RPM does not hold the built package's bytes"
fi
if [ ! -f "$tmp/out-dir/$PUBLISHED_RPM.sha256" ]; then
    fail build "$PUBLISHED_RPM.sha256 not in out-dir"
elif ! (cd "$tmp/out-dir" && sha256sum -c --quiet -- *.sha256) >"$tmp/sha-out" 2>&1; then
    fail build "sha256sum -c fails in out-dir:"
    cat "$tmp/sha-out" >&2
fi
if ! grep -q "^[0-9a-f]\{64\}  $PUBLISHED_RPM\$" "$tmp/out-dir/$PUBLISHED_RPM.sha256"; then
    fail build "the checksum line must carry the bare basename, got:"
    cat "$tmp/out-dir/$PUBLISHED_RPM.sha256" >&2
fi
if compgen -G "$tmp/out-dir/*debuginfo*" >/dev/null; then
    fail build "debuginfo package must not be published:"
    ls "$tmp/out-dir" >&2
fi
if [ ! -s "$STUB_LOG" ]; then
    fail build "the stub rpmbuild was never called"
else
    for want in -bb "$tmp/fixture.spec" --define; do
        if ! grep -Fqx -- "$want" "$STUB_LOG"; then
            fail build "rpmbuild argv lacks '$want':"
            cat "$STUB_LOG" >&2
        fi
    done
    if ! grep -q '^_topdir ' "$STUB_LOG"; then
        fail build "rpmbuild argv has no '_topdir <dir>' define:"
        cat "$STUB_LOG" >&2
    fi
    # Each of these yields a package dnf refuses or a build that skipped
    # its tests (4.5 Dev Agent Record; AC 2, AC 3).
    for forbidden in --nodeps --nocheck --short-circuit debug_package; do
        if grep -Fq -- "$forbidden" "$STUB_LOG"; then
            fail build "rpmbuild argv must not contain '$forbidden':"
            cat "$STUB_LOG" >&2
        fi
    done
fi
# The _topdir is scratch: gone once the script exits.
topdir="$(sed -n 's/^_topdir //p' "$STUB_LOG" | head -n1)"
if [ -n "$topdir" ] && [ -e "$topdir" ]; then
    fail build "_topdir $topdir was not removed"
fi

# A failing rpmbuild: its status is the script's, and nothing is published.
write_stub "$tmp/bin-fail" 7
set +e
PATH="$tmp/bin-fail:$PATH" "$SCRIPT" --spec "$tmp/fixture.spec" "$tmp/sources" "$tmp/out-fail" >"$tmp/out" 2>"$tmp/err"
rc=$?
set -e
expect_rc 'rpmbuild exits 7' 7
expect_empty_stdout 'rpmbuild exits 7'
if compgen -G "$tmp/out-fail/*.rpm" >/dev/null; then
    fail 'rpmbuild exits 7' "out-dir must hold no .rpm after a failed build:"
    ls "$tmp/out-fail" >&2
fi

exit "$status"
