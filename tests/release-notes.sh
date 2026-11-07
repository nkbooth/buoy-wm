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
# Asserts scripts/release-notes.sh: the CHANGELOG.md entry it cuts for a
# tag is the body under the `## [<version>]` heading and nothing else —
# the heading matched whole (rc.10 is not rc.1), the cut stopped at the
# next `## ` heading or the first link-reference line, the surrounding
# blank lines dropped — and a tag with no entry, a tag outside the
# grammar, a missing changelog and a usage error each fail the way the
# script's header promises.
#
# release.yml runs the script only on a tag, where the release page is
# the first thing a visitor reads; this exercises every stop rule on
# fixture changelogs and the real one on every push instead.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly SCRIPT="$REPO_ROOT/scripts/release-notes.sh"
readonly VERSION_SCRIPT="$REPO_ROOT/scripts/release-version.sh"
readonly CHANGELOG="$REPO_ROOT/CHANGELOG.md"
readonly CARGO="$REPO_ROOT/Cargo.toml"

if [ ! -f "$SCRIPT" ]; then
    echo "$SCRIPT: missing" >&2
    exit 1
fi
if [ ! -x "$SCRIPT" ]; then
    echo "$SCRIPT: not executable" >&2
    exit 1
fi

# The script calls release-version.sh, which switches its mismatch lines
# to annotations under CI; the rejected-tag line asserted below is the
# plain form either way, but the environment is kept the same as the
# sister test's.
unset GITHUB_ACTIONS

tmp="$(mktemp -d)"
readonly tmp
trap 'rm -rf "$tmp"' EXIT

status=0
rc=0

# Keeps stdout, stderr and the exit code apart: a missing entry (1) and a
# usage error (2) must be told apart, which `|| true` or `if` would lose.
run_script() {
    set +e
    "$SCRIPT" "$@" >"$tmp/out" 2>"$tmp/err"
    rc=$?
    set -e
}

# expect_body <label> <expected-file> <args…>: exit 0, stdout identical
# to the file, nothing on stderr. Every failure is reported with the
# label and the run goes on, so one run names every case that differs.
expect_body() {
    local label=$1 expected=$2
    shift 2
    run_script "$@"
    if [ "$rc" -ne 0 ]; then
        echo "$label: expected exit 0, got $rc" >&2
        cat "$tmp/err" >&2
        status=1
    fi
    if ! diff -u --label expected --label actual "$expected" "$tmp/out" >&2; then
        echo "$label: body differs" >&2
        status=1
    fi
    if [ -s "$tmp/err" ]; then
        echo "$label: expected nothing on stderr, got:" >&2
        cat "$tmp/err" >&2
        status=1
    fi
}

# expect_exit <label> <expected-exit> <args…>: the exit code, and nothing
# on stdout — a failure must never print a partial body release.yml
# would then publish.
expect_exit() {
    local label=$1 expected=$2
    shift 2
    run_script "$@"
    if [ "$rc" -ne "$expected" ]; then
        echo "$label: expected exit $expected, got $rc" >&2
        cat "$tmp/out" "$tmp/err" >&2
        status=1
    fi
    if [ -s "$tmp/out" ]; then
        echo "$label: a failure must print nothing on stdout, got:" >&2
        cat "$tmp/out" >&2
        status=1
    fi
}

# expect_stderr <label> <needle…>: each fixed string occurs in the last
# run's stderr.
expect_stderr() {
    local label=$1 want
    shift
    for want in "$@"; do
        if ! grep -Fq -- "$want" "$tmp/err"; then
            echo "$label: stderr does not mention '$want':" >&2
            cat "$tmp/err" >&2
            status=1
        fi
    done
}

# expect_stderr_lines <label> <n>: the last run's stderr line count.
expect_stderr_lines() {
    local label=$1 n=$2 lines
    lines="$(wc -l <"$tmp/err")"
    if [ "$lines" -ne "$n" ]; then
        echo "$label: expected $n stderr line(s), got $lines:" >&2
        cat "$tmp/err" >&2
        status=1
    fi
}

# write_changelog <case> <line…>: one fixture directory per case, passed
# with --tree. Fixture files are written at runtime, so they carry no
# licence header.
write_changelog() {
    local dir="$tmp/$1"
    shift
    mkdir -p "$dir"
    printf '%s\n' "$@" >"$dir/CHANGELOG.md"
}

# (a) The real tree, with the tag Cargo.toml names, so this case survives
# the promotion to 1.0.0 and an `## [Unreleased]` heading above the
# entry: the expected body is derived from the real file — the first
# versioned heading (tests/docs.sh's rule) up to the next `## ` heading or
# the first link-reference line, blank lines on either end dropped —
# never a hard-coded line range.
cargo="$(grep -m1 '^version = ' "$CARGO" | cut -d'"' -f2)"
heading_line="$(awk '/^## \[[0-9]/ { print NR; exit }' "$CHANGELOG")"
if [ -z "$heading_line" ]; then
    echo "CHANGELOG.md: no '## [<version>]' heading to derive the real-tree case from" >&2
    exit 1
fi
end_line="$(awk -v start="$heading_line" \
    'NR > start && (/^## / || /^\[[^]]*\]: /) { print NR - 1; exit }' "$CHANGELOG")"
if [ -z "$end_line" ]; then
    end_line="$(wc -l <"$CHANGELOG")"
fi
sed -n "$((heading_line + 1)),${end_line}p" "$CHANGELOG" |
    sed '/./,$!d' |
    sed -e :a -e '/^\n*$/{$d;N;ba' -e '}' >"$tmp/real-expected"
if [ ! -s "$tmp/real-expected" ]; then
    echo "CHANGELOG.md: the entry under line $heading_line is empty; nothing to compare" >&2
    exit 1
fi
expect_body "real tree v$cargo" "$tmp/real-expected" "v$cargo"
if grep -q '^## ' "$tmp/out"; then
    echo "real tree v$cargo: printed a '## ' heading" >&2
    status=1
fi
if grep -q '^\[[^]]*\]: ' "$tmp/out"; then
    echo "real tree v$cargo: printed a link-reference line" >&2
    status=1
fi

# (b) Both stop conditions, neither printed: the next `## ` heading ends
# the 2.0.0 entry and the first link line ends the 1.9.0 entry, with the
# `[Unreleased]` heading above both skipped.
write_changelog stops \
    '# Changelog' \
    '' \
    '## [Unreleased]' \
    '' \
    '- Unreleased bullet one' \
    '- Unreleased bullet two' \
    '' \
    '## [2.0.0]' \
    '' \
    'Line one of 2.0.0.' \
    'Line two of 2.0.0.' \
    'Line three of 2.0.0.' \
    '' \
    '## [1.9.0]' \
    '' \
    '- The one 1.9.0 bullet' \
    '' \
    '[2.0.0]: https://github.com/nkbooth/buoy-wm/releases/tag/v2.0.0' \
    '[1.9.0]: https://github.com/nkbooth/buoy-wm/releases/tag/v1.9.0'
printf '%s\n' 'Line one of 2.0.0.' 'Line two of 2.0.0.' 'Line three of 2.0.0.' >"$tmp/expected"
expect_body 'stops v2.0.0' "$tmp/expected" v2.0.0 --tree "$tmp/stops"
printf '%s\n' '- The one 1.9.0 bullet' >"$tmp/expected"
expect_body 'stops v1.9.0' "$tmp/expected" v1.9.0 --tree "$tmp/stops"

# (c) A dated heading, Keep a Changelog's form for a final release.
write_changelog dated \
    '## [1.0.0] - 2026-12-01' \
    '' \
    '- Dated body' \
    '' \
    '[1.0.0]: https://github.com/nkbooth/buoy-wm/releases/tag/v1.0.0'
printf '%s\n' '- Dated body' >"$tmp/expected"
expect_body 'dated v1.0.0' "$tmp/expected" v1.0.0 --tree "$tmp/dated"

# (d) The whole token: rc.10 is not an entry for rc.1.
write_changelog prefix \
    '## [1.0.0-rc.10]' \
    '' \
    '- The rc.10 body' \
    '' \
    '[1.0.0-rc.10]: https://github.com/nkbooth/buoy-wm/releases/tag/v1.0.0-rc.10'
expect_exit 'prefix v1.0.0-rc.1' 1 v1.0.0-rc.1 --tree "$tmp/prefix"
expect_stderr 'prefix v1.0.0-rc.1' CHANGELOG.md 1.0.0-rc.1
expect_stderr_lines 'prefix v1.0.0-rc.1' 1

# (e) Blank lines around the body are the file's layout, not the notes:
# none leading, none trailing, and the output ends in exactly one newline.
write_changelog padded \
    '## [1.2.3]' \
    '' \
    '' \
    '- Padded body' \
    '' \
    '' \
    '[1.2.3]: https://github.com/nkbooth/buoy-wm/releases/tag/v1.2.3'
printf '%s\n' '- Padded body' >"$tmp/expected"
expect_body 'padded v1.2.3' "$tmp/expected" v1.2.3 --tree "$tmp/padded"
if [ -z "$(head -n1 "$tmp/out")" ]; then
    echo "padded v1.2.3: stdout starts with a blank line" >&2
    status=1
fi
if [ "$(tail -c1 "$tmp/out" | wc -l)" -ne 1 ]; then
    echo "padded v1.2.3: stdout does not end in a newline" >&2
    status=1
fi
if [ "$(tail -c2 "$tmp/out" | wc -l)" -ne 1 ]; then
    echo "padded v1.2.3: stdout ends in more than one newline" >&2
    status=1
fi

# (f) An entry that runs to the end of the file: no link lines and no
# later heading to stop at.
write_changelog tail \
    '## [3.0.0]' \
    '' \
    '- Tail body one' \
    '- Tail body two'
printf '%s\n' '- Tail body one' '- Tail body two' >"$tmp/expected"
expect_body 'tail v3.0.0' "$tmp/expected" v3.0.0 --tree "$tmp/tail"

# (g) A tag outside the grammar is release-version.sh's rejection, passed
# through unchanged: its exit code, its one stderr line, nothing else.
for tag in v1.0.0-beta v01.0.0; do
    set +e
    "$VERSION_SCRIPT" "$tag" >/dev/null 2>"$tmp/version-err"
    set -e
    expect_exit "rejected $tag" 1 "$tag" --tree "$tmp/stops"
    if ! diff -u --label release-version.sh --label release-notes.sh "$tmp/version-err" "$tmp/err" >&2; then
        echo "rejected $tag: stderr is not release-version.sh's line" >&2
        status=1
    fi
done

# (h) Usage errors are not rejections: exit 2, usage on stderr. --help is
# asked for, not a mistake: usage on stdout, exit 0.
for label in 'no arguments' 'two tags' '--bogus'; do
    case $label in
        'no arguments') run_script ;;
        'two tags') run_script v1.0.0 v1.0.1 ;;
        '--bogus') run_script --bogus v1.0.0 ;;
    esac
    if [ "$rc" -ne 2 ]; then
        echo "$label: expected usage exit 2, got $rc" >&2
        status=1
    fi
    if [ -s "$tmp/out" ]; then
        echo "$label: usage must print nothing on stdout" >&2
        status=1
    fi
    if ! grep -q '^usage:' "$tmp/err"; then
        echo "$label: expected usage on stderr, got:" >&2
        cat "$tmp/err" >&2
        status=1
    fi
done
run_script --help
if [ "$rc" -ne 0 ] || ! grep -q '^usage:' "$tmp/out"; then
    echo "--help: expected usage on stdout and exit 0, got $rc:" >&2
    cat "$tmp/out" "$tmp/err" >&2
    status=1
fi

# (i) A tree with no changelog is one line naming the file, not a bash
# error from a read under set -e.
mkdir "$tmp/empty"
expect_exit 'no changelog' 1 v1.0.0 --tree "$tmp/empty"
expect_stderr 'no changelog' CHANGELOG.md
expect_stderr_lines 'no changelog' 1
if grep -Eq 'No such file|line [0-9]+:' "$tmp/err"; then
    echo "no changelog: leaks a shell error:" >&2
    cat "$tmp/err" >&2
    status=1
fi

exit "$status"
