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
# Asserts that ci.yml builds and tests every push on each supported
# release's own toolchain (NFR6), with the list read from the README's
# tier-1 and tier-2 rows through tests/lib.sh exactly as
# tests/release-workflow.sh reads it for the release, so the two
# workflows cannot cover different releases. Also that a tag push no
# longer re-runs CI (release.yml is the tag's gate) and that the step
# running shellcheck lints every shell file because it is one, not because
# someone listed it. grep, awk and sed over the file's own shape.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
# shellcheck source=tests/lib.sh
source "$REPO_ROOT/tests/lib.sh"
readonly README="$REPO_ROOT/README.md"
readonly CI="$REPO_ROOT/.github/workflows/ci.yml"

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

for file in "$README" "$CI"; do
    if [ ! -r "$file" ]; then
        fail "${file#"$REPO_ROOT/"}: missing or unreadable"
    fi
done
if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
    exit "$status"
fi

# The triggers: a push filter naming only branches skips tag pushes, so
# a tag no longer re-runs every job on a commit main's push already
# gated. `pull_request:` stays.
on_block="$(awk '/^on:[[:space:]]*$/ { on = 1; next } on && /^[^[:space:]#]/ { on = 0 } on' "$CI")"
if ! printf '%s\n' "$on_block" | grep -qxF -- '  push:'; then
    fail "ci.yml: no 'push:' trigger"
fi
# The push trigger's own block, comments dropped, is that one line: the
# filter belongs to `push:` (under `pull_request:` it would leave tags
# unfiltered), and a `paths:` or `branches-ignore:` beside it would skip
# CI on pushes it should gate.
push_block="$(printf '%s\n' "$on_block" |
    awk '/^  push:[[:space:]]*$/ { on = 1; next } on && /^  [^[:space:]#]/ { on = 0 } on && !/^[[:space:]]*(#|$)/')"
if [ "$push_block" != '    branches: ["**"]' ]; then
    fail "ci.yml: the push trigger is not filtered by exactly 'branches: [\"**\"]' and nothing else (tag pushes would re-run CI, or pushes skip it); found: ${push_block:-nothing}"
fi
if printf '%s\n' "$on_block" | grep -qE '^[[:space:]]+tags(-ignore)?:'; then
    fail "ci.yml: the push trigger has a 'tags:' key (release.yml is the tag's gate)"
fi
if ! printf '%s\n' "$on_block" | grep -qxF -- '  pull_request:'; then
    fail "ci.yml: no 'pull_request:' trigger"
fi

# The targets job: one leg per supported release, each in its own
# entry's image, on the host-only runner (ADR-015).
supported_targets "$README" >"$tmp/mapped"
grep -v '^- ' "$tmp/mapped" >"$tmp/expected" || true
while read -r _ name; do
    fail "README.md: tier-1 or tier-2 row '$name' maps to no release target (only 'Fedora NN' and 'Ubuntu X.Y[ LTS]' have a rule)"
done < <(grep '^- ' "$tmp/mapped" || true)
if [ ! -s "$tmp/mapped" ]; then
    fail "README.md: no tier-1 or tier-2 rows in the Supported platforms section (the targets matrix is derived from them)"
fi
job_body "$CI" targets >"$tmp/targets"
if [ ! -s "$tmp/targets" ]; then
    fail "ci.yml: no 'targets' job (every push built and tested on each supported release's toolchain, NFR6)"
else
    matrix_pairs "$tmp/targets" >"$tmp/actual"
    while IFS= read -r difference; do
        fail "ci.yml: the targets matrix $difference"
    done < <(matrix_differences "$tmp/expected" "$tmp/actual")
    # shellcheck disable=SC2016 # the literal text of the workflow lines
    for line in '    runs-on: ubuntu-24.04' '    container:' '      fail-fast: false' '      image: ${{ matrix.image }}'; do
        if [ "$(grep -cxF -- "$line" "$tmp/targets" || true)" -ne 1 ]; then
            fail "ci.yml: the targets job does not have exactly one line '${line#"${line%%[! ]*}"}'"
        fi
    done
    # Nothing downstream needs `targets`, so a skipped job or a dropped
    # leg turns nothing red: no job-level `if:`, no `exclude:`.
    if grep -qE '^    if:' "$tmp/targets"; then
        fail "ci.yml: the targets job has a job-level 'if:' (every push must build and test on every target)"
    fi
    if grep -qE '^[[:space:]]*exclude:' "$tmp/targets"; then
        fail "ci.yml: the targets matrix has an 'exclude:' (it would drop a leg the include list names)"
    fi
    # Build and test are whole `run:` lines, once each, and no `if:` in
    # either step can skip them for a family.
    for run_line in 'cargo build --workspace --locked' 'cargo test --workspace --locked'; do
        n="$(count_lines_matching "$tmp/targets" "[[:space:]]*run: $run_line")"
        if [ "$n" -ne 1 ]; then
            fail "ci.yml: the targets job does not have exactly one 'run: $run_line' line (found $n)"
        elif step_with "$tmp/targets" "^[[:space:]]*run: $run_line\$" | grep -qE '^[[:space:]]*if:'; then
            fail "ci.yml: the targets job's '$run_line' step has an 'if:' (every leg must build and test)"
        fi
    done
    # Each family installs its build dependencies from its package's own
    # list, under exactly its family's guard. `ca-certificates` is the
    # Ubuntu legs' one addition: this job fetches crates online and
    # `build-dep` does not bring it.
    while IFS='|' read -r anchor family; do
        n="$(count_lines_matching "$tmp/targets" "[[:space:]]*(run: )?$anchor")"
        if [ "$n" -ne 1 ]; then
            fail "ci.yml: the targets job does not run '${anchor//\\/}' exactly once as a whole line (found $n)"
            continue
        fi
        step_with "$tmp/targets" "^[[:space:]]*(run: )?$anchor\$" >"$tmp/step"
        ifs="$(grep -E '^[[:space:]]*if:' "$tmp/step" | sed 's/^[[:space:]]*//' || true)"
        if [ "$ifs" != "if: startsWith(matrix.target, '$family-')" ]; then
            fail "ci.yml: the targets job's '${anchor//\\/}' step is not guarded by exactly 'if: startsWith(matrix.target, '$family-')' (found: ${ifs:-none})"
        fi
    done <<'EOF'
dnf -y builddep packaging/rpm/buoy-wm\.spec|fedora
apt-get build-dep -y \./packaging/deb|ubuntu
apt-get install -y --no-install-recommends ca-certificates|ubuntu
EOF
    # The images run as root with no sudo, which the action runs.
    if grep -qF -- 'setup-build-deps' "$tmp/targets"; then
        fail "ci.yml: the targets job uses setup-build-deps (it runs sudo, which the container images lack)"
    fi
fi
if grep -qE '^[[:space:]]*continue-on-error:' "$CI"; then
    fail "ci.yml: uses continue-on-error (a failing target must fail the push)"
fi

# The fast path stays: `check` still tests on the runner's toolchain.
job_body "$CI" check >"$tmp/check"
if [ "$(count_lines_matching "$tmp/check" '[[:space:]]*run: cargo test --workspace --locked')" -ne 1 ]; then
    fail "ci.yml: the check job no longer has one 'run: cargo test --workspace --locked' line (the fast path)"
fi

# The shellcheck step lints shell_files' list and names no file itself.
job_body "$CI" hygiene >"$tmp/hygiene"
step_with "$tmp/hygiene" '^[[:space:]]*- name: shellcheck$' >"$tmp/shellcheck"
if [ ! -s "$tmp/shellcheck" ]; then
    fail "ci.yml: the hygiene job has no 'shellcheck' step"
else
    # The step sources the library that holds shell_files; any other .sh
    # path is a hand list coming back.
    if grep -vE '^[[:space:]]*(#|(source|\.) tests/lib\.sh$)' "$tmp/shellcheck" |
        grep -qE '[[:alnum:]_./-]+\.sh([[:space:]"]|$)'; then
        fail "ci.yml: the shellcheck step names a .sh path (it must lint tests/lib.sh's shell_files list)"
    fi
    if ! grep -qE '(^|[[:space:](])shell_files([[:space:])]|$)' "$tmp/shellcheck"; then
        fail "ci.yml: the shellcheck step does not run shell_files"
    fi
    # Mentioning the list is not linting it: one whole line lints every
    # file, and nothing in the step can skip it or swallow its status.
    # shellcheck disable=SC2016 # the literal text of the workflow line
    if [ "$(grep -cxE '[[:space:]]*shellcheck -x "\$\{list\[@\]\}"' "$tmp/shellcheck" || true)" -ne 1 ]; then
        fail "ci.yml: the shellcheck step does not have exactly one whole line 'shellcheck -x \"\${list[@]}\"'"
    fi
    if grep -vE '^[[:space:]]*#' "$tmp/shellcheck" | grep -qE '^[[:space:]]*if:|\|\||(^|[;&[:space:]])(set \+e|true)([;[:space:]]|$)'; then
        fail "ci.yml: the shellcheck step has an 'if:', '||', 'true' or 'set +e' that could skip it or swallow its status"
    fi
fi

# shell_files itself, from the repository root: every tracked tests/*.sh
# and scripts/*.sh and the session supervisor, and not the README (a
# shell code block is not a shell file).
if ! shell_files >"$tmp/shell-files"; then
    fail "tests/lib.sh: shell_files failed (git ls-files did not run); nothing to compare"
else
    if ! git -C "$REPO_ROOT" ls-files -- 'tests/*.sh' 'scripts/*.sh' >"$tmp/tracked"; then
        fail "git ls-files failed; cannot list the tracked shell scripts"
    fi
    echo packaging/buoy-session >>"$tmp/tracked"
    while IFS= read -r file; do
        if ! grep -qxF -- "$file" "$tmp/shell-files"; then
            fail "tests/lib.sh: shell_files does not list '$file'"
        fi
    done <"$tmp/tracked"
    if grep -qxF -- README.md "$tmp/shell-files"; then
        fail "tests/lib.sh: shell_files lists README.md (only a file's first line decides)"
    fi
fi

if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
fi
exit "$status"
