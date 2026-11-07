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
# README and config.example.toml are shipped inside the tarball and the
# RPM, PACKAGING.md is the packager's runbook, CHANGELOG.md and
# SECURITY.md are what a reader of a tag opens first, and install.sh's
# last line and the formula's caveats are the two other places a user
# is told how to run the thing. Each needle is derived from the file
# that owns the fact (Cargo.toml, the spec, buoy.desktop) or is a
# string a review already caught as a drift or as a release that never
# existed; nothing here lints prose.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly README="$REPO_ROOT/README.md"
readonly PACKAGING="$REPO_ROOT/docs/PACKAGING.md"
readonly SPEC="$REPO_ROOT/packaging/rpm/buoy-wm.spec"
readonly DESKTOP="$REPO_ROOT/packaging/buoy.desktop"
readonly FORMULA="$REPO_ROOT/packaging/homebrew/buoy-wm.rb"
readonly INSTALL="$REPO_ROOT/scripts/install.sh"
readonly CARGO="$REPO_ROOT/Cargo.toml"
readonly CHANGELOG="$REPO_ROOT/CHANGELOG.md"
readonly SECURITY="$REPO_ROOT/SECURITY.md"
readonly EXAMPLE="$REPO_ROOT/docs/config.example.toml"

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

for file in "$README" "$PACKAGING" "$SPEC" "$DESKTOP" \
    "$FORMULA" "$INSTALL" "$CARGO" "$CHANGELOG" "$SECURITY" "$EXAMPLE"; do
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
# builds. Exactly one line, so both install blocks share it. The `|| true`
# keeps an absent line on the report: under `set -e` a failed grep in the
# substitution would otherwise end the run before the message is written.
cargo="$(grep -m1 '^version = ' "$CARGO" | cut -d'"' -f2 || true)"
if [ -z "$cargo" ]; then
    fail "Cargo.toml: no 'version = \"…\"' line to derive the README version from"
fi
version_lines="$(count_lines "$README" '^version=')"
if [ "$version_lines" -ne 1 ]; then
    fail "README.md: expected exactly one 'version=' line, found $version_lines"
fi
# Whole-line match: a substring check would accept rc.10 against rc.1.
if ! grep -qxF -- "version=$cargo" "$README"; then
    fail "README.md: the 'version=' line is not 'version=$cargo' (the README's pinned version must be Cargo.toml's)"
fi

# The changelog's first versioned heading is the release Cargo.toml
# builds, so a bump that left the changelog behind is red on the push
# that made it. A leading `## [Unreleased]` is Keep a Changelog's form
# for post-tag changes and is skipped, not forbidden. The whole token
# is matched, optionally followed by a space and a date, so rc.10 cannot
# pass as rc.1; the `|| true` keeps a missing heading on the report.
heading="$(grep -m1 -E '^## \[[0-9]' "$CHANGELOG" || true)"
if [ -z "$heading" ]; then
    fail "CHANGELOG.md: no '## [<version>]' heading"
else
    case $heading in
        "## [$cargo]" | "## [$cargo] "*) ;;
        *) fail "CHANGELOG.md: first release heading is '$heading', not '## [$cargo]' (the changelog must describe the release Cargo.toml builds)" ;;
    esac
fi
# Keep a Changelog's link reference for that heading, derived the same
# way and matched whole-line: the needle is a prefix of a reference whose
# URL kept an `-rc.N` suffix, so a substring match would let the final
# tag leave it pointing at the rc.
link="[$cargo]: https://github.com/nkbooth/buoy-wm/releases/tag/v$cargo"
if ! grep -qxF -- "$link" "$CHANGELOG"; then
    fail "CHANGELOG.md: no line '$link' (the release heading's link reference)"
fi

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
require_text "$PACKAGING" "$vendor_literal" \
    "the rebuild recipe must name the literal file a packager downloads"
require_text "$README" '-1.fc44.x86_64.rpm' \
    "the RPM install block must name the asset"
require_text "$PACKAGING" '-1.fc44.x86_64.rpm' \
    "the release model must name the RPM asset"

# The login entry's Exec= line, quoted verbatim, so the README's account
# of what GDM runs cannot drift from the file tests/buoy-desktop.sh gates.
exec_line="$(grep -m1 '^Exec=' "$DESKTOP" || true)"
if [ -z "$exec_line" ]; then
    fail "packaging/buoy.desktop: no Exec= line"
fi
require_text "$README" "$exec_line" \
    "the packaged-session section must quote the entry's Exec= line"

# Strings earlier reviews caught as drifts: a file or a path that no
# longer exists, or a release that never existed. The project's first
# tag is v1.0.0-rc.1 (ADR-012), so any 0.x release a document describes
# is a phantom; the four caught when Story 6.1 was written were a
# `[0.1.0]` changelog entry, "Since 0.1", "buoy 0.1" and a `0.1.0-1`
# %changelog entry, and the two version checks cover their siblings.
if grep -qE '^## \[0\.' "$CHANGELOG"; then
    fail "CHANGELOG.md: has a '## [0.' heading (a release that never happened; the first tag is 1.0.0-rc.1, ADR-012)"
fi
forbid_text "$SECURITY" 'Since 0.1' \
    "there was no 0.1; the behaviour is a fact of the first release"
forbid_text "$EXAMPLE" 'buoy 0.1' \
    "there was no buoy 0.1 to have documented spellings before"
if grep -qE ' - 0\.[0-9]+\.[0-9]+-[0-9]+$' "$SPEC"; then
    fail "packaging/rpm/buoy-wm.spec: %changelog has a 0.x entry (a package never built)"
fi
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

# A source line number in prose drifts with every edit above it; the
# one caught at 6.1's creation pointed into an unrelated impl block.
if grep -qE '\.rs:[0-9]' "$SECURITY"; then
    fail "SECURITY.md: cites a source line number, which drifts (name the function and file instead)"
fi
# SECURITY.md cites the pinned-terminal spawn by function and file
# instead, so the file is derived from where the function is defined:
# a rename or a move is red here rather than a citation that drifts the
# way the line number did (the 6.1 review found it one move behind).
spawn_fn='spawn_pinned_terminal_or_release_claim'
spawn_file="$(grep -rlF -- "fn $spawn_fn(" "$REPO_ROOT/wm/src" || true)"
if [ -z "$spawn_file" ] || [ "$(printf '%s\n' "$spawn_file" | wc -l)" -ne 1 ]; then
    fail "wm/src: expected exactly one file defining '$spawn_fn' (SECURITY.md cites it by file)"
else
    require_text "$SECURITY" "$spawn_fn" \
        "the spawn SECURITY.md describes, by function name"
    require_text "$SECURITY" "${spawn_file#"$REPO_ROOT/"}" \
        "the file that defines $spawn_fn"
fi
# The release's one attestation names three subjects, and the README's
# verify steps for the RPM and the tarball both point here. The names
# are the shapes SECURITY.md prints: `<version>` is the tag without its
# `v`, and the RPM carries no dist tag on purpose (Story 5.4 owns fc44).
require_text "$SECURITY" 'gh attestation verify' \
    "the verify command the README points at"
require_text "$SECURITY" 'buoy-wm-<version>-vendor.tar.gz' \
    "the vendor tarball is an attestation subject"
require_text "$SECURITY" 'buoy-wm-<version>-1.<dist>.x86_64.rpm' \
    "the RPM is an attestation subject"

# The runbook's first step, the release model and the rebuild recipe name
# the scripts that do the work, so a packager is sent to the one copy
# rather than a prose restatement of it.
require_text "$PACKAGING" 'scripts/release-version.sh --check' \
    "the runbook's bump step is the --check script"
require_text "$PACKAGING" 'scripts/release-notes.sh' \
    "the release notes are the changelog entry that script cuts"
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
