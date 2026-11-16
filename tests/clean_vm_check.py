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
"""clean_vm_check.py <README.md> <generator> <rows-file>: hold readme_blocks.py and vms.py to README.md.

<rows-file> holds the Supported platforms table as tests/lib.sh's
platform_rows prints it (name, tier, river cell, tab-separated), so the
table has one reader. For each tier-1 row this builds the VM's script in
both modes and requires, from README.md parsed here and not by the
generator's own parser:
  - each block the VM runs (package block, PPA block on Ubuntu, the
    multiplexer route, the Status bar file) appears whole, contiguous and
    in the README's order; pre-tag, with each `curl -o "$workdir/X"` line
    as `cp ~/cand/"X" "$workdir/"`; on the release a
    "# Ubuntu X.Y: this line instead" comment names, with that swap made;
  - every other line is the harness's own (a short, fixed set);
and across the table: vms.py names exactly the tier-1 rows, every route
to a multiplexer the README gives is run by some VM, and each VM expects
monitor-only screen sharing exactly when its river cell says so.
Prints one line per failure; exits 1 if there was any.
"""
import pathlib
import re
import subprocess
import sys
import tempfile

# The harness lines a generated script may carry beyond the README's own.
HARNESS = [r"#.*", r"set -e", r"cd ~", r"mkdir -p \S+", r"cat > \S+ <<'BUOY_README_BLOCK'",
           r"BUOY_README_BLOCK", r"rpm -q buoy-wm", r"dpkg-query -W buoy-wm", r"zellij --version",
           r"stat -c '%a %n' ~/\.config/buoy/config\.toml", r"systemctl --user is-enabled waybar\.service", r""]
INSTEAD = re.compile(r"^# Ubuntu ([0-9]+\.[0-9]+): this line instead of the one above\.$")

failures: list[str] = []


def fail(message: str) -> None:
    failures.append(message)


def blocks(lines: list[str], heading: str, lang: str) -> list[list[str]]:
    """The ```lang blocks under the exact heading line, to the next heading outside a fence."""
    if heading not in lines:
        fail(f"README.md: no heading line {heading!r}")
        return []
    found, current, fence = [], None, False
    for line in lines[lines.index(heading) + 1:]:
        if line.startswith("```"):
            fence = not fence
            if line == f"```{lang}":
                current = []
            elif line == "```" and current is not None:
                found.append(current)
                current = None
            continue
        if not fence and re.match(r"#+ ", line):
            break
        if current is not None:
            current.append(line)
    return found


def pre_tag(block: list[str]) -> list[str]:
    out = []
    for line in block:
        m = re.match(r'curl .*-o "\$workdir/([^"]+)"', line)
        out.append(f'cp ~/cand/"{m.group(1)}" "$workdir/"' if m else line)
    return out


def swapped(block: list[str], release: str) -> list[str]:
    out = list(block)
    for i, line in enumerate(block):
        m = INSTEAD.match(line)
        if m and release == f"Ubuntu {m.group(1)}":
            above = max(j for j in range(i) if block[j].startswith("sudo add-apt-repository "))
            out[above] = f"# {block[above]}"
            out[i + 1] = block[i + 1][2:]
    return out


def find(script: list[str], segment: list[str], start: int) -> int:
    """Index at or after start where segment occurs contiguously, or -1."""
    n = len(segment)
    for i in range(start, len(script) - n + 1):
        if script[i:i + n] == segment:
            return i
    return -1


def generate(generator: str, readme: str, vm: str, mode: str, out: pathlib.Path) -> list[str] | None:
    args = ["python3", "-I", generator, readme, vm, str(out)] + (["--no-substitute"] if mode == "post" else [])
    run = subprocess.run(args, capture_output=True, text=True)
    if run.returncode != 0:
        fail(f"{vm} ({mode}): readme_blocks.py failed: {(run.stderr or run.stdout).strip()}")
        return None
    return out.read_text().splitlines()


def check_script(vm: str, mode: str, script: list[str], segments: list[tuple[str, list[str]]]) -> None:
    covered = [False] * len(script)
    at = 0
    for what, segment in segments:
        i = find(script, segment, at)
        if i < 0:
            fail(f"{vm} ({mode}): the README's {what} is not in the script whole and in order")
            continue
        covered[i:i + len(segment)] = [True] * len(segment)
        at = i + len(segment)
    for line, ok in zip(script, covered):
        if not ok and not any(re.fullmatch(p, line) for p in HARNESS):
            fail(f"{vm} ({mode}): line {line!r} is neither the README's nor the harness's")


def main(readme: str, generator: str, rows_file: str) -> int:
    sys.path.insert(0, str(pathlib.Path(generator).resolve().parent))
    from vms import VMS

    lines = pathlib.Path(readme).read_text().splitlines()
    rows = [r.split("\t") for r in pathlib.Path(rows_file).read_text().splitlines() if r]
    tier1 = {name: river for name, tier, river in rows if tier == "1"}
    if not tier1:
        fail("README.md: no tier-1 rows in Supported platforms")
    by_release = {entry.release: name for name, entry in VMS.items()}
    names = {re.sub(r" LTS$", "", n) for n in tier1}
    if names != set(by_release):
        fail(f"vms.py: releases {sorted(by_release)} are not README's tier-1 rows {sorted(names)}")

    rpm = blocks(lines, "### Fedora 44 and 45: the RPM", "sh")
    ubuntu = blocks(lines, "### Ubuntu 26.04 and 26.10: river from a PPA, then the .deb", "sh")
    zellij = blocks(lines, "## Requirements", "sh")
    toml = blocks(lines, "## Requirements", "toml")
    jsonc = blocks(lines, "## Status bar", "jsonc")
    for what, found, count in [("Fedora sh", rpm, 1), ("Ubuntu sh", ubuntu, 2), ("Requirements sh", zellij, 1),
                               ("Requirements toml", toml, 1), ("Status bar jsonc", jsonc, 1)]:
        if len(found) != count:
            fail(f"README.md: {len(found)} {what} blocks, expected {count}")
    if failures:
        return 1
    text = "\n".join(lines)
    start = lines.index("## Requirements")
    end = next(i for i in range(start + 1, len(lines)) if lines[i].startswith("## "))
    requirements = "\n".join(lines[start:end])
    cargo = []
    for command in (r"sudo dnf install cargo perl-core", r"cargo install --locked zellij",
                    r"sudo install ~/\.cargo/bin/zellij /usr/local/bin/"):
        m = re.search(rf"`({command})`", requirements)
        if m:
            cargo.append(m.group(1))
    chmod = ["chmod 600 ~/.config/buoy/config.toml"]
    enable_span = re.search(r"`(systemctl --user enable\s+waybar\.service)`", text)
    enable = [" ".join(enable_span.group(1).split())] if enable_span else []
    if not enable:
        fail("README.md: no `systemctl --user enable waybar.service` in inline code")
    routes = {"asset": [zellij[0]], "cargo": [cargo], "tmux": [toml[0] + ["BUOY_README_BLOCK"], chmod]}
    if len(cargo) != 3:
        fail(f"README.md: {len(cargo)} of the three cargo-route commands in inline code")
    used = {VMS[by_release[r]].multiplexer for r in names if r in by_release}
    if used != set(routes):
        fail(f"vms.py: routes run {sorted(used)}, the README gives {sorted(routes)}")

    with tempfile.TemporaryDirectory() as tmp:
        for name, river in tier1.items():
            release = re.sub(r" LTS$", "", name)
            vm = by_release.get(release)
            if vm is None:
                continue
            want = "u 1" if "monitor-only" in river else "u 3"
            if VMS[vm].source_types != want:
                fail(f"vms.py: {vm} expects {VMS[vm].source_types!r}, the README's river cell implies {want!r}")
            for mode in ("pre", "post"):
                script = generate(generator, readme, vm, mode, pathlib.Path(tmp) / f"{vm}-{mode}.sh")
                if script is None:
                    continue
                if vm.startswith("f"):
                    package = [("RPM block", pre_tag(rpm[0]) if mode == "pre" else rpm[0])]
                else:
                    package = [("PPA block", swapped(ubuntu[0], release)),
                               (".deb block", pre_tag(ubuntu[1]) if mode == "pre" else ubuntu[1])]
                route = VMS[vm].multiplexer
                segments = package + [(f"{route} route", s) for s in routes.get(route, [["<unknown route>"]])]
                segments += [("Status bar file", jsonc[0] + ["BUOY_README_BLOCK"]), ("waybar enable", enable)]
                check_script(vm, mode, script, segments)
    for message in failures:
        print(message, file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    if len(sys.argv) != 4:
        raise SystemExit(__doc__)
    sys.exit(main(*sys.argv[1:]))
