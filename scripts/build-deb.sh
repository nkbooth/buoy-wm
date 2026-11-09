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
# Builds the buoy-wm binary .deb from packaging/deb/debian with the
# distribution's own dpkg-buildpackage and toolchain (ADR-011, ADR-014)
# and publishes it under the name GitHub will serve.
#
# Inputs, both in <sources-dir> and never fetched by this script, the
# same two the RPM is built from (scripts/build-rpm.sh's header says
# how release.yml makes them):
#   buoy-wm-<upstream>.tar.gz          Source0, the tag archive.
#   buoy-wm-<upstream>-vendor.tar.gz   the vendored crates; debian/rules
#       runs cargo --offline, so this is the only way the crates arrive.
# <upstream> is derived from debian/changelog's version, the deb form
# of ADR-012: the Debian revision dropped and `~rc.` written as the
# tag's `-rc.` (1.0.0~rc.2-1 is built from buoy-wm-1.0.0-rc.2.tar.gz).
#
# The packaging lives in packaging/deb/debian, not a top-level debian/:
# a distribution packager owns that name, and a `3.0 (quilt)` orig
# tarball that carries one has it replaced. So the tree handed to
# dpkg-buildpackage is assembled here in a scratch directory: Source0
# unpacked, the vendored crates inside it, and <debian> copied in as
# debian/. A Source0 that already has a debian/ is refused rather than
# overlaid. The build is `dpkg-buildpackage -b -us -uc`: a binary-only
# build (a source package needs the orig-vendor component, which is a
# PPA's concern), unsigned (the attestation is the release's signature),
# and with the tests (no `nocheck`).
#
# Published name: buoy-wm_<upstream>-<revision>.<ID><VERSION_ID>_<arch>.deb,
# e.g. buoy-wm_1.0.0-rc.2-1.ubuntu26.04_amd64.deb, where dpkg names it
# buoy-wm_1.0.0~rc.2-1_amd64.deb. Three reasons: the two Ubuntu legs of
# release.yml build the same version, so dpkg's name would collide in
# `publish`'s merge-multiple download; the release label goes after the
# revision with a dot, as the RPM's `.fc44` does, taken from the build
# container's os-release as rpm takes %{dist} from its own; and GitHub
# rewrites `~` in asset names (scripts/build-rpm.sh's header cites the
# source), so the tag's hyphen is used. `dpkg-deb -f <file> Version`
# still reports 1.0.0~rc.2-1, and apt reads the control data, not the
# filename. The checksum beside it is written from inside <out-dir> so
# the line carries the bare basename and `sha256sum -c` works where it
# is downloaded.
#
# Callers: release.yml (the Ubuntu legs of the `packages` job, each in
# its release's container), a local ubuntu container
# (docs/PACKAGING.md), and tests/build-deb.sh with a stub
# dpkg-buildpackage.
#
#   build-deb.sh [--debian <dir>] [--os-release <file>] <sources-dir> <out-dir>
#       Defaults: packaging/deb/debian beside this script's directory,
#       /etc/os-release. Prints the published .deb's path on stdout, one
#       line, nothing else; dpkg-buildpackage's own output goes to stderr.
#
# Exit codes: 0 built and published; 1 missing input, unreadable
# changelog or os-release, or a Source0 of the wrong shape (every
# missing thing that can be named is reported before exiting; the two
# sources are named from the changelog's version, so a changelog without
# one is reported beside the rest only); 2 usage; dpkg-buildpackage's own
# status if the build fails.
set -euo pipefail

readonly NAME=buoy-wm

usage_text() {
    cat <<'EOF'
usage: build-deb.sh [--debian <dir>] [--os-release <file>] <sources-dir> <out-dir>

Builds the buoy-wm binary .deb from <debian> (default:
packaging/deb/debian) with `dpkg-buildpackage -b -us -uc`, taking
Source0 (buoy-wm-<upstream>.tar.gz) and the vendored crates
(buoy-wm-<upstream>-vendor.tar.gz) from <sources-dir>, and copies the
one binary package into <out-dir> under its published name
(buoy-wm_<upstream>-<revision>.<ID><VERSION_ID>_<arch>.deb, from
<os-release>, default /etc/os-release) with a .sha256 beside it.
Prints the published path.
EOF
}

usage() {
    usage_text >&2
    exit 2
}

debian="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/packaging/deb/debian"
os_release=/etc/os-release
positional=()
while [ $# -gt 0 ]; do
    case $1 in
        --debian | --os-release)
            [ $# -ge 2 ] || usage
            if [ "$1" = --debian ]; then debian=$2; else os_release=$2; fi
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

# dpkg-buildpackage first: it depends on nothing else, so it is named
# beside a bad changelog or missing sources rather than only once those
# are fixed.
if ! command -v dpkg-buildpackage >/dev/null 2>&1; then
    report "dpkg-buildpackage: not found on PATH (install dpkg-dev)"
fi

# The published .deb is the tested one: a nocheck inherited from the
# environment would skip debian/rules' cargo test and still exit 0.
for build_var in DEB_BUILD_OPTIONS DEB_BUILD_PROFILES; do
    case " ${!build_var:-} " in
        *" nocheck "*) report "$build_var: holds 'nocheck'; the published .deb is built with its tests" ;;
    esac
done

# The version is the changelog's top entry, the one dpkg-buildpackage
# builds; scripts/release-version.sh holds it to the tag.
changelog="$debian/changelog"
version=""
if [ ! -f "$changelog" ] || [ ! -r "$changelog" ]; then
    report "$changelog: missing or unreadable changelog"
else
    first="$(head -n1 "$changelog")"
    version="$(sed -n "1s/^$NAME (\([^ )]*\)) .*/\1/p" "$changelog")"
    # A non-native version carries a Debian revision; without one there
    # is no `-<revision>` to strip and the upstream form is unknown.
    case $version in
        *-*) ;;
        *)
            report "$changelog: first line '$first' is not '$NAME (<upstream>-<revision>) …'"
            version=""
            ;;
    esac
fi

# os-release is a shell-quoting format; only these two keys are read,
# with their quotes stripped, and nothing in it is executed.
os_value() {
    sed -n "s/^$1=//p" "$os_release" | head -n1 | sed -e 's/^"\(.*\)"$/\1/' -e "s/^'\(.*\)'\$/\1/"
}
label=""
if [ ! -f "$os_release" ] || [ ! -r "$os_release" ]; then
    report "$os_release: missing or unreadable os-release"
else
    os_id="$(os_value ID)"
    os_version_id="$(os_value VERSION_ID)"
    if [ -z "$os_id" ] || [ -z "$os_version_id" ]; then
        missing_keys=""
        [ -n "$os_id" ] || missing_keys="ID"
        [ -n "$os_version_id" ] || missing_keys="${missing_keys:+$missing_keys and }VERSION_ID"
        report "$os_release: no $missing_keys (the published name's release label)"
    else
        label="$os_id$os_version_id"
    fi
fi

if [ -n "$version" ]; then
    revision="${version##*-}"
    # The grammar admits one `~`, before `rc.` (ADR-012); the tag spells
    # it with a hyphen.
    upstream="${version%-*}"
    upstream="${upstream//\~/-}"
    source0="$sources/$NAME-$upstream.tar.gz"
    vendor="$sources/$NAME-$upstream-vendor.tar.gz"
    for archive in "$source0" "$vendor"; do
        if [ ! -f "$archive" ] || [ ! -r "$archive" ]; then
            report "$archive: missing or unreadable"
        fi
    done
fi
if [ "$status" -ne 0 ]; then
    exit 1
fi

work="$(mktemp -d)"
readonly work
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/src"
tar -C "$work/src" -xzf "$source0"
top="$(ls -A "$work/src")"
if [ "$top" != "$NAME-$upstream" ]; then
    echo "$source0: top directory is '$top', expected '$NAME-$upstream/'" >&2
    exit 1
fi
tree="$work/src/$top"
if [ -e "$tree/debian" ]; then
    echo "$source0: already has a top-level debian/; refusing to overlay $debian" >&2
    exit 1
fi
tar -C "$tree" -xzf "$vendor"
# `--` here and below: the argument loop accepts paths after `--`, so a
# path that starts with `-` must not become an option either.
cp -R -- "$debian" "$tree/debian"

# The build log streams to stderr so stdout stays the one published
# path. dpkg-buildpackage writes its results into the tree's parent.
set +e
(cd -- "$tree" && dpkg-buildpackage -b -us -uc) 1>&2
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
    echo "dpkg-buildpackage exited $rc" >&2
    exit "$rc"
fi

# The one binary package. A `-dbgsym` package shares the version and is
# left behind: none is made today (the release profile carries no debug
# info), but Ubuntu's debhelper emits a .ddeb as soon as there is some.
# `~` and `.` are literal in find's pattern.
mapfile -t built < <(find "$work/src" -maxdepth 1 -type f -name "*_${version}_*.*deb" -not -name '*-dbgsym_*')
if [ "${#built[@]}" -ne 1 ]; then
    echo "expected exactly one $work/src/${NAME}_${version}_<arch>.deb, found ${#built[@]}" >&2
    exit 1
fi
base="${built[0]##*/}"
arch="${base##*_}"
arch="${arch%.deb}"

published="${NAME}_${upstream}-${revision}.${label}_${arch}.deb"
mkdir -p -- "$out"
cp -- "${built[0]}" "$out/$published"
(cd -- "$out" && sha256sum -- "$published" >"$published.sha256")
echo "$out/$published"
