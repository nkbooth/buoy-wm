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
"""The tier-1 VMs of the clean-machine check, one per README tier-1 release.

`python3 -I vms.py` prints `<name> <release>` per line;
`vms.py image|memory|target|gdm <name>` prints one field for the shell
scripts. tests/clean-vm.sh holds this table to README.md: the releases to
the Supported platforms tier-1 rows, `source_types` to the rows that say
monitor-only, and the routes to every way to a multiplexer the README
names.
"""
import sys
from dataclasses import dataclass

FEDORA = "https://dl.fedoraproject.org/pub/fedora/linux/releases"
UBUNTU = "https://cloud-images.ubuntu.com/releases"


@dataclass(frozen=True)
class Vm:
    """One tier-1 release: its cloud image and what its run proves."""

    release: str
    image: str
    image_url: str
    checksum_url: str
    # Which README route to a multiplexer this VM proves: "asset" (the
    # Requirements block), "cargo" (cargo install, then copied) or "tmux"
    # (the [defaults] block).
    multiplexer: str
    # AvailableSourceTypes as busctl prints it: "u 1" is monitors only,
    # "u 3" adds windows. Fedora's river 0.5.0 and 26.10's PPA river 0.4.5
    # capture windows; 26.04's PPA river 0.4.1 cannot (its README row says
    # monitor-only).
    source_types: str
    # Where the greeter draws `buoy` in the session menu and the password
    # field, on the 1280x800 screen, read from rc.3's gear screenshots
    # (shots/<vm>-r4-gear.png). Fedora 45 and Ubuntu 26.10 lay both out
    # differently from their predecessors.
    gdm_buoy: tuple[int, int]
    gdm_password: tuple[int, int]
    memory_mib: int = 4096


def fedora(n: int, build: str, multiplexer: str, gdm_buoy: tuple[int, int],
           gdm_password: tuple[int, int], memory_mib: int = 4096) -> Vm:
    """A Fedora Cloud Base image and its CHECKSUM file."""
    image = f"Fedora-Cloud-Base-Generic-{n}-{build}.x86_64.qcow2"
    base = f"{FEDORA}/{n}/Cloud/x86_64/images"
    return Vm(f"Fedora {n}", image, f"{base}/{image}",
              f"{base}/Fedora-Cloud-{n}-{build}-x86_64-CHECKSUM",
              multiplexer, "u 3", gdm_buoy, gdm_password, memory_mib)


def ubuntu(version: str, path: str, multiplexer: str, source_types: str,
           gdm_buoy: tuple[int, int], gdm_password: tuple[int, int]) -> Vm:
    """An Ubuntu server cloud image and its SHA256SUMS file."""
    image = f"ubuntu-{version}-server-cloudimg-amd64.img"
    base = f"{UBUNTU}/{path}/release"
    return Vm(f"Ubuntu {version}", image, f"{base}/{image}", f"{base}/SHA256SUMS",
              multiplexer, source_types, gdm_buoy, gdm_password)


VMS = {
    "f44": fedora(44, "1.7", "asset", (1112, 577), (620, 489)),
    # cargo install zellij is OOM-killed in 4 GiB beside GNOME.
    "f45": fedora(45, "1.4", "cargo", (1124, 565), (600, 450), memory_mib=8192),
    "u2604": ubuntu("26.04", "26.04", "asset", "u 1", (1123, 604), (620, 488)),
    # releases/26.10/ answered 403 on 2026-11-15; the codename path is the
    # same release.
    "u2610": ubuntu("26.10", "stonking", "tmux", "u 3", (1123, 604), (600, 471)),
}


def vm(name: str) -> Vm:
    """The VM called name; exits naming the known ones otherwise."""
    if name not in VMS:
        raise SystemExit(f"unknown VM {name!r}; known: {' '.join(VMS)}")
    return VMS[name]


def field(query: str, name: str) -> str:
    """One VM's value for a shell script's query (image, memory, target, gdm)."""
    entry = vm(name)
    if query == "image":
        return entry.image
    if query == "memory":
        return str(entry.memory_mib)
    if query == "target":
        # release.yml's matrix name, which candidates.sh builds under.
        return entry.release.lower().replace(" ", "-")
    if query == "gdm":
        return " ".join(map(str, entry.gdm_buoy + entry.gdm_password))
    raise SystemExit(__doc__)


if __name__ == "__main__":
    if len(sys.argv) == 3:
        print(field(sys.argv[1], sys.argv[2]))
    elif len(sys.argv) == 1:
        for name, entry in VMS.items():
            print(name, entry.release)
    else:
        raise SystemExit(__doc__)
