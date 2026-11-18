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
# Asserts that the man pages build and agree with the files that own
# their facts. scripts/build-man.sh is the one build the RPM and the .deb
# run, so it is run here: every page renders under groff with no
# warning, and the script fails (never skips) on no scdoc, no pages or a
# page scdoc rejects. buoy-wm(1)'s FILES section names every path the
# spec's %files installs, and its ENVIRONMENT section every variable the
# crates' non-test sources read by literal name; both sets are derived,
# so a path or a variable added to the code without the page is red
# here. Its OPTIONS section is held both ways to the flag literals
# buoy-wm's main.rs and buoy-common's cli.rs compare against.
# buoy-config(5)'s names are held to the code by the Rust tests in
# wm/src/config (docs_drift.rs and keysym.rs), which can ask serde.
#
# Blind spots, kept on purpose: an `env::var(NAME)` whose argument is not
# a string literal is not seen, and a crate's test module is recognised
# only in the conventional `#[cfg(test)]` + `mod … {` form (further
# attributes between the two allowed). OPTIONS reads only main.rs and
# cli.rs, the two files that hold buoy-wm's argv checks.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
# shellcheck source=tests/lib.sh
source "$REPO_ROOT/tests/lib.sh"
readonly BUILD="$REPO_ROOT/scripts/build-man.sh"
readonly SPEC="$REPO_ROOT/packaging/rpm/buoy-wm.spec"
readonly MAN_WM="$REPO_ROOT/docs/man/buoy-wm.1.scd"

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

for tool in scdoc groff; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        fail "$tool is not installed (the hygiene job installs scdoc and groff-base)"
    fi
done
for file in "$BUILD" "$SPEC" "$MAN_WM"; do
    if [ ! -r "$file" ]; then
        fail "${file#"$REPO_ROOT/"}: missing or unreadable"
    fi
done
if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
    exit "$status"
fi

# The build: one page per docs/man/*.scd, named without the suffix, each
# accepted by groff with tables and every warning on.
if ! bash "$BUILD" "$tmp/man" 2>"$tmp/build.err"; then
    fail "scripts/build-man.sh: failed on the tree's pages: $(cat "$tmp/build.err")"
fi
for source in "$REPO_ROOT"/docs/man/*.scd; do
    page="${source##*/}"
    page="${page%.scd}"
    if [ ! -s "$tmp/man/$page" ]; then
        fail "scripts/build-man.sh: did not write $page from docs/man/$page.scd"
        continue
    fi
    if ! groff -t -man -ww -z -Tutf8 "$tmp/man/$page" 2>"$tmp/groff.err" || [ -s "$tmp/groff.err" ]; then
        fail "docs/man/$page.scd: groff reports: $(cat "$tmp/groff.err")"
    fi
done

# The three refusals, each against a fixture tree holding a copy of the
# script, since it finds its pages beside itself. No scdoc on PATH must
# be an error naming the package: a skip would ship a package with no
# pages and a green build.
# fixture <name> <page-or-empty>: a tree with the script and one page.
fixture() {
    mkdir -p "$tmp/$1/scripts" "$tmp/$1/docs/man"
    cp "$BUILD" "$tmp/$1/scripts/build-man.sh"
    if [ -n "$2" ]; then
        printf '%s' "$2" >"$tmp/$1/docs/man/broken.7.scd"
    fi
}
# expect_refusal <case> <needle> [env...]: the fixture's build fails and
# says <needle> on stderr.
expect_refusal() {
    local name=$1 needle=$2
    shift 2
    if env "$@" bash "$tmp/$name/scripts/build-man.sh" "$tmp/$name/out" >/dev/null 2>"$tmp/$name.err"; then
        fail "scripts/build-man.sh: exited 0 with $name"
    elif ! grep -qF -- "$needle" "$tmp/$name.err"; then
        fail "scripts/build-man.sh: with $name, stderr does not say '$needle': $(cat "$tmp/$name.err")"
    fi
}
fixture no-pages ''
expect_refusal no-pages 'no docs/man/*.scd'
fixture bad-page 'not a title line'
expect_refusal bad-page 'broken.7.scd'
# A PATH with every tool the script needs but scdoc.
fixture no-scdoc ''
mkdir -p "$tmp/no-scdoc-bin"
for tool in bash mkdir dirname; do
    ln -s "$(command -v "$tool")" "$tmp/no-scdoc-bin/$tool"
done
# The needle is the script's own words: the fixture's directory name
# holds "scdoc" too, so any error that prints a path would match that.
expect_refusal no-scdoc 'scdoc is not installed' PATH="$tmp/no-scdoc-bin"

# scd_section <file> <title>: the lines under `# <title>` up to the next
# top-level heading.
scd_section() {
    TITLE="# $2" awk '
        $0 == ENVIRON["TITLE"] { on = 1; next }
        on && /^# / { exit }
        on' "$1"
}

# FILES: every path %files installs, its macros expanded: the lines that
# start with a macro, so %license, %doc and %dir (not paths a user looks
# up) are not read. A man page glob's `*` (brp-compress picks the
# suffix) is dropped.
scd_section "$MAN_WM" FILES >"$tmp/files"
if [ ! -s "$tmp/files" ]; then
    fail "docs/man/buoy-wm.1.scd: no FILES section"
fi
awk '/^%files$/ { on = 1; next } /^%changelog$/ { on = 0 } on && /^%[{]/' "$SPEC" >"$tmp/spec-files"
if [ ! -s "$tmp/spec-files" ]; then
    fail "packaging/rpm/buoy-wm.spec: no %files paths to derive FILES from"
fi
while IFS= read -r line; do
    if ! path="$(expand_spec_path "$line")"; then
        fail "packaging/rpm/buoy-wm.spec: %files path '$line' uses a macro this test does not map"
        continue
    fi
    path="${path%\*}"
    if ! grep -qF -- "$path" "$tmp/files"; then
        fail "docs/man/buoy-wm.1.scd: FILES does not name $path (the spec installs it)"
    fi
done <"$tmp/spec-files"

# Every page the build renders is one the RPM ships (and, through the
# spec's %install, the .deb: tests/deb-package.sh derives its .install
# from that), so a page added to docs/man/ is red here until it is
# packaged. The %files line is the man section's directory and the
# page's name, globbed for brp-compress's suffix.
for source in "$REPO_ROOT"/docs/man/*.scd; do
    page="${source##*/}"
    page="${page%.scd}"
    want="%{_mandir}/man${page##*.}/$page*"
    if ! grep -qxF -- "$want" "$tmp/spec-files"; then
        fail "packaging/rpm/buoy-wm.spec: %files has no '$want' (docs/man/$page.scd is built but not packaged)"
    fi
done

# non_test_source <file>: a Rust source file up to its test module, by
# convention the last item in the file, written on one line or with the
# attribute on the line before.
non_test_source() {
    awk '
        /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*(pub(\([a-z]+\))? )?mod [a-z_]+ \{/ { exit }
        /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*$/ { pending = 1; next }
        pending && /^[[:space:]]*#\[/ { next }
        pending && /^[[:space:]]*(pub(\([a-z]+\))? )?mod [a-z_]+ \{/ { exit }
        { pending = 0; print }' "$1"
}

# OPTIONS: the flag-shaped string literals ("-x", "--flag", digits and
# underscores included) in buoy-wm's main.rs and the shared cli.rs,
# outside their test modules and `//` comments, must be exactly the *-…*
# terms opening a line of the section. Both directions, so an
# undocumented flag and a documented flag that no longer exists are both
# red. Any such literal counts, compared with argv or not: a false red
# names the line to look at, a false green would hide a flag.
scd_section "$MAN_WM" OPTIONS >"$tmp/options"
if [ ! -s "$tmp/options" ]; then
    fail "docs/man/buoy-wm.1.scd: no OPTIONS section"
fi
: >"$tmp/flags"
for source in "$REPO_ROOT/wm/src/main.rs" "$REPO_ROOT/buoy-common/src/cli.rs"; do
    if [ ! -r "$source" ]; then
        fail "${source#"$REPO_ROOT/"}: missing or unreadable (the OPTIONS set is derived from it)"
        continue
    fi
    non_test_source "$source" | sed -E 's@//.*$@@' |
        { grep -oE '"--?[A-Za-z0-9][A-Za-z0-9_-]*"' || true; } | tr -d '"' >>"$tmp/flags"
done
# One collation for sort and comm, whatever the runner's locale.
LC_ALL=C sort -u "$tmp/flags" -o "$tmp/flags"
if [ ! -s "$tmp/flags" ]; then
    fail "no \"--flag\" literals found in wm/src/main.rs or buoy-common/src/cli.rs (the OPTIONS set is derived from them)"
fi
{ grep -oE '^\*--?[A-Za-z0-9][A-Za-z0-9_-]*\*' "$tmp/options" || true; } | tr -d '*' |
    LC_ALL=C sort -u >"$tmp/documented-flags"
while IFS= read -r flag; do
    fail "docs/man/buoy-wm.1.scd: OPTIONS does not document *$flag* (the binary reads it)"
done < <(LC_ALL=C comm -23 "$tmp/flags" "$tmp/documented-flags")
while IFS= read -r flag; do
    fail "docs/man/buoy-wm.1.scd: OPTIONS documents *$flag*, which the binary does not read"
done < <(LC_ALL=C comm -13 "$tmp/flags" "$tmp/documented-flags")

# ENVIRONMENT: every variable a crate's non-test source reads by literal
# name, required as a bold term. Vendored crates are not ours, so a
# `vendor/` tree (cargo vendor, the package builds) is skipped.
scd_section "$MAN_WM" ENVIRONMENT >"$tmp/environment"
if [ ! -s "$tmp/environment" ]; then
    fail "docs/man/buoy-wm.1.scd: no ENVIRONMENT section"
fi
: >"$tmp/variables"
while IFS= read -r -d '' source; do
    non_test_source "$source" |
        grep -oE 'env::var(_os)?\("[A-Za-z_][A-Za-z0-9_]*"\)' |
        sed -E 's/.*\("(.*)"\)/\1/' >>"$tmp/variables" || true
done < <(find "$REPO_ROOT" -mindepth 1 \( -name '.*' -o -name target -o -name vendor \) -prune -o -path '*/src/*.rs' -type f -print0)
sort -u "$tmp/variables" -o "$tmp/variables"
if [ ! -s "$tmp/variables" ]; then
    fail "no env::var(\"…\") reads found in */src (the ENVIRONMENT set is derived from them)"
fi
while IFS= read -r variable; do
    if ! grep -qF -- "*$variable*" "$tmp/environment"; then
        fail "docs/man/buoy-wm.1.scd: ENVIRONMENT does not name *$variable* (a crate reads it)"
    fi
done <"$tmp/variables"

if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
fi
exit "$status"
