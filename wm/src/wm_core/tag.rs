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

//! Tag registry: named, arbitrary-string tags backed by a 64-slot `u64`
//! bitset (ADR-006). Tag creation is idempotent by name; there is no
//! delete-tag API in v1 — that omission is deliberate, not an oversight.

// This module's public API is wired in: `create_tag` (via
// `WmCore::create_tag`, `buoy-tag-picker`'s IPC-driven tag-creation flow),
// `mark_terminal_spawned` (via
// `WmCore::claim_pinned_terminal_spawn`/`ensure_pinned_terminal_spawned`,
// the pinned-terminal lazy-spawn), and this registry's ordering (via
// `ids()`, which `WmCore::cycle_tag` composes with `switch_tag` for the
// tag-cycle keybind).
use std::collections::HashMap;

use super::MAX_TAGS;
use super::ids::TagId;

/// A named tag. `terminal_spawned` tracks whether the lazy-spawn-once
/// terminal for this tag has already been launched (Story 1.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub id: TagId,
    pub name: String,
    pub terminal_spawned: bool,
}

/// Errors returned by [`TagRegistry`] mutators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagRegistryError {
    /// The registry already holds the maximum of 64 tags (ADR-006).
    Full,
    /// No tag with the given [`TagId`] is registered.
    UnknownTag,
}

/// The tag registry: named, arbitrary strings backed by a `u64` bitset of
/// tag IDs. IDs are assigned sequentially starting at 0 and are never
/// reused. There is no delete-tag API in v1 (ADR-006).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TagRegistry {
    tags: Vec<Tag>,
    ids_by_name: HashMap<String, TagId>,
}

impl TagRegistry {
    /// Returns a new, empty tag registry.
    ///
    /// Test-only: production constructs the registry via
    /// `TagRegistry::default()`, so this exists purely so whole-registry
    /// assertions read naturally. `#[cfg(test)]` rather than
    /// `#[allow(dead_code)]` makes that status compiler-enforced instead of
    /// asserted by a comment that could go stale (audit finding J-09).
    #[cfg(test)]
    pub fn new() -> Self {
        TagRegistry::default()
    }

    /// Returns the number of registered tags. Reached from production via
    /// [`WmCore::tag_count`](super::state::WmCore::tag_count), which
    /// `main.rs` uses to decide whether to bootstrap the default tag.
    pub fn count(&self) -> usize {
        self.tags.len()
    }

    /// Returns every registered tag's id, in creation (registration) order
    /// — the order [`WmCore::cycle_tag`](super::state::WmCore::cycle_tag)
    /// treats as canonical. A pure query; never mutates.
    pub fn ids(&self) -> Vec<TagId> {
        self.tags.iter().map(|t| t.id).collect()
    }

    /// Creates a tag with the given `name`, or returns the existing
    /// [`TagId`] if a tag with that name is already registered
    /// (idempotent by name). Fails with [`TagRegistryError::Full`] if the
    /// registry already holds 64 tags.
    pub fn create_tag(&mut self, name: &str) -> Result<TagId, TagRegistryError> {
        if let Some(&id) = self.ids_by_name.get(name) {
            return Ok(id);
        }
        if self.tags.len() >= usize::from(MAX_TAGS) {
            return Err(TagRegistryError::Full);
        }
        let id = TagId(self.tags.len() as u8);
        self.tags.push(Tag {
            id,
            name: name.to_string(),
            terminal_spawned: false,
        });
        self.ids_by_name.insert(name.to_string(), id);
        Ok(id)
    }

    /// Returns the id of the tag named `name`, if one is registered.
    /// Exact match, via the same `ids_by_name` index [`TagRegistry::
    /// create_tag`] uses for its own idempotency check — so a caller that
    /// only needs to *read* a name's id never has to reach for a `&mut
    /// self` mutator to get it.
    pub fn id_by_name(&self, name: &str) -> Option<TagId> {
        self.ids_by_name.get(name).copied()
    }

    /// Returns `true` if `id` is a registered [`TagId`].
    pub fn contains(&self, id: TagId) -> bool {
        (id.0 as usize) < self.tags.len()
    }

    /// Returns the [`Tag`] record for `id`, if registered.
    pub fn get(&self, id: TagId) -> Option<&Tag> {
        self.tags.get(id.0 as usize)
    }

    /// Sets `terminal_spawned` to `true` for the tag with the given id.
    /// Idempotent: calling this again on an already-spawned tag is a
    /// no-op success, not an error. Fails with
    /// [`TagRegistryError::UnknownTag`] for an unregistered id.
    pub fn mark_terminal_spawned(&mut self, id: TagId) -> Result<(), TagRegistryError> {
        let tag = self
            .tags
            .get_mut(id.0 as usize)
            .ok_or(TagRegistryError::UnknownTag)?;
        tag.terminal_spawned = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{TagRegistry, TagRegistryError};

    #[test]
    fn create_tag_assigns_increasing_ids_starting_at_zero() {
        let mut registry = TagRegistry::new();
        let first = registry.create_tag("one").unwrap();
        let second = registry.create_tag("two").unwrap();
        assert_eq!(first.0, 0);
        assert_eq!(second.0, 1);
    }

    #[test]
    fn sixty_four_distinct_tags_all_succeed_with_ids_0_to_64() {
        let mut registry = TagRegistry::new();
        for i in 0..64u8 {
            let id = registry.create_tag(&format!("tag{i}")).unwrap();
            assert_eq!(id.0, i);
        }
        assert_eq!(registry.count(), 64);
    }

    #[test]
    fn sixty_fifth_create_tag_fails_full_and_count_stays_64() {
        let mut registry = TagRegistry::new();
        for i in 0..64u8 {
            registry.create_tag(&format!("tag{i}")).unwrap();
        }
        let result = registry.create_tag("one-too-many");
        assert_eq!(result, Err(TagRegistryError::Full));
        assert_eq!(registry.count(), 64);
    }

    #[test]
    fn recreating_existing_name_returns_existing_id_without_growing_registry() {
        let mut registry = TagRegistry::new();
        let first = registry.create_tag("web").unwrap();
        let again = registry.create_tag("web").unwrap();
        assert_eq!(first, again);
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn ids_returns_empty_vec_when_registry_is_empty() {
        let registry = TagRegistry::new();
        assert_eq!(registry.ids(), Vec::new());
    }

    #[test]
    fn ids_returns_ids_in_creation_order() {
        let mut registry = TagRegistry::new();
        let id_a = registry.create_tag("a").unwrap();
        let id_b = registry.create_tag("b").unwrap();
        let id_c = registry.create_tag("c").unwrap();
        assert_eq!(registry.ids(), vec![id_a, id_b, id_c]);
    }
}
