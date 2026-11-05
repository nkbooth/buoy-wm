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
# Maps a release tag to the version form each packaging format needs
# (ADR-012) and, with --check, reports every manifest that disagrees
# with it. One grammar and one mapping, so release.yml and ci.yml cannot
# drift from each other and the accept/reject cases are testable
# without pushing tags (tests/release-version.sh).
#
#   key         v1.0.0-rc.1     v1.0.0    used by
#   cargo       1.0.0-rc.1      1.0.0     Cargo.toml, Cargo.lock, the tarball name
#   rpm         1.0.0~rc.1      1.0.0     spec Version: (`~` sorts before the final)
#   deb         1.0.0~rc.1-1    1.0.0-1   debian/changelog (Story 5.5)
#   formula     v1.0.0-rc.1     v1.0.0    the Homebrew url tag
#   prerelease  yes             no        `gh release create --prerelease`
#
# Callers: release.yml (Validate the tag, Check tag matches every
# packaged version), ci.yml hygiene (`versions agree`, with the tag
# derived from Cargo.toml so a half-bumped tree is red on the push that
# made it), and tests/release-version.sh.
#
#   release-version.sh <tag>
#       Print the six key=value lines above; exit 1 with one stderr line
#       for a tag that is not vX.Y.Z or vX.Y.Z-rc.N.
#   release-version.sh --check [<tag>] [--tree <dir>]
#       Compare the tree's manifests with the tag (default: `v` + the
#       [workspace.package] version in Cargo.toml). Every disagreement is
#       one line, on stderr or as a `::error file=` annotation on stdout
#       under GITHUB_ACTIONS; exit 1 on any, silent exit 0 on none.
#
# Exit codes: 0 valid/agree, 1 rejected tag or mismatch, 2 usage.
set -euo pipefail

# No leading zeros anywhere: rpm compares `01` equal to `1`, so v01.0.0
# and v1.0.0 would map to one package version and the tag-to-version
# promise would stop being one-to-one.
readonly TAG_RE='^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-rc\.([1-9][0-9]*))?$'

readonly MANIFEST_CARGO=Cargo.toml
readonly MANIFEST_LOCK=Cargo.lock
readonly MANIFEST_SPEC=packaging/rpm/buoy-wm.spec
readonly MANIFEST_FORMULA=packaging/homebrew/buoy-wm.rb

usage_text() {
    cat <<'EOF'
usage: release-version.sh <tag>
       release-version.sh --check [<tag>] [--tree <dir>]

Prints the Cargo, RPM, Debian and formula forms of a vX.Y.Z or
vX.Y.Z-rc.N tag (ADR-012), or with --check reports every manifest in
<dir> (default: this repository) that disagrees with the tag (default:
v + the [workspace.package] version in Cargo.toml).
EOF
}

usage() {
    usage_text >&2
    exit 2
}

check=no
tree="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
positional=()
while [ $# -gt 0 ]; do
    case $1 in
        --check) check=yes ;;
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

if [ "${#positional[@]}" -gt 1 ]; then
    usage
fi
if [ "$check" = no ] && [ "${#positional[@]}" -eq 0 ]; then
    usage
fi

# Sets cargo, rpm, deb, formula and prerelease from the tag, or exits 1.
map_tag() {
    local tag=$1
    if [[ ! "$tag" =~ $TAG_RE ]]; then
        echo "tag '$tag' is not vX.Y.Z or vX.Y.Z-rc.N (ADR-012)" >&2
        exit 1
    fi
    cargo="${tag#v}"
    # The grammar admits one `-rc.`, so one substitution is exact.
    rpm="${cargo/-rc./~rc.}"
    deb="$rpm-1"
    formula="$tag"
    if [ -n "${BASH_REMATCH[4]}" ]; then
        prerelease=yes
    else
        prerelease=no
    fi
}

print_mapping() {
    printf '%s\n' "tag=$tag" "cargo=$cargo" "rpm=$rpm" "deb=$deb" \
        "formula=$formula" "prerelease=$prerelease"
}

status=0

# One line per disagreement, never an abort: a bump that missed four
# files should take one fix, not four failed runs. Under GITHUB_ACTIONS
# the line is an annotation on the manifest itself, as the tests' own
# `::warning` lines are.
report() {
    local relpath=$1 message=$2
    if [ -n "${GITHUB_ACTIONS:-}" ]; then
        echo "::error file=$relpath::$message"
    else
        echo "$relpath: $message" >&2
    fi
    status=1
}

# report_mismatch <relpath> <what> <actual> <expected>
report_mismatch() {
    report "$1" "$2 reads '$3', tag $tag needs '$4'"
}

# expect_value <relpath> <what> <actual> <expected>
expect_value() {
    if [ "$3" != "$4" ]; then
        report_mismatch "$@"
    fi
}

# Returns 1 after reporting, so a missing manifest is one line and the
# remaining manifests are still checked.
readable() {
    local relpath=$1
    if [ ! -r "$tree/$relpath" ]; then
        report "$relpath" "missing or unreadable, tag $tag needs it"
        return 1
    fi
}

# The [workspace.package] version is the first `version = "…"` line, the
# idiom release.yml and the msrv job read the sister field with. `|| true`:
# an absent line is a mismatch to report, not an abort.
workspace_version() {
    grep -m1 '^version = ' "$1" | cut -d'"' -f2 || true
}

check_cargo_toml() {
    local file="$tree/$MANIFEST_CARGO" dep actual
    readable "$MANIFEST_CARGO" || return 0
    expect_value "$MANIFEST_CARGO" '[workspace.package] version' "$(workspace_version "$file")" "$cargo"
    # The three path dependencies carry a version requirement (deny.toml
    # forbids wildcards), and `^0.1.0` does not match a 1.0.0-rc.1 workspace:
    # cargo refuses to resolve, with a message that does not say "bump".
    for dep in buoy-common buoy-status-bar buoy-tag-picker; do
        actual="$(sed -n "s/^$dep = { path = \"[^\"]*\", version = \"\([^\"]*\)\" }\$/\1/p" "$file" | head -n1)"
        expect_value "$MANIFEST_CARGO" "[workspace.dependencies] $dep version" "$actual" "$cargo"
    done
}

check_cargo_lock() {
    local file="$tree/$MANIFEST_LOCK" pkg actual
    readable "$MANIFEST_LOCK" || return 0
    # `cargo update -w` writes exactly these four lines; the lock is what
    # `--locked` builds trust, so it is checked, not just the manifest.
    for pkg in buoy-common buoy-status-bar buoy-tag-picker buoy-wm; do
        actual="$(awk -F'"' -v want="$pkg" \
            '/^name = "/ { name = $2 } /^version = "/ && name == want { print $2; exit }' "$file")"
        expect_value "$MANIFEST_LOCK" "[[package]] $pkg version" "$actual" "$cargo"
    done
}

check_spec() {
    local file="$tree/$MANIFEST_SPEC" version upstream release entry rpm_re
    readable "$MANIFEST_SPEC" || return 0
    version="$(grep -m1 -E '^Version:[[:space:]]+' "$file" | awk '{print $2}' || true)"
    expect_value "$MANIFEST_SPEC" 'Version:' "$version" "$rpm"
    # Source0 and %autosetup name the unpacked archive, which keeps the
    # tag's hyphen; the tilde there fails %prep.
    upstream="$(sed -n 's/^%global upstream_version //p' "$file" | head -n1)"
    expect_value "$MANIFEST_SPEC" '%global upstream_version' "$upstream" "$cargo"
    # rpmlint's incoherent-version-in-changelog and a stale top entry are
    # what a half-bump leaves behind. `~` is literal in an ERE; `.` is not.
    release="$(sed -n 's/^Release:[[:space:]]*\([0-9]*\).*/\1/p' "$file" | head -n1)"
    rpm_re="${rpm//./\\.}"
    if ! grep -Eq "^\* .* - ${rpm_re}-${release}\$" "$file"; then
        entry="$(sed -n '/^%changelog/,$ s/^\* .* - \([^ ]*\)$/\1/p' "$file" | head -n1)"
        report_mismatch "$MANIFEST_SPEC" '%changelog top entry' "$entry" "$rpm-$release"
    fi
}

check_formula() {
    local file="$tree/$MANIFEST_FORMULA" actual
    readable "$MANIFEST_FORMULA" || return 0
    # The old release.yml regex only saw vX.Y.Z URLs; this captures
    # whatever tag the url names and compares it.
    actual="$(sed -n 's|^ *url "https://github.com/nkbooth/buoy-wm/archive/refs/tags/\([^/"]*\)\.tar\.gz"$|\1|p' "$file" | head -n1)"
    expect_value "$MANIFEST_FORMULA" 'url tag' "$actual" "$formula"
}

if [ "${#positional[@]}" -eq 1 ]; then
    tag="${positional[0]}"
else
    # Tagless --check: the tree is checked against itself, so a push that
    # bumped Cargo.toml and nothing else is red before any tag exists.
    if [ ! -r "$tree/$MANIFEST_CARGO" ]; then
        report "$MANIFEST_CARGO" "missing or unreadable, needed to derive the tag"
        exit 1
    fi
    tag="v$(workspace_version "$tree/$MANIFEST_CARGO")"
fi

map_tag "$tag"

if [ "$check" = no ]; then
    print_mapping
    exit 0
fi

check_cargo_toml
check_cargo_lock
check_spec
check_formula
exit "$status"
