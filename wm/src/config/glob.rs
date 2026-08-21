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

//! `*`-wildcard name matching for `[[input]]` device patterns.
//!
//! Device names come from libinput and are long, vendor-specific, and not
//! something a user should have to transcribe exactly — this machine's
//! touchpad is `PIXA3854:00 093A:0274 Touchpad`. A pattern of
//! `*Touchpad*` has to work, so the config needs *some* wildcard. `*` is
//! the only metacharacter, deliberately: a full glob (`?`, `[a-z]`) or a
//! regex would be more to learn, more to get wrong, and no more capable
//! for matching a device name.

/// Whether `name` matches `pattern`, where `*` in the pattern matches any
/// run of characters (including none). Every other character, `?` and `[`
/// included, is matched literally. Comparison is case-sensitive: libinput
/// device names are stable strings, not user input.
pub fn matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    let (mut p, mut n) = (0, 0);
    // Where to resume from when the most recent `*` turns out to have
    // swallowed too little. Without this, `*ab*c` against `xabyabc` would
    // commit the second `*` to the first `ab` it saw and then fail on `c`
    // with input still left — a wildcard has to be able to give ground.
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if pattern.get(p) == Some(&'*') {
            star = Some((p, n));
            p += 1;
        } else if pattern.get(p) == Some(&name[n]) {
            p += 1;
            n += 1;
        } else if let Some((star_p, star_n)) = star {
            p = star_p + 1;
            n = star_n + 1;
            star = Some((star_p, star_n + 1));
        } else {
            return false;
        }
    }
    // Name exhausted: whatever is left of the pattern can only still match
    // if it is nothing but wildcards.
    pattern[p..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The concrete case this module exists for: the real device name on
    /// the laptop that motivated `[[input]]` at all.
    #[test]
    fn contains_wildcard_matches_a_real_touchpad_name() {
        assert!(matches("*Touchpad*", "PIXA3854:00 093A:0274 Touchpad"));
    }

    #[test]
    fn a_pattern_with_no_wildcard_must_match_the_whole_name_exactly() {
        assert!(matches("Some Keyboard", "Some Keyboard"));
        assert!(!matches("Some Keyboard", "Some Keyboard 2"));
        assert!(!matches("Keyboard", "Some Keyboard"));
    }

    #[test]
    fn a_bare_wildcard_matches_every_name_including_the_empty_one() {
        assert!(matches("*", "PIXA3854:00 093A:0274 Touchpad"));
        assert!(matches("*", ""));
    }

    #[test]
    fn a_trailing_wildcard_anchors_the_prefix() {
        assert!(matches("PIXA*", "PIXA3854:00 093A:0274 Touchpad"));
        assert!(!matches("ELAN*", "PIXA3854:00 093A:0274 Touchpad"));
    }

    #[test]
    fn a_leading_wildcard_anchors_the_suffix() {
        assert!(matches("*Touchpad", "PIXA3854:00 093A:0274 Touchpad"));
        assert!(!matches("*Mouse", "PIXA3854:00 093A:0274 Touchpad"));
    }

    /// A wildcard matches an empty run, so an interior `*` must not
    /// require at least one character between its neighbours.
    #[test]
    fn a_wildcard_matches_an_empty_run() {
        assert!(matches("a*b", "ab"));
        assert!(matches("**", ""));
        assert!(matches("*Touchpad*", "Touchpad"));
    }

    #[test]
    fn multiple_interior_wildcards_all_have_to_match_in_order() {
        assert!(matches("*093A*Touchpad", "PIXA3854:00 093A:0274 Touchpad"));
        assert!(!matches(
            "*Touchpad*093A*",
            "PIXA3854:00 093A:0274 Touchpad"
        ));
    }

    /// Greedy left-to-right matching must not strand a later literal:
    /// consuming the first `ab` for `*ab` would leave nothing for `c`.
    #[test]
    fn matching_backtracks_rather_than_committing_to_the_first_candidate() {
        assert!(matches("*ab*c", "xabyabc"));
        assert!(matches("*a*a*a", "aaa"));
        assert!(!matches("*ab*c", "xabyab"));
    }

    #[test]
    fn an_empty_pattern_matches_only_an_empty_name() {
        assert!(matches("", ""));
        assert!(!matches("", "Touchpad"));
    }

    /// `?` and `[` carry no special meaning — a device name containing
    /// them must still be matchable literally.
    #[test]
    fn only_the_star_is_a_metacharacter() {
        assert!(matches("a?b", "a?b"));
        assert!(!matches("a?b", "axb"));
        assert!(matches("*[0]*", "dev[0] Touchpad"));
    }

    #[test]
    fn matching_is_case_sensitive() {
        assert!(!matches("*touchpad*", "PIXA3854:00 093A:0274 Touchpad"));
    }

    /// The matcher collects `chars()` rather than bytes, so a multi-byte
    /// pattern character is one unit on both sides — but nothing proved it,
    /// and a keyboard or tablet whose vendor string carries an accent is
    /// not exotic (audit finding T-05).
    #[test]
    fn a_non_ascii_pattern_matches_whole_characters_not_bytes() {
        assert!(matches("*Tastatur*", "SEM Präzisions-Tastatur 2"));
        assert!(matches("Wacom*é*", "Wacom Intuos Précision"));
        assert!(!matches("*Präzision*", "SEM Prazision"));
        // A `*` must not be able to stop half way through a character.
        assert!(matches("é*", "élan"));
        assert!(!matches("?*", "élan"));
    }

    /// A pattern built to make a naive matcher backtrack exponentially.
    /// This one is linear by construction — one `star` position, never a
    /// stack — and the test exists to keep it that way: the pattern comes
    /// from a config file, and the matcher runs once per device per
    /// `[[input]]` entry at startup.
    #[test]
    fn a_pathological_wildcard_pattern_terminates() {
        let pattern = "*a".repeat(12);
        let name = "a".repeat(48);
        assert!(matches(&pattern, &name));
        assert!(!matches(&format!("{pattern}b"), &name));
    }
}
