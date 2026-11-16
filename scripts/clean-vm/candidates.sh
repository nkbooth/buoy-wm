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
# candidates.sh: the packages a pre-tag run installs, built from this
# checkout's HEAD the way release.yml builds them: Source0 by `git
# archive`, the vendor tarball in ubuntu:26.04 with release.yml's own tar
# line, then each tier-1 target's leg in its release container
# (build-rpm.sh or build-deb.sh, lintian on the .deb), then release.yml's
# verify replay on each package in a fresh container. Output in
# cand/out/<target>/ with cand/out/COMMIT naming the commit built (which
# run_readme.sh checks against HEAD), logs in cand/. A checkout with
# uncommitted changes is refused: Source0 is HEAD, while the version, the
# targets and the README the VM runs are read from the working tree.
set -euo pipefail
# shellcheck source=scripts/clean-vm/env.sh
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
# shellcheck source=tests/lib.sh
source "$REPO_ROOT/tests/lib.sh"

if [ -n "$(git -C "$REPO_ROOT" status --porcelain)" ]; then
    echo "uncommitted changes in $REPO_ROOT: commit them, HEAD is what is built" >&2
    exit 1
fi
version="$(grep -m1 '^version = ' "$REPO_ROOT/Cargo.toml" | cut -d'"' -f2 || true)"
if [ -z "$version" ]; then
    echo "Cargo.toml: no 'version = \"…\"' line" >&2
    exit 1
fi
targets=()
while IFS=$'\t' read -r name tier _river; do
    [ "$tier" = 1 ] || continue
    pair="$(release_target "$name")" || { echo "README.md: tier-1 row '$name' maps to no target" >&2; exit 1; }
    targets+=("$pair")
done < <(platform_rows "$REPO_ROOT/README.md")
if [ "${#targets[@]}" -eq 0 ]; then
    echo "README.md: no tier-1 rows to build" >&2
    exit 1
fi
epoch="$(git -C "$REPO_ROOT" log -1 --format=%ct)"
cand="$BUOY_VM_DIR/cand"
rm -rf "$cand/sources" "$cand/out"
mkdir -p "$cand/sources" "$cand/out"
git -C "$REPO_ROOT" archive --format=tar.gz --prefix="buoy-wm-${version}/" \
    -o "$cand/sources/buoy-wm-${version}.tar.gz" HEAD

# Shared labels (:z): the legs run side by side on the same directories,
# and a private label (:Z) would take them away from every container but
# the last one started.
container() { # container <image> <log> <script> [args...]
    local image=$1 log=$2
    shift 2
    podman run --rm --network=host -v "$cand:/cand:z" -v "$CLEAN_VM_HOME:/harness:ro,z" \
        -e "VERSION=$version" -e "SOURCE_DATE_EPOCH=$epoch" "$image" bash "$@" >"$log" 2>&1
}

# each_target <script> <log-prefix>: run the script once per target, all
# at once, each in its own release's container; status 1 if any failed.
each_target() {
    local script=$1 prefix=$2 pair target image pids=() failed=0
    for pair in "${targets[@]}"; do
        read -r target image <<<"$pair"
        container "$image" "$cand/$prefix$target.log" "/harness/$script" "$target" &
        pids+=("$!")
    done
    for pid in "${pids[@]}"; do
        wait "$pid" || failed=1
    done
    return "$failed"
}

if ! container docker.io/library/ubuntu:26.04 "$cand/vendor.log" /harness/vendor.sh; then
    tail -n 20 "$cand/vendor.log" >&2
    exit 1
fi
status=0
each_target leg.sh "" || status=1
if [ "$status" -eq 0 ]; then
    each_target verify.sh v- || status=1
fi
grep -hE '^(LEG|VERIFY) ' "$cand"/*.log || true
if [ "$status" -eq 0 ]; then
    git -C "$REPO_ROOT" rev-parse HEAD >"$cand/out/COMMIT"
fi
exit "$status"
