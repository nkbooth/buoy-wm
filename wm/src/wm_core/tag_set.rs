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

//! `TagSet`: a `u64` bitset of tag membership, one bit per `TagId`
//! (0..64 per ADR-006).

use super::MAX_TAGS;

/// A bitset of tag membership. Bit position `n` corresponds to
/// `TagId(n)`; valid positions are `0..64` per ADR-006.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TagSet(u64);

impl TagSet {
    /// Returns a `TagSet` with no bits set.
    ///
    /// Test-only. `contains`/`insert`/`remove` are reached from production
    /// via `WmCore::toggle_view_tag`, but `View`'s own `tags` field is
    /// constructed by `Default::default()` in `register_view`, so this
    /// constructor never is. `#[cfg(test)]` rather than
    /// `#[allow(dead_code)]` so that claim is compiler-enforced instead of
    /// asserted by a comment that could go stale (audit finding J-09).
    #[cfg(test)]
    pub fn empty() -> Self {
        TagSet(0)
    }

    /// Returns `true` if bit `pos` is set. `pos >= 64` is out of range
    /// (ADR-006 caps the tag registry at 64 entries) and is treated as
    /// absent rather than panicking or reading a wrapped bit — `TagId` and
    /// `TagSet`'s inner field are both `pub`, so this guard has to live
    /// here, not just at `TagRegistry`'s call sites (NFR2).
    pub fn contains(&self, pos: u8) -> bool {
        pos < MAX_TAGS && (self.0 & (1u64 << pos) != 0)
    }

    /// Sets bit `pos`. A no-op for `pos >= 64` (see [`Self::contains`]).
    pub fn insert(&mut self, pos: u8) {
        if pos < MAX_TAGS {
            self.0 |= 1u64 << pos;
        }
    }

    /// Clears bit `pos`. A no-op for `pos >= 64` (see [`Self::contains`]).
    pub fn remove(&mut self, pos: u8) {
        if pos < MAX_TAGS {
            self.0 &= !(1u64 << pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TagSet;

    #[test]
    fn empty_set_is_empty() {
        let set = TagSet::empty();
        for bit in 0..64u8 {
            assert!(
                !set.contains(bit),
                "bit {bit} should not be present in an empty set"
            );
        }
    }

    #[test]
    fn insert_then_contains_round_trips() {
        let mut set = TagSet::empty();
        set.insert(5);
        assert!(set.contains(5));
    }

    #[test]
    fn remove_clears_bit() {
        let mut set = TagSet::empty();
        set.insert(5);
        set.remove(5);
        assert!(!set.contains(5));
    }

    #[test]
    fn bit_63_round_trips() {
        let mut set = TagSet::empty();
        assert!(!set.contains(63));
        set.insert(63);
        assert!(set.contains(63));
        set.remove(63);
        assert!(!set.contains(63));
    }

    #[test]
    fn out_of_range_pos_does_not_panic_and_is_treated_as_absent() {
        let set = TagSet::empty();
        // NFR2: an out-of-range pos must never panic (debug) or corrupt an
        // in-range bit via a wrapped shift (release).
        assert!(!set.contains(64));
        assert!(!set.contains(255));
    }

    #[test]
    fn out_of_range_insert_and_remove_do_not_panic_or_corrupt_in_range_bits() {
        let mut set = TagSet::empty();
        set.insert(5);
        set.insert(64);
        set.insert(255);
        assert!(set.contains(5));
        assert!(!set.contains(64));

        set.remove(64);
        assert!(set.contains(5), "out-of-range remove must not affect bit 5");
    }
}
