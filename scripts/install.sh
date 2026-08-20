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
# Installs an already-built release workspace out of the project directory,
# so `cargo clean` (or moving/deleting the checkout) can't take the running
# session's binaries with it.
#
# This runs on the HOST and only copies files. Building stays a separate
# in-devcontainer step (`cargo build --workspace --release`) — the container's
# /workspaces/buoy-wm/target IS this repo's target/ via the workspace mount,
# so there is nothing to shuttle. It is deliberately not wrapped into one
# script: both container entry points this project has used are unreliable
# (`devpod ssh` failed throughout Epics 1-2; the `podman exec` substitute
# needs a container name that changes on every recreate), so a script driving
# either would break more often than the manual step it replaced.
#
# All three binaries land in ONE directory because the WM locates `buoy-tag-picker`
# as a sibling of its own `current_exe()` (see `tag_picker_path` in
# wm/src/main.rs) — splitting them across directories breaks the picker at
# runtime, not at install time.

set -euo pipefail

readonly BINARIES=(buoy-wm buoy-tag-picker buoy-status-bar)

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly script_dir
repo_root="$(cd -- "$script_dir/.." && pwd)"
readonly repo_root

# One script serves two layouts. In a source checkout the binaries are in
# target/release; in a release tarball they sit next to this script. Probing
# for the script's own directory first means the tarball needs no second,
# near-identical copy of this file that could drift from it.
if [[ -x "$script_dir/buoy-wm" ]]; then
    build_dir="${BUOY_BUILD_DIR:-$script_dir}"
else
    build_dir="${BUOY_BUILD_DIR:-$repo_root/target/release}"
fi
readonly build_dir
# Not an XDG_DATA_HOME path: that variable points at ~/.local/share, while
# these are executables belonging under ~/.local/lib.
install_dir="${BUOY_INSTALL_DIR:-$HOME/.local/lib/buoy-wm}"
readonly install_dir

# Verify the whole set before copying any of it. A half-installed directory
# is worse than a stale one: river `exec`s the WM as the session leader, so a
# missing or mismatched binary means the session dies and GDM bounces you
# back to the login screen with no shell to fix it from.
missing=()
for binary in "${BINARIES[@]}"; do
    [[ -x "$build_dir/$binary" ]] || missing+=("$binary")
done

if (( ${#missing[@]} > 0 )); then
    printf 'error: no release build found in %s\n' "$build_dir" >&2
    printf '  missing: %s\n' "${missing[*]}" >&2
    printf '  build first:\n' >&2
    printf '    cargo build --workspace --release\n' >&2
    exit 1
fi

install -d -m 0755 "$install_dir"
install -m 0755 -t "$install_dir" \
    "${BINARIES[@]/#/$build_dir/}"

printf 'installed %s -> %s\n' "${BINARIES[*]}" "$install_dir"

# Convenience only: nothing resolves the WM through PATH (river's init and
# waybar's module both use absolute paths, and buoy-tag-picker is found as a
# sibling). `current_exe()` reads /proc/self/exe, which resolves the symlink
# to the real path, so sibling lookup still works when launched via this.
# The two helper binaries stay out of PATH — `buoy-tag-picker` and `buoy-status-bar`
# are generic enough names to collide with something else one day.
bin_dir="$HOME/.local/bin"
if [[ -d "$bin_dir" ]]; then
    ln -sfn "$install_dir/buoy-wm" "$bin_dir/buoy-wm"
    printf 'linked %s -> %s\n' "$bin_dir/buoy-wm" "$install_dir/buoy-wm"
fi
