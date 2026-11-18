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
# shellcheck source=tests/lib.sh
source "$REPO_ROOT/tests/lib.sh"
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
readonly ADRS="$REPO_ROOT/docs/adrs.md"
readonly CONTRIBUTING="$REPO_ROOT/CONTRIBUTING.md"
readonly RELEASE="$REPO_ROOT/.github/workflows/release.yml"
readonly CI="$REPO_ROOT/.github/workflows/ci.yml"
readonly MAN_WM="$REPO_ROOT/docs/man/buoy-wm.1.scd"
readonly MAN_CONFIG="$REPO_ROOT/docs/man/buoy-config.5.scd"

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
    "$FORMULA" "$INSTALL" "$CARGO" "$CHANGELOG" "$SECURITY" "$EXAMPLE" \
    "$ADRS" "$CONTRIBUTING" "$RELEASE" "$CI" "$MAN_WM" "$MAN_CONFIG"; do
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

# section_text <file> <heading>: everything under the exact heading line
# up to the next heading outside a fence, as one line with whitespace
# runs squeezed, so a phrase a line break splits is still one phrase.
section_text() {
    HEADING=$2 awk '
        $0 == ENVIRON["HEADING"] { inside = 1; next }
        !inside { next }
        /^```/ { fence = !fence }
        !fence && /^#+ / { exit }
        { print }
    ' "$1" | tr -s ' \n\t' '   '
}

# block_line <block> <where> <line> <why>: the line occurs in the block
# exactly once, whole, so a commented-out copy, an indented one or a
# second one cannot stand in for it.
block_line() {
    local block=$1 where=$2 line=$3 why=$4 found
    found="$(printf '%s\n' "$block" | grep -cxF -- "$line" || true)"
    if [ "$found" -ne 1 ]; then
        fail "README.md: $where has $found whole lines '$line', not one ($why)"
    fi
}

# The README's pinned version is the one it ships with: the file travels
# in the tarball and the RPM, so it must name the release Cargo.toml
# builds. The RPM and .deb blocks each open with their own line, since a
# reader copies one block (each block's own line is required below), so
# every 'version=' line, indented or not, must be that version.
# The `|| true` keeps an absent line on the report: under `set -e` a failed
# grep in the substitution would otherwise end the run before the message
# is written.
cargo="$(grep -m1 '^version = ' "$CARGO" | cut -d'"' -f2 || true)"
if [ -z "$cargo" ]; then
    fail "Cargo.toml: no 'version = \"…\"' line to derive the README version from"
fi
version_lines="$(count_lines "$README" '^[[:space:]]*version=')"
# Whole-line match: a substring check would accept rc.10 against rc.1.
cargo_version_lines="$(grep -cxF -- "version=$cargo" "$README" || true)"
if [ "$version_lines" -eq 0 ]; then
    fail "README.md: no 'version=' line (the install blocks build the asset name from it)"
elif [ "$cargo_version_lines" -ne "$version_lines" ]; then
    fail "README.md: $((version_lines - cargo_version_lines)) of $version_lines 'version=' lines are not 'version=$cargo' (the README's pinned version must be Cargo.toml's)"
fi

# The toolchain floor is a contract, not prose: the number a packager
# reads in PACKAGING.md, the README and CONTRIBUTING.md must be the one
# the manifest enforces, so it is derived from Cargo.toml's sister
# field the same way (the msrv CI job and tests/rpm-spec.sh read the
# same line) and never spelled here. CONTRIBUTING.md is gated because
# its build sentence is the README's twin, and a gate on one copy
# invites the other to drift. The PACKAGING heading is counted exactly
# once because the glibc floor shares the section (Story 5.4), so it is
# extended, never duplicated. Only the double-quoted spelling is read; any
# other value is reported rather than turned into a needle.
msrv_status=0
msrv="$(read_msrv "$CARGO")" || msrv_status=$?
case $msrv_status in
    1) fail "Cargo.toml: no 'rust-version = \"…\"' line to derive the toolchain floor from" ;;
    2)
        fail "Cargo.toml: rust-version '$msrv' is not digits and dots (only the double-quoted spelling is read)"
        msrv=""
        ;;
esac
floor_lines="$(count_lines "$PACKAGING" '^## Toolchain floor$')"
if [ "$floor_lines" -ne 1 ]; then
    fail "docs/PACKAGING.md: expected exactly one '## Toolchain floor' heading, found $floor_lines"
fi
# The README and CONTRIBUTING sentence is counted exactly once (any
# version), so a stale second spelling, in prose or a code block,
# cannot pass beside the gated one.
readme_floor_lines="$(count_lines "$README" 'Rust [0-9]+(\.[0-9]+)+ or newer')"
if [ "$readme_floor_lines" -ne 1 ]; then
    fail "README.md: expected exactly one 'Rust <version> or newer' line, found $readme_floor_lines"
fi
contributing_floor_lines="$(count_lines "$CONTRIBUTING" 'Rust [0-9]+(\.[0-9]+)+ or newer')"
if [ "$contributing_floor_lines" -ne 1 ]; then
    fail "CONTRIBUTING.md: expected exactly one 'Rust <version> or newer' line, found $contributing_floor_lines"
fi
# Only with a floor in hand: an empty number would turn each needle
# into a prefix that any value satisfies. The PACKAGING needles end at
# the colon and the closing backtick for the same reason: without them
# `1.93` is a prefix of `1.93.1` and `1.930`.
if [ -n "$msrv" ]; then
    require_text "$PACKAGING" "\`rust-version\` in \`Cargo.toml\` is $msrv:" \
        "the floor PACKAGING.md states must be the manifest's"
    require_text "$PACKAGING" "\`BuildRequires: rust >= $msrv\`" \
        "the spec line PACKAGING.md quotes must carry the manifest's floor"
    require_text "$PACKAGING" "\`Build-Depends: rustc (>= $msrv)\`" \
        "the debian/control relation PACKAGING.md quotes must carry the manifest's floor"
    require_text "$README" "Rust $msrv or newer" \
        "the build requirement the README states must be the manifest's floor"
    require_text "$CONTRIBUTING" "Rust $msrv or newer" \
        "the build requirement CONTRIBUTING.md states must be the manifest's floor"
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
# While an `[Unreleased]` section exists, its compare link starts at
# the release Cargo.toml builds, derived the same way; otherwise a bump
# that promoted the section but kept the link would point at the tag
# before last forever.
if grep -qxF -- '## [Unreleased]' "$CHANGELOG"; then
    unreleased="[Unreleased]: https://github.com/nkbooth/buoy-wm/compare/v$cargo...HEAD"
    if ! grep -qxF -- "$unreleased" "$CHANGELOG"; then
        fail "CHANGELOG.md: no line '$unreleased' (the [Unreleased] compare link must start at Cargo.toml's release)"
    fi
fi

# Asset names are derived from the spec (Source1 is the vendor tarball
# with the tag's hyphen) and from release.yml (one RPM per Fedora leg
# of the packages matrix, below).
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

# PACKAGING.md describes what release.yml builds, so its facts are read
# from the workflow: one RPM per Fedora target of the packages matrix,
# the container the tarball is built in (named without its registry
# prefix, as the prose does) and the pinned rustup toolchain. Each is
# required in backticks or with the asset suffix, so a bare number in
# another sentence cannot stand in for it. The retired runner label is
# forbidden in both documents that used to name it.
packages_job="$(job_body "$RELEASE" packages || true)"
# Split on whitespace, as tests/release-workflow.sh reads the same
# lines, so a target that gate accepts is never dropped here.
fedora_releases="$(printf '%s\n' "$packages_job" | awk '$1 == "-" && $2 == "target:" && $3 ~ /^fedora-[0-9]+$/ {
        sub(/^fedora-/, "", $3); print $3
    }')"
if [ -z "$fedora_releases" ]; then
    fail "release.yml: no '- target: fedora-NN' in the packages job (PACKAGING.md's RPM asset names are derived from them)"
fi
for fedora in $fedora_releases; do
    require_text "$PACKAGING" "-1.fc$fedora.x86_64.rpm" \
        "the release model must name every RPM asset release.yml builds"
done
# One .deb per Ubuntu target, named with the build container's
# os-release label (scripts/build-deb.sh), read the same way.
ubuntu_releases="$(printf '%s\n' "$packages_job" | awk '$1 == "-" && $2 == "target:" && $3 ~ /^ubuntu-[0-9]+\.[0-9]+$/ {
        sub(/^ubuntu-/, "", $3); print $3
    }')"
if [ -z "$ubuntu_releases" ]; then
    fail "release.yml: no '- target: ubuntu-X.Y' in the packages job (PACKAGING.md's .deb asset names are derived from them)"
fi
for ubuntu in $ubuntu_releases; do
    require_text "$PACKAGING" "-1.ubuntu${ubuntu}_amd64.deb" \
        "the release model must name every .deb asset release.yml builds"
done
# The README's platforms table names each release's own asset, read from
# the same matrix, in the Download cell of exactly one row: the cell, not
# a mention anywhere in the file.
download_cells="$(awk -F'|' '
    function trim(s) { gsub(/^[ \t]+|[ \t]+$/, "", s); return s }
    /^## Supported platforms$/ { in_section = 1; next }
    /^## / { in_section = 0 }
    in_section && /^\|/ { print trim($(NF - 1)) }
' "$README")"
# shellcheck disable=SC2016 # the cell names the reader's ${version}, not expanded here
for fedora in $fedora_releases; do
    cell='`buoy-wm-${version}-1.fc'"$fedora"'.x86_64.rpm`'
    found="$(printf '%s\n' "$download_cells" | grep -cxF -- "$cell" || true)"
    if [ "$found" -ne 1 ]; then
        fail "README.md: $found Supported platforms Download cells are $cell, not one (release.yml builds that RPM)"
    fi
done
# shellcheck disable=SC2016 # the cell names the reader's ${version}, not expanded here
for ubuntu in $ubuntu_releases; do
    cell='`buoy-wm_${version}-1.ubuntu'"$ubuntu"'_amd64.deb`'
    found="$(printf '%s\n' "$download_cells" | grep -cxF -- "$cell" || true)"
    if [ "$found" -ne 1 ]; then
        fail "README.md: $found Supported platforms Download cells are $cell, not one (release.yml builds that .deb)"
    fi
done
# Each install block derives the file name from the running system
# rather than naming one release, so a reader on any supported release
# downloads the package built for it, and installs it only once its
# checksum passed: a pasted block has no `set -e`, so a 404 or a mismatch
# would otherwise go on to install whatever arrived. Each line is held
# whole and once inside its own block, the one a reader copies.
# shellcheck disable=SC2016 # literal shell the reader runs, not expanded here
{
    rpm_line='rpm="buoy-wm-${version}-1.fc$(rpm -E %fedora).x86_64.rpm"'
    deb_line='deb="buoy-wm_${version}-1.ubuntu$(. /etc/os-release && echo "$VERSION_ID")_amd64.deb"'
    base_line='base="https://github.com/nkbooth/buoy-wm/releases/download/v${version}"'
}
rpm_block="$(fenced_block "$README" '### Fedora 44 and 45: the RPM' 1)"
deb_block="$(fenced_block "$README" '### Ubuntu 26.04 and 26.10: river from a PPA, then the .deb' 2)"
tarball_block="$(fenced_block "$README" '### From a release' 1)"
# shellcheck disable=SC2016 # literal shell the reader runs, not expanded here
{
    block_line "$rpm_block" "the RPM block" "version=$cargo" "the asset name is built from it"
    block_line "$rpm_block" "the RPM block" "$rpm_line" "the Fedora block derives its RPM from the running release"
    block_line "$rpm_block" "the RPM block" "$base_line" "the release the version names"
    block_line "$rpm_block" "the RPM block" 'curl --proto '"'"'=https'"'"' --tlsv1.2 -fsSL -o "$workdir/$rpm" "$base/$rpm"' "the package download"
    block_line "$rpm_block" "the RPM block" '(cd "$workdir" && sha256sum -c "$rpm.sha256") &&' "install only after the checksum passes"
    block_line "$rpm_block" "the RPM block" 'sudo dnf install "$workdir/$rpm"' "installs the file just checked"
    block_line "$deb_block" "the .deb block" "version=$cargo" "the asset name is built from it"
    block_line "$deb_block" "the .deb block" "$deb_line" "the Ubuntu block derives its .deb from the running release"
    block_line "$deb_block" "the .deb block" "$base_line" "the release the version names"
    block_line "$deb_block" "the .deb block" 'curl --proto '"'"'=https'"'"' --tlsv1.2 -fsSL -o "$workdir/$deb" "$base/$deb"' "the package download"
    block_line "$deb_block" "the .deb block" '(cd "$workdir" && sha256sum -c "$deb.sha256") &&' "install only after the checksum passes"
    block_line "$deb_block" "the .deb block" 'sudo apt install "$workdir/$deb"' "installs the file just checked"
    block_line "$tarball_block" "the tarball block" '(cd "$workdir" && sha256sum -c "$archive.sha256") &&' "extract only after the checksum passes"
    block_line "$tarball_block" "the tarball block" 'tar -xzf "$workdir/$archive" -C "$workdir" &&' "install only what extracted"
}
# A clean Fedora or Ubuntu has no GitHub CLI, so an uncommented `gh`
# line in a block is `command not found` for every reader.
gh_lines="$(count_lines "$README" '^gh ')"
if [ "$gh_lines" -ne 0 ]; then
    fail "README.md: $gh_lines uncommented 'gh ...' lines in an install block (a clean machine has no gh; leave it commented for the reader who does)"
fi
# Every supported release has its package now, so nothing is promised
# for a later one, also not across a line break.
readme_joined="$(tr -s ' \n\t' '   ' <"$README")"
for promise in 'arrives with the next release' "no \`.deb\` yet"; do
    if printf '%s\n' "$readme_joined" | grep -qF -- "$promise"; then
        fail "README.md: still says '$promise' (every tier-1 release has its own asset; the Ubuntu install path is the .deb)"
    fi
done
# The .deb exists now, so the runbook describes it in the present tense
# and names no story it is waiting for: the version table's "(Story
# 5.5)", the jobs paragraph's "arrives with Story 5.5" and the toolchain
# paragraph's "once Story 5.5 lands", which wraps across two lines and
# so is caught only by the shorter needle.
forbid_text "$PACKAGING" 'Story 5.5' \
    "the .deb is built by dpkg-buildpackage in the Ubuntu containers and debian/changelog is in the tree"
build_job="$(job_body "$RELEASE" build || true)"
build_image="$(printf '%s\n' "$build_job" | sed -n 's/^[[:space:]]*image: //p' | head -n1)"
if [ -z "$build_image" ]; then
    fail "release.yml: no container 'image:' in the build job (PACKAGING.md names the tarball's build image from it)"
else
    require_text "$PACKAGING" "\`${build_image#docker.io/library/}\`" \
        "the container the tarball is built in, which sets its glibc floor"
fi
toolchain="$(sed -n 's/^[[:space:]]*toolchain:[[:space:]]*//p' "$RELEASE" | head -n1)"
if [ -z "$toolchain" ]; then
    fail "release.yml: no 'toolchain:' line (PACKAGING.md names the tarball's pinned toolchain from it)"
else
    require_text "$PACKAGING" "\`$toolchain\`" \
        "the pinned rustup toolchain the tarball is built with"
fi
forbid_text "$PACKAGING" 'ubuntu-22.04' \
    "the tarball is built in a container, not on the retiring runner (ADR-015)"
forbid_text "$SECURITY" 'ubuntu-22.04' \
    "the tarball is built in a container, not on the retiring runner (ADR-015)"

# The login entry's Exec= line, quoted verbatim, so the README's account
# of what GDM runs cannot drift from the file tests/buoy-desktop.sh gates.
exec_line="$(grep -m1 '^Exec=' "$DESKTOP" || true)"
if [ -z "$exec_line" ]; then
    fail "packaging/buoy.desktop: no Exec= line"
fi
require_text "$README" "$exec_line" \
    "the packaged-session section must quote the entry's Exec= line"

# What a clean machine following the README verbatim tripped on (Story
# 5.7). The status-bar example is the packaged session's: its `exec`
# names the directory the login entry runs the supervisor from, with no
# `output` key, because the connector names of the old example matched
# no real machine and a bar pinned to a missing output is never drawn.
packaged_libexec="${exec_line##* }"
packaged_libexec="${packaged_libexec%/*}"
bar_block="$(fenced_block "$README" '## Status bar' 1)"
bar_exec="$(printf '%s\n' "$bar_block" | grep -F '"exec":' | sed 's/^[[:space:]]*//' || true)"
if [ "$bar_exec" != "\"exec\": \"$packaged_libexec/buoy-status-bar 0\"," ]; then
    fail "README.md: the Status bar example's exec lines are '$bar_exec', not the one packaged binary '$packaged_libexec/buoy-status-bar 0'"
fi
if printf '%s\n' "$bar_block" | grep -qF '"output":'; then
    fail "README.md: the Status bar example pins an \"output\" (a connector name that matches no clean machine leaves the bar undrawn)"
fi
# shellcheck disable=SC2088 # the literal text a reader would copy, not a path here
forbid_text "$README" '"exec": "~/.local/lib/buoy-wm/buoy-status-bar' \
    "a bar example that runs the tarball's binary under the packaged session"
# Fedora's waybar is not pulled by the RPM, and "First login" enables its
# unit, which does not exist until it is installed.
block_line "$rpm_block" "the RPM block" 'sudo dnf install waybar' \
    "installs the bar the packaged session enables"
# The packaged session's PATH is the user manager's (/usr/local/bin:/usr/bin
# on Fedora): no shell profile runs, so ~/.local/bin and ~/.cargo/bin are
# not on it and a zellij there is never found. The binary is checked
# against upstream's checksum and installed by `install`, owned by root:
# a `sudo tar` into the PATH keeps the archive's owner, uid 1001.
# shellcheck disable=SC2088 # the literal text a reader would copy, not a path here
forbid_text "$README" '~/.local/bin/zellij' \
    "the packaged session cannot find a zellij there"
forbid_text "$README" 'sudo tar' \
    "tar run as root keeps the archive's owner"
zellij_block="$(fenced_block "$README" '### Session multiplexer' 1)"
# shellcheck disable=SC2016 # literal shell the reader runs, not expanded here
{
    block_line "$zellij_block" "the zellij block" 'zellij_asset="zellij-x86_64-unknown-linux-musl"' "the asset the session runs"
    block_line "$zellij_block" "the zellij block" '(cd "$zdir" && awk '"'"'{ print $1 "  zellij" }'"'"' "$zellij_asset.sha256sum" | sha256sum -c) &&' "install only after upstream's checksum passes"
    block_line "$zellij_block" "the zellij block" 'sudo install -m 755 "$zdir/zellij" /usr/local/bin/zellij' "zellij goes where the packaged session's PATH finds it, owned by root"
}
# The pin is one bare version, and the runbook moves it to upstream's
# current release before each tag, so the tier-1 check verifies the
# zellij the README names and nothing else notices a newer one.
zellij_pins="$(printf '%s\n' "$zellij_block" | grep -cE '^zellij_version=[0-9]+\.[0-9]+\.[0-9]+$' || true)"
if [ "$zellij_pins" -ne 1 ]; then
    fail "README.md: the zellij block has $zellij_pins 'zellij_version=X.Y.Z' lines, not one (the asset URL and the runbook's bump step read it)"
fi
require_text "$PACKAGING" "\`zellij_version\`" \
    "the runbook bumps the README's zellij pin before each tier-1 check"
# The tier-1 check's recipe is in the tree, so the runbook names where,
# not a story record outside it.
# shellcheck disable=SC2016 # the backticks are the Markdown the runbook carries
require_text "$PACKAGING" '`scripts/clean-vm/`' \
    "the runbook's tier-1 check names the harness that runs it"
forbid_text "$PACKAGING" "Story 5.7's harness" \
    "the harness is committed; a story record is not where it lives"
# The Ubuntu flow writes the config, so it carries the step that clears
# the trust warning a new file raises; the trust-check section saying it
# is not enough.
if ! section_text "$README" '### Ubuntu 26.04 and 26.10: river from a PPA, then the .deb' | grep -qF -- "and \`chmod 600\` it"; then
    fail "README.md: the Ubuntu install section no longer says to 'chmod 600' the config it has the reader write"
fi
# OpenSSL's vendored build in `cargo install zellij` runs perl's Configure,
# which needs core modules a clean Fedora does not have.
require_text "$README" 'sudo dnf install cargo perl-core' \
    "cargo install zellij needs perl's core modules on Fedora"
# Fedora's and Ubuntu's umask makes a new file 664, which the trust check
# reports, and a clean session has no notification daemon.
forbid_text "$README" "The default \`644\` is fine" \
    "a new file is 664 on Fedora and Ubuntu"
require_text "$README" 'chmod 600 ~/.config/buoy/config.toml' \
    "the step that clears the trust warning on a new file"
require_text "$README" 'notification daemon' \
    "desktop notifications need one, and a clean session has none"

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
# The formula's url is the tag archive and the archive contains the
# formula, so a real checksum here would change the archive it describes
# (ADR-011). The tap copy carries the real one; this copy keeps zeros.
formula_sha_lines="$(count_lines "$FORMULA" '^[[:space:]]*sha256\b')"
formula_placeholders="$(count_lines "$FORMULA" '^[[:space:]]*sha256 "0{64}"$')"
if [ "$formula_sha_lines" -ne 1 ] || [ "$formula_placeholders" -ne 1 ]; then
    fail "packaging/homebrew/buoy-wm.rb: has $formula_sha_lines sha256 lines and $formula_placeholders all-zero placeholders, not exactly one of each (the real checksum lives only in the tap copy, PACKAGING runbook step 5)"
fi
# Ruby's last sha256 call wins, and one can sit mid-line
# (`license "x"; sha256 "…"`, `stable { sha256 "…" }`), so any 64-hex
# run other than the zeros is a real checksum.
formula_real_hashes="$(grep -oE '[0-9a-fA-F]{64}' "$FORMULA" | grep -cvE '^0{64}$' || true)"
if [ "$formula_real_hashes" -ne 0 ]; then
    fail "packaging/homebrew/buoy-wm.rb: carries $formula_real_hashes 64-hex value(s) other than the all-zero placeholder (the real checksum lives only in the tap copy, PACKAGING runbook step 5)"
fi

# The README's "Supported platforms" table is what the CI and release
# matrices are derived from (ADR-013: the list lives in the README), so
# its heading and the first cell of each supported row are a contract,
# not prose: a job that greps for a release name must find it spelled
# this way, inside that section and not somewhere else in the file. The
# unsupported row is required too, so the README says what it does not
# support. The two forbids after it are drifts a review caught —
# PACKAGING.md's old verdict conflated archive inclusion (blocked by
# RPL 1.5 against the DFSG) with release-asset and PPA distribution
# (not blocked, ADR-011), and ADR-013's rationale listed zellij among
# what Fedora 44 and 45 ship when `dnf repoquery zellij` is empty on
# both (the 4.8 review's deferral).
heading_lines="$(count_lines "$README" '^## Supported platforms$')"
if [ "$heading_lines" -ne 1 ]; then
    fail "README.md: expected exactly one '## Supported platforms' heading, found $heading_lines"
fi
# The rows, read by tests/lib.sh's platform_rows, the reader
# tests/release-workflow.sh and tests/ci-workflow.sh derive the matrices
# from, so the names are read, never spelled here. Each name exactly
# once (a duplicated row would put a target in a matrix twice), at
# least one tier-1 row (the matrices are derived from them) and at least
# one unsupported row (the README says what it does not support).
platform_rows "$README" >"$tmp/rows" || true
cut -f1 "$tmp/rows" | LC_ALL=C sort | uniq -d >"$tmp/duplicates"
while IFS= read -r name; do
    fail "README.md: the Supported platforms section has the row '$name' more than once"
done <"$tmp/duplicates"
if ! cut -f2 "$tmp/rows" | grep -qxF 1; then
    fail "README.md: the Supported platforms section has no tier-1 row (the matrices are derived from them)"
fi
if ! cut -f2 "$tmp/rows" | grep -qxF unsupported; then
    fail "README.md: the Supported platforms section has no 'unsupported' row (the README says what it does not support)"
fi
# The stable PPA's name is a prefix of the unstable one's, so a plain
# substring check could never fail while the unstable name is present.
if ! grep -qE 'ppa:tchavadar/river([^-]|$)' "$README"; then
    fail "README.md: does not mention 'ppa:tchavadar/river' (where Ubuntu 26.04 gets river 0.4+; no Ubuntu archive carries it)"
fi
require_text "$README" 'ppa:tchavadar/river-unstable' \
    "where Ubuntu 26.10 gets river 0.4+; the stable PPA has no 26.10 build"
require_text "$README" 'bugs.launchpad.net/ubuntu/+bug/2142921' \
    "the open request to put river in the Ubuntu archive"
# The two config lines the platforms section and the Ubuntu steps send
# the reader to: the off switch, derived from the file that owns the
# key, and the tmux spelling of the pinned-terminal command.
if ! grep -qE '^pinned_terminals = (true|false)$' "$EXAMPLE"; then
    fail "docs/config.example.toml: no 'pinned_terminals = true|false' line (the README's off switch is derived from it)"
fi
require_text "$README" 'pinned_terminals = false' \
    "the way to run without any multiplexer, which the platforms section relies on"
# The tmux pinned_terminal_args line the Ubuntu steps point at, as the
# one whole line a user copies: exactly once, so a second, bare copy
# cannot hide behind the gated one, and directly under `[defaults]`
# (whole-line too), because a file holding only the bare key is
# rejected (`unknown field pinned_terminal_args`). The `|| true` keeps
# a zero count on the report rather than ending the run.
tmux_line='pinned_terminal_args = ["-a", "{app_id}", "tmux", "new-session", "-A", "-s", "{session}"]'
tmux_lines="$(grep -cxF -- "$tmux_line" "$README" || true)"
if [ "$tmux_lines" -ne 1 ]; then
    fail "README.md: expected exactly one line '$tmux_line' (the tmux pinned_terminal_args line the Ubuntu steps point at), found $tmux_lines"
elif ! grep -B1 -xF -- "$tmux_line" "$README" | grep -qxF -- '[defaults]'; then
    fail "README.md: the tmux pinned_terminal_args line is not directly under a '[defaults]' line (a file holding only the bare key is rejected)"
fi
# Two facts verified in containers on 2026-11-08 (Story 5.2). tmux 3.7
# and newer cannot re-attach a session whose name holds `.` or `:`, so
# a tag named that way loses its pinned terminal on the second login;
# the program message a user sees is the fact, not the prose around
# it. And Ubuntu 26.04's river 0.4.1 advertises no window capture
# source, so screen sharing there is monitor-only; that is a cell of
# the 26.04 row's river cell, read by platform_rows, so moving the
# words to another row or another column is red.
require_text "$README" "can't find pane" \
    "the tmux error a tag named with '.' or ':' produces on the second login"
ubuntu_2604_river="$(awk -F'\t' '$1 == "Ubuntu 26.04 LTS" { print $3 }' "$tmp/rows")"
if [ -z "$ubuntu_2604_river" ]; then
    fail "README.md: the Supported platforms section has no 'Ubuntu 26.04 LTS' row with a river cell"
elif ! printf '%s\n' "$ubuntu_2604_river" | grep -qF -- 'monitor-only'; then
    fail "README.md: the 'Ubuntu 26.04 LTS' row's river cell does not say 'monitor-only' (river 0.4.1 has no window capture source; screen sharing is monitor-only there)"
fi
# The RPM install heading, quoted by PACKAGING.md, derived from the
# README so a rename there is red here rather than a dangling quote.
rpm_heading="$(grep -m1 -E '^### Fedora .*: the RPM$' "$README" || true)"
if [ -z "$rpm_heading" ]; then
    fail "README.md: no '### Fedora …: the RPM' heading for PACKAGING.md to quote"
else
    require_text "$PACKAGING" "\"${rpm_heading#\#\#\# }\"" \
        "the README heading PACKAGING.md's install-by-URL sentence quotes"
fi
# Case-insensitive: the conflated verdict is wrong in any spelling.
if grep -qiF -- 'not viable' "$PACKAGING"; then
    fail "docs/PACKAGING.md: still says 'not viable' (archive inclusion is blocked, release-asset and PPA distribution are not (ADR-011))"
fi
forbid_text "$ADRS" 'ship river 0.5.0, zellij' \
    "Fedora 44 and 45 do not package zellij (the 4.8 review's deferral)"

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
# The release's one attestation names the tarball, the vendor tarball
# and each RPM, and the README's verify steps for the RPM and the
# tarball both point here. The names are the shapes SECURITY.md prints:
# `<version>` is the tag without its `v`, and the RPM carries `<dist>`
# rather than a dist tag because there is one RPM per Fedora release.
require_text "$SECURITY" 'gh attestation verify' \
    "the verify command the README points at"
require_text "$SECURITY" 'buoy-wm-<version>-vendor.tar.gz' \
    "the vendor tarball is an attestation subject"
require_text "$SECURITY" 'buoy-wm-<version>-1.<dist>.x86_64.rpm' \
    "the RPM is an attestation subject"
require_text "$SECURITY" 'buoy-wm_<version>-1.<series>_amd64.deb' \
    "the .deb is an attestation subject"

# The runbook's first step, the release model and the rebuild recipe name
# the scripts that do the work, so a packager is sent to the one copy
# rather than a prose restatement of it.
require_text "$PACKAGING" 'scripts/release-version.sh --check' \
    "the runbook's bump step is the --check script"
require_text "$PACKAGING" 'scripts/release-notes.sh' \
    "the release notes are the changelog entry that script cuts"
require_text "$PACKAGING" 'scripts/build-rpm.sh' \
    "the offline rebuild is the build script"
require_text "$PACKAGING" 'scripts/build-deb.sh' \
    "the .deb's offline rebuild is its build script"
# Every package is installed and run in a clean container before the
# release is published (NFR6); the runbook no longer asks for those
# installs by hand, because publish would not have run without them.
# shellcheck disable=SC2016 # a literal backticked job name
require_text "$PACKAGING" '`verify`' \
    "the release job that installs each package in a clean container"
require_text "$PACKAGING" 'scripts/check-installed.sh' \
    "the script verify runs against the installed binaries"
forbid_text "$PACKAGING" 'in a fresh container of its own release' \
    "the verify legs install every package; the runbook's by-hand container installs are gone"

# Every in-page link must land on a heading of the same file, by
# GitHub's slug rule: lowercased, every character but letters, digits,
# space, `-` and `_` dropped (backticks and punctuation included), each
# space turned into `-`, and `-1`, `-2` appended to repeats. A `#` line
# inside a fence is a comment in a code block, not a heading.
# dangling_anchors <file>: each `](#slug)` link with no such heading.
dangling_anchors() {
    awk '
        /^```/ { fence = !fence; next }
        fence { next }
        /^#+ / {
            slug = $0
            sub(/^#+ /, "", slug)
            slug = tolower(slug)
            gsub(/[^a-z0-9 _-]/, "", slug)
            gsub(/ /, "-", slug)
            if (slug in repeats) { repeats[slug]++; slug = slug "-" repeats[slug] } else repeats[slug] = 0
            anchors[slug] = 1
        }
        {
            line = $0
            while (match(line, /\]\(#[^)]*\)/)) {
                links[++n] = substr(line, RSTART + 3, RLENGTH - 4)
                line = substr(line, RSTART + RLENGTH)
            }
        }
        END { for (i = 1; i <= n; i++) if (!(links[i] in anchors)) print links[i] }
    ' "$1"
}
for doc in "$README" "$CONTRIBUTING"; do
    while IFS= read -r slug; do
        fail "${doc#"$REPO_ROOT/"}: the link '](#$slug)' names no heading in the same file"
    done < <(dangling_anchors "$doc")
done

# The journal query the documents give is the one that works once the
# supervisor hands the WM's output to journald under its own name (Story
# 6.2): before that, GDM's stream carried the lines with neither the
# identifier nor the severity, so `--identifier=buoy-wm -p err` matched
# nothing. The README's troubleshooting section opens with it as a line
# a reader copies; the example, SECURITY.md and both man pages name it.
journal_query='journalctl --user -b --identifier=buoy-wm -p err'
troubleshooting_block="$(fenced_block "$README" '## Troubleshooting' 1)"
block_line "$troubleshooting_block" "the Troubleshooting section's first block" \
    "$journal_query" "the failures-only query, once the supervisor names the stream"
# shellcheck disable=SC2016 # a literal journalctl match a reader copies
require_text "$README" "journalctl --user -b _COMM=buoy-wm --grep '^<3>'" \
    "rc.3 and earlier log through the display manager's stream, where only _COMM finds the WM"
for doc in "$EXAMPLE" "$SECURITY" "$MAN_WM" "$MAN_CONFIG"; do
    require_text "$doc" "$journal_query" "the documented failures-only query"
done
# shellcheck disable=SC2016 # Markdown backticks, literal
forbid_text "$README" 'beside the WM'"'"'s `--identifier=buoy-wm` lines' \
    "the supervisor's own lines stay in the display manager's stream, under _COMM=buoy-session"
# A malformed config raises a notification and a journal line; nothing
# reads the WM's stderr in the packaged session.
forbid_text "$SECURITY" 'check stderr' \
    "the problem is a desktop notification plus a journal line"

# The coverage floor's exempt files are listed twice: as ci.yml's
# `--ignore-filename-regex` and, with a reason each, as the bullets of
# CONTRIBUTING.md's "The coverage floor". The regex is the one that runs,
# so the bullets are held to it both ways.

# floor_regex_paths <regex>: the paths an ignore regex of the form
# `prefix(a|b)suffix$|…` names, one per line; `!<part>` for a part that
# does not expand to a plain path, so the caller names it.
floor_regex_paths() {
    local regex=$1 part="" depth=0 ch i parts=()
    for ((i = 0; i < ${#regex}; i++)); do
        ch=${regex:i:1}
        case $ch in
            '(') depth=$((depth + 1)) ;;
            ')') depth=$((depth - 1)) ;;
        esac
        if [ "$ch" = '|' ] && [ "$depth" -eq 0 ]; then
            parts+=("$part")
            part=""
        else
            part+=$ch
        fi
    done
    parts+=("$part")
    local prefix group suffix alt alts path
    for part in "${parts[@]}"; do
        part=${part%'$'}
        part=${part//'\.'/.}
        if [[ $part == *'('*')'* ]]; then
            prefix=${part%%'('*}
            group=${part#*'('}
            suffix=${group#*')'}
            group=${group%%')'*}
            IFS='|' read -ra alts <<<"$group"
        else
            prefix=$part suffix="" alts=("")
        fi
        for alt in "${alts[@]}"; do
            path=$prefix$alt$suffix
            if [[ $path =~ ^[A-Za-z0-9_./-]+$ ]]; then
                printf '%s\n' "$path"
            else
                printf '!%s\n' "$part"
            fi
        done
    done
}

floor_step="$(step_with "$CI" '^[[:space:]]*- name: cargo llvm-cov \(floor below the composition roots\)$')"
floor_regex="$(printf '%s\n' "$floor_step" | sed -nE "s/.*--ignore-filename-regex '([^']+)'.*/\1/p")"
if [ -z "$floor_regex" ]; then
    fail "ci.yml: no '--ignore-filename-regex' in the step 'cargo llvm-cov (floor below the composition roots)'"
else
    # The bullet list under the heading, wrapped lines and blank lines
    # between items included (Markdown still renders one list), up to the
    # first unindented line that is not an item; only `….rs` paths count.
    # shellcheck disable=SC2016 # literal Markdown backticks around a path
    floor_bullets="$(awk '
        $0 == "### The coverage floor" { inside = 1; next }
        inside && /^#+ / { exit }
        inside && /^- / { list = 1 }
        list && /^$/ { next }
        list && !/^- / && !/^[ \t]/ { exit }
        list' "$CONTRIBUTING" | grep -oE '`[A-Za-z0-9_./-]+\.rs`' | tr -d '`' | sort -u || true)"
    if [ -z "$floor_bullets" ]; then
        fail "CONTRIBUTING.md: no '.rs' paths in the bullet list under '### The coverage floor'"
    fi
    floor_exempt="$(floor_regex_paths "$floor_regex")"
    # The regex matches anywhere in a path, so expanding it is not enough:
    # the tracked .rs files it really matches must be exactly the expansion
    # (an unanchored `src/main\.rs$` or an unescaped `.` reaches further).
    if ! tracked_rs="$(git -C "$REPO_ROOT" ls-files -- '*.rs')"; then
        fail "git ls-files failed; cannot hold the floor regex to the tracked files"
    else
        floor_matched="$(grep -E -- "$floor_regex" <<<"$tracked_rs" | sort -u || true)"
        if [ "$floor_matched" != "$(grep -v '^!' <<<"$floor_exempt" | sort -u)" ]; then
            fail "ci.yml: the floor regex matches these tracked files: $(tr '\n' ' ' <<<"$floor_matched")— not exactly the files it spells out"
        fi
    fi
    # The floor itself, and nothing that could skip it or swallow it.
    floor_percent="$(printf '%s\n' "$floor_step" | sed -nE 's/.*--fail-under-lines ([0-9]+).*/\1/p')"
    if [ -z "$floor_percent" ]; then
        fail "ci.yml: the floor step has no '--fail-under-lines <n>'"
    elif ! section_text "$CONTRIBUTING" '### The coverage floor' | grep -qF -- "**$floor_percent%**"; then
        fail "CONTRIBUTING.md: '### The coverage floor' does not state the floor ci.yml enforces (**$floor_percent%**)"
    fi
    if printf '%s\n' "$floor_step" | grep -qE '^[[:space:]]*(if|continue-on-error):'; then
        fail "ci.yml: the floor step has an 'if:' or 'continue-on-error:' that could skip it or swallow its failure"
    fi
    while IFS= read -r path; do
        case $path in
            '!'*) fail "ci.yml: the floor's ignore regex part '${path#!}' does not expand to a plain path" ;;
            *)
                if ! grep -qxF -- "$path" <<<"$floor_bullets"; then
                    fail "CONTRIBUTING.md: '### The coverage floor' does not list '$path', which ci.yml's floor exempts"
                fi
                ;;
        esac
    done <<<"$floor_exempt"
    while IFS= read -r path; do
        [ -n "$path" ] || continue
        if ! grep -qxF -- "$path" <<<"$floor_exempt"; then
            fail "CONTRIBUTING.md: '### The coverage floor' lists '$path' as exempt, but ci.yml's floor gates it"
        fi
    done <<<"$floor_bullets"
fi

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
if ! tail -n1 "$tmp/out" | grep -qF 'First login'; then
    fail "scripts/install.sh: last line does not point at the README's 'First login' section, got: $(tail -n1 "$tmp/out")"
fi

if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
fi
exit "$status"
