<!--
SPDX-FileCopyrightText: © 2026 Nick Booth
Portions © 2026 Julian Andrews, originally distributed under 0BSD as
part of tinyrwm <https://codeberg.org/river/tinyrwm>.
SPDX-License-Identifier: RPL-1.5

Unless explicitly acquired and licensed from Licensor under another
license, the contents of this file are subject to the Reciprocal Public
License ("RPL") Version 1.5, or subsequent versions as allowed by the
RPL, and You may not copy or use this file in either source code or
executable form, except in compliance with the terms and conditions of
the RPL.

All software distributed under the RPL is provided strictly on an "AS
IS" basis, WITHOUT WARRANTY OF ANY KIND, EITHER EXPRESS OR IMPLIED, AND
LICENSOR HEREBY DISCLAIMS ALL SUCH WARRANTIES, INCLUDING WITHOUT
LIMITATION, ANY WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
PURPOSE, QUIET ENJOYMENT, OR NON-INFRINGEMENT. See the RPL for specific
language governing rights and limitations under the RPL.
-->

# buoy-wm

The window manager binary of [buoy](https://github.com/nkbooth/buoy-wm) — a
keyboard-driven window manager for the river Wayland compositor.

This crate owns all tag state, speaks `river-window-management-v1`, and
serves the IPC socket that the `buoy-tag-picker` and `buoy-status-bar` companion
binaries consume. It is not useful on its own: the WM locates `buoy-tag-picker` as
a sibling of its own executable, so install the whole workspace together.

See the [project README](../README.md) for the tag model, keybindings,
configuration and installation.

## Layout

| Path | Contents |
| --- | --- |
| `src/wm_core/` | Tag, view and output state — pure logic, no Wayland types |
| `src/config/` | TOML schema, keysym resolution, glob matching |
| `src/ipc/` | Unix-socket server and the JSON wire protocol |
| `src/compositor/` | Wayland glue: generated protocol bindings, `Dispatch` impls, the proxy-carrying records, keybind execution, process spawning |
| `src/main.rs` | Composition root: connect, load config, bind the socket, dispatch until the connection ends |
| `protocol/` | Vendored river protocol XML (MIT — see [NOTICE.md](../NOTICE.md)) |
