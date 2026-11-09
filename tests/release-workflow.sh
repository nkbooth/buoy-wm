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
# Asserts that release.yml builds what the README promises, on hosts
# that are not retiring. release.yml runs only on a tag, so a package
# matrix that drifts from the README's Supported platforms table, a
# runner label past its retirement or a toolchain that floats would
# otherwise be found on the release that ships it. The targets are read
# from the README's tier-1 rows (ADR-013: the list lives there) and
# mapped to container images by rule; nothing here spells a release.
# grep, awk and sed over the file's own shape, because the hygiene
# runner guarantees neither yq nor a YAML library.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly README="$REPO_ROOT/README.md"
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

for file in "$README" "$RELEASE"; do
    if [ ! -r "$file" ]; then
        fail "${file#"$REPO_ROOT/"}: missing or unreadable"
    fi
done
if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
    exit "$status"
fi

# job_body <job>: the lines of one job, from its two-space key to the
# next job key, so a `target:` or `image:` elsewhere cannot satisfy a
# check about this job.
job_body() {
    awk -v key="  $1:" '$0 == key { in_job = 1; next }
        /^  [a-z]/ { in_job = 0 }
        in_job' "$RELEASE"
}

# (a) Every job runs on a label that is not retiring: GitHub removes
# ubuntu-22.04 on 2027-04-17 after brownouts from 2027-03-23
# (actions/runner-images#14254). Whole line after the indentation, so a
# trailing comment or a second label cannot pass; at least three, one
# per job.
runs_on_lines="$(grep -E '^[[:space:]]*runs-on:' "$RELEASE" || true)"
runs_on_count="$(printf '%s' "$runs_on_lines" | grep -c . || true)"
if [ "$runs_on_count" -lt 3 ]; then
    fail "release.yml: expected at least three 'runs-on:' lines (build, packages, publish), found $runs_on_count"
fi
while IFS= read -r line; do
    [ -n "$line" ] || continue
    trimmed="$(printf '%s' "$line" | sed 's/^[[:space:]]*//')"
    if [ "$trimmed" != 'runs-on: ubuntu-24.04' ]; then
        fail "release.yml: '$trimmed' is not 'runs-on: ubuntu-24.04' (the runner is only the host, ADR-015)"
    fi
done <<<"$runs_on_lines"
if grep -qF -- 'ubuntu-22.04' "$RELEASE"; then
    fail "release.yml: still names 'ubuntu-22.04' (retiring; brownouts from 2027-03-23)"
fi

# (b) The README's tier-1 rows, read the way tests/docs.sh reads the
# section: from its heading to the next `## `. A row's second cell is
# its tier; the first is the release name the target is derived from.
platforms="$(awk '/^## Supported platforms$/ { in_section = 1; next }
    /^## / { in_section = 0 }
    in_section' "$README" || true)"
tier1_names="$(printf '%s\n' "$platforms" | awk -F'|' '/^\|/ {
        name = $2; tier = $3
        gsub(/^[ \t]+|[ \t]+$/, "", name)
        gsub(/^[ \t]+|[ \t]+$/, "", tier)
        if (tier == "1") print name
    }')"
# A tier cell this rule cannot read ("1 (from May)", "**1**") would
# otherwise drop its row from the matrix without a word; the header and
# the separator row are the only other shapes the table has.
unread_tiers="$(printf '%s\n' "$platforms" | awk -F'|' '/^\|/ {
        name = $2; tier = $3
        gsub(/^[ \t]+|[ \t]+$/, "", name)
        gsub(/^[ \t]+|[ \t]+$/, "", tier)
        if (tier !~ /^(1|2|unsupported|Tier|-+)$/) print name " -> " tier
    }')"
while IFS= read -r row; do
    [ -n "$row" ] || continue
    fail "README.md: Supported platforms row '$row' has a tier cell that is not '1', '2' or 'unsupported'"
done <<<"$unread_tiers"
if [ -z "$tier1_names" ]; then
    fail "README.md: no tier-1 rows in the Supported platforms section (the release matrix is derived from them)"
fi

# Release name -> "target image" by rule; anything else is reported,
# never skipped, so a new kind of row cannot silently leave the matrix.
: >"$tmp/expected"
lts_images=()
while IFS= read -r name; do
    [ -n "$name" ] || continue
    if [[ "$name" =~ ^Fedora\ ([0-9]+)$ ]]; then
        n="${BASH_REMATCH[1]}"
        printf 'fedora-%s registry.fedoraproject.org/fedora:%s\n' "$n" "$n" >>"$tmp/expected"
    elif [[ "$name" =~ ^Ubuntu\ ([0-9]+\.[0-9]+)(\ LTS)?$ ]]; then
        v="${BASH_REMATCH[1]}"
        printf 'ubuntu-%s docker.io/library/ubuntu:%s\n' "$v" "$v" >>"$tmp/expected"
        if [ -n "${BASH_REMATCH[2]}" ]; then
            lts_images+=("docker.io/library/ubuntu:$v")
        fi
    else
        fail "README.md: tier-1 row '$name' maps to no release target (only 'Fedora NN' and 'Ubuntu X.Y[ LTS]' have a rule)"
    fi
done <<<"$tier1_names"

# The packages job's matrix: each `- target:` line followed directly by
# its `image:` line.
job_body packages >"$tmp/packages"
if [ ! -s "$tmp/packages" ]; then
    fail "release.yml: no 'packages' job (the per-release matrix)"
fi
awk '/^[[:space:]]*- target: / {
        target = $0; sub(/^[[:space:]]*- target: /, "", target)
        if ((getline next_line) > 0) {
            image = next_line
            if (sub(/^[[:space:]]*image: /, "", image)) print target, image
            else print target, "-"
        } else print target, "-"
    }' "$tmp/packages" >"$tmp/actual"
while read -r target image; do
    count="$(awk -v t="$target" '$1 == t' "$tmp/actual" | grep -c . || true)"
    if [ "$count" -eq 0 ]; then
        fail "release.yml: the packages matrix has no '- target: $target' (a README tier-1 row)"
    elif [ "$count" -gt 1 ]; then
        fail "release.yml: the packages matrix lists '- target: $target' $count times (each target exactly once)"
    elif ! awk -v t="$target" -v i="$image" '$1 == t && $2 == i' "$tmp/actual" | grep -q .; then
        fail "release.yml: '- target: $target' is not followed by 'image: $image'"
    fi
done <"$tmp/expected"
while read -r target _; do
    if ! awk -v t="$target" '$1 == t' "$tmp/expected" | grep -q .; then
        fail "release.yml: the packages matrix has '- target: $target', which no README tier-1 row maps to"
    fi
done <"$tmp/actual"
# The list means nothing unless each leg runs in its own entry's image.
# shellcheck disable=SC2016 # the literal text of the workflow line
if [ "$(grep -cxF -- '      image: ${{ matrix.image }}' "$tmp/packages" || true)" -ne 1 ]; then
    fail "release.yml: the packages job's container is not exactly 'image: \${{ matrix.image }}' (every leg would share one image)"
fi

# (g) Every leg can fail the release and every leg's package reaches it:
# a per-target artifact name (a shared one lets `overwrite` replace one
# leg's RPM with another's), `publish` waiting on `packages` and
# downloading every `package-*`, and no `continue-on-error` anywhere.
# shellcheck disable=SC2016 # the literal text of the workflow line
if [ "$(grep -cxF -- '          name: package-${{ matrix.target }}' "$tmp/packages" || true)" -ne 1 ]; then
    fail "release.yml: the packages job does not upload exactly one 'name: package-\${{ matrix.target }}' artifact"
fi
job_body publish >"$tmp/publish"
if ! grep -qxF -- '    needs: [build, packages]' "$tmp/publish"; then
    fail "release.yml: publish does not say 'needs: [build, packages]' (a failed leg must block the release)"
fi
if ! grep -qxF -- '          pattern: package-*' "$tmp/publish"; then
    fail "release.yml: publish does not download 'pattern: package-*' (every leg's package)"
fi
if grep -qE '^[[:space:]]*continue-on-error:' "$RELEASE"; then
    fail "release.yml: uses continue-on-error (a failing target must block the release)"
fi

# (h) Each family builds its package with its script, and both reach the
# release under the one attestation. Whole `run:` lines, exactly once
# and inside the packages job, so a comment quoting a command, a second
# copy or a step in another job cannot stand in for it. The checkout
# and the upload serve both families, so neither may carry an `if:`
# anywhere in its step; with one unconditional upload, check (g)'s
# exactly-one artifact name holds for the Ubuntu legs too. Each family's
# build step, and the lint step, carries exactly its family's guard, so
# a dead condition cannot skip one quietly.
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
for run_line in 'scripts/build-rpm\.sh sources dist' 'scripts/build-deb\.sh sources dist' \
    'lintian --fail-on error( [^ ]+)* dist/\*\.deb'; do
    n="$(count_lines_matching "$tmp/packages" "[[:space:]]*run: $run_line")"
    if [ "$n" -ne 1 ]; then
        fail "release.yml: the packages job does not have exactly one 'run: ${run_line//\\/}' line (found $n)"
    fi
done
n="$(count_lines_matching "$tmp/packages" '[[:space:]]*apt-get build-dep -y \./packaging/deb')"
if [ "$n" -ne 1 ]; then
    fail "release.yml: the packages job does not run 'apt-get build-dep -y ./packaging/deb' exactly once as a whole line (debian/control's Build-Depends is the Ubuntu legs' one list; found $n)"
fi
for action in actions/checkout actions/upload-artifact; do
    n="$(grep -cE -- "^[[:space:]]*- uses: $action@" "$tmp/packages" || true)"
    if [ "$n" -ne 1 ]; then
        fail "release.yml: expected exactly one '- uses: $action@' step in the packages job, found $n"
    elif step_with "$tmp/packages" "^[[:space:]]*- uses: $action@" | grep -qE '^[[:space:]]*if:'; then
        fail "release.yml: the packages job's $action step has an 'if:' (both families need it)"
    fi
done
while IFS='|' read -r anchor family; do
    step_with "$tmp/packages" "^[[:space:]]*run: $anchor" >"$tmp/step"
    ifs="$(grep -E '^[[:space:]]*if:' "$tmp/step" | sed 's/^[[:space:]]*//' || true)"
    if [ "$ifs" != "if: startsWith(matrix.target, '$family-')" ]; then
        fail "release.yml: the packages job's 'run: ${anchor//\\/}' step is not guarded by exactly 'if: startsWith(matrix.target, '$family-')' (found: ${ifs:-none})"
    fi
done <<'EOF'
scripts/build-rpm\.sh sources dist|fedora
scripts/build-deb\.sh sources dist|ubuntu
lintian --fail-on error|ubuntu
EOF
# The attestation's subjects: the lines of publish's `subject-path:`
# block, not any bare line elsewhere in the job.
awk '/^[[:space:]]*subject-path:[[:space:]]*\|[[:space:]]*$/ { match($0, /^[[:space:]]*/); indent = RLENGTH; on = 1; next }
    on && match($0, /^[[:space:]]*/) && (RLENGTH <= indent || $0 ~ /^[[:space:]]*$/) { on = 0 }
    on { sub(/^[[:space:]]+/, ""); print }' "$tmp/publish" >"$tmp/subjects"
for subject in 'dist/*.tar.gz' 'dist/*.rpm' 'dist/*.deb'; do
    n="$(grep -cxF -- "$subject" "$tmp/subjects" || true)"
    if [ "$n" -ne 1 ]; then
        fail "release.yml: publish's attestation subject-path does not list '$subject' exactly once as a whole line (found $n)"
    fi
done

# (c) The tarball is built in the oldest-glibc supported container, the
# LTS row's image (ADR-015), so its glibc floor is a fact of the table.
job_body build >"$tmp/build"
build_images="$(grep -E '^[[:space:]]*image: ' "$tmp/build" | sed 's/^[[:space:]]*image: //' || true)"
build_image_count="$(printf '%s' "$build_images" | grep -c . || true)"
if [ "${#lts_images[@]}" -ne 1 ]; then
    fail "README.md: expected exactly one tier-1 row ending in ' LTS' (the build container), found ${#lts_images[@]}"
elif [ "$build_image_count" -ne 1 ]; then
    fail "release.yml: expected exactly one 'image:' line in the build job (its container), found $build_image_count"
elif [ "$build_images" != "${lts_images[0]}" ]; then
    fail "release.yml: the build job's container is '$build_images', not '${lts_images[0]}' (the tier-1 LTS row's image, ADR-015)"
fi

# (d) One pinned toolchain: the tarball and the vendored crates are
# what the attestation names, so the compiler is a version, never a
# channel or an expression.
toolchain_lines="$(grep -E '^[[:space:]]*toolchain:' "$RELEASE" || true)"
toolchain_count="$(printf '%s' "$toolchain_lines" | grep -c . || true)"
if [ "$toolchain_count" -ne 1 ]; then
    fail "release.yml: expected exactly one 'toolchain:' line, found $toolchain_count"
else
    toolchain="$(printf '%s' "$toolchain_lines" | sed 's/^[[:space:]]*toolchain:[[:space:]]*//')"
    if [[ ! "$toolchain" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        fail "release.yml: toolchain '$toolchain' is not a pinned X.Y.Z version"
    fi
fi

# (e) No container image here has sudo, which setup-build-deps runs;
# and git refuses a workspace the runner's user owns once the container
# runs it as root, unless the workspace is trusted by hand.
# Any mention, so a quoted or reformatted `uses:` cannot slip past.
if grep -qF -- 'setup-build-deps' "$RELEASE"; then
    fail "release.yml: names ./.github/actions/setup-build-deps (it runs sudo, which the container images lack)"
fi
# A `run:` line of the build job, so a comment quoting it cannot pass.
# shellcheck disable=SC2016 # the literal text of the workflow line
if ! grep -qxE -- '[[:space:]]*run: git config --global --add safe\.directory "\$GITHUB_WORKSPACE"' "$tmp/build"; then
    fail "release.yml: the build job does not run 'git config --global --add safe.directory \"\$GITHUB_WORKSPACE\"' (git refuses the runner-owned workspace as root)"
fi

# (f) A tag push and a dispatch of the same tag queue, never race: one
# block, keyed on the tag, that never cancels a running publish.
concurrency_count="$(grep -c '^concurrency:' "$RELEASE" || true)"
if [ "$concurrency_count" -ne 1 ]; then
    fail "release.yml: expected exactly one top-level 'concurrency:' block, found $concurrency_count"
fi
# shellcheck disable=SC2016 # the literal text of the workflow line
for line in '  group: release-${{ inputs.tag || github.ref_name }}' \
    '  cancel-in-progress: false'; do
    if [ "$(grep -cxF -- "$line" "$RELEASE" || true)" -ne 1 ]; then
        fail "release.yml: expected the concurrency line '${line#  }' exactly once"
    fi
done

if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
fi
exit "$status"
