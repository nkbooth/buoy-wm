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
"""summary.py <state-dir> <label>: one verdict table per VM from session.sh's logs.

Reads runs/<vm>-<label>-{check,logout,relogin}.log for every VM in vms.py
and prints `== <vm>: passed/total`, then a FAIL line per failed check;
the logout line also carries the gap between the two target stops. A
missing log or section is a failed check, not an error. Exits 1 if any
check failed on any VM.
"""
import pathlib
import re
import sys

# Run with -I, which leaves the script's own directory off sys.path.
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from vms import VMS, Vm  # noqa: E402


def read(path: pathlib.Path) -> str:
    return path.read_text() if path.exists() else ""


def section(text: str, name: str) -> str | None:
    """The body of `== <name>` up to the next `== ` heading, or None if absent."""
    parts = text.split(f"== {name}")
    return parts[1].split("\n== ")[0] if len(parts) > 1 else None


def checks(entry: Vm, c: str, lo: str, rl: str) -> dict[str, bool]:
    """Every verdict for one VM, True where the evidence shows it."""
    def has(name: str, test) -> bool:
        body = section(c, name)
        return body is not None and test(body)

    survivors = section(lo, "survivors")
    return {
        "logs present": bool(c and lo and rl),
        "b tree gdm→river→buoy-session→buoy-wm": has("(b) process tree", lambda t: all(
            s in t for s in ["gdm-wayland-session", "river -c /usr/libexec/buoy-wm/buoy-session",
                             "/usr/libexec/buoy-wm/buoy-wm"])),
        "b XDG_CURRENT_DESKTOP=buoy": "XDG_CURRENT_DESKTOP=buoy" in c,
        "b both targets active": bool(re.search(r"buoy-session.target\s+active", c))
        and bool(re.search(r"graphical-session.target\s+active", c)),
        "b no failed units": has("(b) failed units", lambda t: t.strip() == ""),
        "b WM logs as buoy-wm, errors at err": has(
            "(b) journal identity", lambda t: "switch-tag from" in t and "rejected" in t),
        "c three portals active": all(re.search(rf"{u}\s+active", c) for u in [
            "xdg-desktop-portal.service", "xdg-desktop-portal-wlr.service", "xdg-desktop-portal-gtk.service"]),
        "c no backend errors": has("(c) portal backend errors", lambda t: "(none)" in t),
        f"c AvailableSourceTypes {entry.source_types}": has(
            "(c) AvailableSourceTypes", lambda t: t.strip() == entry.source_types),
        "c Screenshot portal answers": has("(c) Screenshot portal", lambda t: bool(re.match(r"u \d+$", t.strip()))),
        "d waybar active, buoy-status-bar child": bool(re.search(r"waybar.service\s+active", c))
        and has("(d) waybar children", lambda t: "buoy-status-bar 0" in t),
        "e outputs + current_tag": '"outputs":[{"id":0,"current_tag":0}]' in c,
        "e pinned terminal + multiplexer session": has(
            "(e) pinned terminals", lambda t: "pinned-term-0" in t and "tag-default" in t),
        "f stop order buoy before graphical": bool(re.search(
            r"Stopped target buoy-session\.target.*\n.*Stopped target graphical-session\.target", lo)),
        "f targets inactive after logout": bool(re.search(r"buoy-session.target\s+inactive", lo))
        and bool(re.search(r"graphical-session.target\s+inactive", lo)),
        "f no survivors": survivors is not None and "(none)" in survivors,
        "f user manager kept (ssh hold)": "sleep 3000" in lo,
        "f relogin targets active": bool(re.search(r"buoy-session.target\s+active", rl))
        and bool(re.search(r"graphical-session.target\s+active", rl)),
        "f relogin no start-limit": "start request repeated: 0" in rl and "failed: 0" in rl,
    }


def stop_gap(lo: str) -> str:
    """The time between the two target stops, across midnight too."""
    stops = re.findall(r"(\d\d:\d\d:\d\d\.\d+) \S+ systemd\[\d+\]: Stopped target (buoy-session|graphical-session)", lo)
    if len(stops) < 2:
        return ""
    t = [sum(float(x) * m for x, m in zip(s[0].split(":"), (3600, 60, 1))) for s in stops[-2:]]
    return f" (stop gap {1000 * ((t[1] - t[0]) % 86400):.0f} ms)"


def main(state: pathlib.Path, label: str) -> int:
    """Print every VM's table; 1 if any check failed."""
    runs = state / "runs"
    failed = False
    for vm, entry in VMS.items():
        c, lo, rl = (read(runs / f"{vm}-{label}-{kind}.log") for kind in ("check", "logout", "relogin"))
        rows = checks(entry, c, lo, rl)
        print(f"== {vm}: {sum(rows.values())}/{len(rows)}{stop_gap(lo)}")
        for name, ok in rows.items():
            if not ok:
                failed = True
                print(f"   FAIL {name}")
    return 1 if failed else 0


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    sys.exit(main(pathlib.Path(sys.argv[1]), sys.argv[2]))
