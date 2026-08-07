// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! `TagSet`: a `u64` bitset of tag membership, one bit per `TagId`
//! (0..64 per ADR-006).

/// A bitset of tag membership. Bit position `n` corresponds to
/// `TagId(n)`; valid positions are `0..64` per ADR-006.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TagSet(u64);

impl TagSet {
    /// Returns a `TagSet` with no bits set.
    pub fn empty() -> Self {
        TagSet(0)
    }

    /// Returns `true` if bit `pos` is set. `pos >= 64` is out of range
    /// (ADR-006 caps the tag registry at 64 entries) and is treated as
    /// absent rather than panicking or reading a wrapped bit — `TagId` and
    /// `TagSet`'s inner field are both `pub`, so this guard has to live
    /// here, not just at `TagRegistry`'s call sites (NFR2).
    pub fn contains(&self, pos: u8) -> bool {
        pos < 64 && (self.0 & (1u64 << pos) != 0)
    }

    /// Sets bit `pos`. A no-op for `pos >= 64` (see [`Self::contains`]).
    pub fn insert(&mut self, pos: u8) {
        if pos < 64 {
            self.0 |= 1u64 << pos;
        }
    }

    /// Clears bit `pos`. A no-op for `pos >= 64` (see [`Self::contains`]).
    pub fn remove(&mut self, pos: u8) {
        if pos < 64 {
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
