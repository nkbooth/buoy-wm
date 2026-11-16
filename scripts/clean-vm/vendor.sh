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
# vendor.sh: runs in ubuntu:26.04 under candidates.sh. The vendor tarball
# as release.yml's build job makes it (the same cargo vendor and tar
# lines), from Source0 in /cand/sources. VERSION and SOURCE_DATE_EPOCH come
# from candidates.sh.
set -euo pipefail
apt-get update -qq
apt-get install -y -qq --no-install-recommends cargo ca-certificates >/dev/null
tar -xzf "/cand/sources/buoy-wm-$VERSION.tar.gz" -C /tmp
cd "/tmp/buoy-wm-$VERSION"
cargo vendor --locked --versioned-dirs vendor >/dev/null
vendor="buoy-wm-${VERSION}-vendor.tar.gz"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$SOURCE_DATE_EPOCH" -I 'gzip -n' -cf "/cand/sources/$vendor" vendor
ls -l /cand/sources
