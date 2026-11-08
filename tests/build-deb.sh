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
# Asserts scripts/build-deb.sh: the preflight that names every missing
# input before exiting, the unpacked tree dpkg-buildpackage is handed
# (Source0, the vendored crates inside it, debian/ copied in), the
# dpkg-buildpackage invocation, the dbgsym filter and the published name
# with the release label from os-release.
#
# The .deb is built only on a tag (release.yml), so this is the only
# place the script's contract runs on every push. A stub
# `dpkg-buildpackage` first on PATH records its argv, checks the tree it
# runs in and writes fake packages beside it the way the real one does;
# the fixture debian/ directory, os-release and archives are written at
# runtime. Nothing here needs dpkg installed.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly SCRIPT="$REPO_ROOT/scripts/build-deb.sh"

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

# The stub reads this; the script under test inherits it and never
# looks at it.
export STUB_LOG="$tmp/argv.log"

readonly FIXTURE_VERSION='1.0.0~rc.1-1'
readonly FIXTURE_UPSTREAM='1.0.0-rc.1'
readonly SOURCE0="buoy-wm-$FIXTURE_UPSTREAM.tar.gz"
readonly SOURCE1="buoy-wm-$FIXTURE_UPSTREAM-vendor.tar.gz"
readonly PUBLISHED_2604="buoy-wm_$FIXTURE_UPSTREAM-1.ubuntu26.04_amd64.deb"
readonly PUBLISHED_2610="buoy-wm_$FIXTURE_UPSTREAM-1.ubuntu26.10_amd64.deb"
readonly DEB_BYTES='fake deb payload'

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

# write_debian <dir> <first-changelog-line>: the one file the script
# reads from debian/, plus a control file so the copy is visibly a
# debian/ directory.
write_debian() {
    local dir=$1 first=$2
    mkdir -p "$dir"
    {
        printf '%s\n' "$first"
        echo
        echo '  * Fixture entry.'
        echo
        echo ' -- Fixture <fixture@example.org>  Sun, 08 Nov 2026 13:30:00 -0500'
    } >"$dir/changelog"
    echo 'Source: buoy-wm' >"$dir/control"
}

# write_os_release <path> <id> <version-id>; an empty value omits the key.
write_os_release() {
    local path=$1 id=$2 version_id=$3
    {
        echo 'NAME="Ubuntu"'
        if [ -n "$id" ]; then echo "ID=$id"; fi
        echo 'ID_LIKE=debian'
        if [ -n "$version_id" ]; then echo "VERSION_ID=\"$version_id\""; fi
        echo 'PRETTY_NAME="Fixture"'
    } >"$path"
}

# write_source0 <dir> <top>: a tag archive whose top directory is <top>.
write_source0() {
    local dir=$1 top=$2
    mkdir -p "$tmp/s0/$top"
    echo '[workspace]' >"$tmp/s0/$top/Cargo.toml"
    tar -C "$tmp/s0" -czf "$dir/$SOURCE0" "$top"
    rm -rf "$tmp/s0"
}

# write_stub <bin-dir> <exit-code>: a `dpkg-buildpackage` that logs its
# argv one per line, refuses to run unless its cwd is an unpacked tree
# with debian/changelog, vendor/ and Cargo.toml (the message must reach
# the caller's stderr), and writes into `..`, as the real one does, the
# package, a dbgsym package, a .changes and a .buildinfo for the
# changelog's version, then exits <exit-code>.
write_stub() {
    local bin=$1 code=$2
    mkdir -p "$bin"
    cat >"$bin/dpkg-buildpackage" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" >> "$STUB_LOG"
echo 'stub dpkg-buildpackage: build log line that must not reach the caller stdout'
for f in debian/changelog Cargo.toml; do
    if [ ! -f "$f" ]; then
        echo "stub dpkg-buildpackage: $f missing in $(pwd)" >&2
        exit 3
    fi
done
if [ ! -d vendor ]; then
    echo "stub dpkg-buildpackage: vendor/ missing in $(pwd)" >&2
    exit 3
fi
v=$(sed -n '1s/^buoy-wm (\([^)]*\)).*/\1/p' debian/changelog)
printf '%s\n' 'STUB_DEB_BYTES' > "../buoy-wm_${v}_amd64.deb"
printf '%s\n' 'fake dbgsym payload' > "../buoy-wm-dbgsym_${v}_amd64.ddeb"
printf '%s\n' 'fake changes' > "../buoy-wm_${v}_amd64.changes"
printf '%s\n' 'fake buildinfo' > "../buoy-wm_${v}_amd64.buildinfo"
EOF
    sed -i -e "s|STUB_DEB_BYTES|$DEB_BYTES|" "$bin/dpkg-buildpackage"
    printf 'exit %s\n' "$code" >>"$bin/dpkg-buildpackage"
    chmod 0755 "$bin/dpkg-buildpackage"
}

# A PATH holding every tool the script needs except dpkg-buildpackage,
# so the missing-tool case cannot find a real one.
make_tools_bin() {
    local dir=$1 tool
    mkdir -p "$dir"
    for tool in bash sh sed grep awk head mktemp mkdir cp find sha256sum rm dirname cat basename tar gzip ls; do
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

expect_stub_not_run() {
    if [ -s "$STUB_LOG" ]; then
        fail "$1" "dpkg-buildpackage must not run"
    fi
}

write_debian "$tmp/debian" "buoy-wm ($FIXTURE_VERSION) unstable; urgency=medium"
write_debian "$tmp/bad-debian" 'not a changelog line'
write_os_release "$tmp/os-2604" ubuntu 26.04
write_os_release "$tmp/os-2610" ubuntu 26.10
write_os_release "$tmp/os-no-version" ubuntu ''
mkdir -p "$tmp/sources" "$tmp/empty-sources" "$tmp/wrong-top" "$tmp/with-debian"
write_source0 "$tmp/sources" "buoy-wm-$FIXTURE_UPSTREAM"
mkdir -p "$tmp/v/vendor/libc-0.2.1"
echo 'vendored crate' >"$tmp/v/vendor/libc-0.2.1/Cargo.toml"
tar -C "$tmp/v" -czf "$tmp/sources/$SOURCE1" vendor
cp "$tmp/sources/$SOURCE1" "$tmp/wrong-top/"
cp "$tmp/sources/$SOURCE1" "$tmp/with-debian/"
write_source0 "$tmp/wrong-top" "buoy-wm-main"
mkdir -p "$tmp/s0/buoy-wm-$FIXTURE_UPSTREAM/debian"
echo '[workspace]' >"$tmp/s0/buoy-wm-$FIXTURE_UPSTREAM/Cargo.toml"
echo 'a distribution packager owns this' >"$tmp/s0/buoy-wm-$FIXTURE_UPSTREAM/debian/control"
tar -C "$tmp/s0" -czf "$tmp/with-debian/$SOURCE0" "buoy-wm-$FIXTURE_UPSTREAM"
rm -rf "$tmp/s0"
write_stub "$tmp/bin" 0
make_tools_bin "$tmp/tools-bin"

common=(--debian "$tmp/debian" --os-release "$tmp/os-2604")

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
expect_empty_stdout '(one argument)'
run_script --bogus "$tmp/sources" "$tmp/out-dir"
expect_rc --bogus 2
expect_empty_stdout --bogus

# No dpkg-buildpackage anywhere on PATH: named, exit 1, and no bash
# error text from a tool the preflight itself could not find.
set +e
PATH="$tmp/tools-bin" "$SCRIPT" "${common[@]}" "$tmp/sources" "$tmp/out-no-dpkg" >"$tmp/out" 2>"$tmp/err"
rc=$?
set -e
expect_rc 'no dpkg-buildpackage' 1
expect_stderr_mentions 'no dpkg-buildpackage' dpkg-buildpackage dpkg-dev
expect_stderr_lines 'no dpkg-buildpackage' 1
if grep -q 'command not found' "$tmp/err"; then
    fail 'no dpkg-buildpackage' "the preflight needs a tool the tools-bin does not hold:"
    cat "$tmp/err" >&2
fi
if [ -e "$tmp/out-no-dpkg" ]; then
    fail 'no dpkg-buildpackage' "out-dir must not be created when the preflight fails"
fi

# Every missing source is one line, and both are reported at once.
export PATH="$tmp/bin:$PATH"
run_script "${common[@]}" "$tmp/empty-sources" "$tmp/out-empty"
expect_rc 'empty sources' 1
expect_stderr_mentions 'empty sources' "$tmp/empty-sources/$SOURCE0" "$tmp/empty-sources/$SOURCE1"
expect_stderr_lines 'empty sources' 2
expect_empty_stdout 'empty sources'
expect_stub_not_run 'empty sources'
if [ -e "$tmp/out-empty" ]; then
    fail 'empty sources' "out-dir must not be created when the preflight fails"
fi

# A changelog whose first line names no version, and an os-release with
# no VERSION_ID, are each named; both at once, beside nothing else
# (the sources cannot be named without the version).
run_script --debian "$tmp/bad-debian" --os-release "$tmp/os-no-version" "$tmp/sources" "$tmp/out-bad"
expect_rc 'bad changelog and os-release' 1
expect_stderr_mentions 'bad changelog and os-release' "$tmp/bad-debian/changelog" "$tmp/os-no-version" VERSION_ID
expect_stderr_lines 'bad changelog and os-release' 2
expect_empty_stdout 'bad changelog and os-release'
expect_stub_not_run 'bad changelog and os-release'

run_script --debian "$tmp/no-such-dir" --os-release "$tmp/os-2604" "$tmp/sources" "$tmp/out-nodeb"
expect_rc 'missing debian dir' 1
expect_stderr_mentions 'missing debian dir' "$tmp/no-such-dir/changelog"
expect_stub_not_run 'missing debian dir'

run_script --debian "$tmp/debian" --os-release "$tmp/no-such-os-release" "$tmp/sources" "$tmp/out-noos"
expect_rc 'missing os-release' 1
expect_stderr_mentions 'missing os-release' "$tmp/no-such-os-release"
expect_stderr_lines 'missing os-release' 1
expect_stub_not_run 'missing os-release'

# The build: the tree assembled, `-b -us -uc` and nothing else, the one
# package published under its release-labelled name with its checksum,
# the dbgsym package left behind, and nothing but the path on stdout.
rm -f "$STUB_LOG"
run_script "${common[@]}" "$tmp/sources" "$tmp/out-dir"
expect_rc build 0
if [ "$(cat "$tmp/out")" != "$tmp/out-dir/$PUBLISHED_2604" ]; then
    fail build "stdout must be exactly the published path, got:"
    cat "$tmp/out" "$tmp/err" >&2
fi
if [ "$(wc -l <"$tmp/out")" -ne 1 ]; then
    fail build "stdout must be one line"
fi
if [ ! -f "$tmp/out-dir/$PUBLISHED_2604" ]; then
    fail build "$PUBLISHED_2604 not in out-dir:"
    ls -A "$tmp/out-dir" >&2 2>/dev/null || true
elif [ "$(cat "$tmp/out-dir/$PUBLISHED_2604")" != "$DEB_BYTES" ]; then
    fail build "published .deb does not hold the built package's bytes"
fi
if [ ! -f "$tmp/out-dir/$PUBLISHED_2604.sha256" ]; then
    fail build "$PUBLISHED_2604.sha256 not in out-dir"
elif ! (cd "$tmp/out-dir" && sha256sum -c --quiet -- *.sha256) >"$tmp/sha-out" 2>&1; then
    fail build "sha256sum -c fails in out-dir:"
    cat "$tmp/sha-out" >&2
fi
if ! grep -q "^[0-9a-f]\{64\}  $PUBLISHED_2604\$" "$tmp/out-dir/$PUBLISHED_2604.sha256" 2>/dev/null; then
    fail build "the checksum line must carry the bare basename"
fi
published_count="$(find "$tmp/out-dir" -type f | grep -c . || true)"
if [ "$published_count" -ne 2 ]; then
    fail build "out-dir must hold the .deb and its .sha256 only (no dbgsym, .changes or .buildinfo):"
    ls -A "$tmp/out-dir" >&2
fi
if [ "$(cat "$STUB_LOG" 2>/dev/null)" != $'-b\n-us\n-uc' ]; then
    fail build "dpkg-buildpackage argv must be exactly '-b -us -uc', got:"
    cat "$STUB_LOG" >&2 2>/dev/null || true
fi

# The release label is the build container's, not a constant.
run_script --debian "$tmp/debian" --os-release "$tmp/os-2610" "$tmp/sources" "$tmp/out-2610"
expect_rc 'ubuntu 26.10' 0
if [ "$(cat "$tmp/out")" != "$tmp/out-2610/$PUBLISHED_2610" ]; then
    fail 'ubuntu 26.10' "stdout must be the .ubuntu26.10_ published path, got:"
    cat "$tmp/out" "$tmp/err" >&2
fi

# A Source0 whose top directory is not buoy-wm-<upstream>/ is another
# tree, or another version's, and is refused before dpkg runs.
rm -f "$STUB_LOG"
run_script "${common[@]}" "$tmp/wrong-top" "$tmp/out-wrong-top"
expect_rc 'wrong top directory' 1
expect_stderr_mentions 'wrong top directory' "buoy-wm-$FIXTURE_UPSTREAM/"
expect_empty_stdout 'wrong top directory'
expect_stub_not_run 'wrong top directory'

# A Source0 that already has a debian/ is refused rather than overlaid.
run_script "${common[@]}" "$tmp/with-debian" "$tmp/out-with-debian"
expect_rc 'Source0 with debian/' 1
expect_stderr_mentions 'Source0 with debian/' 'debian/'
expect_empty_stdout 'Source0 with debian/'
expect_stub_not_run 'Source0 with debian/'

# A failing dpkg-buildpackage: its status is the script's, and nothing
# is published.
write_stub "$tmp/bin-fail" 7
set +e
PATH="$tmp/bin-fail:$PATH" "$SCRIPT" "${common[@]}" "$tmp/sources" "$tmp/out-fail" >"$tmp/out" 2>"$tmp/err"
rc=$?
set -e
expect_rc 'dpkg-buildpackage exits 7' 7
expect_empty_stdout 'dpkg-buildpackage exits 7'
if compgen -G "$tmp/out-fail/*.deb" >/dev/null; then
    fail 'dpkg-buildpackage exits 7' "out-dir must hold no .deb after a failed build"
fi

# A dpkg-buildpackage that exits 0 without writing the package: the
# script says so and publishes nothing.
write_stub "$tmp/bin-none" 0
sed -i '/_amd64\.deb"$/d' "$tmp/bin-none/dpkg-buildpackage"
set +e
PATH="$tmp/bin-none:$PATH" "$SCRIPT" "${common[@]}" "$tmp/sources" "$tmp/out-none" >"$tmp/out" 2>"$tmp/err"
rc=$?
set -e
expect_rc 'dpkg-buildpackage builds nothing' 1
expect_stderr_mentions 'dpkg-buildpackage builds nothing' 'expected exactly one'
expect_empty_stdout 'dpkg-buildpackage builds nothing'
if [ -e "$tmp/out-none" ]; then
    fail 'dpkg-buildpackage builds nothing' "out-dir must not be created when no package was built"
fi

exit "$status"
