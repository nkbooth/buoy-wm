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

//! Which built-in entries a config's own lists leave out.
//!
//! # Rationale
//!
//! Declaring any `[[keybind]]`, `[[mousebind]]` or `[[input]]` replaces
//! that whole built-in list. That rule is right — a merge would make a
//! built-in impossible to remove — but it made one `[[input]]` block for a
//! mouse silently cost the touchpad its tap-to-click. Nothing here changes
//! what is loaded; it only observes the effective config against
//! [`Config::default`], so the user is told.

use std::fmt;
use std::mem::discriminant;

use super::{Action, Config, Defaults, InputConfig, Modifier, binding_label};

/// The built-in entries a loaded config no longer has, as the lines a
/// notification shows. Built only from built-in entries, described with the
/// built-in `[defaults]`, so it never carries text the user wrote — in
/// particular never an `exec` command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedBuiltIns {
    keybinds: Vec<String>,
    mousebinds: Vec<String>,
    inputs: Vec<String>,
}

impl Config {
    /// The built-ins this config's lists leave out, or `None` when it keeps
    /// them all.
    ///
    /// A built-in binding is dropped when no binding in either list has its
    /// action: moving `close` to another key, or to a mouse button, keeps
    /// it, and losing it entirely is the harm worth a notification. A
    /// built-in `[[input]]` setting is dropped when the entry that applies
    /// to the built-in pattern — found with [`Config::input_for`], the WM's
    /// own first-match rule, using the pattern's text as the device name —
    /// does not set it at all; setting it to another value is a choice, not
    /// a drop. That is an approximation in both directions: a user pattern
    /// such as `*Touchpad` matches real touchpads but not the text
    /// `*Touchpad*`, so that user is told `tap` was dropped; and an earlier
    /// entry such as `*Synaptics*` that matches a real touchpad but not that
    /// text can take tap away unreported. A list that fell back to the
    /// built-ins equals them, so it drops nothing with no special case.
    pub fn dropped_built_ins(&self) -> Option<DroppedBuiltIns> {
        // Destructured so a new list is a compile error here rather than
        // a list whose drops go unreported.
        let Config {
            // Built-ins are described with the built-in defaults.
            defaults: _,
            keybinds,
            mousebinds,
            // Read through `input_for` below, so the WM's own matching rule decides.
            inputs: _,
        } = self;
        let bound = |action: &Action| {
            keybinds.iter().any(|bind| bind.action == *action)
                || mousebinds.iter().any(|bind| bind.action == *action)
        };
        let built_in = Config::default();
        let dropped = DroppedBuiltIns {
            keybinds: built_in
                .keybinds
                .iter()
                .filter(|kept| !bound(&kept.action))
                .map(|kept| binding_line(&kept.mods, &kept.key, &kept.action, &built_in.defaults))
                .collect(),
            mousebinds: built_in
                .mousebinds
                .iter()
                .filter(|kept| !bound(&kept.action))
                .map(|kept| {
                    binding_line(
                        &kept.mods,
                        kept.button.label(),
                        &kept.action,
                        &built_in.defaults,
                    )
                })
                .collect(),
            inputs: built_in
                .inputs
                .iter()
                .flat_map(|kept| self.dropped_input_settings(kept))
                .collect(),
        };
        (!dropped.is_empty()).then_some(dropped)
    }

    /// The settings `built_in` makes that the entry applying to its
    /// pattern does not, one line each.
    fn dropped_input_settings(&self, built_in: &InputConfig) -> Vec<String> {
        let applied: Vec<_> = self
            .input_for(&built_in.name)
            .map(|entry| entry.settings().iter().map(discriminant).collect())
            .unwrap_or_default();
        built_in
            .settings()
            .iter()
            .filter(|setting| !applied.contains(&discriminant(*setting)))
            .map(|setting| {
                format!(
                    "{:?} {} (now libinput's own default)",
                    built_in.name,
                    setting.config_name()
                )
            })
            .collect()
    }
}

/// A built-in binding as the cheat-sheet labels and describes it.
fn binding_line(mods: &[Modifier], trigger: &str, action: &Action, defaults: &Defaults) -> String {
    format!(
        "{}  {}",
        binding_label(mods, trigger),
        action.description(defaults)
    )
}

impl DroppedBuiltIns {
    fn is_empty(&self) -> bool {
        self.keybinds.is_empty() && self.mousebinds.is_empty() && self.inputs.is_empty()
    }
}

impl fmt::Display for DroppedBuiltIns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "your config no longer has these built-ins:")?;
        for (list, lines) in [
            ("[[keybind]]", &self.keybinds),
            ("[[mousebind]]", &self.mousebinds),
            ("[[input]]", &self.inputs),
        ] {
            for line in lines {
                write!(f, "\n  {list} {line}")?;
            }
        }
        write!(
            f,
            "\nDeclaring a [[keybind]], [[mousebind]] or [[input]] list \
             replaces that list's built-ins rather than adding to them. To \
             keep one, restate it in your list; \
             `buoy-wm --print-default-config` writes out every built-in."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dropped(contents: &str) -> Option<DroppedBuiltIns> {
        let (config, _skipped) = Config::parse_lenient(contents).expect("the file is valid");
        config.dropped_built_ins()
    }

    fn keybinds_dropped(contents: &str) -> Vec<String> {
        dropped(contents).map(|d| d.keybinds).unwrap_or_default()
    }

    const ONLY_A_TERMINAL_BIND: &str = r#"
[[keybind]]
mod = ["super"]
key = "Return"
action = "terminal"
"#;

    #[test]
    fn one_declared_keybind_reports_every_other_built_in() {
        let reported = keybinds_dropped(ONLY_A_TERMINAL_BIND);
        assert_eq!(reported.len(), Config::default().keybinds.len() - 1);
        assert!(
            reported
                .iter()
                .any(|line| line.contains("Super+q") && line.contains("Close focused window"))
        );
        assert!(!reported.iter().any(|line| line.contains("Spawn terminal")));
    }

    #[test]
    fn every_built_in_restated_in_another_order_and_spelling_reports_nothing() {
        let contents = r#"
[[keybind]]
mod = ["shift", "Super"]
key = "?"
action = "hotkeys"
[[keybind]]
mod = ["super"]
key = "Space"
action = "terminal"
[[keybind]]
mod = ["super"]
key = "r"
action = "launcher"
[[keybind]]
mod = ["super"]
key = "Escape"
action = "exit"
[[keybind]]
mod = ["super"]
key = "q"
action = "close"
[[keybind]]
mod = ["super"]
key = "n"
action = "focus_next"
[[keybind]]
mod = ["super"]
key = "Tab"
action = "cycle_tag"
[[keybind]]
mod = ["super"]
key = "a"
action = "tag_picker"
[[keybind]]
mod = ["super"]
key = "s"
action = "open_switch_picker"
"#;
        assert_eq!(dropped(contents), None);
    }

    /// Rebinding an action to another key keeps it; only an action bound
    /// to nothing is a drop.
    #[test]
    fn close_moved_to_another_key_is_not_a_drop() {
        let contents = r#"
[[keybind]]
mod = ["super"]
key = "w"
action = "close"
"#;
        let reported = keybinds_dropped(contents);
        assert!(
            !reported.is_empty(),
            "the other built-ins are still dropped"
        );
        assert!(
            !reported
                .iter()
                .any(|line| line.contains("Close focused window")),
            "close is still bound: {reported:?}"
        );
    }

    #[test]
    fn one_declared_mousebind_reports_the_other() {
        let contents = r#"
[[mousebind]]
mod = ["super"]
button = "middle"
action = "move"
"#;
        let reported = dropped(contents).expect("resize is gone");
        assert_eq!(reported.mousebinds.len(), 1);
        assert!(reported.mousebinds[0].contains("Super+RightClick"));
        assert!(reported.mousebinds[0].contains("Resize window"));
        assert!(reported.keybinds.is_empty() && reported.inputs.is_empty());
    }

    /// The case the ROADMAP names: a block for a mouse replaces the
    /// built-in list, so the touchpad loses tap-to-click.
    #[test]
    fn an_input_block_for_another_device_reports_the_touchpad_tap() {
        let contents = r#"
[[input]]
name = "*Logitech*"
natural_scroll = true
"#;
        let reported = dropped(contents).expect("tap is gone");
        assert_eq!(
            reported.inputs,
            vec!["\"*Touchpad*\" tap (now libinput's own default)".to_string()]
        );
    }

    #[test]
    fn a_touchpad_block_without_tap_reports_tap() {
        let contents = r#"
[[input]]
name = "*Touchpad*"
natural_scroll = true
"#;
        let reported = dropped(contents).expect("tap is gone");
        assert_eq!(
            reported.inputs,
            vec!["\"*Touchpad*\" tap (now libinput's own default)".to_string()]
        );
    }

    /// A built-in action moved to the other list is still bound.
    #[test]
    fn a_key_action_moved_to_a_mouse_button_is_not_a_drop() {
        let contents = r#"
[[keybind]]
mod = ["super"]
key = "Return"
action = "terminal"

[[mousebind]]
mod = ["super"]
button = "middle"
action = "close"
"#;
        let reported = keybinds_dropped(contents);
        assert!(
            !reported
                .iter()
                .any(|line| line.contains("Close focused window")),
            "close is bound to a button: {reported:?}"
        );
    }

    /// The message describes a built-in with the built-in programs, never
    /// with text the user wrote in `[defaults]`.
    #[test]
    fn a_dropped_launcher_is_described_with_the_built_in_program() {
        let contents = r#"
[defaults]
launcher = "my-secret-launcher --token abc"

[[keybind]]
mod = ["super"]
key = "Return"
action = "terminal"
"#;
        let message = dropped(contents)
            .expect("the launcher bind is gone")
            .to_string();
        assert!(!message.contains("my-secret-launcher"), "{message}");
        assert!(message.contains("App launcher (fuzzel)"), "{message}");
    }

    /// The documented blind spot, pinned: an earlier entry that matches a
    /// real touchpad but not the built-in pattern's own text is not seen.
    #[test]
    fn an_earlier_entry_that_only_matches_real_touchpads_is_not_seen() {
        let contents = r#"
[[input]]
name = "*Synaptics*"
natural_scroll = true

[[input]]
name = "*Touchpad*"
tap = true
"#;
        assert_eq!(dropped(contents), None);
    }

    /// Turning a setting off is a choice the user wrote, not a drop.
    #[test]
    fn an_explicit_tap_false_is_not_a_drop() {
        let contents = r#"
[[input]]
name = "*Touchpad*"
tap = false
"#;
        assert_eq!(dropped(contents), None);
    }

    /// The entry that applies is found the WM's own way, first match wins,
    /// so a catch-all that sets tap keeps it.
    #[test]
    fn a_catch_all_that_sets_tap_keeps_it() {
        let contents = r#"
[[input]]
name = "*"
tap = true

[[input]]
name = "*Touchpad*"
natural_scroll = true
"#;
        assert_eq!(dropped(contents), None);
    }

    /// A list that fell back to the built-ins is the built-in list.
    #[test]
    fn a_keybind_list_emptied_by_skipping_drops_nothing() {
        let contents = r#"
[[keybind]]
mod = ["super"]
key = "NoSuchKeysym"
action = "close"
"#;
        let (config, skipped) = Config::parse_lenient(contents).expect("the file is valid");
        assert_eq!(skipped.len(), 1);
        assert_eq!(config.dropped_built_ins(), None);
    }

    #[test]
    fn an_empty_file_drops_nothing() {
        assert_eq!(dropped(""), None);
    }

    /// The message names the list and each item, says why, and says how
    /// to get them back — without reading as an error.
    #[test]
    fn the_message_names_each_drop_and_how_to_restore_it() {
        let contents = r#"
[[keybind]]
mod = ["super"]
key = "Return"
action = "terminal"

[[input]]
name = "*Logitech*"
natural_scroll = true
"#;
        let message = dropped(contents).expect("drops").to_string();
        for needle in [
            "[[keybind]]",
            "Super+q",
            "Close focused window",
            "[[input]]",
            "\"*Touchpad*\" tap",
            "replaces",
            "buoy-wm --print-default-config",
        ] {
            assert!(message.contains(needle), "no {needle:?} in:\n{message}");
        }
        assert!(!message.contains("\n  [[mousebind]]"), "{message}");
        assert!(!message.to_lowercase().contains("error"), "{message}");
    }
}
