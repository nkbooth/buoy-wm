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
# Builds the buoy-wm binary RPM from packaging/rpm/buoy-wm.spec with the
# distribution's own rpmbuild and toolchain (ADR-011, ADR-014) and
# publishes it under the name GitHub will serve.
#
# Inputs, both in <sources-dir> and never fetched by this script:
#   buoy-wm-<upstream_version>.tar.gz          Source0, the tag archive.
#       release.yml makes it with `git archive --prefix=buoy-wm-<v>/`
#       from the checked-out tag; the spec's Source0 is a URL, but
#       rpmbuild resolves it by basename in SOURCES, so the job builds the
#       tree it validated rather than a download.
#   buoy-wm-<upstream_version>-vendor.tar.gz   Source1, the vendored crates.
#       release.yml makes it with `cargo vendor --locked --versioned-dirs`;
#       %build runs --offline, so this is the only way the crates arrive.
# <upstream_version> is the spec's `%global upstream_version`; the
# spec's `Version:` is the rpm form (ADR-012: 1.0.0-rc.1 is packaged as
# 1.0.0~rc.1).
#
# The build is a full `rpmbuild -bb` into a scratch _topdir: no
# --nodeps (the spec's BuildRequires are the contract), no --nocheck
# (%check is the test suite), no --short-circuit (the result carries
# rpmlib(ShortCircuited) and dnf refuses to install it), and no
# debug_package define (Fedora's exported RUSTFLAGS produce the debuginfo
# subpackages; they stay in the scratch dir and are not published).
#
# Published name: the built basename with `Version:` replaced by
# upstream_version, e.g. buoy-wm-1.0.0~rc.1-1.fc44.x86_64.rpm becomes
# buoy-wm-1.0.0-rc.1-1.fc44.x86_64.rpm. GitHub rewrites special
# characters in release-asset names on upload (REST "Upload a release
# asset": "GitHub renames asset filenames that have special characters";
# softprops/action-gh-release#159 shows `1.1.0-1~buster` served as
# `1.1.0-1.buster`), so the tilde name would be served as
# buoy-wm-1.0.0.rc.1-…, which no checksum line, attestation subject or
# install URL could predict from the spec. The hyphen form is the one
# spelling the tag and every other asset already use; `rpm -qp` on the
# file still reports 1.0.0~rc.1, and dnf reads headers, not filenames.
# For a final release the two values are equal and nothing is renamed.
# The checksum beside it is written from inside <out-dir> so the line
# carries the bare basename and `sha256sum -c` works where it is
# downloaded.
#
# Callers: release.yml (the `rpm` job, in a fedora container), a local
# fedora container (docs/PACKAGING.md), and tests/build-rpm.sh with a
# stub rpmbuild.
#
#   build-rpm.sh [--spec <path>] <sources-dir> <out-dir>
#       Default spec: packaging/rpm/buoy-wm.spec beside this script's
#       directory. Prints the published RPM's path on stdout, one line,
#       nothing else; rpmbuild's own output goes to stderr.
#
# Exit codes: 0 built and published; 1 missing input or unreadable spec
# (every missing thing that can be named is reported before exiting; the
# two sources are named from the spec's versions, so a spec without them
# is reported beside rpmbuild only); 2 usage; rpmbuild's own status if
# the build fails.
set -euo pipefail

readonly NAME=buoy-wm

usage_text() {
    cat <<'EOF'
usage: build-rpm.sh [--spec <path>] <sources-dir> <out-dir>

Builds the buoy-wm binary RPM from <spec> (default:
packaging/rpm/buoy-wm.spec) with `rpmbuild -bb`, taking Source0
(buoy-wm-<upstream_version>.tar.gz) and Source1
(buoy-wm-<upstream_version>-vendor.tar.gz) from <sources-dir>, and
copies the one binary package into <out-dir> under its published name
(the spec Version's `~` written as the tag's `-`) with a .sha256 beside
it. Prints the published path.
EOF
}

usage() {
    usage_text >&2
    exit 2
}

spec="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/packaging/rpm/buoy-wm.spec"
positional=()
while [ $# -gt 0 ]; do
    case $1 in
        --spec)
            [ $# -ge 2 ] || usage
            spec=$2
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

if [ "${#positional[@]}" -ne 2 ]; then
    usage
fi
sources="${positional[0]}"
out="${positional[1]}"

status=0

# One line per problem, never an abort: a run with nothing staged should
# name both archives, not fail twice.
report() {
    echo "$1" >&2
    status=1
}

# rpmbuild first: it depends on nothing else, so it is named beside a
# bad spec or missing sources rather than only once those are fixed.
if ! command -v rpmbuild >/dev/null 2>&1; then
    report "rpmbuild: not found on PATH (install rpm-build)"
fi

if [ ! -f "$spec" ] || [ ! -r "$spec" ]; then
    report "$spec: missing or unreadable spec"
    exit 1
fi

# The same two forms scripts/release-version.sh reads, so the three
# scripts that look at the spec cannot disagree about where the versions
# are. `|| true`: an absent line is reported below, not an abort.
upstream="$(sed -n 's/^%global upstream_version //p' "$spec" | head -n1)"
version="$(grep -m1 -E '^Version:[[:space:]]+' "$spec" | awk '{print $2}' || true)"
if [ -z "$upstream" ]; then
    report "$spec: no '%global upstream_version' line"
fi
if [ -z "$version" ]; then
    report "$spec: no 'Version:' line"
fi
# The source names need both versions, so this is as far as a bad spec
# can be reported.
if [ "$status" -ne 0 ]; then
    exit 1
fi

source0="$sources/$NAME-$upstream.tar.gz"
source1="$sources/$NAME-$upstream-vendor.tar.gz"
for archive in "$source0" "$source1"; do
    if [ ! -f "$archive" ] || [ ! -r "$archive" ]; then
        report "$archive: missing or unreadable"
    fi
done
if [ "$status" -ne 0 ]; then
    exit 1
fi

topdir="$(mktemp -d)"
readonly topdir
trap 'rm -rf "$topdir"' EXIT
mkdir -p "$topdir/SOURCES" "$topdir/BUILD" "$topdir/RPMS" "$topdir/SRPMS"
# `--` throughout: the argument loop accepts paths after `--`, so a path
# that starts with `-` must not become an option here either.
cp -- "$source0" "$source1" "$topdir/SOURCES/"

# The build log streams to stderr so stdout stays the one published path.
set +e
rpmbuild --define "_topdir $topdir" -bb "$spec" 1>&2
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
    echo "rpmbuild exited $rc" >&2
    exit "$rc"
fi

# The one binary package; the debuginfo and debugsource subpackages
# Fedora's RUSTFLAGS produce share the version and are left behind.
# `~` and `.` are literal in find's pattern.
mapfile -t built < <(find "$topdir/RPMS" -type f -name "$NAME-$version-*.rpm" -not -name "$NAME-debug*")
if [ "${#built[@]}" -ne 1 ]; then
    echo "expected exactly one $topdir/RPMS/*/$NAME-$version-*.rpm, found ${#built[@]}" >&2
    exit 1
fi

published="${built[0]##*/}"
# Quoting the pattern makes `~` and `.` literal, and quoting the
# replacement keeps bash 5.2's `&` (the matched text) out of it; one
# substitution is exact because the version occurs once in an rpm
# basename.
published="${published/"$version"/"$upstream"}"
mkdir -p -- "$out"
cp -- "${built[0]}" "$out/$published"
(cd -- "$out" && sha256sum -- "$published" >"$published.sha256")
echo "$out/$published"
