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
"""answer.py <log> -- <ssh argv...>

Runs the command under ssh -tt with stdin a pipe, streams output to <log>, and types an
answer only when a known interactive prompt appears at the end of the output, as a person
would: 'y' for dnf/apt confirmations, Enter for add-apt-repository. Each answer is logged.
A run that prints nothing for IDLE_LIMIT seconds is waiting on a prompt this does not
know (a dnf key import, a conffile question): it is killed, logged, and exits 124.
"""
import os
import re
import select
import subprocess
import sys
import time

# Longer than the slowest silent step seen (cargo install zellij compiles
# for minutes between lines, 13 minutes in all on rc.3's Fedora 45).
IDLE_LIMIT = 1800
PROMPTS = [
    (re.compile(rb"Is this ok \[y/N\]: ?$"), b"y\r", "dnf confirm"),
    (re.compile(rb"Do you want to continue\? \[Y/n\] ?$"), b"y\r", "apt confirm"),
    (re.compile(rb"^Continue\? \[Y/n\] ?$", re.M), b"y\r", "apt 3 confirm"),
    (re.compile(rb"Press \[ENTER\] to continue or Ctrl-c to cancel\.?\s*$"), b"\r", "add-apt-repository Enter"),
]


def run(log_path: str, argv: list[str]) -> int:
    """Run argv, answering known prompts, logging everything; its exit status, or 124 when idle."""
    proc = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    tail = b""
    last_output = time.monotonic()
    with open(log_path, "ab") as log:
        while True:
            ready, _, _ = select.select([proc.stdout], [], [], 2.0)
            if ready:
                chunk = os.read(proc.stdout.fileno(), 65536)
                if not chunk:
                    break
                log.write(chunk)
                log.flush()
                tail = (tail + chunk)[-400:]
                last_output = time.monotonic()
            elif time.monotonic() - last_output > IDLE_LIMIT:
                proc.kill()
                log.write(f"\n[answer.py] {time.strftime('%H:%M:%S')} no output for {IDLE_LIMIT}s, "
                          f"killed; last lines: {tail[-200:]!r}\n".encode())
                proc.wait()
                return 124
            stripped = re.sub(rb"\x1b\[[0-9;?]*[A-Za-z]", b"", tail).rstrip(b"\r\n") if not ready else b""
            for pattern, answer, label in PROMPTS:
                if stripped and pattern.search(stripped):
                    time.sleep(0.5)
                    proc.stdin.write(answer)
                    proc.stdin.flush()
                    log.write(f"\n[answer.py] {time.strftime('%H:%M:%S')} {label}: sent {answer!r}\n".encode())
                    tail = b""
                    break
        proc.wait()
    return proc.returncode


if __name__ == "__main__":
    if len(sys.argv) < 4 or sys.argv[2] != "--":
        raise SystemExit(__doc__)
    sys.exit(run(sys.argv[1], sys.argv[3:]))
