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
"""images.py <img-dir>: download every VM's cloud image and check its sha256.

The sum comes from the checksum file published beside the image (Fedora's
CHECKSUM, Ubuntu's SHA256SUMS); their GPG signatures are not checked, so
this catches a corrupt or replaced download, not a compromised mirror
serving both. An image already present is kept if it matches; otherwise
(absent, or Ubuntu republished a point build under the same name) it is
downloaded once. Exits non-zero naming any image that does not match.
"""
import hashlib
import pathlib
import re
import sys
import urllib.request

# Run with -I, which leaves the script's own directory off sys.path.
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from vms import VMS, Vm  # noqa: E402


def fetch(url: str, dest: pathlib.Path) -> None:
    """Download url to dest."""
    with urllib.request.urlopen(url) as reply, open(dest, "wb") as out:
        while chunk := reply.read(1 << 20):
            out.write(chunk)


def sha256(path: pathlib.Path) -> str:
    """The file's sha256 as hex."""
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            digest.update(chunk)
    return digest.hexdigest()


def published(entry: Vm) -> str:
    """The image's hex digest from its checksum file (Fedora BSD style or Ubuntu *file)."""
    with urllib.request.urlopen(entry.checksum_url) as reply:
        text = reply.read().decode()
    image = re.escape(entry.image)
    m = re.search(rf"^SHA256 \({image}\) = ([0-9a-f]{{64}})$", text, re.M) or \
        re.search(rf"^([0-9a-f]{{64}}) \*{image}$", text, re.M)
    if not m:
        raise SystemExit(f"{entry.checksum_url} names no sha256 for {entry.image}")
    return m.group(1)


def main(img_dir: pathlib.Path) -> int:
    """Fetch and check every image; 1 if any does not match its sum."""
    img_dir.mkdir(parents=True, exist_ok=True)
    bad = []
    for name, entry in VMS.items():
        want = published(entry)
        path = img_dir / entry.image
        if not (path.exists() and sha256(path) == want):
            fetch(entry.image_url, path)
        if sha256(path) == want:
            print(f"OK {name} {entry.image} {want}")
        else:
            bad.append(entry.image)
            print(f"BAD {name} {entry.image}: does not match {want}", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    sys.exit(main(pathlib.Path(sys.argv[1])))
