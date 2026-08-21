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
# `postCreateCommand` for both devcontainer configs. A script rather than a
# 900-character JSON string so it can be read, reviewed and shellcheck'd —
# this file installs software into the environment every build, test and
# lint of this project then runs inside, so its trust level is the trust
# level of the whole build.
set -euo pipefail

# Pinned by content, not by tag: a mutable git tag can be moved, and this
# script is piped into a root shell. Regenerate with
#   curl -fsSL "$ACT_INSTALLER_URL" | sha256sum
ACT_VERSION=v0.2.89
ACT_INSTALLER_COMMIT=4f411281417e88660bea1c1a1749aa71ae0bd60f
ACT_INSTALLER_SHA256=11abcff86ac5ce8b147d4c4ddb4fcee710874adaaf546dc82b7881ee7e0a5778
ACT_INSTALLER_URL="https://raw.githubusercontent.com/nektos/act/${ACT_INSTALLER_COMMIT}/install.sh"

install_build_deps() {
    sudo apt-get update
    sudo apt-get install -y --no-install-recommends \
        libwayland-dev pkg-config pre-commit ca-certificates curl
}

# Debian suite for the Docker apt repo is derived from /etc/os-release (not
# hardcoded) so this survives the base image being retagged to a different
# Debian release without 404ing on `apt-get update`.
install_docker_cli() {
    sudo install -m 0755 -d /etc/apt/keyrings
    curl --proto '=https' --tlsv1.2 -fsSL https://download.docker.com/linux/debian/gpg \
        | sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg
    sudo chmod a+r /etc/apt/keyrings/docker.gpg
    # shellcheck disable=SC1091  # provided by the base image, not this repo
    . /etc/os-release
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/debian ${VERSION_CODENAME} stable" \
        | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
    sudo apt-get update
    sudo apt-get install -y --no-install-recommends docker-ce-cli
    sudo rm -rf /var/lib/apt/lists/*
}

# `act` runs CI locally without a GitHub remote. It needs a container engine
# to launch runner images, which only the `act` devcontainer config wires up
# — see .devcontainer/act/devcontainer.json.
install_act() {
    local installer
    installer="$(mktemp)"
    curl --proto '=https' --tlsv1.2 -fsSL "$ACT_INSTALLER_URL" -o "$installer"
    echo "${ACT_INSTALLER_SHA256}  ${installer}" | sha256sum -c -
    sudo bash "$installer" -b /usr/local/bin "$ACT_VERSION"
    rm -f "$installer"
}

# The socket-path resolver has no `/tmp` fallback (it was squattable, and
# a production fallback existed only for this container's benefit), so a
# binary run by hand in here needs a real runtime directory. Mode 0700
# because the WM verifies exactly that before binding.
create_runtime_dir() {
    install -d -m 700 "${XDG_RUNTIME_DIR:-/tmp/xdg-runtime-vscode}"
}

install_build_deps
install_docker_cli
install_act
create_runtime_dir
