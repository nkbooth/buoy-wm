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
# verify.sh <target>: runs in a fresh container of the target's release
# under candidates.sh. release.yml's verify leg against the candidate:
# checksum, install, check-installed.sh and the three --version lines.
set -euo pipefail
target=$1
start=$(date +%s)
tar -xzf "/cand/sources/buoy-wm-$VERSION.tar.gz" -C /tmp
cd "/tmp/buoy-wm-$VERSION"
mkdir dist
cp "/cand/out/$target"/* dist/
(cd dist && sha256sum -c -- *.sha256)
case $target in
    fedora-*) dnf -y install ./dist/*.rpm ;;
    ubuntu-*)
        export DEBIAN_FRONTEND=noninteractive
        apt-get update -qq
        apt-get install -y --no-install-recommends ./dist/*.deb
        ;;
esac
scripts/check-installed.sh "$VERSION"
for binary in buoy-wm buoy-tag-picker buoy-status-bar; do
    "/usr/libexec/buoy-wm/$binary" --version
done
echo "VERIFY $target ok in $(($(date +%s) - start))s"
