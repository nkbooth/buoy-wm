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
# Prints the CHANGELOG.md entry for a release tag: the body under the
# `## [<version>]` heading, where <version> is the tag's Cargo form from
# scripts/release-version.sh (one grammar, one mapping). The release page
# is the first thing a visitor to a tag reads, and GitHub's generated
# notes would list merged pull requests since the previous tag — this
# repository merges none — so the entry is cut here, by a script whose
# stop rules are tested on every push (tests/release-notes.sh) instead
# of only on a tag.
#
# The heading is matched whole: `## [<version>]` exactly, or followed by
# a space (Keep a Changelog's ` - <date>`), so an rc.10 heading is never
# an rc.1 entry — the rule tests/docs.sh applies. The body is every line
# after the heading up to the next `## ` heading or the first
# link-reference line (`[name]: url`), with blank lines (empty or only
# spaces and tabs) on either end removed; an entry that runs to the end
# of the file is printed whole. A heading with nothing under it is a
# failure, not empty notes: the release page would be blank and the run
# green.
#
# Callers: release.yml `build` (writes the notes file `publish` passes
# to `gh release create --notes-file`), tests/release-notes.sh, and the
# docs/PACKAGING.md runbook.
#
#   release-notes.sh <tag> [--tree <dir>]
#       Print the entry for <tag> from <dir>/CHANGELOG.md (default: the
#       repository this script lives in).
#
# Exit codes: 0 printed; 1 with one stderr line for a tag outside the
# grammar (release-version.sh's line), a missing or unreadable
# CHANGELOG.md, no heading for the version, or a heading with nothing
# under it; 2 usage.
set -euo pipefail

usage_text() {
    cat <<'EOF'
usage: release-notes.sh <tag> [--tree <dir>]

Prints the CHANGELOG.md entry for a vX.Y.Z or vX.Y.Z-rc.N tag: every
line under the `## [<version>]` heading up to the next `## ` heading or
the first link-reference line, with the blank lines around it removed.
<dir> defaults to this repository.
EOF
}

usage() {
    usage_text >&2
    exit 2
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly script_dir
tree="$(cd "$script_dir/.." && pwd)"
positional=()
while [ $# -gt 0 ]; do
    case $1 in
        --tree)
            [ $# -ge 2 ] || usage
            tree=$2
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

if [ "${#positional[@]}" -ne 1 ]; then
    usage
fi
tag="${positional[0]}"

# The grammar and the mapping live in one place; a rejected tag is that
# script's exit code and stderr line, with nothing printed here.
mapping="$("$script_dir/release-version.sh" "$tag")" || exit $?
cargo="$(sed -n 's/^cargo=//p' <<<"$mapping")"

# `-f` as well as `-r`: a directory passes `-r`, and awk's read error
# would then be the message instead of this one.
changelog="$tree/CHANGELOG.md"
if [ ! -f "$changelog" ] || [ ! -r "$changelog" ]; then
    echo "CHANGELOG.md: missing or unreadable in $tree, tag $tag needs it" >&2
    exit 1
fi

# One pass. The version is compared literally (`==` and index), never as
# a regex: it holds dots and a hyphen. The body is buffered so the blank
# lines on either end can be dropped before anything is printed; `exit`
# in the main rule still runs END. Exit 3 is "no heading" and 4 is
# "nothing under it"; both become exit 1 with a line naming the right
# fault, because a heading over nothing would otherwise publish blank
# notes under a green run.
set +e
awk -v want="$cargo" '
    !found {
        if ($0 == "## [" want "]" || index($0, "## [" want "] ") == 1) {
            found = 1
        }
        next
    }
    /^## / || /^\[[^]]*\]: / { exit }
    { lines[++n] = $0 }
    END {
        if (!found) {
            exit 3
        }
        first = 1
        last = n
        while (first <= last && lines[first] ~ /^[ \t]*$/) first++
        while (last >= first && lines[last] ~ /^[ \t]*$/) last--
        if (last < first) {
            exit 4
        }
        for (i = first; i <= last; i++) print lines[i]
    }
' "$changelog"
rc=$?
set -e

case $rc in
    0) exit 0 ;;
    3)
        echo "CHANGELOG.md: no '## [$cargo]' heading for tag $tag" >&2
        exit 1
        ;;
    4)
        echo "CHANGELOG.md: nothing under '## [$cargo]' for tag $tag" >&2
        exit 1
        ;;
    *) exit "$rc" ;;
esac
