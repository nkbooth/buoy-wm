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
# Fails if any file named on the command line lacks the RPL-1.5 SPDX
# identifier near its top. RPL 1.5 §6.4(a) requires the notice to travel
# with every source file, which is why the 16-line block is replicated
# across the tree rather than deduplicated — the only part of that which
# can be automated is noticing a new file that forgot it.
#
# Scanned window rather than a fixed line: wm/src/main.rs carries a
# third-party attribution notice above its SPDX line (see NOTICE.md).
set -euo pipefail

readonly HEADER_WINDOW_LINES=20
status=0

for file in "$@"; do
    if ! head -n "$HEADER_WINDOW_LINES" "$file" |
        grep -q 'SPDX-License-Identifier: RPL-1.5'; then
        echo "$file: missing the RPL-1.5 SPDX licence header" >&2
        status=1
    fi
done

exit "$status"
