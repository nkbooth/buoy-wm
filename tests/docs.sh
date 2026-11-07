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
# Asserts that the documents agree with the files they describe. The
# README is shipped inside the tarball and the RPM, PACKAGING.md is the
# packager's runbook, and install.sh's last line and the formula's
# caveats are the two other places a user is told how to run the thing.
# Each needle is derived from the file that owns the fact (Cargo.toml,
# the spec, buoy.desktop) or is a string a code review already caught
# as a drift; nothing here lints prose.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly README="$REPO_ROOT/README.md"
readonly PACKAGING="$REPO_ROOT/docs/PACKAGING.md"
readonly CHANGELOG="$REPO_ROOT/CHANGELOG.md"
readonly SPEC="$REPO_ROOT/packaging/rpm/buoy-wm.spec"
readonly DESKTOP="$REPO_ROOT/packaging/buoy.desktop"
readonly FORMULA="$REPO_ROOT/packaging/homebrew/buoy-wm.rb"
readonly INSTALL="$REPO_ROOT/scripts/install.sh"
readonly CARGO="$REPO_ROOT/Cargo.toml"

tmp="$(mktemp -d)"
readonly tmp
trap 'rm -rf "$tmp"' EXIT

status=0
: >"$tmp/report"

# Every failure is appended and printed together at the end, so one run
# names everything that disagrees rather than the first thing.
fail() {
    printf '%s\n' "$1" >>"$tmp/report"
    status=1
}

for file in "$README" "$PACKAGING" "$CHANGELOG" "$SPEC" "$DESKTOP" \
    "$FORMULA" "$INSTALL" "$CARGO"; do
    if [ ! -r "$file" ]; then
        fail "${file#"$REPO_ROOT/"}: missing or unreadable"
    fi
done
if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
    exit "$status"
fi

# require_text <file> <needle> <why>: the fixed string must occur.
require_text() {
    local file=$1 needle=$2 why=$3
    if ! grep -qF -- "$needle" "$file"; then
        fail "${file#"$REPO_ROOT/"}: does not mention '$needle' ($why)"
    fi
}

# require_either <file> <needle-a> <needle-b> <why>: one of two spellings.
require_either() {
    local file=$1 a=$2 b=$3 why=$4
    if ! grep -qF -- "$a" "$file" && ! grep -qF -- "$b" "$file"; then
        fail "${file#"$REPO_ROOT/"}: mentions neither '$a' nor '$b' ($why)"
    fi
}

# forbid_text <file> <needle> <why>: the fixed string must not occur.
forbid_text() {
    local file=$1 needle=$2 why=$3
    if grep -qF -- "$needle" "$file"; then
        fail "${file#"$REPO_ROOT/"}: still says '$needle' ($why)"
    fi
}

# count_lines <file> <regex>: how many lines match, 0 included.
count_lines() {
    local file=$1 regex=$2
    grep -cE -- "$regex" "$file" || true
}

# The README's pinned version is the one it ships with: the file travels
# in the tarball and the RPM, so it must name the release Cargo.toml
# builds. Exactly one line, so both install blocks share it.
cargo="$(grep -m1 '^version = ' "$CARGO" | cut -d'"' -f2)"
if [ -z "$cargo" ]; then
    fail "Cargo.toml: no 'version = \"…\"' line to derive the README version from"
fi
version_lines="$(count_lines "$README" '^version=')"
if [ "$version_lines" -ne 1 ]; then
    fail "README.md: expected exactly one 'version=' line, found $version_lines"
fi
require_text "$README" "version=$cargo" \
    "the README's pinned version must be Cargo.toml's"

# Asset names are derived from the spec: Source1 is the vendor tarball
# with the tag's hyphen, and the RPM is served under the fc44 name.
upstream="$(sed -n 's/^%global upstream_version //p' "$SPEC" | head -n1)"
if [ -z "$upstream" ]; then
    fail "packaging/rpm/buoy-wm.spec: no '%global upstream_version' line"
fi
vendor_literal="buoy-wm-${upstream}-vendor.tar.gz"
vendor_macro='%{name}-%{upstream_version}-vendor.tar.gz'
require_either "$README" "$vendor_literal" "$vendor_macro" \
    "the vendor tarball is a release asset"
require_either "$PACKAGING" "$vendor_literal" "$vendor_macro" \
    "the vendor tarball is Source1"
require_text "$PACKAGING" "$vendor_literal" \
    "the rebuild recipe must name the literal file a packager downloads"
require_text "$README" '-1.fc44.x86_64.rpm' \
    "the RPM install block must name the asset"
require_text "$PACKAGING" '-1.fc44.x86_64.rpm' \
    "the release model must name the RPM asset"

# The login entry's Exec= line, quoted verbatim, so the README's account
# of what GDM runs cannot drift from the file tests/buoy-desktop.sh gates.
exec_line="$(grep -m1 '^Exec=' "$DESKTOP")"
if [ -z "$exec_line" ]; then
    fail "packaging/buoy.desktop: no Exec= line"
fi
require_text "$README" "$exec_line" \
    "the packaged-session section must quote the entry's Exec= line"

# Strings earlier reviews caught as drifts; each would send a reader to
# a file or a path that no longer exists.
forbid_text "$PACKAGING" 'Source2' \
    "the spec has had one source besides the crates since e610d7e"
forbid_text "$PACKAGING" '0.1.0-vendor' \
    "the vendor tarball is named from upstream_version, not a fixed 0.1.0"
forbid_text "$README" 'river -c /usr/libexec/buoy-wm/buoy-wm' \
    "the entry runs the supervisor, not the WM (ADR-010)"
forbid_text "$PACKAGING" 'river -c /usr/libexec/buoy-wm/buoy-wm' \
    "the entry runs the supervisor, not the WM (ADR-010)"
if grep -qE '^\s*zellij\b.*\(required\)' "$FORMULA"; then
    fail "packaging/homebrew/buoy-wm.rb: caveats call zellij '(required)' (it is the default multiplexer, swappable in config)"
fi

# The runbook's first step and the rebuild recipe name the scripts that
# do the work, so a packager is sent to the one copy rather than a prose
# restatement of it.
require_text "$PACKAGING" 'scripts/release-version.sh --check' \
    "the runbook's bump step is the --check script"
require_text "$PACKAGING" 'scripts/build-rpm.sh' \
    "the offline rebuild is the build script"

# install.sh's last line points at the README section that explains how
# to start a session, now that there are two ways. A fixture build dir
# with stub executables keeps the test free of the toolchain; HOME has
# no ~/.local/bin, so no symlink is made and the install line is the
# one before the pointer.
mkdir -p "$tmp/build" "$tmp/home"
for stub in buoy-wm buoy-tag-picker buoy-status-bar; do
    printf '#!/bin/sh\nexit 0\n' >"$tmp/build/$stub"
    chmod 0755 "$tmp/build/$stub"
done
set +e
HOME="$tmp/home" BUOY_BUILD_DIR="$tmp/build" BUOY_INSTALL_DIR="$tmp/lib" \
    "$INSTALL" >"$tmp/out" 2>"$tmp/err"
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
    fail "scripts/install.sh: expected exit 0 on a complete build dir, got $rc: $(cat "$tmp/err")"
fi
for stub in buoy-wm buoy-tag-picker buoy-status-bar; do
    if [ ! -x "$tmp/lib/$stub" ]; then
        fail "scripts/install.sh: did not install $stub into BUOY_INSTALL_DIR"
    fi
done
if [ -e "$tmp/home/.local/bin/buoy-wm" ]; then
    fail "scripts/install.sh: made a symlink although ~/.local/bin does not exist"
fi
if ! tail -n1 "$tmp/out" | grep -qF 'Running it'; then
    fail "scripts/install.sh: last line does not point at the README's 'Running it' section, got: $(tail -n1 "$tmp/out")"
fi

if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
fi
exit "$status"
