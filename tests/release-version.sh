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
# Asserts scripts/release-version.sh: the tag grammar, the mapping of a
# tag to the Cargo, RPM, Debian and formula version forms (ADR-012), and
# the --check mode that reports every manifest disagreeing with a tag.
#
# The mapping used to be a run: block in release.yml, where its accept
# and reject cases could only be exercised by pushing tags. ADR-012
# promises the mapping is one-to-one and testable; this is the test.
# Fixture trees are built under a temp dir so --check is exercised
# against every disagreement it must report, not only the real tree.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly SCRIPT="$REPO_ROOT/scripts/release-version.sh"

if [ ! -f "$SCRIPT" ]; then
    echo "$SCRIPT: missing" >&2
    exit 1
fi
if [ ! -x "$SCRIPT" ]; then
    echo "$SCRIPT: not executable" >&2
    exit 1
fi

# CI sets this, which switches the script's mismatch lines from stderr
# to stdout annotations; every case below but one asserts the plain
# form, and that one sets the variable itself.
unset GITHUB_ACTIONS

tmp="$(mktemp -d)"
readonly tmp
trap 'rm -rf "$tmp"' EXIT

status=0
rc=0

# Keeps stdout, stderr and the exit code apart: a rejected tag (1) and a
# usage error (2) must be told apart, which `|| true` or `if` would lose.
run_script() {
    set +e
    "$SCRIPT" "$@" >"$tmp/out" 2>"$tmp/err"
    rc=$?
    set -e
}

expect_map() {
    local tag=$1 cargo=$2 rpm=$3 deb=$4 formula=$5 prerelease=$6
    printf '%s\n' "tag=$tag" "cargo=$cargo" "rpm=$rpm" "deb=$deb" \
        "formula=$formula" "prerelease=$prerelease" >"$tmp/expected"
    run_script "$tag"
    if [ "$rc" -ne 0 ]; then
        echo "$tag: expected exit 0, got $rc" >&2
        cat "$tmp/err" >&2
        status=1
    fi
    if ! diff -u --label expected --label actual "$tmp/expected" "$tmp/out" >&2; then
        echo "$tag: mapping differs" >&2
        status=1
    fi
}

expect_reject() {
    local tag=$1
    run_script "$tag"
    if [ "$rc" -ne 1 ]; then
        echo "'$tag': expected exit 1, got $rc" >&2
        status=1
    fi
    if [ -s "$tmp/out" ]; then
        echo "'$tag': rejected tag must print nothing on stdout" >&2
        status=1
    fi
    if [ "$(wc -l <"$tmp/err")" -ne 1 ]; then
        echo "'$tag': expected one stderr line, got:" >&2
        cat "$tmp/err" >&2
        status=1
    fi
    if ! grep -Fq -- "'$tag' is not vX.Y.Z or vX.Y.Z-rc.N" "$tmp/err"; then
        echo "'$tag': stderr does not name the tag and the accepted shapes:" >&2
        cat "$tmp/err" >&2
        status=1
    fi
}

expect_usage() {
    run_script "$@"
    if [ "$rc" -ne 2 ]; then
        echo "'$*': expected usage exit 2, got $rc" >&2
        status=1
    fi
    if [ -s "$tmp/out" ]; then
        echo "'$*': usage must print nothing on stdout" >&2
        status=1
    fi
}

# A minimal tree with every file --check reads. The changelog header
# follows the spec Version, as a correct bump leaves it. Fixture files
# are written at runtime, so they carry no licence header.
write_tree() {
    local dir=$1 pkg=$2 deps=$3 spec_version=$4 upstream=$5 formula_tag=$6
    mkdir -p "$dir/packaging/rpm" "$dir/packaging/homebrew"
    {
        echo '[workspace]'
        echo 'resolver = "2"'
        echo
        echo '[workspace.package]'
        echo "version = \"$pkg\""
        echo 'edition = "2024"'
        echo
        echo '[workspace.dependencies]'
        echo "buoy-common = { path = \"buoy-common\", version = \"$deps\" }"
        echo "buoy-status-bar = { path = \"buoy-status-bar\", version = \"$deps\" }"
        echo "buoy-tag-picker = { path = \"buoy-tag-picker\", version = \"$deps\" }"
    } >"$dir/Cargo.toml"
    {
        echo 'version = 4'
        local name
        for name in buoy-common buoy-status-bar buoy-tag-picker buoy-wm; do
            echo
            echo '[[package]]'
            echo "name = \"$name\""
            echo "version = \"$pkg\""
        done
        # A third-party crate whose version must not be mistaken for ours.
        echo
        echo '[[package]]'
        echo 'name = "libc"'
        echo 'version = "0.2.1"'
    } >"$dir/Cargo.lock"
    {
        echo "%global upstream_version $upstream"
        echo
        echo 'Name:           buoy-wm'
        echo "Version:        $spec_version"
        echo 'Release:        1%{?dist}'
        echo
        echo '%changelog'
        echo "* Wed Nov 04 2026 Nick Booth <n1cck@onlyhams.radio> - $spec_version-1"
        echo '- Fixture entry'
    } >"$dir/packaging/rpm/buoy-wm.spec"
    {
        echo 'class BuoyWm < Formula'
        echo "  url \"https://github.com/nkbooth/buoy-wm/archive/refs/tags/$formula_tag.tar.gz\""
        echo 'end'
    } >"$dir/packaging/homebrew/buoy-wm.rb"
}

run_check() {
    local dir=$1 tag=$2
    if [ -n "$tag" ]; then
        run_script --check "$tag" --tree "$dir"
    else
        run_script --check --tree "$dir"
    fi
}

# expect_check <dir> <tag-or-empty> <expected-exit> <stderr substring…>
# An expected exit of 0 also asserts silence on both streams.
expect_check() {
    local dir=$1 tag=$2 expected=$3 label
    shift 3
    label="--check ${tag:-(derived)} --tree ${dir#"$tmp/"}"
    run_check "$dir" "$tag"
    if [ "$rc" -ne "$expected" ]; then
        echo "$label: expected exit $expected, got $rc" >&2
        cat "$tmp/out" "$tmp/err" >&2
        status=1
    fi
    if [ "$expected" -eq 0 ] && { [ -s "$tmp/out" ] || [ -s "$tmp/err" ]; }; then
        echo "$label: an agreeing tree must be silent, got:" >&2
        cat "$tmp/out" "$tmp/err" >&2
        status=1
    fi
    local want
    for want in "$@"; do
        if ! grep -Fq -- "$want" "$tmp/err"; then
            echo "$label: stderr does not mention '$want':" >&2
            cat "$tmp/err" >&2
            status=1
        fi
    done
}

# expect_check_lines <dir> <tag> <test-op> <n>: the stderr line count,
# e.g. `-eq 1` proves only the one wrong thing is reported.
expect_check_lines() {
    local dir=$1 tag=$2 op=$3 n=$4 lines
    run_check "$dir" "$tag"
    lines="$(wc -l <"$tmp/err")"
    if ! test "$lines" "$op" "$n"; then
        echo "--check $tag --tree ${dir#"$tmp/"}: expected stderr lines $op $n, got $lines:" >&2
        cat "$tmp/err" >&2
        status=1
    fi
}

# The three accepted shapes (AC 1).
expect_map v1.0.0 1.0.0 1.0.0 1.0.0-1 v1.0.0 no
expect_map v1.0.0-rc.1 1.0.0-rc.1 1.0.0~rc.1 1.0.0~rc.1-1 v1.0.0-rc.1 yes
expect_map v10.20.30-rc.12 10.20.30-rc.12 10.20.30~rc.12 10.20.30~rc.12-1 v10.20.30-rc.12 yes

# Rejected tags (AC 2), including the leading-zero forms rpm would
# compare equal to an accepted one.
expect_reject v1.0.0-beta
expect_reject v1.0.0-rc1
expect_reject v1.0.0-RC.1
expect_reject v1.0.0-rc.1-rc.1
expect_reject 1.0.0-rc.1
expect_reject v1.0
expect_reject v1.0.0.1
expect_reject 'v1.0.0 '
expect_reject ''
expect_reject v01.0.0
expect_reject v1.0.0-rc.0
expect_reject v1.0.0-rc.01

# Usage errors are not rejections.
expect_usage
expect_usage --bogus

# A tree that agrees with v1.0.0-rc.1, with and without the tag.
write_tree "$tmp/good" 1.0.0-rc.1 1.0.0-rc.1 1.0.0~rc.1 1.0.0-rc.1 v1.0.0-rc.1
expect_check "$tmp/good" v1.0.0-rc.1 0
expect_check "$tmp/good" '' 0

# A tree still at 0.1.0: every manifest is named in one run.
write_tree "$tmp/old" 0.1.0 0.1.0 0.1.0 0.1.0 v0.1.0
expect_check "$tmp/old" v1.0.0-rc.1 1 \
    'Cargo.toml' '[workspace.package]' buoy-common buoy-status-bar buoy-tag-picker \
    'Cargo.lock' buoy-wm \
    'packaging/rpm/buoy-wm.spec' 'Version:' upstream_version '%changelog' \
    'packaging/homebrew/buoy-wm.rb' url
expect_check_lines "$tmp/old" v1.0.0-rc.1 -ge 11

# The tilde where the hyphen belongs: %autosetup would fail %prep.
write_tree "$tmp/tilde" 1.0.0-rc.1 1.0.0-rc.1 1.0.0~rc.1 1.0.0~rc.1 v1.0.0-rc.1
expect_check "$tmp/tilde" v1.0.0-rc.1 1 upstream_version
expect_check_lines "$tmp/tilde" v1.0.0-rc.1 -eq 1

# A bump that left the changelog behind.
cp -r "$tmp/good" "$tmp/changelog"
sed -i 's/ - 1\.0\.0~rc\.1-1$/ - 0.1.0-1/' "$tmp/changelog/packaging/rpm/buoy-wm.spec"
expect_check "$tmp/changelog" v1.0.0-rc.1 1 '%changelog'
expect_check_lines "$tmp/changelog" v1.0.0-rc.1 -eq 1

# A formula URL the old release.yml regex could not even see.
write_tree "$tmp/formula" 1.0.0-rc.1 1.0.0-rc.1 1.0.0~rc.1 1.0.0-rc.1 v1.0.0
expect_check "$tmp/formula" v1.0.0-rc.1 1 'packaging/homebrew/buoy-wm.rb'
expect_check_lines "$tmp/formula" v1.0.0-rc.1 -eq 1

# On GitHub the mismatch is an annotation on the manifest, on stdout.
set +e
GITHUB_ACTIONS=1 "$SCRIPT" --check v1.0.0-rc.1 --tree "$tmp/old" >"$tmp/out" 2>"$tmp/err"
rc=$?
set -e
if [ "$rc" -ne 1 ]; then
    echo "GITHUB_ACTIONS --check: expected exit 1, got $rc" >&2
    status=1
fi
if ! grep -q '^::error file=packaging/rpm/buoy-wm.spec::' "$tmp/out"; then
    echo "GITHUB_ACTIONS --check: no ::error annotation for the spec on stdout:" >&2
    cat "$tmp/out" >&2
    status=1
fi
if [ -s "$tmp/err" ]; then
    echo "GITHUB_ACTIONS --check: stderr must be empty when annotating, got:" >&2
    cat "$tmp/err" >&2
    status=1
fi

# A tree with nothing in it: one line per file, and no bash error text,
# which `set -e` would otherwise produce on the first missing file.
mkdir "$tmp/empty"
expect_check "$tmp/empty" v1.0.0-rc.1 1 \
    Cargo.toml Cargo.lock packaging/rpm/buoy-wm.spec packaging/homebrew/buoy-wm.rb
expect_check_lines "$tmp/empty" v1.0.0-rc.1 -eq 4
if grep -Eq 'No such file|line [0-9]+:' "$tmp/err"; then
    echo "--check on an empty tree leaks a shell error:" >&2
    cat "$tmp/err" >&2
    status=1
fi

exit "$status"
