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
# The readers the gates share, each written once. Sourced, never
# executed (mode 100644; the shebang names the dialect for shellcheck
# and the shebang rule). Callers: tests/docs.sh, tests/rpm-spec.sh,
# tests/deb-package.sh, tests/man.sh, tests/release-workflow.sh,
# tests/ci-workflow.sh, and ci.yml's `msrv` and `shellcheck` steps.
# Every reader prints or returns a status and never reports, so each
# caller keeps its own message. grep, awk and sed only: the gates run on
# hosts and in images with no Rust toolchain, no yq and no YAML library.
#
# Blind spots of read_msrv, kept on purpose: only the first double-quoted
# `rust-version = "…"` line is read, so a `rust-version.workspace = true`
# member, a single-quoted value or a `[package]` floor that differs from
# the workspace's is not followed; a value that is not digits and dots is
# returned as status 2 rather than turned into a needle.

# read_msrv <Cargo.toml>: prints the toolchain floor. Status 1 with
# nothing printed when there is no `rust-version = "…"` line; status 2,
# printing the raw value, when the value is not digits and dots.
read_msrv() {
    local msrv
    msrv="$(grep -m1 '^rust-version = ' "$1" 2>/dev/null | cut -d'"' -f2 || true)"
    if [ -z "$msrv" ]; then
        return 1
    fi
    printf '%s\n' "$msrv"
    case $msrv in
        *[!0-9.]*) return 2 ;;
    esac
}

# platform_rows <README>: the Supported platforms table (from its
# heading to the next `## `), one line per row that is neither the
# header (the table's first row) nor a separator, as `name<TAB>tier<TAB>
# river` with each cell trimmed.
platform_rows() {
    awk -F'|' '
        function trim(s) { gsub(/^[ \t]+|[ \t]+$/, "", s); return s }
        /^## Supported platforms$/ { in_section = 1; next }
        /^## / { in_section = 0 }
        !in_section || !/^\|/ { next }
        !seen_header { seen_header = 1; next }
        {
            separator = 1
            for (i = 2; i < NF; i++) if (trim($i) !~ /^:?-+:?$/) separator = 0
            if (separator) next
            printf "%s\t%s\t%s\n", trim($2), trim($3), trim($4)
        }' "$1"
}

# expand_spec_path <path>: prints a spec path with the macros the spec's
# %install and %files use expanded to their Fedora and Debian values (the
# two agree for every one of them). Status 1 when a macro is left, so the
# caller names the line rather than gating a path with `%{` in it.
expand_spec_path() {
    local p=$1
    p="${p//'%{name}'/buoy-wm}"
    p="${p//'%{_libexecdir}'//usr/libexec}"
    p="${p//'%{_bindir}'//usr/bin}"
    p="${p//'%{_mandir}'//usr/share/man}"
    p="${p//'%{_datadir}'//usr/share}"
    p="${p//'%{_userunitdir}'//usr/lib/systemd/user}"
    printf '%s\n' "$p"
    case $p in
        *%*) return 1 ;;
    esac
}

# release_target <name>: prints `<target> <image>` for a README release
# name by rule; status 1 for a name no rule maps.
release_target() {
    if [[ "$1" =~ ^Fedora\ ([0-9]+)$ ]]; then
        printf 'fedora-%s registry.fedoraproject.org/fedora:%s\n' "${BASH_REMATCH[1]}" "${BASH_REMATCH[1]}"
    elif [[ "$1" =~ ^Ubuntu\ ([0-9]+\.[0-9]+)(\ LTS)?$ ]]; then
        printf 'ubuntu-%s docker.io/library/ubuntu:%s\n' "${BASH_REMATCH[1]}" "${BASH_REMATCH[1]}"
    else
        return 1
    fi
}

# supported_targets <README>: every matrix's expected list. Tier 1 and
# tier 2 rows alike (ADR-013: tier 2 builds, installs and passes its
# tests in CI; only the clean-machine session check, which no workflow
# runs, separates them). One `<target> <image>` line per row, or
# `- <name>` for a row release_target cannot map, so the caller names it.
supported_targets() {
    local name tier _river
    while IFS=$'\t' read -r name tier _river; do
        case $tier in
            1 | 2) release_target "$name" || printf -- '- %s\n' "$name" ;;
        esac
    done < <(platform_rows "$1")
}

# shell_files: every tracked file, relative to the repository root, whose
# first line is a `#!` naming sh, bash or dash, directly or through env.
# The first line only: a shell code block in README.md matches a
# whole-file grep. Status 1 when `git ls-files` fails, so a caller
# reports rather than lints nothing.
shell_files() {
    local root list file first
    root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
    list="$(git -C "$root" ls-files)" || return 1
    while IFS= read -r file; do
        [ -f "$root/$file" ] || continue
        first=""
        IFS= read -r first <"$root/$file" 2>/dev/null || true
        if [[ "$first" =~ ^#![[:space:]]*/([^[:space:]]*/)?(sh|bash|dash)([[:space:]]|$) ]] ||
            [[ "$first" =~ ^#![[:space:]]*/([^[:space:]]*/)?env([[:space:]]+-[^[:space:]]+)*[[:space:]]+(sh|bash|dash)([[:space:]]|$) ]]; then
            printf '%s\n' "$file"
        fi
    done <<<"$list"
}

# job_body <workflow> <job>: the lines of one job, from its two-space key
# to the next job key, so a line elsewhere cannot satisfy a check about
# this job.
job_body() {
    awk -v key="  $2:" '$0 == key { in_job = 1; next }
        /^  [a-z]/ { in_job = 0 }
        in_job' "$1"
}

# matrix_pairs <job-body-file>: each `- target:` line of a matrix and the
# `image:` on the line after it, as `<target> <image>` (`-` when the next
# line is not an `image:`).
matrix_pairs() {
    awk '/^[[:space:]]*- target: / {
            target = $0; sub(/^[[:space:]]*- target: /, "", target)
            if ((getline next_line) > 0) {
                image = next_line
                if (sub(/^[[:space:]]*image: /, "", image)) print target, image
                else print target, "-"
            } else print target, "-"
        }' "$1"
}

# matrix_differences <expected-file> <actual-file>: one line per way the
# matrix differs from the expected `<target> <image>` list; nothing when
# each expected target appears exactly once with its image and no other
# target appears.
matrix_differences() {
    local target image count
    while read -r target image; do
        [ "$target" != - ] || continue
        count="$(awk -v t="$target" '$1 == t' "$2" | grep -c . || true)"
        if [ "$count" -eq 0 ]; then
            printf "has no '- target: %s' (a README tier-1 or tier-2 row)\n" "$target"
        elif [ "$count" -gt 1 ]; then
            printf "lists '- target: %s' %s times (each target exactly once)\n" "$target" "$count"
        elif ! awk -v t="$target" -v i="$image" '$1 == t && $2 == i' "$2" | grep -q .; then
            printf "has '- target: %s' not followed by 'image: %s'\n" "$target" "$image"
        fi
    done <"$1"
    while read -r target _; do
        if ! awk -v t="$target" '$1 == t' "$1" | grep -q .; then
            printf "has '- target: %s', which no README tier-1 or tier-2 row maps to\n" "$target"
        fi
    done <"$2"
}

# fenced_block <file> <heading> <n>: the body of the n-th fenced code
# block under the exact heading line, up to the next heading outside a
# fence (a `# comment` inside a shell block is not one). A reader copies
# one block, so a line that must be in it is looked for there and not
# anywhere in the file. Empty when there is no such block.
fenced_block() {
    HEADING=$2 awk -v want="$3" '
        $0 == ENVIRON["HEADING"] { inside = 1; next }
        !inside { next }
        !fence && /^#+ / { exit }
        /^```/ {
            if (fence) { fence = 0; if (n == want) exit } else { fence = 1; n++ }
            next
        }
        fence && n == want { print }
    ' "$1"
}

# count_lines_matching <file> <regex>: how many whole lines match.
count_lines_matching() {
    grep -cxE -- "$2" "$1" || true
}

# step_with <file> <regex>: every step of the job's `steps:` list that
# holds a line matching <regex>, from its `- ` line to the next step.
step_with() {
    # Through the environment: `-v` would turn the regex's `\.` into `.`.
    STEP_PAT="$2" awk '
        BEGIN { pat = ENVIRON["STEP_PAT"] }
        function flush() { if (hit) printf "%s", buf; buf = ""; hit = 0 }
        /^[[:space:]]*steps:[[:space:]]*$/ { in_steps = 1; next }
        !in_steps { next }
        !have_indent && match($0, /^[[:space:]]*- /) { indent = RLENGTH - 2; have_indent = 1 }
        have_indent && match($0, /^[[:space:]]*[^[:space:]]/) && RLENGTH - 1 < indent { flush(); in_steps = 0; next }
        have_indent && match($0, /^[[:space:]]*- /) && RLENGTH - 2 == indent { flush() }
        { buf = buf $0 "\n"; if ($0 ~ pat) hit = 1 }
        END { flush() }' "$1"
}
