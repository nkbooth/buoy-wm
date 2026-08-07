// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! Tag registry: named, arbitrary-string tags backed by a 64-slot `u64`
//! bitset (ADR-006). Tag creation is idempotent by name; there is no
//! delete-tag API in v1 — that omission is deliberate, not an oversight.

// This module's public API (`WmCore::create_tag`/`toggle_view_tag`/
// `mark_terminal_spawned`/`switch_tag` are its only production entry
// points) is not yet wired into `main.rs` — that lands in Story 1.5
// (pinned terminal) and Story 1.7 (tag switching). Narrowly scoped to
// this module only (not a blanket crate-wide allow), same precedent as
// Story 1.2's original `mod wm_core` allow in `main.rs`.
#![allow(dead_code)]

use std::collections::HashMap;

use super::ids::TagId;

/// The maximum number of tags a `TagRegistry` may hold (ADR-006: one bit
/// per tag in a `u64` bitset).
const MAX_TAGS: usize = 64;

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
    pub fn new() -> Self {
        TagRegistry::default()
    }

    /// Returns the number of registered tags.
    pub fn count(&self) -> usize {
        self.tags.len()
    }

    /// Creates a tag with the given `name`, or returns the existing
    /// [`TagId`] if a tag with that name is already registered
    /// (idempotent by name). Fails with [`TagRegistryError::Full`] if the
    /// registry already holds 64 tags.
    pub fn create_tag(&mut self, name: &str) -> Result<TagId, TagRegistryError> {
        if let Some(&id) = self.ids_by_name.get(name) {
            return Ok(id);
        }
        if self.tags.len() >= MAX_TAGS {
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
}
