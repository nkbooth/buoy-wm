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

//! Resolves a config file's `key = "..."` name to the X11 keysym the
//! `river_xkb_bindings_v1` protocol expects.
//!
//! `libxkbcommon`'s own `xkb_keysym_from_name` is the canonical
//! implementation, but binding to it would put a C library on this crate's
//! build path for one string lookup. The keysym space this WM actually
//! needs is small and almost entirely algorithmic instead — see
//! [`from_name`].

/// Keysyms that have no algorithmic relationship to their name. Values are
/// from `xkbcommon/xkbcommon-keysyms.h`, the same header `main.rs`' own
/// keysym constants cite.
const NAMED: &[(&str, u32)] = &[
    ("space", 0x20),
    ("Return", 0xff0d),
    ("Escape", 0xff1b),
    ("Tab", 0xff09),
    ("BackSpace", 0xff08),
    ("Delete", 0xffff),
    ("Insert", 0xff63),
    ("Home", 0xff50),
    ("End", 0xff57),
    ("Page_Up", 0xff55),
    ("Page_Down", 0xff56),
    ("Left", 0xff51),
    ("Up", 0xff52),
    ("Right", 0xff53),
    ("Down", 0xff54),
    ("Print", 0xff61),
    ("Pause", 0xff13),
    ("Menu", 0xff67),
];

/// The keysym for `F1`. `F2`..`F35` follow it contiguously, which is what
/// lets [`from_name`] compute them instead of listing 35 entries.
const F1: u32 = 0xffbe;

/// The highest function key with a keysym in the contiguous `F1` block —
/// `F36` would collide with the modifier keysyms starting at `0xffe1`.
const MAX_FUNCTION_KEY: u32 = 35;

/// Resolves a config `key = "..."` name to its X11 keysym, or `None` if the
/// name isn't one this WM can bind.
///
/// Three cases, tried in order: an explicitly [`NAMED`] key, a function key
/// (`F1`-`F35`, computed from the number), or a single ASCII printable
/// character, which *is* its own keysym. The function-key check runs before
/// the single-character one so a bare `"F"` still falls through and binds
/// the letter F.
///
/// Deliberately strict — an unresolvable name is a config error the caller
/// reports at load, never a binding that silently never fires.
/// [`NAMED`] is matched case-insensitively: X11 spells these keysyms
/// `space`/`Page_Up`, but users reasonably write `Space` or `page_up`, and
/// rejecting those would be pedantry. Case still matters for single
/// characters — `a` (0x61) and `A` (0x41) are genuinely different keysyms —
/// which stays safe because no [`NAMED`] entry is a single character, so
/// they never reach this lookup.
pub fn from_name(name: &str) -> Option<u32> {
    if let Some((_, keysym)) = NAMED
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
    {
        return Some(*keysym);
    }
    if let Some(keysym) = function_key(name) {
        return Some(keysym);
    }
    single_ascii(name)
}

fn function_key(name: &str) -> Option<u32> {
    let number: u32 = name
        .strip_prefix('F')
        .or_else(|| name.strip_prefix('f'))?
        .parse()
        .ok()?;
    (1..=MAX_FUNCTION_KEY)
        .contains(&number)
        .then(|| F1 + number - 1)
}

fn single_ascii(name: &str) -> Option<u32> {
    let mut chars = name.chars();
    let first = chars.next()?;
    if chars.next().is_some() || !first.is_ascii_graphic() {
        return None;
    }
    Some(first as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ASCII printables are their own keysym, which is what makes a table
    /// unnecessary for the bulk of real bindings.
    #[test]
    fn single_ascii_characters_are_their_own_keysym() {
        assert_eq!(from_name("a"), Some(0x61));
        assert_eq!(from_name("z"), Some(0x7a));
        assert_eq!(from_name("A"), Some(0x41));
        assert_eq!(from_name("1"), Some(0x31));
        assert_eq!(from_name("?"), Some(0x3f));
        assert_eq!(from_name("/"), Some(0x2f));
    }

    #[test]
    fn named_special_keys_resolve() {
        assert_eq!(from_name("Return"), Some(0xff0d));
        assert_eq!(from_name("Escape"), Some(0xff1b));
        assert_eq!(from_name("Tab"), Some(0xff09));
        assert_eq!(from_name("space"), Some(0x20));
        assert_eq!(from_name("BackSpace"), Some(0xff08));
        assert_eq!(from_name("Delete"), Some(0xffff));
    }

    /// X11 spells it `space`, but a config author writes `Space` — and a
    /// single character must stay case-*sensitive* even so.
    #[test]
    fn named_keys_are_case_insensitive_but_single_characters_are_not() {
        assert_eq!(from_name("Space"), Some(0x20));
        assert_eq!(from_name("RETURN"), Some(0xff0d));
        assert_eq!(from_name("page_up"), Some(0xff55));
        assert_ne!(from_name("a"), from_name("A"));
    }

    #[test]
    fn function_key_names_are_case_insensitive() {
        assert_eq!(from_name("f1"), Some(0xffbe));
    }

    #[test]
    fn arrow_and_navigation_keys_resolve() {
        assert_eq!(from_name("Left"), Some(0xff51));
        assert_eq!(from_name("Up"), Some(0xff52));
        assert_eq!(from_name("Right"), Some(0xff53));
        assert_eq!(from_name("Down"), Some(0xff54));
        assert_eq!(from_name("Home"), Some(0xff50));
        assert_eq!(from_name("End"), Some(0xff57));
        assert_eq!(from_name("Page_Up"), Some(0xff55));
        assert_eq!(from_name("Page_Down"), Some(0xff56));
    }

    #[test]
    fn function_keys_are_computed_from_their_number() {
        assert_eq!(from_name("F1"), Some(0xffbe));
        assert_eq!(from_name("F12"), Some(0xffc9));
        assert_eq!(from_name("F35"), Some(0xffe0));
    }

    /// F36 does not exist in the keysym space — the range stops at F35, and
    /// running past it would collide with the modifier keysyms that start
    /// at 0xffe1.
    #[test]
    fn function_keys_past_the_real_range_are_rejected() {
        assert_eq!(from_name("F0"), None);
        assert_eq!(from_name("F36"), None);
        assert_eq!(from_name("F99"), None);
    }

    #[test]
    fn unknown_names_are_rejected() {
        assert_eq!(from_name("Retrun"), None);
        assert_eq!(from_name(""), None);
        assert_eq!(from_name("NotAKey"), None);
    }

    /// Multi-byte characters have no single-byte ASCII keysym, and the
    /// Unicode keysym range is deliberately out of scope — rejecting is
    /// better than registering a binding that silently never fires.
    #[test]
    fn non_ascii_characters_are_rejected() {
        assert_eq!(from_name("é"), None);
        assert_eq!(from_name("→"), None);
    }

    /// The names used by the built-in default binds must all resolve, or
    /// `Config::default()` would fail to register its own keybinds.
    #[test]
    fn every_name_used_by_the_built_in_defaults_resolves() {
        for name in ["space", "q", "n", "Escape", "Tab", "a", "s", "r", "?"] {
            assert!(from_name(name).is_some(), "{name} failed to resolve");
        }
    }
}
