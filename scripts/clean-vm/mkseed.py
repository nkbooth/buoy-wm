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
"""mkseed.py <state-dir> <user> <password>: the ssh key and a NoCloud seed per VM.

Writes <state-dir>/id_vm (made once, never committed) and
<state-dir>/seed-<vm>/{meta-data,user-data,vendor-data}. The account is a
throwaway on a throwaway VM: password login for GDM, the key for ssh,
passwordless sudo so the README's own sudo lines run unattended. A seed
that exists is left alone: its instance-id is the one the VM's desktop
layer was built under, and a new one would re-run cloud-init's
per-instance steps on every overlay of it.
"""
import pathlib
import subprocess
import sys

# Run with -I, which leaves the script's own directory off sys.path.
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from vms import VMS  # noqa: E402


def main(state: pathlib.Path, user: str, password: str) -> None:
    """Make the key if missing and a seed for every VM that has none."""
    key = state / "id_vm"
    if not key.exists():
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "buoy-vm", "-f", str(key)],
                       check=True)
    public = (state / "id_vm.pub").read_text().strip()
    for name in VMS:
        group = "wheel" if name.startswith("f") else "sudo"
        seed = state / f"seed-{name}"
        if (seed / "user-data").exists():
            continue
        seed.mkdir(parents=True, exist_ok=True)
        (seed / "meta-data").write_text(f"instance-id: {name}-clean-vm\nlocal-hostname: {name}\n")
        (seed / "vendor-data").write_text("")
        (seed / "user-data").write_text(f"""#cloud-config
users:
  - name: {user}
    groups: [{group}]
    shell: /bin/bash
    sudo: "ALL=(ALL) NOPASSWD:ALL"
    lock_passwd: false
    plain_text_passwd: {password}
    ssh_authorized_keys: ["{public}"]
ssh_pwauth: false
""")
    print(f"key {key}, seeds for {' '.join(VMS)}")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        raise SystemExit(__doc__)
    main(pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3])
