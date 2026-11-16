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
    "$ADRS" "$CONTRIBUTING" "$RELEASE"; do
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
# builds. The RPM and .deb blocks each open with their own line, since a
# reader copies one block, so every 'version=' line must be that version.
# The `|| true` keeps an absent line on the report: under `set -e` a failed
# grep in the substitution would otherwise end the run before the message
# is written.
cargo="$(grep -m1 '^version = ' "$CARGO" | cut -d'"' -f2 || true)"
if [ -z "$cargo" ]; then
    fail "Cargo.toml: no 'version = \"…\"' line to derive the README version from"
fi
version_lines="$(count_lines "$README" '^version=')"
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
# the same matrix, and its install blocks derive the file name from the
# running system rather than naming one release, so a reader on any
# supported release downloads the package built for it. Whole lines,
# because each is the line a reader copies.
for fedora in $fedora_releases; do
    require_text "$README" "-1.fc$fedora.x86_64.rpm" \
        "the platforms table must name every RPM asset release.yml builds"
done
for ubuntu in $ubuntu_releases; do
    require_text "$README" "-1.ubuntu${ubuntu}_amd64.deb" \
        "the platforms table must name every .deb asset release.yml builds"
done
# shellcheck disable=SC2016 # literal shell the reader runs, not expanded here
rpm_line='rpm="buoy-wm-${version}-1.fc$(rpm -E %fedora).x86_64.rpm"'
# shellcheck disable=SC2016 # literal shell the reader runs, not expanded here
deb_line='deb="buoy-wm_${version}-1.ubuntu$(. /etc/os-release && echo "$VERSION_ID")_amd64.deb"'
if ! grep -qxF -- "$rpm_line" "$README"; then
    fail "README.md: no whole line '$rpm_line' (the Fedora block derives its RPM from the running release)"
fi
if ! grep -qxF -- "$deb_line" "$README"; then
    fail "README.md: no whole line '$deb_line' (the Ubuntu block derives its .deb from the running release)"
fi
# Every supported release has its package now, so nothing is promised
# for a later one.
forbid_text "$README" 'arrives with the next release' \
    "every tier-1 release has its own asset"
forbid_text "$README" "no \`.deb\` yet" \
    "the Ubuntu install path is the .deb"
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
require_text "$README" "\"exec\": \"$packaged_libexec/buoy-status-bar 0\"" \
    "the status-bar example must run the packaged binary"
# Fedora's waybar is not pulled by the RPM, and "Running it" enables its
# unit, which does not exist until it is installed.
require_text "$README" 'sudo dnf install waybar' \
    "the RPM block installs the bar the packaged session enables"
# The packaged session's PATH is the user manager's (/usr/local/bin:/usr/bin
# on Fedora): no shell profile runs, so ~/.local/bin and ~/.cargo/bin are
# not on it and a zellij there is never found.
# shellcheck disable=SC2088 # the literal text a reader would copy, not a path here
forbid_text "$README" '~/.local/bin/zellij' \
    "the packaged session cannot find a zellij there"
require_text "$README" 'sudo tar -xz -C /usr/local/bin zellij' \
    "zellij goes where the packaged session's PATH finds it"
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
