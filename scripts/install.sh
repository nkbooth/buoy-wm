#!/usr/bin/env bash
# SPDX-FileCopyrightText: © 2026 Nick Booth
# SPDX-License-Identifier: 0BSD
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
# All three binaries land in ONE directory because the WM locates `tag-picker`
# as a sibling of its own `current_exe()` (see `tag_picker_path` in
# wm/src/main.rs) — splitting them across directories breaks the picker at
# runtime, not at install time.

set -euo pipefail

readonly BINARIES=(buoy-wm tag-picker status-bar)

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
readonly repo_root

readonly build_dir="$repo_root/target/release"
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
    printf '  build first, inside the devcontainer:\n' >&2
    printf '    cargo build --workspace --release\n' >&2
    exit 1
fi

install -d -m 0755 "$install_dir"
install -m 0755 -t "$install_dir" \
    "${BINARIES[@]/#/$build_dir/}"

printf 'installed %s -> %s\n' "${BINARIES[*]}" "$install_dir"

# Convenience only: nothing resolves the WM through PATH (river's init and
# waybar's module both use absolute paths, and tag-picker is found as a
# sibling). `current_exe()` reads /proc/self/exe, which resolves the symlink
# to the real path, so sibling lookup still works when launched via this.
# The two helper binaries stay out of PATH — `tag-picker` and `status-bar`
# are generic enough names to collide with something else one day.
bin_dir="$HOME/.local/bin"
if [[ -d "$bin_dir" ]]; then
    ln -sfn "$install_dir/buoy-wm" "$bin_dir/buoy-wm"
    printf 'linked %s -> %s\n' "$bin_dir/buoy-wm" "$install_dir/buoy-wm"
fi
