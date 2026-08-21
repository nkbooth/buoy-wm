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

//! The libinput devices river has told us about, and the `[[input]]`
//! entries applied to them.
//!
//! # Rationale
//!
//! A registry of its own rather than two more maps on the WM aggregate.
//! The three functions here read nothing else the aggregate owns — not
//! `windows`, not `outputs`, not `seats`, not `wm_core` — and two of those
//! maps had already leaked memory and needed their own pruning commits,
//! which is what ownership belonging to nobody looks like (audit finding
//! J-10). With the maps private, adding and forgetting a device is this
//! module's job and there is one place to get it wrong.
//!
//! The two halves of a device's identity arrive on two different objects
//! in an order the protocol does not specify: the configurable device
//! comes from `river_libinput_config_v1` and its *name* from a
//! `river_input_device_v1` reached through a second event. So both arrival
//! paths call [`LibinputRegistry::configure`], whichever lands second is
//! the one that finds a device ready, and `configured` is what keeps it to
//! once.

use std::collections::HashMap;

use buoy_common::log_info;
use buoy_wm::config::{self, Config};
use wayland_backend::client::ObjectId;
use wayland_client::{Proxy, QueueHandle};

use crate::compositor::manager::AppData;
use crate::compositor::river::river_libinput_device_v1::RiverLibinputDeviceV1;
use crate::compositor::wire::apply_libinput_setting;

/// A libinput-configurable device, and the correlation state needed before
/// its `[[input]]` entry can be applied.
#[derive(Debug)]
struct LibinputDevice {
    proxy: RiverLibinputDeviceV1,
    /// The `river_input_device_v1` this configures, from the `input_device`
    /// event. `None` until that arrives — the name lives on that object, so
    /// there is nothing to match a config entry against until then.
    input_device_id: Option<ObjectId>,
    /// Whether the config has already been applied. The two halves of the
    /// device's identity arrive in an unspecified order, so both arrival
    /// paths attempt to configure and this is what keeps it to once.
    configured: bool,
}

/// Every `river_libinput_device_v1` river has created, plus the device
/// names needed to match them against `[[input]]` entries.
#[derive(Debug, Default)]
pub(crate) struct LibinputRegistry {
    /// Each `river_input_device_v1`'s name, keyed by that proxy's object
    /// id. Held separately from `devices` because the two objects are
    /// created by two different globals and the protocol makes no promise
    /// about which arrives first.
    device_names: HashMap<ObjectId, String>,
    /// Each `river_libinput_device_v1`, keyed by its own object id.
    devices: HashMap<ObjectId, LibinputDevice>,
}

impl LibinputRegistry {
    /// Records an input device's name, from its `name` event.
    pub(crate) fn record_device_name(&mut self, input_device_id: ObjectId, name: String) {
        self.device_names.insert(input_device_id, name);
    }

    /// Drops an input device's name, from its `removed` event. Paired with
    /// [`LibinputRegistry::record_device_name`] so this map cannot grow
    /// without bound across a session's worth of hotplugs.
    pub(crate) fn forget_device_name(&mut self, input_device_id: &ObjectId) {
        self.device_names.remove(input_device_id);
    }

    /// Records a newly created configurable device, before anything is
    /// known about which physical device it is.
    pub(crate) fn add_device(&mut self, proxy: RiverLibinputDeviceV1) {
        self.devices.insert(
            proxy.id(),
            LibinputDevice {
                proxy,
                input_device_id: None,
                configured: false,
            },
        );
    }

    /// Drops a configurable device, from its `removed` event.
    pub(crate) fn remove_device(&mut self, libinput_id: &ObjectId) {
        self.devices.remove(libinput_id);
    }

    /// Ties a configurable device to the `river_input_device_v1` whose name
    /// identifies it, from the `input_device` event. A device that is not
    /// registered is ignored rather than resurrected: its `removed` event
    /// has already arrived (NFR2).
    pub(crate) fn correlate_input_device(
        &mut self,
        libinput_id: &ObjectId,
        input_device_id: ObjectId,
    ) {
        if let Some(device) = self.devices.get_mut(libinput_id) {
            device.input_device_id = Some(input_device_id);
        }
    }

    /// The device name behind a `river_libinput_device_v1`, once both its
    /// `input_device` event and that device's `name` event have arrived.
    fn device_name(&self, libinput_id: &ObjectId) -> Option<&str> {
        let input_device_id = self.devices.get(libinput_id)?.input_device_id.as_ref()?;
        self.device_names.get(input_device_id).map(String::as_str)
    }

    /// The name of the device behind `libinput_id`, but only when the user
    /// actually has an `[[input]]` entry for it — the gate that keeps
    /// device-state logging to devices the config speaks about.
    pub(crate) fn configured_device_name(
        &self,
        libinput_id: &ObjectId,
        config: &Config,
    ) -> Option<&str> {
        let name = self.device_name(libinput_id)?;
        config.input_for(name).map(|_| name)
    }

    /// Applies each device's matching `[[input]]` entry, once.
    ///
    /// A device with no matching entry is marked configured too: leaving
    /// libinput's defaults alone is a decision, not unfinished work, and
    /// re-deciding it on every later event would be pointless.
    pub(crate) fn configure(&mut self, config: &Config, qh: &QueueHandle<AppData>) {
        let ready: Vec<(ObjectId, Option<Vec<config::LibinputSetting>>, String)> = self
            .devices
            .iter()
            .filter(|(_, device)| !device.configured)
            .filter_map(|(id, _)| {
                let name = self.device_name(id)?.to_string();
                let settings = config.input_for(&name).map(|input| input.settings());
                Some((id.clone(), settings, name))
            })
            .collect();
        for (id, settings, name) in ready {
            // Borrowed separately from the scan above: `device_name` needs
            // `&self` while sending the requests needs the device mutably,
            // and the two cannot overlap.
            let Some(device) = self.devices.get_mut(&id) else {
                continue;
            };
            device.configured = true;
            let Some(settings) = settings else {
                continue;
            };
            log_info!(
                "libinput {name:?}: applying {} setting(s) from [[input]]",
                settings.len()
            );
            for setting in settings {
                apply_libinput_setting(&device.proxy, setting, &name, qh);
            }
        }
    }
}
