// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! `WmCore`: the aggregator that owns the view/tag/output registries and
//! exposes `wm-core`'s public mutating API.

use std::collections::{HashMap, VecDeque};

use super::ids::{OutputId, TagId, ViewId};
use super::output::Output;
use super::tag::{TagRegistry, TagRegistryError};
use super::view::{Geometry, View};

/// Errors returned by [`WmCore`]'s mutating API. Every variant indicates a
/// reference to an id that is not currently registered; no `WmCore`
/// mutator panics on invalid input (NFR2).
// Variant names deliberately share the `Unknown` prefix per the story
// spec's explicit API (`WmCoreError::UnknownView`/`UnknownTag`/
// `UnknownOutput`) — not an accidental naming smell.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WmCoreError {
    /// No view with the given `ViewId` is registered.
    UnknownView,
    /// No tag with the given `TagId` is registered.
    UnknownTag,
    /// No output with the given `OutputId` is registered.
    UnknownOutput,
    /// The tag registry already holds the maximum of 64 tags (ADR-006).
    TagLimitReached,
}

/// The single in-memory source of truth for window/tag membership, view
/// geometry/floating state, focus, output current-tag, the tag registry,
/// stacking/render order, and terminal-spawned status. Pure state + logic,
/// no I/O. A freshly constructed `WmCore` starts with no persisted state
/// (data-model.md: no persistence layer in v1).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct WmCore {
    views: HashMap<ViewId, View>,
    next_view_id: u64,
    tags: TagRegistry,
    /// The single WM-wide focused view, if any (data-model.md: `View` has
    /// one `focused: bool` field, not a per-seat focus set).
    focused_view: Option<ViewId>,
    outputs: HashMap<OutputId, Output>,
    next_output_id: u64,
    /// Stacking/render order, front=bottom, back=top (mirrors the
    /// vendored `main.rs` `WindowManager.windows: VecDeque<Window>`
    /// convention).
    stacking_order: VecDeque<ViewId>,
}

impl WmCore {
    /// Returns a new, empty `WmCore`.
    pub fn new() -> Self {
        WmCore::default()
    }

    /// Creates a tag with the given `name` via the tag registry, or
    /// returns the existing [`TagId`] if a tag with that name is already
    /// registered (idempotent by name; see
    /// [`TagRegistry::create_tag`](super::tag::TagRegistry::create_tag)).
    /// Fails with [`WmCoreError::TagLimitReached`] if the registry already
    /// holds 64 tags.
    pub fn create_tag(&mut self, name: impl Into<String>) -> Result<TagId, WmCoreError> {
        let name = name.into();
        self.tags.create_tag(&name).map_err(|err| match err {
            TagRegistryError::Full => WmCoreError::TagLimitReached,
            TagRegistryError::UnknownTag => WmCoreError::UnknownTag,
        })
    }

    /// Registers a new view for `app_id`, returning a fresh, unique
    /// [`ViewId`]. The new view starts with empty tags, `floating ==
    /// true`, `focused == false`, and zeroed [`Geometry`](super::view::Geometry).
    pub fn register_view(&mut self, app_id: &str) -> ViewId {
        let id = ViewId(self.next_view_id);
        self.next_view_id += 1;
        self.views.insert(
            id,
            View {
                id,
                app_id: app_id.to_string(),
                tags: Default::default(),
                floating: true,
                geometry: Default::default(),
                focused: false,
            },
        );
        self.stacking_order.push_back(id);
        id
    }

    /// Removes a registered view, including from the stacking order. If
    /// the removed view was the currently-focused view, clears
    /// `focused_view` too, preserving the "at most one focused view,
    /// always live" invariant. Fails with [`WmCoreError::UnknownView`] if
    /// `id` is not currently registered; never panics.
    pub fn unregister_view(&mut self, id: ViewId) -> Result<(), WmCoreError> {
        self.views.remove(&id).ok_or(WmCoreError::UnknownView)?;
        self.stacking_order.retain(|&v| v != id);
        if self.focused_view == Some(id) {
            self.focused_view = None;
        }
        Ok(())
    }

    /// Toggles `tag_id`'s membership on the view identified by `view_id`:
    /// adds it if absent, removes it if present. Fails with
    /// [`WmCoreError::UnknownView`] or [`WmCoreError::UnknownTag`] if
    /// either id is not registered, leaving state unchanged.
    pub fn toggle_view_tag(&mut self, view_id: ViewId, tag_id: TagId) -> Result<(), WmCoreError> {
        if !self.tags.contains(tag_id) {
            return Err(WmCoreError::UnknownTag);
        }
        let view = self
            .views
            .get_mut(&view_id)
            .ok_or(WmCoreError::UnknownView)?;
        if view.tags.contains(tag_id.0) {
            view.tags.remove(tag_id.0);
        } else {
            view.tags.insert(tag_id.0);
        }
        Ok(())
    }

    /// Sets a view's geometry. Fails with [`WmCoreError::UnknownView`] for
    /// an unregistered id, leaving state unchanged.
    pub fn set_view_geometry(&mut self, id: ViewId, geometry: Geometry) -> Result<(), WmCoreError> {
        let view = self.views.get_mut(&id).ok_or(WmCoreError::UnknownView)?;
        view.geometry = geometry;
        Ok(())
    }

    /// Sets a view's floating flag. Fails with [`WmCoreError::UnknownView`]
    /// for an unregistered id, leaving state unchanged.
    pub fn set_view_floating(&mut self, id: ViewId, floating: bool) -> Result<(), WmCoreError> {
        let view = self.views.get_mut(&id).ok_or(WmCoreError::UnknownView)?;
        view.floating = floating;
        Ok(())
    }

    /// Focuses the given view, unfocusing whichever view (if any) was
    /// previously focused. At most one view is focused at a time. Fails
    /// with [`WmCoreError::UnknownView`] for an unregistered id, leaving
    /// the currently-focused view (if any) unchanged.
    pub fn set_focus(&mut self, id: ViewId) -> Result<(), WmCoreError> {
        if !self.views.contains_key(&id) {
            return Err(WmCoreError::UnknownView);
        }
        self.clear_focus();
        self.views
            .get_mut(&id)
            .expect("presence checked above")
            .focused = true;
        self.focused_view = Some(id);
        Ok(())
    }

    /// Clears whichever view is currently focused, if any. A no-op if no
    /// view is focused.
    pub fn clear_focus(&mut self) {
        if let Some(id) = self.focused_view.take()
            && let Some(view) = self.views.get_mut(&id)
        {
            view.focused = false;
        }
    }

    /// Registers a new output, returning a fresh, unique [`OutputId`].
    /// The new output starts with `current_tag == None`.
    pub fn register_output(&mut self) -> OutputId {
        let id = OutputId(self.next_output_id);
        self.next_output_id += 1;
        self.outputs.insert(
            id,
            Output {
                id,
                current_tag: None,
            },
        );
        id
    }

    /// Sets an output's current tag. `Some(tag_id)` fails with
    /// [`WmCoreError::UnknownTag`] if `tag_id` is not registered; `None`
    /// always succeeds and clears the field. Fails with
    /// [`WmCoreError::UnknownOutput`] if `output_id` is not registered.
    /// Leaves state unchanged on any error. This is the raw field-level
    /// primitive only — one-tag-per-output enforcement belongs to
    /// [`WmCore::switch_tag`], layered on top.
    pub fn set_output_current_tag(
        &mut self,
        output_id: OutputId,
        tag_id: Option<TagId>,
    ) -> Result<(), WmCoreError> {
        if let Some(tag_id) = tag_id
            && !self.tags.contains(tag_id)
        {
            return Err(WmCoreError::UnknownTag);
        }
        let output = self
            .outputs
            .get_mut(&output_id)
            .ok_or(WmCoreError::UnknownOutput)?;
        output.current_tag = tag_id;
        Ok(())
    }

    /// Displays `tag_id` on `output_id`, enforcing the ADR-005 one-tag-
    /// per-output invariant (FR2): if `tag_id` is currently displayed on a
    /// *different* registered output, that output's `current_tag` is
    /// cleared (rerouted) before the target output is updated, so no two
    /// outputs ever simultaneously report the same `Some(tag)`. Reroutes
    /// rather than rejects (see this story's Technical notes). A no-op
    /// success if `tag_id` is already `output_id`'s current tag. Both
    /// `output_id` and `tag_id` are validated before any mutation, so an
    /// unregistered id never has the side effect of clearing a real,
    /// unrelated output's tag: fails with [`WmCoreError::UnknownOutput`]
    /// or [`WmCoreError::UnknownTag`] and leaves all state unchanged.
    pub fn switch_tag(&mut self, output_id: OutputId, tag_id: TagId) -> Result<(), WmCoreError> {
        if !self.tags.contains(tag_id) {
            return Err(WmCoreError::UnknownTag);
        }
        if !self.outputs.contains_key(&output_id) {
            return Err(WmCoreError::UnknownOutput);
        }
        for (&other_id, other) in self.outputs.iter_mut() {
            if other_id != output_id && other.current_tag == Some(tag_id) {
                other.current_tag = None;
            }
        }
        self.set_output_current_tag(output_id, Some(tag_id))
    }

    /// Marks the tag's lazy-spawn-once terminal as having been spawned.
    /// Idempotent: calling this again on an already-spawned tag is a
    /// no-op success. Fails with [`WmCoreError::UnknownTag`] for an
    /// unregistered id.
    pub fn mark_terminal_spawned(&mut self, id: TagId) -> Result<(), WmCoreError> {
        self.tags
            .mark_terminal_spawned(id)
            .map_err(|_| WmCoreError::UnknownTag)
    }

    /// Returns the stacking/render order, front-to-back (front=bottom,
    /// back=top).
    pub fn stacking_order(&self) -> Vec<ViewId> {
        self.stacking_order.iter().copied().collect()
    }

    /// Moves an already-registered view to the back (top) of the stacking
    /// order. Fails with [`WmCoreError::UnknownView`] for an unregistered
    /// id.
    pub fn raise_view(&mut self, id: ViewId) -> Result<(), WmCoreError> {
        if !self.views.contains_key(&id) {
            return Err(WmCoreError::UnknownView);
        }
        self.stacking_order.retain(|&v| v != id);
        self.stacking_order.push_back(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{WmCore, WmCoreError};
    use crate::wm_core::ids::{TagId, ViewId};
    use crate::wm_core::view::Geometry;

    #[test]
    fn register_view_returns_fresh_unique_ids() {
        let mut core = WmCore::new();
        let first = core.register_view("app-one");
        let second = core.register_view("app-two");
        assert_ne!(first, second);
    }

    #[test]
    fn newly_registered_view_has_expected_defaults() {
        let mut core = WmCore::new();
        let id = core.register_view("app-one");
        let view = core
            .views
            .get(&id)
            .expect("registered view must be present");
        assert_eq!(view.app_id, "app-one");
        assert!(view.tags == Default::default());
        assert!(view.floating);
        assert!(!view.focused);
        assert_eq!(view.geometry, Geometry::default());
    }

    #[test]
    fn unregister_view_removes_registered_view() {
        let mut core = WmCore::new();
        let id = core.register_view("app-one");
        assert_eq!(core.unregister_view(id), Ok(()));
        assert!(!core.views.contains_key(&id));
    }

    #[test]
    fn double_unregister_returns_unknown_view_error() {
        let mut core = WmCore::new();
        let id = core.register_view("app-one");
        core.unregister_view(id).unwrap();
        assert_eq!(core.unregister_view(id), Err(WmCoreError::UnknownView));
    }

    #[test]
    fn unregister_view_never_registered_returns_unknown_view_error() {
        let mut core = WmCore::new();
        let never_registered = core.register_view("throwaway");
        core.unregister_view(never_registered).unwrap();
        assert_eq!(
            core.unregister_view(never_registered),
            Err(WmCoreError::UnknownView)
        );
    }

    #[test]
    fn unregister_focused_view_clears_focused_view() {
        let mut core = WmCore::new();
        let id = core.register_view("app-one");
        core.set_focus(id).unwrap();
        core.unregister_view(id).unwrap();
        assert_eq!(
            core.focused_view, None,
            "unregistering the focused view must clear focused_view, not leave it dangling"
        );
    }

    #[test]
    fn unregister_non_focused_view_leaves_focus_unaffected() {
        let mut core = WmCore::new();
        let first = core.register_view("app-one");
        let second = core.register_view("app-two");
        core.set_focus(first).unwrap();
        core.unregister_view(second).unwrap();
        assert_eq!(core.focused_view, Some(first));
        assert!(core.views.get(&first).unwrap().focused);
    }

    #[test]
    fn toggle_view_tag_adds_tag_when_absent() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let tag_id = core.create_tag("web").unwrap();
        core.toggle_view_tag(view_id, tag_id).unwrap();
        assert!(core.views.get(&view_id).unwrap().tags.contains(tag_id.0));
    }

    #[test]
    fn toggle_view_tag_removes_tag_when_present() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let tag_id = core.create_tag("web").unwrap();
        core.toggle_view_tag(view_id, tag_id).unwrap();
        core.toggle_view_tag(view_id, tag_id).unwrap();
        assert!(!core.views.get(&view_id).unwrap().tags.contains(tag_id.0));
    }

    #[test]
    fn toggle_view_tag_unknown_view_returns_error_and_does_not_mutate() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("web").unwrap();
        let bogus_view = ViewId(999);
        let result = core.toggle_view_tag(bogus_view, tag_id);
        assert_eq!(result, Err(WmCoreError::UnknownView));
    }

    #[test]
    fn toggle_view_tag_unknown_tag_returns_error_and_does_not_mutate() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let bogus_tag = TagId(63);
        let result = core.toggle_view_tag(view_id, bogus_tag);
        assert_eq!(result, Err(WmCoreError::UnknownTag));
        assert!(!core.views.get(&view_id).unwrap().tags.contains(bogus_tag.0));
    }

    #[test]
    fn set_view_geometry_updates_fields() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let geometry = Geometry {
            x: 10,
            y: 20,
            width: 300,
            height: 400,
        };
        core.set_view_geometry(view_id, geometry).unwrap();
        assert_eq!(core.views.get(&view_id).unwrap().geometry, geometry);
    }

    #[test]
    fn set_view_geometry_unknown_view_returns_error() {
        let mut core = WmCore::new();
        let bogus_view = ViewId(999);
        let geometry = Geometry {
            x: 10,
            y: 20,
            width: 300,
            height: 400,
        };
        assert_eq!(
            core.set_view_geometry(bogus_view, geometry),
            Err(WmCoreError::UnknownView)
        );
    }

    #[test]
    fn set_view_floating_flips_flag() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        assert!(core.views.get(&view_id).unwrap().floating);
        core.set_view_floating(view_id, false).unwrap();
        assert!(!core.views.get(&view_id).unwrap().floating);
    }

    #[test]
    fn set_view_floating_unknown_view_returns_error() {
        let mut core = WmCore::new();
        let bogus_view = ViewId(999);
        assert_eq!(
            core.set_view_floating(bogus_view, false),
            Err(WmCoreError::UnknownView)
        );
    }

    #[test]
    fn set_focus_sets_focused_true_on_target_view() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        core.set_focus(view_id).unwrap();
        assert!(core.views.get(&view_id).unwrap().focused);
    }

    #[test]
    fn set_focus_on_second_view_clears_first() {
        let mut core = WmCore::new();
        let first = core.register_view("app-one");
        let second = core.register_view("app-two");
        core.set_focus(first).unwrap();
        core.set_focus(second).unwrap();
        assert!(!core.views.get(&first).unwrap().focused);
        assert!(core.views.get(&second).unwrap().focused);
    }

    #[test]
    fn clear_focus_clears_currently_focused_view() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        core.set_focus(view_id).unwrap();
        core.clear_focus();
        assert!(!core.views.get(&view_id).unwrap().focused);
    }

    #[test]
    fn set_focus_unknown_id_returns_error_and_does_not_change_focus() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        core.set_focus(view_id).unwrap();
        let bogus_view = ViewId(999);
        assert_eq!(core.set_focus(bogus_view), Err(WmCoreError::UnknownView));
        assert!(core.views.get(&view_id).unwrap().focused);
    }

    #[test]
    fn register_output_returns_fresh_id_with_no_current_tag() {
        let mut core = WmCore::new();
        let first = core.register_output();
        let second = core.register_output();
        assert_ne!(first, second);
        assert_eq!(core.outputs.get(&first).unwrap().current_tag, None);
    }

    #[test]
    fn set_output_current_tag_updates_when_tag_exists() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_id = core.create_tag("web").unwrap();
        core.set_output_current_tag(output_id, Some(tag_id))
            .unwrap();
        assert_eq!(
            core.outputs.get(&output_id).unwrap().current_tag,
            Some(tag_id)
        );
    }

    #[test]
    fn set_output_current_tag_unregistered_tag_returns_error_and_leaves_unchanged() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let bogus_tag = TagId(0);
        let result = core.set_output_current_tag(output_id, Some(bogus_tag));
        assert_eq!(result, Err(WmCoreError::UnknownTag));
        assert_eq!(core.outputs.get(&output_id).unwrap().current_tag, None);
    }

    #[test]
    fn set_output_current_tag_unregistered_output_returns_error() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("web").unwrap();
        let bogus_output = crate::wm_core::ids::OutputId(999);
        let result = core.set_output_current_tag(bogus_output, Some(tag_id));
        assert_eq!(result, Err(WmCoreError::UnknownOutput));
    }

    #[test]
    fn set_output_current_tag_none_clears_it() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_id = core.create_tag("web").unwrap();
        core.set_output_current_tag(output_id, Some(tag_id))
            .unwrap();
        core.set_output_current_tag(output_id, None).unwrap();
        assert_eq!(core.outputs.get(&output_id).unwrap().current_tag, None);
    }

    #[test]
    fn switch_tag_sets_current_tag_on_output_showing_none() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_id = core.create_tag("web").unwrap();
        assert_eq!(core.switch_tag(output_id, tag_id), Ok(()));
        assert_eq!(
            core.outputs.get(&output_id).unwrap().current_tag,
            Some(tag_id)
        );
    }

    #[test]
    fn switch_tag_replaces_output_own_previously_displayed_tag() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_a = core.create_tag("web").unwrap();
        let tag_b = core.create_tag("term").unwrap();
        core.switch_tag(output_id, tag_a).unwrap();
        assert_eq!(core.switch_tag(output_id, tag_b), Ok(()));
        assert_eq!(
            core.outputs.get(&output_id).unwrap().current_tag,
            Some(tag_b)
        );
    }

    #[test]
    fn switch_tag_already_displayed_on_same_output_is_idempotent_no_op() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_id = core.create_tag("web").unwrap();
        core.switch_tag(output_id, tag_id).unwrap();
        assert_eq!(core.switch_tag(output_id, tag_id), Ok(()));
        assert_eq!(
            core.outputs.get(&output_id).unwrap().current_tag,
            Some(tag_id)
        );
    }

    #[test]
    fn switch_tag_reroutes_tag_away_from_previous_output() {
        let mut core = WmCore::new();
        let o1 = core.register_output();
        let o2 = core.register_output();
        let tag_b = core.create_tag("web").unwrap();
        core.switch_tag(o1, tag_b).unwrap();

        assert_eq!(core.switch_tag(o2, tag_b), Ok(()));
        assert_eq!(core.outputs.get(&o2).unwrap().current_tag, Some(tag_b));
        assert_eq!(
            core.outputs.get(&o1).unwrap().current_tag,
            None,
            "rerouting tag_b onto o2 must clear it from o1"
        );
    }

    #[test]
    fn switch_tag_reroute_leaves_unrelated_outputs_and_tags_untouched() {
        let mut core = WmCore::new();
        let o1 = core.register_output();
        let o2 = core.register_output();
        let o3 = core.register_output();
        let tag_b = core.create_tag("web").unwrap();
        let tag_c = core.create_tag("term").unwrap();
        core.switch_tag(o1, tag_b).unwrap();
        core.switch_tag(o2, tag_c).unwrap();

        assert_eq!(core.switch_tag(o3, tag_b), Ok(()));
        assert_eq!(
            core.outputs.get(&o1).unwrap().current_tag,
            None,
            "o1 loses tag_b to the reroute"
        );
        assert_eq!(
            core.outputs.get(&o2).unwrap().current_tag,
            Some(tag_c),
            "o2's unrelated tag_c must be untouched by a tag_b reroute"
        );
        assert_eq!(core.outputs.get(&o3).unwrap().current_tag, Some(tag_b));
    }

    /// Scans `outputs` for two different outputs simultaneously reporting
    /// the same `Some(tag)` — the ADR-005 invariant `switch_tag` must
    /// never let slip, even transiently between calls in a sequence.
    fn assert_no_duplicate_current_tags(core: &WmCore) {
        let mut seen = Vec::new();
        for output in core.outputs.values() {
            if let Some(tag) = output.current_tag {
                assert!(
                    !seen.contains(&tag),
                    "tag {tag:?} is simultaneously displayed on two outputs"
                );
                seen.push(tag);
            }
        }
    }

    #[test]
    fn switch_tag_sequence_across_three_outputs_never_duplicates_a_tag() {
        let mut core = WmCore::new();
        let o1 = core.register_output();
        let o2 = core.register_output();
        let o3 = core.register_output();
        let tag_b = core.create_tag("web").unwrap();
        let tag_c = core.create_tag("term").unwrap();

        core.switch_tag(o1, tag_b).unwrap();
        assert_no_duplicate_current_tags(&core);

        core.switch_tag(o2, tag_b).unwrap();
        assert_no_duplicate_current_tags(&core);

        core.switch_tag(o1, tag_c).unwrap();
        assert_no_duplicate_current_tags(&core);

        core.switch_tag(o3, tag_b).unwrap();
        assert_no_duplicate_current_tags(&core);
    }

    #[test]
    fn switch_tag_unregistered_output_returns_error_and_leaves_state_unchanged() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("web").unwrap();
        let bogus_output = crate::wm_core::ids::OutputId(999);
        let snapshot = core.clone();
        assert_eq!(
            core.switch_tag(bogus_output, tag_id),
            Err(WmCoreError::UnknownOutput)
        );
        assert_eq!(core, snapshot);
    }

    #[test]
    fn switch_tag_unregistered_tag_returns_error_and_leaves_state_unchanged() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let bogus_tag = TagId(63);
        let snapshot = core.clone();
        assert_eq!(
            core.switch_tag(output_id, bogus_tag),
            Err(WmCoreError::UnknownTag)
        );
        assert_eq!(core, snapshot);
    }

    /// Regression guard: an unregistered `tag_id` must fail validation
    /// before the reroute loop ever runs, so a real, unrelated, valid
    /// output's real tag is never cleared as a side effect of a failed
    /// call. Would fail under a naive "reroute first, validate last"
    /// ordering (the reroute loop would find no output showing the bogus
    /// tag and leave O1 alone by coincidence in the 2-output case, but the
    /// point is to lock the *ordering* in place as a named guard against
    /// that bug class regardless of implementation strategy).
    #[test]
    fn switch_tag_bogus_tag_cannot_clear_unrelated_valid_output() {
        let mut core = WmCore::new();
        let o1 = core.register_output();
        let o2 = core.register_output();
        let tag_a = core.create_tag("web").unwrap();
        core.switch_tag(o1, tag_a).unwrap();
        let bogus_tag = TagId(63);

        assert_eq!(core.switch_tag(o2, bogus_tag), Err(WmCoreError::UnknownTag));
        assert_eq!(
            core.outputs.get(&o1).unwrap().current_tag,
            Some(tag_a),
            "a failed switch_tag call must not clear an unrelated, valid output's tag"
        );
    }

    #[test]
    fn create_tag_returns_fresh_id_via_public_api() {
        let mut core = WmCore::new();
        let id = core.create_tag("web").unwrap();
        assert_eq!(core.tags.get(id).unwrap().name, "web");
    }

    #[test]
    fn create_tag_is_idempotent_by_name_via_public_api() {
        let mut core = WmCore::new();
        let first = core.create_tag("web").unwrap();
        let second = core.create_tag("web").unwrap();
        assert_eq!(first, second);
        assert_eq!(core.tags.count(), 1);
    }

    #[test]
    fn create_tag_returns_tag_limit_reached_when_registry_full() {
        let mut core = WmCore::new();
        for i in 0..64 {
            core.create_tag(format!("tag{i}")).unwrap();
        }
        assert_eq!(
            core.create_tag("one-too-many"),
            Err(WmCoreError::TagLimitReached)
        );
        assert_eq!(core.tags.count(), 64);
    }

    #[test]
    fn new_tag_starts_with_terminal_spawned_false() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("web").unwrap();
        assert!(!core.tags.get(tag_id).unwrap().terminal_spawned);
    }

    #[test]
    fn mark_terminal_spawned_sets_flag_true() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("web").unwrap();
        core.mark_terminal_spawned(tag_id).unwrap();
        assert!(core.tags.get(tag_id).unwrap().terminal_spawned);
    }

    #[test]
    fn mark_terminal_spawned_is_idempotent() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("web").unwrap();
        core.mark_terminal_spawned(tag_id).unwrap();
        assert_eq!(core.mark_terminal_spawned(tag_id), Ok(()));
        assert!(core.tags.get(tag_id).unwrap().terminal_spawned);
    }

    #[test]
    fn mark_terminal_spawned_unknown_tag_returns_error() {
        let mut core = WmCore::new();
        let bogus_tag = TagId(0);
        assert_eq!(
            core.mark_terminal_spawned(bogus_tag),
            Err(WmCoreError::UnknownTag)
        );
    }

    #[test]
    fn register_view_appends_to_back_of_stacking_order() {
        let mut core = WmCore::new();
        let first = core.register_view("app-one");
        let second = core.register_view("app-two");
        assert_eq!(core.stacking_order(), vec![first, second]);
    }

    #[test]
    fn raise_view_moves_view_to_back() {
        let mut core = WmCore::new();
        let first = core.register_view("app-one");
        let second = core.register_view("app-two");
        core.raise_view(first).unwrap();
        assert_eq!(core.stacking_order(), vec![second, first]);
    }

    #[test]
    fn unregister_view_removes_from_stacking_order() {
        let mut core = WmCore::new();
        let first = core.register_view("app-one");
        let second = core.register_view("app-two");
        core.unregister_view(first).unwrap();
        assert_eq!(core.stacking_order(), vec![second]);
    }

    #[test]
    fn raise_view_unknown_id_returns_error() {
        let mut core = WmCore::new();
        let bogus_view = ViewId(999);
        assert_eq!(core.raise_view(bogus_view), Err(WmCoreError::UnknownView));
    }

    #[test]
    fn fresh_wm_core_is_fully_empty() {
        let core = WmCore::new();
        assert_eq!(core.tags.count(), 0);
        assert_eq!(core.views.len(), 0);
        assert_eq!(core.outputs.len(), 0);
        assert_eq!(core.focused_view, None);
        assert!(core.stacking_order().is_empty());

        let default_core = WmCore::default();
        assert_eq!(default_core.tags.count(), 0);
        assert_eq!(default_core.views.len(), 0);
        assert_eq!(default_core.outputs.len(), 0);
        assert_eq!(default_core.focused_view, None);
        assert!(default_core.stacking_order().is_empty());
    }

    /// Consolidated regression guard (Task 11): every public `WmCore`
    /// mutator from Tasks 3-9, called with an id that was never
    /// registered, must return a typed `Err` and leave observable state
    /// byte-for-byte unchanged. This is a regression guard, not new
    /// behavior — it should already pass if Tasks 3-9 were implemented
    /// correctly.
    #[test]
    fn every_mutator_rejects_unknown_ids_without_mutating_state() {
        let mut core = WmCore::new();
        let valid_view = core.register_view("app-one");
        let valid_tag = core.create_tag("web").unwrap();
        let valid_output = core.register_output();

        let bogus_view = ViewId(9999);
        let bogus_tag = TagId(63);
        let bogus_output = crate::wm_core::ids::OutputId(9999);

        let snapshot = core.clone();

        assert_eq!(
            core.unregister_view(bogus_view),
            Err(WmCoreError::UnknownView)
        );
        assert_eq!(core, snapshot);

        assert_eq!(
            core.toggle_view_tag(bogus_view, valid_tag),
            Err(WmCoreError::UnknownView)
        );
        assert_eq!(core, snapshot);

        assert_eq!(
            core.toggle_view_tag(valid_view, bogus_tag),
            Err(WmCoreError::UnknownTag)
        );
        assert_eq!(core, snapshot);

        let geometry = Geometry {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        };
        assert_eq!(
            core.set_view_geometry(bogus_view, geometry),
            Err(WmCoreError::UnknownView)
        );
        assert_eq!(core, snapshot);

        assert_eq!(
            core.set_view_floating(bogus_view, false),
            Err(WmCoreError::UnknownView)
        );
        assert_eq!(core, snapshot);

        assert_eq!(core.set_focus(bogus_view), Err(WmCoreError::UnknownView));
        assert_eq!(core, snapshot);

        assert_eq!(
            core.set_output_current_tag(bogus_output, Some(valid_tag)),
            Err(WmCoreError::UnknownOutput)
        );
        assert_eq!(core, snapshot);

        assert_eq!(
            core.set_output_current_tag(valid_output, Some(bogus_tag)),
            Err(WmCoreError::UnknownTag)
        );
        assert_eq!(core, snapshot);

        assert_eq!(
            core.mark_terminal_spawned(bogus_tag),
            Err(WmCoreError::UnknownTag)
        );
        assert_eq!(core, snapshot);

        assert_eq!(core.raise_view(bogus_view), Err(WmCoreError::UnknownView));
        assert_eq!(core, snapshot);
    }
}
