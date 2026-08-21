// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: RPL-1.5
//
// Unless explicitly acquired and licensed from Licensor under another
// license, the contents of this file are subject to the Reciprocal Public
// License ("RPL") Version 1.5, or subsequent versions as allowed by the
// RPL, and You may not copy or use this file in either source code or
// executable form, except in compliance with the terms and conditions of
// the RPL.
//
// All software distributed under the RPL is provided strictly on an "AS
// IS" basis, WITHOUT WARRANTY OF ANY KIND, EITHER EXPRESS OR IMPLIED, AND
// LICENSOR HEREBY DISCLAIMS ALL SUCH WARRANTIES, INCLUDING WITHOUT
// LIMITATION, ANY WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
// PURPOSE, QUIET ENJOYMENT, OR NON-INFRINGEMENT. See the RPL for specific
// language governing rights and limitations under the RPL.

//! The `wayland-scanner`-generated `river` protocol bindings.
//!
//! # Rationale
//!
//! A module of its own rather than a block inside `main.rs`, and still
//! inside the *binary* rather than behind `buoy_wm`'s library boundary.
//! `wayland-scanner` marks every generated `Event` enum
//! `#[non_exhaustive]`, which is exhaustive to match on inside its own
//! crate and not across a crate boundary — so moving these bindings into
//! the library while the eleven `Dispatch::event` matches stay here would
//! force eleven `_ =>` arms and turn "the vendored protocol XML grew an
//! event" from a compile error into a silently ignored message. Within one
//! crate a file boundary costs nothing (audit finding J-10).
//!
//! The `"./protocol/..."` paths are resolved by `wayland-scanner` against
//! `$CARGO_MANIFEST_DIR`, not against this file, so they are unchanged by
//! the move — the audit's warning that they would break from a
//! subdirectory does not hold.

pub extern crate wayland_client;
pub use wayland_client::protocol::*;

mod interfaces {
    pub(super) mod rwm {
        pub use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocol/river-window-management-v1.xml");
    }

    pub(super) mod rxkb {
        use super::rwm::*;
        wayland_scanner::generate_interfaces!("./protocol/river-xkb-bindings-v1.xml");
    }

    pub(super) mod rlayer {
        use super::rwm::*;
        wayland_scanner::generate_interfaces!("./protocol/river-layer-shell-v1.xml");
    }

    pub(super) mod rinput {
        // Needs `wl_output` in scope for `map_to_output`, and nothing
        // from `rwm` — input management is a standalone tree, unlike
        // `rxkb`/`rlayer` which both hang off `river_seat_v1`.
        pub use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocol/river-input-management-v1.xml");
    }

    pub(super) mod rlibinput {
        // `river_libinput_device_v1.input_device` carries a
        // `river_input_device_v1`, so this must be generated after (and
        // importing) `rinput`.
        use super::rinput::*;
        wayland_scanner::generate_interfaces!("./protocol/river-libinput-config-v1.xml");
    }
}

use self::interfaces::rinput::*;
use self::interfaces::rlayer::*;
use self::interfaces::rlibinput::*;
use self::interfaces::rwm::*;
use self::interfaces::rxkb::*;
wayland_scanner::generate_client_code!("./protocol/river-window-management-v1.xml");
wayland_scanner::generate_client_code!("./protocol/river-xkb-bindings-v1.xml");
wayland_scanner::generate_client_code!("./protocol/river-layer-shell-v1.xml");
wayland_scanner::generate_client_code!("./protocol/river-input-management-v1.xml");
wayland_scanner::generate_client_code!("./protocol/river-libinput-config-v1.xml");
