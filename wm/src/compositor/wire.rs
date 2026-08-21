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

//! Translation from the user's config vocabulary into the protocol's.
//!
//! # Rationale
//!
//! Every table here is total over a small config enum, and a swapped arm
//! is silently wrong for the whole session — the user's touchpad does the
//! other thing and nothing fails. They are separated from the one function
//! that needs a live protocol object ([`apply_libinput_setting`]) so that
//! all of them can be asserted arm by arm; before that split nothing could
//! check any of them (audit finding T-01).

use buoy_wm::config;

use crate::compositor::manager::AppData;
use crate::compositor::river;
use crate::compositor::river::river_libinput_device_v1::RiverLibinputDeviceV1;
use crate::compositor::river::river_seat_v1::Modifiers;
use wayland_client::QueueHandle;

/// Sends the one `river_libinput_device_v1` request `setting` stands for.
///
/// Each request allocates a `river_libinput_result_v1` whose udata is a
/// description of what was attempted, so an `unsupported`/`invalid` reply
/// names the device and setting it came from instead of arriving bare.
/// Neither input protocol mentions manage sequences, so these can be sent
/// as soon as a device is known rather than inside a transaction.
pub(crate) fn apply_libinput_setting(
    device: &RiverLibinputDeviceV1,
    setting: config::LibinputSetting,
    device_name: &str,
    qh: &QueueHandle<AppData>,
) {
    use config::LibinputSetting;
    let label = |what: &str| format!("libinput {device_name:?}: {what}");
    match setting {
        LibinputSetting::Tap(enabled) => {
            device.set_tap(wire_tap_state(enabled), qh, label("tap"));
        }
        LibinputSetting::TapButtonMap(map) => {
            device.set_tap_button_map(wire_tap_button_map(map), qh, label("tap_button_map"));
        }
        LibinputSetting::ClickMethod(method) => {
            device.set_click_method(wire_click_method(method), qh, label("click_method"));
        }
        LibinputSetting::NaturalScroll(enabled) => {
            device.set_natural_scroll(
                wire_natural_scroll_state(enabled),
                qh,
                label("natural_scroll"),
            );
        }
        LibinputSetting::DisableWhileTyping(enabled) => {
            device.set_dwt(wire_dwt_state(enabled), qh, label("disable_while_typing"));
        }
        LibinputSetting::AccelSpeed(speed) => {
            // The protocol carries doubles as a native-endian byte array —
            // Wayland has no float type, and this protocol's own preamble
            // documents `type="array" summary="double"` as exactly that.
            device.set_accel_speed(speed.to_ne_bytes().to_vec(), qh, label("accel_speed"));
        }
    }
}

/// Translates a config `tap_button_map` into the protocol's enum.
///
/// Extracted from [`apply_libinput_setting`], which needs a live protocol
/// object and so could not be tested at all: a swapped `Lrm`/`Lmr` arm here
/// silently gives the user the other two-finger-tap button for the whole
/// session and nothing would have failed (audit finding T-01).
pub(crate) fn wire_tap_button_map(
    map: config::TapButtonMap,
) -> river::river_libinput_device_v1::TapButtonMap {
    use crate::compositor::river::river_libinput_device_v1::TapButtonMap as Wire;
    match map {
        config::TapButtonMap::Lrm => Wire::Lrm,
        config::TapButtonMap::Lmr => Wire::Lmr,
    }
}

/// Translates a config `click_method` into the protocol's enum. Extracted
/// for the same reason as [`wire_tap_button_map`].
pub(crate) fn wire_click_method(
    method: config::ClickMethod,
) -> river::river_libinput_device_v1::ClickMethod {
    use crate::compositor::river::river_libinput_device_v1::ClickMethod as Wire;
    match method {
        config::ClickMethod::None => Wire::None,
        config::ClickMethod::ButtonAreas => Wire::ButtonAreas,
        config::ClickMethod::Clickfinger => Wire::Clickfinger,
    }
}

/// Translates a config `tap` boolean into the protocol's enum.
pub(crate) fn wire_tap_state(enabled: bool) -> river::river_libinput_device_v1::TapState {
    use crate::compositor::river::river_libinput_device_v1::TapState as Wire;
    if enabled {
        Wire::Enabled
    } else {
        Wire::Disabled
    }
}

/// Translates a config `natural_scroll` boolean into the protocol's enum.
pub(crate) fn wire_natural_scroll_state(
    enabled: bool,
) -> river::river_libinput_device_v1::NaturalScrollState {
    use crate::compositor::river::river_libinput_device_v1::NaturalScrollState as Wire;
    if enabled {
        Wire::Enabled
    } else {
        Wire::Disabled
    }
}

/// Translates a config `disable_while_typing` boolean into the protocol's
/// enum.
pub(crate) fn wire_dwt_state(enabled: bool) -> river::river_libinput_device_v1::DwtState {
    use crate::compositor::river::river_libinput_device_v1::DwtState as Wire;
    if enabled {
        Wire::Enabled
    } else {
        Wire::Disabled
    }
}

/// Translates a config modifier set into the protocol's bitfield.
pub(crate) fn river_modifiers(mods: &[config::Modifier]) -> Modifiers {
    mods.iter().fold(Modifiers::empty(), |acc, modifier| {
        acc | match modifier {
            config::Modifier::Super => Modifiers::Mod4,
            config::Modifier::Ctrl => Modifiers::Ctrl,
            config::Modifier::Alt => Modifiers::Mod1,
            config::Modifier::Shift => Modifiers::Shift,
        }
    })
}

/// Linux input event codes for the three pointer buttons river's
/// `create_pointer_binding` takes, from `linux/input-event-codes.h`.
const BTN_LEFT: u32 = 0x110;

const BTN_RIGHT: u32 = 0x111;

const BTN_MIDDLE: u32 = 0x112;

/// Translates a config pointer button into its Linux input event code.
pub(crate) fn input_event_code(button: config::Button) -> u32 {
    match button {
        config::Button::Left => BTN_LEFT,
        config::Button::Right => BTN_RIGHT,
        config::Button::Middle => BTN_MIDDLE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A swapped arm in any of these five is a setting the user configured
    /// silently doing the other thing for the whole session, and until they
    /// were pulled out of `apply_libinput_setting` — which needs a live
    /// protocol object — nothing could check them (audit finding T-01).
    #[test]
    fn each_libinput_tap_button_map_maps_to_its_own_protocol_value() {
        use crate::compositor::river::river_libinput_device_v1::TapButtonMap as Wire;

        assert_eq!(wire_tap_button_map(config::TapButtonMap::Lrm), Wire::Lrm);
        assert_eq!(wire_tap_button_map(config::TapButtonMap::Lmr), Wire::Lmr);
    }

    #[test]
    fn each_libinput_click_method_maps_to_its_own_protocol_value() {
        use crate::compositor::river::river_libinput_device_v1::ClickMethod as Wire;

        assert_eq!(wire_click_method(config::ClickMethod::None), Wire::None);
        assert_eq!(
            wire_click_method(config::ClickMethod::ButtonAreas),
            Wire::ButtonAreas
        );
        assert_eq!(
            wire_click_method(config::ClickMethod::Clickfinger),
            Wire::Clickfinger
        );
    }

    #[test]
    fn the_three_boolean_libinput_settings_enable_on_true_and_disable_on_false() {
        use crate::compositor::river::river_libinput_device_v1::{
            DwtState, NaturalScrollState, TapState,
        };

        assert_eq!(wire_tap_state(true), TapState::Enabled);
        assert_eq!(wire_tap_state(false), TapState::Disabled);
        assert_eq!(wire_natural_scroll_state(true), NaturalScrollState::Enabled);
        assert_eq!(
            wire_natural_scroll_state(false),
            NaturalScrollState::Disabled
        );
        assert_eq!(wire_dwt_state(true), DwtState::Enabled);
        assert_eq!(wire_dwt_state(false), DwtState::Disabled);
    }

    #[test]
    fn each_config_modifier_maps_to_its_own_protocol_bit() {
        assert_eq!(river_modifiers(&[]), Modifiers::empty());
        assert_eq!(
            river_modifiers(&[config::Modifier::Super]),
            Modifiers::Mod4,
            "Super is Mod4, not Mod1 — the one mapping in this table that is \
             not its own name"
        );
        assert_eq!(river_modifiers(&[config::Modifier::Ctrl]), Modifiers::Ctrl);
        assert_eq!(river_modifiers(&[config::Modifier::Alt]), Modifiers::Mod1);
        assert_eq!(
            river_modifiers(&[config::Modifier::Shift]),
            Modifiers::Shift
        );
    }

    #[test]
    fn a_modifier_set_is_the_union_of_its_members_bits() {
        assert_eq!(
            river_modifiers(&[config::Modifier::Super, config::Modifier::Shift]),
            Modifiers::Mod4 | Modifiers::Shift
        );
        // Repeats are a union, not a toggle: `[[keybind]] modifiers =
        // ["super", "super"]` must not cancel itself out.
        assert_eq!(
            river_modifiers(&[config::Modifier::Super, config::Modifier::Super]),
            Modifiers::Mod4
        );
    }

    #[test]
    fn each_pointer_button_maps_to_its_own_linux_event_code() {
        assert_eq!(input_event_code(config::Button::Left), 0x110);
        assert_eq!(input_event_code(config::Button::Right), 0x111);
        assert_eq!(input_event_code(config::Button::Middle), 0x112);
    }
}
