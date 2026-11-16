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
"""Drive a qemu VM over QMP: screendump, typed text, key chords, absolute clicks.

usage:
  qmp.py <sock> shot <out.png>
  qmp.py <sock> type <text>            (ascii letters, digits, a few symbols)
  qmp.py <sock> keys <chord> [...]     (chord like ret, tab, meta_l-esc)
  qmp.py <sock> click <x> <y> <w> <h>  (pixel coords on a w x h screen; needs a tablet device)
  qmp.py <sock> quit
"""
import json
import socket
import sys
import time

SHIFTED = {'!': '1', '@': '2', '#': '3', '$': '4', '%': '5', '^': '6', '&': '7',
           '*': '8', '(': '9', ')': '0', '_': 'minus', '+': 'equal', ':': 'semicolon'}
PLAIN = {'-': 'minus', '=': 'equal', '.': 'dot', ',': 'comma', '/': 'slash',
         ' ': 'spc', ';': 'semicolon'}


class Qmp:
    """One QMP connection, capabilities negotiated."""

    def __init__(self, path: str) -> None:
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.connect(path)
        self.file = self.sock.makefile('rw')
        self._read()
        self.cmd('qmp_capabilities')

    def _read(self) -> dict:
        while True:
            msg = json.loads(self.file.readline())
            if 'event' not in msg:
                return msg

    def cmd(self, name: str, **args) -> dict:
        """Run one command; its reply, or RuntimeError on a QMP error."""
        self.file.write(json.dumps({'execute': name, 'arguments': args}) + '\n')
        self.file.flush()
        reply = self._read()
        if 'error' in reply:
            raise RuntimeError(f'{name}: {reply["error"]}')
        return reply


def chord(q: Qmp, spec: str) -> None:
    """Press and release a qcode chord such as ret or meta_l-esc."""
    keys = [{'type': 'qcode', 'data': k} for k in spec.split('-')]
    q.cmd('send-key', keys=keys, **{'hold-time': 80})
    time.sleep(0.12)


def type_text(q: Qmp, text: str) -> None:
    """Type text key by key on a US layout."""
    for ch in text:
        if ch.isalpha():
            chord(q, f'shift-{ch.lower()}' if ch.isupper() else ch)
        elif ch.isdigit():
            chord(q, ch)
        elif ch in SHIFTED:
            chord(q, f'shift-{SHIFTED[ch]}')
        elif ch in PLAIN:
            chord(q, PLAIN[ch])
        else:
            raise ValueError(f'no key for {ch!r}')


def click(q: Qmp, x: int, y: int, w: int, h: int) -> None:
    """Left-click at pixel (x, y) of a w x h screen through the tablet device."""
    ax, ay = x * 0x7FFF // w, y * 0x7FFF // h
    move = [{'type': 'abs', 'data': {'axis': 'x', 'value': ax}},
            {'type': 'abs', 'data': {'axis': 'y', 'value': ay}}]
    q.cmd('input-send-event', events=move)
    time.sleep(0.2)
    for down in (True, False):
        q.cmd('input-send-event', events=[{'type': 'btn', 'data': {'down': down, 'button': 'left'}}])
        time.sleep(0.1)


# How many arguments each operation takes after its name; None is one or more.
ARITY = {"shot": 1, "type": 1, "keys": None, "click": 4, "quit": 0}


def main(argv: list[str]) -> None:
    """Run one operation from the command line."""
    if len(argv) < 3 or argv[2] not in ARITY:
        raise SystemExit(__doc__)
    op, rest = argv[2], argv[3:]
    want = ARITY[op]
    if (want is None and not rest) or (want is not None and len(rest) != want):
        raise SystemExit(__doc__)
    q = Qmp(argv[1])
    if op == 'shot':
        q.cmd('screendump', filename=rest[0], format='png')
    elif op == 'type':
        type_text(q, rest[0])
    elif op == 'keys':
        for spec in rest:
            chord(q, spec)
    elif op == 'click':
        click(q, *map(int, rest))
    elif op == 'quit':
        q.cmd('quit')
    else:
        raise SystemExit(__doc__)


if __name__ == '__main__':
    main(sys.argv)
