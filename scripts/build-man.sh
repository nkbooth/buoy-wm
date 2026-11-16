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
# build-man.sh <outdir>: renders every docs/man/<name>.scd with scdoc into
# <outdir>/<name>. The one build of the man pages: the spec's %build and
# debian/rules run it, and tests/man.sh runs it on every push. A missing
# scdoc, an empty page set or a page scdoc rejects is an error, never a
# skip, because a skipped build is a package that ships without its pages.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: build-man.sh <outdir>" >&2
    exit 2
fi
readonly out=$1
src="$(cd "$(dirname "${BASH_SOURCE[0]}")/../docs/man" && pwd)"
readonly src

if ! command -v scdoc >/dev/null 2>&1; then
    echo "build-man.sh: scdoc is not installed (install the scdoc package)" >&2
    exit 1
fi

shopt -s nullglob
pages=("$src"/*.scd)
if [ "${#pages[@]}" -eq 0 ]; then
    echo "build-man.sh: no docs/man/*.scd pages to build in $src" >&2
    exit 1
fi

mkdir -p "$out"
for page in "${pages[@]}"; do
    name="${page##*/}"
    if ! scdoc <"$page" >"$out/${name%.scd}"; then
        echo "build-man.sh: scdoc rejected docs/man/$name" >&2
        exit 1
    fi
done
