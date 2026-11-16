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
# leg.sh <target>: runs in the target's release container under
# candidates.sh. release.yml's packages leg from Source0 and the vendor
# tarball: build-rpm.sh on Fedora, build-deb.sh plus lintian on Ubuntu.
set -euo pipefail
target=$1
start=$(date +%s)
tar -xzf "/cand/sources/buoy-wm-$VERSION.tar.gz" -C /tmp
cd "/tmp/buoy-wm-$VERSION"
mkdir sources
cp "/cand/sources/buoy-wm-$VERSION.tar.gz" "/cand/sources/buoy-wm-$VERSION-vendor.tar.gz" sources/
case $target in
    fedora-*)
        dnf -y install git-core rpm-build dnf5-plugins
        dnf -y builddep packaging/rpm/buoy-wm.spec
        scripts/build-rpm.sh sources dist
        ;;
    ubuntu-*)
        export DEBIAN_FRONTEND=noninteractive
        apt-get update
        apt-get build-dep -y ./packaging/deb
        apt-get install -y --no-install-recommends lintian
        scripts/build-deb.sh sources dist
        lintian --fail-on error --no-tag-display-limit dist/*.deb
        ;;
    *)
        echo "unknown target $target" >&2
        exit 2
        ;;
esac
mkdir -p "/cand/out/$target"
cp dist/* "/cand/out/$target/"
echo "LEG $target ok in $(($(date +%s) - start))s"
