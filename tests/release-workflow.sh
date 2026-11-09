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
# Asserts that release.yml builds and verifies what the README promises,
# on hosts that are not retiring. release.yml runs only on a tag, so a
# package matrix that drifts from the README's Supported platforms
# table, a runner label past its retirement or a toolchain that floats
# would otherwise be found on the release that ships it. The targets are
# read from the README's tier-1 and tier-2 rows (ADR-013: the list lives
# there) and mapped to container images by rule, both through
# tests/lib.sh; nothing here spells a release. grep, awk and sed over
# the file's own shape, because the hygiene runner guarantees neither yq
# nor a YAML library.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
# shellcheck source=tests/lib.sh
source "$REPO_ROOT/tests/lib.sh"
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

# (a) Every job runs on a label that is not retiring: GitHub removes
# ubuntu-22.04 on 2027-04-17 after brownouts from 2027-03-23
# (actions/runner-images#14254). Whole line after the indentation, so a
# trailing comment or a second label cannot pass; at least four, one
# per job.
runs_on_lines="$(grep -E '^[[:space:]]*runs-on:' "$RELEASE" || true)"
runs_on_count="$(printf '%s' "$runs_on_lines" | grep -c . || true)"
if [ "$runs_on_count" -lt 4 ]; then
    fail "release.yml: expected at least four 'runs-on:' lines (build, packages, verify, publish), found $runs_on_count"
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

# (b) The README's rows (tests/lib.sh's platform_rows). Every matrix is
# the tier-1 and tier-2 rows (ADR-013: tier 2 builds, installs and
# passes its tests in CI); a tier cell this rule cannot read ("1 (from
# May)", "**1**") would otherwise drop its row from the matrix without
# a word.
platform_rows "$README" >"$tmp/rows"
while IFS=$'\t' read -r name tier _; do
    case $tier in
        1 | 2 | unsupported) ;;
        *) fail "README.md: Supported platforms row '$name -> $tier' has a tier cell that is not '1', '2' or 'unsupported'" ;;
    esac
done <"$tmp/rows"
# Release name -> "target image" by rule; anything else is reported,
# never skipped, so a new kind of row cannot silently leave the matrix.
supported_targets "$README" >"$tmp/mapped"
grep -v '^- ' "$tmp/mapped" >"$tmp/expected" || true
while read -r _ name; do
    fail "README.md: tier-1 or tier-2 row '$name' maps to no release target (only 'Fedora NN' and 'Ubuntu X.Y[ LTS]' have a rule)"
done < <(grep '^- ' "$tmp/mapped" || true)
if [ ! -s "$tmp/mapped" ]; then
    fail "README.md: no tier-1 or tier-2 rows in the Supported platforms section (the release matrix is derived from them)"
fi
# The build container is the tier-1 LTS row's image (check (c)).
lts_images=()
while IFS=$'\t' read -r name tier _; do
    if [ "$tier" = 1 ] && [[ "$name" == *' LTS' ]] && pair="$(release_target "$name")"; then
        lts_images+=("${pair#* }")
    fi
done <"$tmp/rows"

# The packages job's matrix: each `- target:` line followed directly by
# its `image:` line.
job_body "$RELEASE" packages >"$tmp/packages"
if [ ! -s "$tmp/packages" ]; then
    fail "release.yml: no 'packages' job (the per-release matrix)"
fi
matrix_pairs "$tmp/packages" >"$tmp/actual"
while IFS= read -r difference; do
    fail "release.yml: the packages matrix $difference"
done < <(matrix_differences "$tmp/expected" "$tmp/actual")
# The list means nothing unless each leg runs in its own entry's image.
# shellcheck disable=SC2016 # the literal text of the workflow line
if [ "$(grep -cxF -- '      image: ${{ matrix.image }}' "$tmp/packages" || true)" -ne 1 ]; then
    fail "release.yml: the packages job's container is not exactly 'image: \${{ matrix.image }}' (every leg would share one image)"
fi

# (g) Every leg can fail the release and every leg's package reaches it:
# a per-target artifact name (a shared one lets `overwrite` replace one
# leg's RPM with another's), `publish` waiting on `packages` and
# `verify` and downloading every `package-*`, and no
# `continue-on-error` anywhere.
# shellcheck disable=SC2016 # the literal text of the workflow line
if [ "$(grep -cxF -- '          name: package-${{ matrix.target }}' "$tmp/packages" || true)" -ne 1 ]; then
    fail "release.yml: the packages job does not upload exactly one 'name: package-\${{ matrix.target }}' artifact"
fi
job_body "$RELEASE" publish >"$tmp/publish"
if ! grep -qxF -- '    needs: [build, packages, verify]' "$tmp/publish"; then
    fail "release.yml: publish does not say 'needs: [build, packages, verify]' (a failed leg or a failed install must block the release, NFR6)"
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
# a dead condition cannot skip one quietly. count_lines_matching and
# step_with are tests/lib.sh's, shared with tests/ci-workflow.sh.
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
job_body "$RELEASE" build >"$tmp/build"
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

# (i) Every package is installed into a clean container of its own
# release and its binaries run before anything is published (NFR6). The
# same matrix as `packages`, each leg in its entry's image, downloading
# its own leg's artifact; the install and the check as whole `run:`
# lines, each install under exactly its family's guard and the check
# under none. Clean means nothing but the package and what it pulls, so
# no build tool may appear anywhere in the job.
job_body "$RELEASE" verify >"$tmp/verify"
if [ ! -s "$tmp/verify" ]; then
    fail "release.yml: no 'verify' job (each package installed and run in a clean container, NFR6)"
else
    matrix_pairs "$tmp/verify" >"$tmp/verify-actual"
    while IFS= read -r difference; do
        fail "release.yml: the verify matrix $difference"
    done < <(matrix_differences "$tmp/expected" "$tmp/verify-actual")
    # shellcheck disable=SC2016 # the literal text of the workflow lines
    for line in '      image: ${{ matrix.image }}' '    needs: [build, packages]' \
        '          name: package-${{ matrix.target }}'; do
        if [ "$(grep -cxF -- "$line" "$tmp/verify" || true)" -ne 1 ]; then
            fail "release.yml: the verify job does not have exactly one line '${line#"${line%%[! ]*}"}'"
        fi
    done
    # shellcheck disable=SC2016 # the literal text of the workflow line
    n="$(count_lines_matching "$tmp/verify" '[[:space:]]*run: scripts/check-installed\.sh "\$VERSION"')"
    if [ "$n" -ne 1 ]; then
        fail "release.yml: the verify job does not have exactly one 'run: scripts/check-installed.sh \"\$VERSION\"' line (found $n)"
    elif step_with "$tmp/verify" '^[[:space:]]*run: scripts/check-installed\.sh' | grep -qE '^[[:space:]]*if:'; then
        fail "release.yml: the verify job's check-installed.sh step has an 'if:' (every leg must run the binaries)"
    fi
    while IFS='|' read -r anchor family; do
        n="$(count_lines_matching "$tmp/verify" "[[:space:]]*(run: )?$anchor")"
        if [ "$n" -ne 1 ]; then
            fail "release.yml: the verify job does not run '${anchor//\\/}' exactly once as a whole line (found $n)"
            continue
        fi
        step_with "$tmp/verify" "^[[:space:]]*(run: )?$anchor\$" >"$tmp/step"
        ifs="$(grep -E '^[[:space:]]*if:' "$tmp/step" | sed 's/^[[:space:]]*//' || true)"
        if [ "$ifs" != "if: startsWith(matrix.target, '$family-')" ]; then
            fail "release.yml: the verify job's '${anchor//\\/}' step is not guarded by exactly 'if: startsWith(matrix.target, '$family-')' (found: ${ifs:-none})"
        fi
    done <<'EOF'
dnf -y install \./dist/\*\.rpm|fedora
apt-get install -y --no-install-recommends \./dist/\*\.deb|ubuntu
EOF
    if ! grep -qE '^[[:space:]]*(run: )?(cd dist && )?sha256sum -c' "$tmp/verify"; then
        fail "release.yml: the verify job does not check the download with 'sha256sum -c'"
    fi
    for tool in builddep build-dep cargo rpmbuild dpkg-buildpackage lintian; do
        if grep -qF -- "$tool" "$tmp/verify"; then
            fail "release.yml: the verify job names '$tool' (the container must be clean but for the package and what it pulls)"
        fi
    done
fi

if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
fi
exit "$status"
