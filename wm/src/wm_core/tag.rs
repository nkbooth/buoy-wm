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

/// The longest tag name the registry will accept, in bytes.
///
/// Bytes rather than characters because every sink a name reaches is
/// byte-bounded — a zellij session name (a filesystem path component), a
/// waybar label, a fuzzel row, a journal line — and because the name
/// arrives over IPC bounded only by the 64 KiB request-line cap (audit
/// finding E-01). 64 is the width of a comfortable picker row and matches
/// the registry's own 64-slot capacity.
pub const MAX_TAG_NAME_BYTES: usize = 64;

/// Errors returned by [`TagRegistry`] mutators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagRegistryError {
    /// The registry already holds the maximum of 64 tags (ADR-006).
    Full,
    /// No tag with the given [`TagId`] is registered.
    UnknownTag,
    /// The proposed tag name is empty, whitespace-only, over
    /// [`MAX_TAG_NAME_BYTES`], a bare `.`/`..`, or contains a path
    /// separator or a control character.
    InvalidName,
}

/// Journal text. Same separation as [`WmCoreError`](super::state::WmCoreError)'s
/// own `Display`: nothing on the wire is rendered from here, so this
/// wording carries no compatibility obligation.
impl std::fmt::Display for TagRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TagRegistryError::Full => {
                write!(f, "the registry is already full at {MAX_TAGS} tags")
            }
            TagRegistryError::UnknownTag => f.write_str("no such tag is registered"),
            TagRegistryError::InvalidName => write!(
                f,
                "the name is empty, whitespace-only, over {MAX_TAG_NAME_BYTES} \
                 bytes, a bare `.` or `..`, or contains a path separator or a \
                 control character"
            ),
        }
    }
}

impl std::error::Error for TagRegistryError {}

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
    /// registry already holds 64 tags, and with
    /// [`TagRegistryError::InvalidName`] if `name` is empty,
    /// whitespace-only, longer than [`MAX_TAG_NAME_BYTES`], exactly `.` or
    /// `..`, or contains `/`, `\` or a control character.
    ///
    /// This is the single ingest point for both name paths — the IPC
    /// `create-tag` request and the keybind that names a tag from the
    /// config — so nothing downstream has to re-check (audit finding
    /// E-01).
    pub fn create_tag(&mut self, name: &str) -> Result<TagId, TagRegistryError> {
        validate_tag_name(name)?;
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

    /// Clears `terminal_spawned` for the tag with the given id, so the
    /// lazy-spawn-once slot can be claimed again. Exists for exactly one
    /// caller — [`release_pinned_terminal_claim`](super::state::WmCore::release_pinned_terminal_claim),
    /// rolling back a claim whose
    /// spawn failed — because the claim is committed before the process it
    /// claims for exists (audit finding D-01). Idempotent; fails with
    /// [`TagRegistryError::UnknownTag`] for an unregistered id.
    pub fn unmark_terminal_spawned(&mut self, id: TagId) -> Result<(), TagRegistryError> {
        let tag = self
            .tags
            .get_mut(id.0 as usize)
            .ok_or(TagRegistryError::UnknownTag)?;
        tag.terminal_spawned = false;
        Ok(())
    }
}

// The rules exist because a tag name is not just a label: it becomes a
// zellij session name (a path component, hence `/`, `\`, `.` and `..`), a
// Pango-rendered waybar label and a journal line (hence control characters
// and ANSI escapes), and it arrives over IPC bounded only by the 64 KiB
// request cap (hence the length). Names are never deletable (ADR-006), so a
// bad one is permanent for the session.
fn validate_tag_name(name: &str) -> Result<(), TagRegistryError> {
    if name.trim().is_empty() || name.len() > MAX_TAG_NAME_BYTES {
        return Err(TagRegistryError::InvalidName);
    }
    if name == "." || name == ".." {
        return Err(TagRegistryError::InvalidName);
    }
    if name
        .chars()
        .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return Err(TagRegistryError::InvalidName);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Audit finding F-04: the registry's own error had no `Display`, so
    /// anything that logged it printed `Full` at the user.
    #[test]
    fn tag_registry_error_reads_as_a_sentence_not_a_rust_identifier() {
        let rendered = super::TagRegistryError::Full.to_string();
        assert!(
            rendered.contains(&crate::wm_core::MAX_TAGS.to_string()),
            "{rendered}"
        );
        assert_ne!(rendered, format!("{:?}", super::TagRegistryError::Full));
    }

    #[test]
    fn tag_registry_error_is_a_std_error() {
        let boxed: Box<dyn std::error::Error> = Box::new(super::TagRegistryError::UnknownTag);
        assert!(boxed.source().is_none());
    }
    use super::{MAX_TAG_NAME_BYTES, TagId, TagRegistry, TagRegistryError};

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

    /// Audit finding E-01: `create_tag` checked uniqueness and the 64-tag
    /// cap and nothing else, so a name arriving over IPC reached a zellij
    /// session name, the waybar label and the journal as-is — empty,
    /// whitespace-only, ~64 KiB, or carrying `/`, `..` and control
    /// characters.
    #[test]
    fn create_tag_rejects_an_empty_or_whitespace_only_name() {
        let mut registry = TagRegistry::new();
        for name in ["", " ", "\t", "   \t "] {
            assert_eq!(
                registry.create_tag(name),
                Err(TagRegistryError::InvalidName),
                "{name:?} was accepted"
            );
        }
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn create_tag_rejects_a_name_over_the_byte_cap() {
        let mut registry = TagRegistry::new();
        let too_long = "a".repeat(MAX_TAG_NAME_BYTES + 1);
        assert_eq!(
            registry.create_tag(&too_long),
            Err(TagRegistryError::InvalidName)
        );
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn create_tag_accepts_a_name_exactly_at_the_byte_cap() {
        let mut registry = TagRegistry::new();
        let at_cap = "a".repeat(MAX_TAG_NAME_BYTES);
        assert!(registry.create_tag(&at_cap).is_ok());
    }

    /// The cap is in bytes, not chars, because every sink downstream is
    /// byte-bounded — but a name well inside it must survive intact
    /// whatever alphabet it is written in.
    #[test]
    fn create_tag_accepts_a_multi_byte_name_within_the_cap() {
        let mut registry = TagRegistry::new();
        assert!(registry.create_tag("café 日本語 🌊").is_ok());
    }

    #[test]
    fn create_tag_rejects_path_separators_and_bare_dot_components() {
        let mut registry = TagRegistry::new();
        for name in ["web/dev", "..", ".", "a\\b", "../../etc", "/"] {
            assert_eq!(
                registry.create_tag(name),
                Err(TagRegistryError::InvalidName),
                "{name:?} was accepted"
            );
        }
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn create_tag_rejects_control_characters() {
        let mut registry = TagRegistry::new();
        for name in ["web\nmail", "web\u{0}mail", "web\u{1b}[31m", "web\u{7f}"] {
            assert_eq!(
                registry.create_tag(name),
                Err(TagRegistryError::InvalidName),
                "{name:?} was accepted"
            );
        }
        assert_eq!(registry.count(), 0);
    }

    /// A rejected name must not be resolvable afterwards either — the
    /// idempotency lookup runs after validation, not before it.
    #[test]
    fn a_rejected_name_is_not_registered_under_any_id() {
        let mut registry = TagRegistry::new();
        assert!(registry.create_tag("web/dev").is_err());
        assert_eq!(registry.id_by_name("web/dev"), None);
    }

    #[test]
    fn unmark_terminal_spawned_clears_the_flag_so_a_failed_spawn_can_retry() {
        let mut registry = TagRegistry::new();
        let id = registry.create_tag("web").unwrap();
        registry.mark_terminal_spawned(id).unwrap();
        registry.unmark_terminal_spawned(id).unwrap();
        assert!(!registry.get(id).unwrap().terminal_spawned);
    }

    #[test]
    fn unmark_terminal_spawned_is_idempotent() {
        let mut registry = TagRegistry::new();
        let id = registry.create_tag("web").unwrap();
        registry.unmark_terminal_spawned(id).unwrap();
        assert_eq!(registry.unmark_terminal_spawned(id), Ok(()));
        assert!(!registry.get(id).unwrap().terminal_spawned);
    }

    #[test]
    fn unmark_terminal_spawned_unknown_tag_returns_error() {
        let mut registry = TagRegistry::new();
        let id = registry.create_tag("web").unwrap();
        let bogus = TagId(id.0 + 1);
        assert_eq!(
            registry.unmark_terminal_spawned(bogus),
            Err(TagRegistryError::UnknownTag)
        );
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
