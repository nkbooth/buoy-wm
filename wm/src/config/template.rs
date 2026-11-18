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

//! The built-in configuration, written out as a commented config file.
//!
//! # Rationale
//!
//! `docs/config.example.toml` used to be a hand-kept copy of
//! [`Config::default`], and a hand-kept copy drifts: its comments claimed
//! values the code no longer had. It is now this module's output under a
//! licence header, held byte-for-byte by a test, so the only way to change
//! what it says is to change the defaults.
//!
//! Hand-rendered rather than serialized: no config type derives
//! `Serialize`, because [`Action`]'s hand-written `Debug` exists to keep an
//! `exec` command line out of accidental output (audit finding G-05) and a
//! derived serializer would be another such path. The round-trip tests
//! below are what keep a hand renderer honest. The one public entry point
//! renders the built-ins, which hold no `exec`.

use super::{
    Action, ClickMethod, Config, Defaults, InputConfig, Keybind, Modifier, Mousebind, TapButtonMap,
};

/// The built-in configuration as a complete, commented config file: what
/// `buoy-wm --print-default-config` prints, and what
/// `docs/config.example.toml` holds under its licence header.
pub fn default_config_toml() -> String {
    render(&Config::default())
}

/// `config` as a config file that parses back to it. Every value written
/// comes from `config`; the prose around the values is fixed.
fn render(config: &Config) -> String {
    let Config {
        defaults,
        keybinds,
        mousebinds,
        inputs,
    } = config;
    let mut out = String::from(PREAMBLE);
    render_defaults(&mut out, defaults);
    out.push_str(KEYBIND_PROSE);
    for keybind in keybinds {
        render_keybind(&mut out, keybind);
    }
    out.push_str(KEYBIND_EXAMPLES);
    out.push_str(MOUSEBIND_PROSE);
    for mousebind in mousebinds {
        render_mousebind(&mut out, mousebind);
    }
    out.push_str(INPUT_PROSE);
    for input in inputs {
        render_input(&mut out, input);
    }
    out.push_str(INPUT_EXAMPLES);
    out
}

const PREAMBLE: &str = "\
# buoy-wm configuration: every built-in default, written out.
#
# This is what `buoy-wm --print-default-config` prints, and exactly what
# buoy-wm runs with when there is no config file at all. To start your own
# config from it:
#   buoy-wm --print-default-config > ~/.config/buoy/config.toml
#   chmod 600 ~/.config/buoy/config.toml
# Every section is optional. A [defaults] key you delete keeps its built-in
# value, and a list you delete entirely ([[keybind]], [[mousebind]] or
# [[input]]) comes back as the built-in list written below.
#
# An entry the parser can read but the WM cannot use — an unknown key name,
# an uppercase letter without \"shift\", `move` or `resize` on a key, a
# duplicate binding, or an [[input]] block with an empty or repeated
# `name`, no setting or an out-of-range `accel_speed` — is skipped and
# reported, and everything else still applies. Anything the parser itself
# refuses rejects the whole file: invalid TOML, a misspelled field, action,
# modifier, button or input value, a wrong type, a missing `key`, `button`,
# `action` or `name`, and any wrong value in [defaults]. A rejected file
# falls back to the built-in defaults in full, keybinds included. Either way
# a desktop notification names the problem, and
#   journalctl --user -b --identifier=buoy-wm -p err
# shows it in the journal. `man 5 buoy-config` has the full reference.

[defaults]
";

fn render_defaults(out: &mut String, defaults: &Defaults) {
    let Defaults {
        terminal,
        launcher,
        default_tag,
        pinned_terminal_args,
        pinned_terminals,
    } = defaults;
    out.push_str("# Spawned by the `terminal` action, and used for each tag's pinned terminal.\n");
    out.push_str(&format!("terminal = {}\n", toml_string(terminal)));
    out.push_str(
        "\
# Spawned by the `launcher` action, and only that. `fuzzel` stays hardcoded
# where it is driven as a menu rather than a launcher — the tag pickers and
# the `hotkeys` cheat-sheet — since those pass fuzzel-specific flags.
",
    );
    out.push_str(&format!("launcher = {}\n", toml_string(launcher)));
    out.push_str("# The tag created and shown at login when no tag exists yet.\n");
    out.push_str(&format!("default_tag = {}\n", toml_string(default_tag)));
    out.push_str(
        "\
# Argv passed to `terminal` to spawn a tag's pinned terminal. `{app_id}` is
# required — it is how the WM recognizes that window, and which tag it
# belongs to, and is replaced per tag with `pinned-term-<tag id>` — and
# `{session}` is replaced with `tag-<tag name>`, the session name for
# whichever multiplexer runs there. Both are replaced inside each argument,
# never through a shell. foot sets a window's app id with `-a`; most other
# terminals use `--class`, e.g.
#   pinned_terminal_args = [\"--class\", \"{app_id}\", \"-e\", \"zellij\", \"attach\", \"--create\", \"{session}\"]
",
    );
    let args: Vec<String> = pinned_terminal_args
        .iter()
        .map(|arg| toml_string(arg))
        .collect();
    out.push_str(&format!("pinned_terminal_args = [{}]\n", args.join(", ")));
    out.push_str(
        "\
# Whether each tag gets a pinned terminal at all. Turn it off — this is the
# supported way — if your terminal or zellij is broken, or if you simply do
# not want one; tagging, the pickers and the status bar are unaffected. An
# empty `pinned_terminal_args` is NOT the off switch and is rejected as a
# mistake, because it would spawn a terminal that nothing could recognize.
",
    );
    out.push_str(&format!("pinned_terminals = {pinned_terminals}\n"));
}

const KEYBIND_PROSE: &str = "
# --- Keybinds ---------------------------------------------------------------
#
# Declaring ANY [[keybind]] replaces the entire built-in set, so list every
# binding you want. Mousebinds are a separate list with the same rule. When
# your list leaves a built-in action bound to nothing, a notification at
# login names it; binding that action to a different key is not a drop.
#
#   mod    = any of \"super\" (\"mod4\", \"logo\", \"win\"), \"ctrl\" (\"control\"),
#            \"alt\" (\"mod1\"), \"shift\". Optional — omit it for an unmodified
#            key. The PascalCase spellings (\"Super\", \"Ctrl\", …) are
#            accepted as well, and always will be.
#   key    = a keysym name: a single character (\"x\", \"1\", \"?\"), one of the
#            named keys `man 5 buoy-config` lists (\"Return\", \"Tab\",
#            \"Escape\", …), or a function key (\"F1\"..\"F35\", \"f1\" too).
#            Named keys are case-insensitive;
#            single characters are NOT — \"x\" and \"X\" are different keysyms,
#            and an uppercase letter is the *shifted* symbol, so it must be
#            bound with \"shift\" or it can never fire. Write \"x\", not \"X\",
#            for Super+x. (Checked at load; a binding that fails is skipped
#            and reported, not silently dead.)
#   action = a bare string, or a single-key table for the two that take an
#            argument.
#
# Actions: terminal, launcher, close, focus_next, exit, cycle_tag,
#          open_assign_picker, open_switch_picker, hotkeys,
#          { switch_tag = \"<name>\" }, { exec = \"<command line>\" }
# The older spellings `tag_picker` and `tag_switch` still work; they were
# renamed because `tag_switch` (open the picker) and `switch_tag` (switch
# now) were one transposition apart.
# `move` and `resize` drive a pointer drag, so they are mousebind-only; a
# mousebind takes any action.
#
# Binding the same mod+key twice is reported rather than a silent race: the
# second binding is skipped. Two bindings count as the same only when they
# are written the same way: [\"super\", \"shift\"] and [\"shift\", \"super\"], or
# \"Space\" and \"space\", are not caught.
";

fn render_keybind(out: &mut String, keybind: &Keybind) {
    let Keybind { mods, key, action } = keybind;
    out.push_str(&format!(
        "\n[[keybind]]\nmod = {}\nkey = {}\naction = {}\n",
        modifier_list(mods),
        toml_string(key),
        action_value(action)
    ));
}

const KEYBIND_EXAMPLES: &str = "
# Not built in — uncomment to add. A named tag is created the first time its
# bind is pressed, so `switch_tag` works on a fresh session with no setup.
# `exec` runs its command line through `sh -c`, so pipes, arguments and `~`
# work as they would in a shell. Uncommenting one keeps every binding above,
# because they are still listed.
#
# [[keybind]]
# mod = [\"super\"]
# key = \"1\"
# action = { switch_tag = \"email\" }
#
# [[keybind]]
# mod = [\"super\"]
# key = \"p\"
# action = { exec = \"grim -g \\\"$(slurp)\\\" ~/screenshot.png\" }
";

const MOUSEBIND_PROSE: &str = "
# --- Mousebinds -------------------------------------------------------------
#
# button = \"left\", \"right\" or \"middle\"
";

fn render_mousebind(out: &mut String, mousebind: &Mousebind) {
    let Mousebind {
        mods,
        button,
        action,
    } = mousebind;
    out.push_str(&format!(
        "\n[[mousebind]]\nmod = {}\nbutton = {}\naction = {}\n",
        modifier_list(mods),
        toml_string(button.config_name()),
        action_value(action)
    ));
}

const INPUT_PROSE: &str = "
# --- Input devices ----------------------------------------------------------
#
# Each [[input]] block configures every libinput device whose name matches
# `name`. Find the names with `libinput list-devices` (needs root), or read
# them out of `/proc/bus/input/devices`.
#
#   name  = a pattern matched against the device name. `*` matches any run of
#           characters and is the ONLY wildcard — `?` and `[...]` are literal.
#           Case-sensitive. Matched top to bottom, first match wins, so put
#           specific devices above a catch-all `name = \"*\"`.
#
# Every setting is optional, and OMITTING one is not the same as setting it
# to false: an omitted setting sends no request at all, leaving libinput's
# own default in place, while `tap = false` actively disables tap. That is
# why there is no way to write \"leave this alone\" explicitly — just don't
# list it. The commented-out settings below are ones the built-in entries
# leave alone.
#
# As with [[keybind]]/[[mousebind]], declaring ANY [[input]] replaces the
# whole built-in list, which is every [[input]] block below. If you add a
# block for some other device, restate these too; a notification at login
# names each built-in setting your list no longer applies.
#
# Skipped and reported at load (rather than looking configured and doing
# nothing): an empty `name`, a block that sets nothing, a repeated `name`,
# and an `accel_speed` outside -1.0..=1.0. A `name` that matches no device
# is accepted silently, so check it against `libinput list-devices`.
";

fn render_input(out: &mut String, input: &InputConfig) {
    let InputConfig {
        name,
        tap,
        tap_button_map,
        click_method,
        natural_scroll,
        disable_while_typing,
        accel_speed,
    } = input;
    out.push_str(&format!("\n[[input]]\nname = {}\n", toml_string(name)));
    out.push_str(
        "\
# Tap-to-click. libinput ships it DISABLED on any device that has physical
# buttons, which is every laptop clickpad, so without it a touchpad moves
# the cursor and scrolls but ignores taps. Tap-and-drag and double-tap come
# along with it — they are the same libinput feature.
",
    );
    setting(out, "tap", tap.map(|on| on.to_string()), "true");
    out.push_str(&format!(
        "# Which button each finger count taps: {lrm} = 1/2/3 fingers ->\n\
         # left/right/middle, {lmr} = left/middle/right.\n",
        lrm = toml_string(TapButtonMap::Lrm.config_name()),
        lmr = toml_string(TapButtonMap::Lmr.config_name()),
    ));
    setting(
        out,
        "tap_button_map",
        tap_button_map.map(|map| toml_string(map.config_name())),
        &toml_string(TapButtonMap::Lrm.config_name()),
    );
    out.push_str(&format!(
        "# How a PHYSICAL press resolves to a button, independent of tap:\n\
         #   {areas:<14} — the bottom of the pad is split into left/right zones\n\
         #   {finger:<14} — the finger count decides, anywhere on the pad\n\
         #   {none:<14} — no physical click buttons\n\
         # Leave unset to keep whatever the device defaults to; for any device\n\
         # configured here the WM logs `click method is …` and `tap-to-click is …`.\n",
        areas = toml_string(ClickMethod::ButtonAreas.config_name()),
        finger = toml_string(ClickMethod::Clickfinger.config_name()),
        none = toml_string(ClickMethod::None.config_name()),
    ));
    setting(
        out,
        "click_method",
        click_method.map(|method| toml_string(method.config_name())),
        &toml_string(ClickMethod::ButtonAreas.config_name()),
    );
    out.push_str("# Content follows the fingers, as on a phone.\n");
    setting(
        out,
        "natural_scroll",
        natural_scroll.map(|on| on.to_string()),
        "true",
    );
    out.push_str("# libinput calls this \"dwt\": ignore the touchpad briefly while typing.\n");
    setting(
        out,
        "disable_while_typing",
        disable_while_typing.map(|on| on.to_string()),
        "true",
    );
    out.push_str(
        "\
# Pointer acceleration, -1.0 (slowest) to 1.0 (fastest). 0.0 is the device
# default, not \"no acceleration\".
",
    );
    // `{:?}` keeps a whole number a TOML float (`0.0`, not `0`) and prints
    // the shortest text that parses back to the same `f64`.
    setting(
        out,
        "accel_speed",
        accel_speed.map(|speed| format!("{speed:?}")),
        "0.3",
    );
}

const INPUT_EXAMPLES: &str = "
# Not built in — uncomment to add. Other devices are left entirely alone
# unless you name them; uncommenting this keeps the blocks above, because
# they are still listed.
#
# [[input]]
# name = \"*Logitech*\"
# natural_scroll = false
# accel_speed = 0.0
";

/// One `[[input]]` setting: active when the entry sets it, otherwise
/// commented out with `example`, so the knob is documented but inert.
fn setting(out: &mut String, key: &str, value: Option<String>, example: &str) {
    match value {
        Some(value) => out.push_str(&format!("{key} = {value}\n")),
        None => out.push_str(&format!("# {key} = {example}\n")),
    }
}

fn modifier_list(mods: &[Modifier]) -> String {
    let names: Vec<String> = mods
        .iter()
        .map(|modifier| toml_string(modifier.config_name()))
        .collect();
    format!("[{}]", names.join(", "))
}

fn action_value(action: &Action) -> String {
    match action {
        Action::Exec(argument) | Action::SwitchTag(argument) => {
            format!("{{ {} = {} }}", action.config_name(), toml_string(argument))
        }
        Action::Terminal
        | Action::Launcher
        | Action::Close
        | Action::FocusNext
        | Action::Exit
        | Action::CycleTag
        | Action::OpenAssignPicker
        | Action::OpenSwitchPicker
        | Action::Hotkeys
        | Action::Move
        | Action::Resize => toml_string(action.config_name()),
    }
}

/// `text` as a TOML basic string. TOML forbids raw control characters
/// other than tab inside one, so those are escaped too.
fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::super::docs_drift::{expected_names, rejection};
    use super::super::{Button, RawConfig};
    use super::*;

    const EXAMPLE: &str = include_str!("../../../docs/config.example.toml");

    fn parses_back(config: &Config) {
        let rendered = render(config);
        let reparsed = Config::parse(&rendered).unwrap_or_else(|error| {
            panic!("the rendered config does not parse strictly ({error}):\n{rendered}")
        });
        assert_eq!(&reparsed, config, "rendered:\n{rendered}");
    }

    #[test]
    fn the_rendered_defaults_parse_back_to_the_defaults() {
        parses_back(&Config::default());
    }

    /// Every string the renderer writes is user text in general, so each
    /// one has to survive quotes, backslashes, newlines, control
    /// characters and non-ASCII; and every optional `[[input]]` field is
    /// set, so each one's active form is proved too.
    #[test]
    fn strings_needing_escapes_and_every_set_field_round_trip() {
        let awkward = "a \"quoted\" back\\slash\nnew line\ttab \u{7}bell \u{7f}del é ✓";
        let mut config = Config::default();
        config.defaults.terminal = awkward.to_string();
        config.defaults.launcher = format!("{awkward} launcher");
        config.defaults.default_tag = format!("{awkward} tag");
        config.defaults.pinned_terminal_args = vec!["{app_id}".to_string(), awkward.to_string()];
        config.defaults.pinned_terminals = false;
        config.keybinds = vec![
            Keybind {
                mods: vec![Modifier::Super, Modifier::Ctrl, Modifier::Alt],
                key: "p".to_string(),
                action: Action::Exec(awkward.to_string()),
            },
            Keybind {
                mods: Vec::new(),
                key: "F1".to_string(),
                action: Action::SwitchTag(awkward.to_string()),
            },
        ];
        config.mousebinds = vec![Mousebind {
            mods: vec![Modifier::Shift],
            button: Button::Middle,
            action: Action::Close,
        }];
        config.inputs = vec![InputConfig {
            name: awkward.to_string(),
            tap: Some(false),
            tap_button_map: Some(TapButtonMap::Lmr),
            click_method: Some(ClickMethod::Clickfinger),
            natural_scroll: Some(true),
            disable_while_typing: Some(false),
            accel_speed: Some(-1.0),
        }];
        parses_back(&config);
    }

    /// serde's `rename_all = "snake_case"` spelling of a variant, derived
    /// from its `Debug` name: the canonical spelling, as opposed to the
    /// aliases serde also accepts.
    fn snake_case(debug_name: &str) -> String {
        let mut out = String::new();
        for (i, c) in debug_name.chars().enumerate() {
            if c.is_ascii_uppercase() && i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        }
        out
    }

    /// Every variant serde accepts for one enum, found by parsing each name
    /// in serde's own "expected one of" list, must be spelled by `spell` as
    /// its canonical name, and that spelling must parse back to it.
    fn check_spellings<T: Copy + PartialEq + std::fmt::Debug>(
        document: impl Fn(&str) -> String,
        extract: impl Fn(RawConfig) -> T,
        spell: impl Fn(T) -> &'static str,
    ) {
        let parse = |name: &str| {
            toml::from_str::<RawConfig>(&document(name))
                .map(&extract)
                .unwrap_or_else(|error| panic!("`{name}` does not parse: {error}"))
        };
        let mut variants: Vec<T> = Vec::new();
        for name in expected_names(&rejection(&document("bogus"))) {
            let variant = parse(&name);
            if !variants.contains(&variant) {
                variants.push(variant);
            }
        }
        for variant in variants {
            let spelling = spell(variant);
            assert_eq!(
                spelling,
                snake_case(&format!("{variant:?}")),
                "{variant:?} is not rendered with its canonical spelling"
            );
            assert_eq!(
                parse(spelling),
                variant,
                "`{spelling}` parses to another variant"
            );
        }
    }

    #[test]
    fn every_modifier_is_rendered_with_its_canonical_spelling() {
        check_spellings(
            |name| format!("[[keybind]]\nmod = [\"{name}\"]\nkey = \"a\"\naction = \"close\""),
            |raw| raw.keybinds[0].mods[0],
            Modifier::config_name,
        );
    }

    #[test]
    fn every_button_is_rendered_with_its_canonical_spelling() {
        check_spellings(
            |name| format!("[[mousebind]]\nbutton = \"{name}\"\naction = \"move\""),
            |raw| raw.mousebinds[0].button,
            Button::config_name,
        );
    }

    #[test]
    fn every_tap_button_map_is_rendered_with_its_canonical_spelling() {
        check_spellings(
            |name| format!("[[input]]\nname = \"x\"\ntap_button_map = \"{name}\""),
            |raw| raw.inputs[0].tap_button_map.expect("set in the document"),
            TapButtonMap::config_name,
        );
    }

    #[test]
    fn every_click_method_is_rendered_with_its_canonical_spelling() {
        check_spellings(
            |name| format!("[[input]]\nname = \"x\"\nclick_method = \"{name}\""),
            |raw| raw.inputs[0].click_method.expect("set in the document"),
            ClickMethod::config_name,
        );
    }

    /// The commented-out blocks of the rendered defaults: each run of lines
    /// that opens with a commented-out table header (`# [[keybind]]`) and
    /// continues while the lines stay commented-out TOML, returned as the
    /// index of its first line and its line count.
    fn commented_out_blocks(lines: &[&str]) -> Vec<(usize, usize)> {
        // A whole-line header, so prose that merely opens with a table name
        // is not taken for an example.
        let is_header = |line: &str| line.starts_with("# [[") && line.ends_with("]]");
        let mut blocks = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            if is_header(lines[i]) {
                let start = i;
                i += 1;
                while i < lines.len() && lines[i].starts_with("# ") && !is_header(lines[i]) {
                    i += 1;
                }
                blocks.push((start, i - start));
            } else {
                i += 1;
            }
        }
        blocks
    }

    /// The examples that are not built in are commented out so the active
    /// file stays exactly the defaults; uncommenting any one of them, in
    /// place, must still give a file with nothing wrong in it, so an
    /// example cannot rot.
    #[test]
    fn every_commented_out_example_parses_once_uncommented() {
        let rendered = default_config_toml();
        let lines: Vec<&str> = rendered.lines().collect();
        let blocks = commented_out_blocks(&lines);
        assert!(
            !blocks.is_empty(),
            "the defaults carry no commented-out example"
        );
        let built_in = Config::default();
        for (start, len) in blocks {
            let mut edited: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
            for line in &mut edited[start..start + len] {
                *line = line["# ".len()..].to_string();
            }
            let uncommented = edited.join("\n");
            let config = Config::parse(&uncommented).unwrap_or_else(|error| {
                panic!(
                    "the example at line {} does not parse once uncommented ({error}):\n{}",
                    start + 1,
                    edited[start..start + len].join("\n")
                )
            });
            let added = (config.keybinds.len() + config.mousebinds.len() + config.inputs.len())
                - (built_in.keybinds.len() + built_in.mousebinds.len() + built_in.inputs.len());
            assert_eq!(
                added,
                1,
                "the example at line {} should add exactly one entry",
                start + 1
            );
        }
    }

    /// The licence header block of the shipped example: its lines up to
    /// the first blank line.
    fn split_example(example: &str) -> (&str, &str) {
        example
            .split_once("\n\n")
            .expect("docs/config.example.toml has a header block and a blank line")
    }

    /// `docs/config.example.toml` is generated output: its licence header,
    /// one blank line, then exactly what `--print-default-config` prints.
    #[test]
    fn the_shipped_example_is_the_licence_header_plus_the_rendered_defaults() {
        let (header, body) = split_example(EXAMPLE);
        assert!(
            header.starts_with("# SPDX-FileCopyrightText:")
                && header.lines().all(|line| line.starts_with('#')),
            "docs/config.example.toml must open with its SPDX comment block"
        );
        let rendered = default_config_toml();
        if body != rendered {
            let first_difference = body
                .lines()
                .zip(rendered.lines())
                .enumerate()
                .find(|(_, (shipped, generated))| shipped != generated)
                .map(|(n, (shipped, generated))| {
                    format!(
                        "first difference at body line {}:\n  shipped:   {shipped}\n  generated: {generated}",
                        n + 1
                    )
                })
                .unwrap_or_else(|| "one is a prefix of the other".to_string());
            panic!(
                "docs/config.example.toml is not the rendered defaults. Regenerate it: keep \
                 its header block and the blank line after it, then append the output of\n  \
                 cargo run -q -p buoy-wm -- --print-default-config\n{first_difference}"
            );
        }
    }

    /// The printed config is the user's own file, not RPL-licensed, so the
    /// licence header belongs to the shipped copy only.
    #[test]
    fn the_rendered_defaults_carry_no_licence_header() {
        assert!(!default_config_toml().contains("SPDX"));
    }

    /// A strict parse of the shipped example is the defaults: nothing that
    /// is not built in is active in it.
    #[test]
    fn the_shipped_example_config_is_exactly_the_defaults() {
        let config = Config::parse(EXAMPLE).expect("docs/config.example.toml must parse");
        assert_eq!(config, Config::default());
    }
}
