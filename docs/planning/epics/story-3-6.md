---
baseline_commit: 77b3811
---

# Story 3.6: Configurable libinput device settings (`[[input]]`)

Epic: 3 | Priority: H | Status: done

**Depends on Story 3.1/3.5** (`wm/src/config/mod.rs` — the `Config` type,
the TOML grammar, `Config::parse`/`load`, and the startup wiring that hands
the parsed config to the live `WindowManager`). This story adds a third
list to that grammar and a new pair of Wayland globals to consume it.

## Description

Reported from daily driving: the laptop touchpad moved the cursor and
scrolled, but **tap, double-tap and two-finger tap did nothing at all**.

The cause was not in buoy's pointer-binding code. buoy bound exactly three
globals — `river_window_manager_v1`, `river_xkb_bindings_v1`,
`river_layer_shell_v1` — and so never configured an input device in its
life. Every device therefore ran on raw libinput defaults, and libinput
ships **tap-to-click disabled** on any device that has physical buttons,
which is every laptop clickpad. Nothing was broken; nothing had ever
turned tap on.

**How that was established** (worth recording, because the first two
theories were both wrong):

1. The hardware was ruled out first. `/proc/bus/input/devices` shows the
   device as `PIXA3854:00 093A:0274 Touchpad` with `B: PROP=5`
   (`INPUT_PROP_POINTER | INPUT_PROP_BUTTONPAD`) and
   `B: KEY=e520 30000 0 0 0 0`, whose word-4 bits 16/17 are `BTN_LEFT`
   (0x110) and `BTN_RIGHT` (0x111). The kernel does emit a button.
2. buoy itself was ruled out: 11h uptime, zero stderr output, no panic.
3. The decisive measurement was a `WAYLAND_DEBUG=1 foot` probe, clicked
   into by hand. Over one interaction pass with the pointer resident on
   the client's surface it logged 1233 `wl_pointer.motion`, 184
   `wl_pointer.axis` (with `axis_source(1)` = finger, i.e. two-finger
   scroll), and **exactly two** `wl_pointer.button` events:
   `button(2413, 41208799, 272, 1)` / `button(2414, 41209052, 272, 0)` —
   a `BTN_LEFT` press and release 253ms apart, from the physical pad
   press. Nine seconds of one- and two-finger tapping either side of it
   produced not a single event.

   So motion, scroll and physical click all worked end to end. Only tap
   produced nothing, which is precisely the `tap` setting sitting at its
   libinput default.
4. The same probe's registry dump confirmed river 0.4.5 advertises
   `river_input_manager_v1` and `river_libinput_config_v1`, neither of
   which buoy bound.

An intermediate theory — that buoy's `Super`+click pointer bindings were
swallowing button events — was ruled out by step 3: the button event
reached the client normally. The reason a physical click *looked* dead to
the user is that the probe window was running `sleep`, which has nothing to
show for a click.

**What this story adds.** A `[[input]]` config list, applied over
`river-libinput-config-v1`, whose built-in default enables tap on
touchpads so a fresh session with no config file behaves like any other
desktop.

**Also in scope**: an unrelated `expect()` found while tracing the pointer
path, which could take the whole session down. See Task 5.

## Acceptance criteria

1. With no config file at all, a one-finger tap on a laptop touchpad is a
   left click, a two-finger tap is a right click, and tap-and-drag works.
2. `[[input]]` blocks match devices by libinput name with `*` wildcards,
   and configure `tap`, `tap_button_map`, `click_method`,
   `natural_scroll`, `disable_while_typing` and `accel_speed`.
3. An omitted setting sends no request, leaving libinput's own default in
   place; an explicit `false` actively disables the feature.
4. Declaring any `[[input]]` replaces the built-in list, matching the
   documented rule for `[[keybind]]`/`[[mousebind]]`; the three lists are
   independent of one another.
5. Entries are matched in file order, first match wins.
6. A config that could only ever be a silent no-op is rejected at load,
   naming the offender: empty `name`, an entry setting nothing, a
   duplicate `name`, an `accel_speed` outside `-1.0..=1.0`.
7. Absence of either new global is non-fatal: the WM logs once and every
   device keeps libinput's defaults, exactly as before this story.
8. A setting the device rejects (`unsupported`/`invalid`) is named on
   stderr rather than silently dropped.
9. A `window_interaction` event naming a window that is no longer managed
   does not panic.

## Tasks / Subtasks

- [x] **Task 1: Fix the `expect()` panic (AC: 9)** — done first and
  independently, since it is a live session-killer unrelated to the rest.
- [x] **Task 2: Vendor the protocols (AC: 2)**
  - [x] 2.1 `wm/protocol/river-input-management-v1.xml` and
    `wm/protocol/river-libinput-config-v1.xml`, taken from river tag
    `v0.4.5` to match the installed compositor.
  - [x] 2.2 Two new `generate_interfaces!` modules in `mod river`, plus
    two `generate_client_code!` calls. `rinput` needs
    `wayland_client::protocol::__interfaces::*` for `map_to_output`'s
    `wl_output`; `rlibinput` must import `rinput` because
    `river_libinput_device_v1.input_device` carries a
    `river_input_device_v1`.
- [x] **Task 3: `[[input]]` config (AC: 2, 3, 4, 5, 6)** — TDD.
  - [x] 3.1 RED/GREEN `wm/src/config/glob.rs`: `matches(pattern, name)`,
    11 tests. `*` only.
  - [x] 3.2 RED/GREEN `InputConfig`, `TapButtonMap`, `ClickMethod`, the
    four new `ConfigError` variants, `Config::inputs`,
    `Config::input_for`, and the built-in touchpad entry. 16 tests.
  - [x] 3.3 RED/GREEN `InputConfig::settings() -> Vec<LibinputSetting>`,
    the pure config-to-intent translation. 5 tests.
- [x] **Task 4: Bind and apply (AC: 1, 7, 8)**
  - [x] 4.1 Bind both globals in the registry handler, non-fatally.
  - [x] 4.2 `Dispatch` impls for the five new interfaces.
  - [x] 4.3 `WindowManager::configure_libinput_devices` and the
    `apply_libinput_setting` request fan-out.
- [x] **Task 5: Docs** — README section, `docs/config.example.toml`, this
  spec.

## Technical notes

**The correlation race.** The device *name* and the *configurable device*
arrive on two different objects created by two different globals:
`river_input_manager_v1.input_device` produces a `river_input_device_v1`
which then sends `name`, while `river_libinput_config_v1.libinput_device`
produces a `river_libinput_device_v1` which then sends `input_device`
pointing back at the former. Nothing in either protocol orders these
relative to each other, so neither arrival can be treated as "the" trigger.

Handled the same way Story 2.9 handles `wl_output` names: two maps
(`input_device_names` keyed by input-device id, `libinput_devices` keyed by
libinput-device id, the latter holding the correlation), and both arrival
paths call `configure_libinput_devices`, which configures every device that
has since become ready. A `configured` flag keeps that to once per device.
A device with no matching entry is marked configured too — leaving
libinput's defaults alone is a decision, not unfinished work.

**No manage sequence.** Neither new protocol mentions manage sequences
(`grep -c "manage sequence"` is 0 in both), unlike every window-management
request. So `set_*` can be sent the moment a device is known, with no
transaction to join. This is why the apply path lives outside
`handle_manage_start` entirely.

**Doubles as byte arrays.** Wayland has no float type, and this protocol
documents `type="array" summary="double"` as a native-endian IEEE-754
64-bit value. `set_accel_speed` therefore takes
`speed.to_ne_bytes().to_vec()`.

**`Config` lost its `Eq`.** `accel_speed` is an `f64`. Nothing needed total
equality — every use is `assert_eq!` in tests, which `PartialEq` covers.

**Name matching, not type matching.** `river_input_device_v1` reports a
`type` enum, but a touchpad, an external mouse and a trackball are all
`pointer`, and forcing tap on a mouse is not wanted. The kernel names
every touchpad with "Touchpad" in it, so `*Touchpad*` is the built-in
pattern; anything it misses gets its own entry, which is what the section
is for.

**Replace, not merge.** The list is taken wholesale when the user declares
any entry, consistent with `[[keybind]]`/`[[mousebind]]`. Merging was
considered — a device you never mentioned arguably shouldn't lose its
default — and rejected: with name globs, merge has no unambiguous answer
for a device matching both a built-in and a user entry, and a third
different rule for the third list is worse than one the user already
knows. The cost is the documented footgun that adding an entry for some
other device drops the touchpad default, so `docs/config.example.toml`
ships the built-in entry written out and both it and the README call the
trap out explicitly.

**Settings deliberately not exposed.** The protocol carries roughly twenty
more (`send_events`, `drag`, `drag_lock`, `three_finger_drag`,
`middle_emulation`, `scroll_method`, `scroll_button`, `left_handed`,
`rotation`, `calibration_matrix`, `dwtp`, `accel_profile`, the custom
accel-curve objects, plus `set_repeat_info`/`set_scroll_factor` on the
input device itself). None is needed to fix the reported bug and none was
requested; each is one match arm in `apply_libinput_setting` plus one
field, so adding one on demand is cheap. Tap-and-drag and double-tap
needed nothing extra — they are the same libinput `tap` feature, and
libinput defaults `drag` to enabled whenever tap is.

**Logging posture.** Every device sends `tap_current`/`click_method_current`
regardless of configuration, so logging all of them would bury the
touchpad under keyboards and lid switches. Both are gated on the device
having a matching `[[input]]` entry. river re-sends `*_current` after a
successful `set_*`, which makes the second line the confirmation that the
setting took effect — the specific thing that was unverifiable while
diagnosing this bug. `success` results stay quiet; `unsupported` and
`invalid` are named, with the device and setting, because a dropped
`unsupported` makes a setting that cannot work on this device look
identical to one that was never configured.

**Task 5's `expect()`.** `manage_seats` did
`.position(...).expect("Interacted window not found")` on the window named
by a `window_interaction` event. That event and the `closed` event which
removes the window from `self.windows` race, with no ordering promised, so
a click that outlived its target aborted the WM — and the WM is the
session leader, so that is a black screen and a bounce to GDM (NFR2). Now
a labelled block: a miss logs and skips only the raise/reorder, leaving
focus-latch expiry, `focus_top`, `do_action` and op handling to run
normally. A labelled block rather than `continue`, which would skip the
rest of the loop body, and rather than re-indenting forty lines into a
`match`.

## Test plan

348 workspace tests pass (`cargo test --workspace`), up from 316:
`+27` config, `+5` settings translation. Clippy clean at `-D warnings`.

Untested, and why: everything in Task 4 is Wayland protocol glue in
`main.rs` — proxies cannot be constructed without a live compositor. This
is the same carve-out that leaves the rest of `main.rs` covered only by the
three pure `tag_picker_path` tests. The story's response is to push every
decision that *can* be pure into `config` and test it exhaustively there
(`glob::matches`, `InputConfig::matches`, `Config::input_for`,
`InputConfig::settings`), leaving `apply_libinput_setting` as a mechanical
one-arm-per-variant match with no branching logic of its own, and
`configure_libinput_devices` as the only genuinely stateful piece.

Live verification is therefore the acceptance gate for AC 1, 7 and 8, and
is what the `WAYLAND_DEBUG` probe above exists to re-run.

## FR coverage

Not an FR — a defect fix against daily-driver use, plus the config surface
needed to make it configurable rather than hardcoded. NFR2 (no crash on
malformed input) covers Task 1 and the four new load-time rejections.

## Dev Agent Record

- Diagnosis is recorded in Description rather than here because two of the
  three initial theories were wrong, and the measurement that settled it
  (a `WAYLAND_DEBUG=1` client, counting `wl_pointer` event kinds) is worth
  reusing the next time input behaves oddly.
- `event_created_child!` was initially added to
  `Dispatch<RiverLibinputDeviceV1>` for the result objects. That was wrong:
  `river_libinput_result_v1` is created by *requests*, not events, so its
  udata is supplied at the call site. Only server-created new_ids need the
  macro — here, `input_device` on the manager and `libinput_device` on the
  config global.
- Four `_ => {}` arms were removed after `unreachable_pattern` warnings;
  these generated event enums are exhaustive, unlike
  `river_libinput_device_v1`'s, which has enough variants to still need
  one.
