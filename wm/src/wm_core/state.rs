// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! `WmCore`: the aggregator that owns the view/tag/output registries and
//! exposes `wm-core`'s public mutating API.

use std::collections::{HashMap, VecDeque};

use super::ids::{OutputId, TagId, ViewId};
use super::output::Output;
use super::tag::{TagRegistry, TagRegistryError};
use super::view::{Geometry, View};

/// The `app_id` reserved for the lazily-spawned pinned terminal (Story
/// 1.5 will pass this literal to `foot -a pinned-term`). Views registered
/// with this `app_id` are never returned by
/// [`WmCore::closable_focused_view`] — the architectural constraint that
/// the pinned terminal is never closed via a routed keybind
/// (`architectural-constraints.md`).
pub const PINNED_TERM_APP_ID: &str = "pinned-term";

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
    // Constructible via a live path since Story 1.7 wired `switch_tag`
    // (through `cycle_tag`), `set_output_current_tag`, and `create_tag`
    // into `main.rs`'s tag-cycle/tag-create keybinds. `toggle_view_tag`
    // remains unwired (deferred to Epic 2's assign-mode picker), but this
    // variant has other live producers.
    UnknownTag,
    /// No output with the given `OutputId` is registered.
    // Constructible via a live path since Story 1.7 wired `switch_tag`
    // (through `cycle_tag`) into `main.rs`'s tag-cycle keybind.
    UnknownOutput,
    /// The tag registry already holds the maximum of 64 tags (ADR-006).
    // Constructible via a live path since Story 1.7 wired `create_tag`
    // (through `create_tag_with_generated_name`) into `main.rs`'s
    // tag-create keybind.
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
    // Not yet called from production `main.rs` — `WindowManager`'s
    // `#[derive(Default)]` constructs its `wm_core` field via
    // `WmCore::default()` instead. Kept as public constructor API/for
    // test ergonomics.
    #[allow(dead_code)]
    pub fn new() -> Self {
        WmCore::default()
    }

    /// Creates a tag with the given `name` via the tag registry, or
    /// returns the existing [`TagId`] if a tag with that name is already
    /// registered (idempotent by name; see
    /// [`TagRegistry::create_tag`](super::tag::TagRegistry::create_tag)).
    /// Fails with [`WmCoreError::TagLimitReached`] if the registry already
    /// holds 64 tags.
    // Wired into `main.rs`'s tag-create keybind since Story 1.7, via
    // `create_tag_with_generated_name`.
    pub fn create_tag(&mut self, name: impl Into<String>) -> Result<TagId, WmCoreError> {
        let name = name.into();
        self.tags.create_tag(&name).map_err(|err| match err {
            TagRegistryError::Full => WmCoreError::TagLimitReached,
            TagRegistryError::UnknownTag => WmCoreError::UnknownTag,
        })
    }

    /// Creates a tag with a generated placeholder name (`tag<N>`, where
    /// `N` is the registry's current tag count at the moment of creation),
    /// delegating entirely to [`WmCore::create_tag`] — no separately
    /// invented id-assignment or dedup logic. This is the pure decision
    /// `main.rs`'s tag-create keybind calls in place of a text-input UI
    /// that doesn't exist yet (Epic 2). Fails with
    /// [`WmCoreError::TagLimitReached`] if the registry already holds 64
    /// tags.
    ///
    /// Known, accepted collision risk: since `create_tag` is idempotent by
    /// name, a tag literally named `tag<N>` created through some other path
    /// (e.g. Epic 2's future free-text picker) before this generator's own
    /// counter reaches `N` would cause this method to silently return that
    /// existing tag's id rather than creating a new one. Harmless (no data
    /// corruption, no panic) but flagged, not hidden — see this story's
    /// Technical notes ("Generated-name collision risk"). Not a concern
    /// today: this keybind is the only tag-creation path that exists.
    pub fn create_tag_with_generated_name(&mut self) -> Result<TagId, WmCoreError> {
        let name = format!("tag{}", self.tags.count());
        self.create_tag(name)
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
    // Not yet wired into `main.rs` — deliberately out of scope for Story
    // 1.7, which wires tag switching/creation but not window-tag toggling.
    // View-tag assignment is Epic 2's assign-mode picker.
    #[allow(dead_code)]
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
    // Wired into `main.rs`'s `init_new_windows` since Story 1.6, which
    // applies `DEFAULT_FLOATING_GEOMETRY` to every non-pinned view on
    // registration.
    pub fn set_view_geometry(&mut self, id: ViewId, geometry: Geometry) -> Result<(), WmCoreError> {
        let view = self.views.get_mut(&id).ok_or(WmCoreError::UnknownView)?;
        view.geometry = geometry;
        Ok(())
    }

    /// Sets a view's floating flag. Fails with [`WmCoreError::UnknownView`]
    /// for an unregistered id, leaving state unchanged.
    // Wired into `main.rs`'s `init_new_windows` since Story 1.5, which
    // forces the pinned terminal non-floating on registration.
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
    // Wired into `main.rs`'s `Event::Output` handler since Story 1.7,
    // which registers a fresh `wm-core` output the moment a real output
    // appears.
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
    // Wired into `main.rs` since Story 1.7, transitively via `switch_tag`
    // (itself called from `cycle_tag`, the tag-cycle keybind's decision).
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
    // Wired into `main.rs`'s tag-cycle keybind since Story 1.7, via
    // `cycle_tag` (which composes this rather than duplicating its
    // enforcement).
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

    /// Advances `output_id`'s current tag to the next tag in the registry's
    /// creation order (wrapping back to the first tag after the last, and
    /// starting at the first tag when `current_tag` is `None` or is a tag
    /// no longer found in the registry — defensive only; unreachable today
    /// since tags are never deleted, ADR-006). Returns `Ok(Some(next_tag))`
    /// on success, or `Ok(None)` (no mutation) if the registry holds no
    /// tags. Fails with [`WmCoreError::UnknownOutput`] for an unregistered
    /// `output_id`, leaving state unchanged.
    ///
    /// This is the pure decision `main.rs`'s tag-cycle keybind calls. It
    /// deliberately delegates the actual field write and ADR-005
    /// cross-output reroute enforcement to [`WmCore::switch_tag`] rather
    /// than reimplementing it — the same functions Epic 2's IPC handlers
    /// will call, so there is only one place the "next tag" rule can live.
    pub fn cycle_tag(&mut self, output_id: OutputId) -> Result<Option<TagId>, WmCoreError> {
        let output = self
            .outputs
            .get(&output_id)
            .ok_or(WmCoreError::UnknownOutput)?;
        let current_tag = output.current_tag;
        let ids = self.tags.ids();
        if ids.is_empty() {
            return Ok(None);
        }
        let next = match current_tag.and_then(|current| ids.iter().position(|&id| id == current)) {
            Some(index) => ids[(index + 1) % ids.len()],
            None => ids[0],
        };
        self.switch_tag(output_id, next)?;
        Ok(Some(next))
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

    /// The single atomic "check + claim" decision for the pinned
    /// terminal's lazy-spawn-once invariant: if `tag_id`'s terminal has
    /// not yet been spawned, marks it spawned and returns
    /// `Ok(Some("tag-<name>"))` — the zellij session name the caller
    /// should spawn `foot -a pinned-term zellij attach --create` with. On
    /// every subsequent call for the same tag, returns `Ok(None)` without
    /// side effects. Deliberately bundled into one method (mirroring
    /// `cycle_focus`'s precedent of composing several `wm-core`-internal
    /// steps into one atomic call) rather than exposing separate
    /// `tag_terminal_spawned`/`tag_name` queries, so no caller can
    /// accidentally check without claiming or claim twice. Fails with
    /// [`WmCoreError::UnknownTag`] for an unregistered id.
    // Wired into `main.rs`'s tag-cycle/tag-create keybind path since Story
    // 1.7, via `ensure_pinned_terminal_spawned`.
    pub fn claim_pinned_terminal_spawn(
        &mut self,
        tag_id: TagId,
    ) -> Result<Option<String>, WmCoreError> {
        let tag = self.tags.get(tag_id).ok_or(WmCoreError::UnknownTag)?;
        if tag.terminal_spawned {
            return Ok(None);
        }
        let session_name = format!("tag-{}", tag.name);
        self.mark_terminal_spawned(tag_id)
            .expect("tag_id was just confirmed registered above");
        Ok(Some(session_name))
    }

    /// Returns the stacking/render order, front-to-back (front=bottom,
    /// back=top).
    // Story 1.4 code-review follow-up (efficiency): `cycle_focus` now
    // reads `self.stacking_order.front()` directly (O(1), no allocation)
    // instead of going through this O(n)-allocating accessor, so this has
    // no current production call site. Kept as public API and exercised
    // extensively by existing tests (whole-order assertions read far more
    // naturally as a `Vec`) — Story 1.6's floating-placement work is a
    // plausible future production caller.
    #[allow(dead_code)]
    pub fn stacking_order(&self) -> Vec<ViewId> {
        self.stacking_order.iter().copied().collect()
    }

    /// Moves an already-registered view to the back (top) of the stacking
    /// order. A no-op success (state unchanged) for the pinned terminal
    /// (`app_id == PINNED_TERM_APP_ID`): FR4 requires it always be
    /// rendered at the bottom of the render order, and both of `main.rs`'s
    /// reordering call sites — `cycle_focus` (via `Action::FocusNext`) and
    /// click-to-focus (`manage_seats`'s `interacted` handling) — route
    /// through this method, so guarding it here is the single place that
    /// covers both without duplicating an `app_id` check at each call site
    /// (Task 6). The pinned terminal can still be focused by either path
    /// (`set_focus` is unaffected by this guard) — only its position in
    /// the stacking order is pinned. Fails with
    /// [`WmCoreError::UnknownView`] for an unregistered id.
    pub fn raise_view(&mut self, id: ViewId) -> Result<(), WmCoreError> {
        let view = self.views.get(&id).ok_or(WmCoreError::UnknownView)?;
        if view.app_id == PINNED_TERM_APP_ID {
            return Ok(());
        }
        self.stacking_order.retain(|&v| v != id);
        self.stacking_order.push_back(id);
        Ok(())
    }

    /// Moves an already-registered view to the front (bottom) of the
    /// stacking order — the exact mirror of [`WmCore::raise_view`]. This is
    /// the primitive Task 5's `init_new_windows` wiring uses to satisfy
    /// FR4's "always rendered at the bottom of render order" for the pinned
    /// terminal. Fails with [`WmCoreError::UnknownView`] for an
    /// unregistered id.
    pub fn lower_view(&mut self, id: ViewId) -> Result<(), WmCoreError> {
        if !self.views.contains_key(&id) {
            return Err(WmCoreError::UnknownView);
        }
        self.stacking_order.retain(|&v| v != id);
        self.stacking_order.push_front(id);
        Ok(())
    }

    /// Returns the currently-focused view's id, unless it is the pinned
    /// terminal (`app_id == PINNED_TERM_APP_ID`) or nothing is focused. A
    /// pure decision query, not a close operation — the actual close
    /// request and eventual `unregister_view` still happen separately,
    /// driven by the compositor's own `Closed` event. Never panics
    /// (NFR2).
    // Story 1.4 code-review follow-up: no longer called from `main.rs`'s
    // `Action::Close` arm. This query reflects `WmCore`'s single WM-wide
    // `focused_view`, which can diverge from a specific seat's own real
    // focus target under multiple seats (last-seat-processed-wins races
    // ahead of this call) — using it as the pinned-terminal-close gate
    // could let the exclusion be bypassed. `main.rs` now checks the
    // acting seat's own focused `Window.app_id` directly instead. Kept as
    // public, still-tested API — pure query, harmless, and cheap to
    // rewire if a future story needs a WM-wide "what's focused and
    // closable" answer.
    #[allow(dead_code)]
    pub fn closable_focused_view(&self) -> Option<ViewId> {
        let id = self.focused_view?;
        let view = self.views.get(&id)?;
        (view.app_id != PINNED_TERM_APP_ID).then_some(id)
    }

    /// Cycles keyboard focus to the next view in stacking order (FR12):
    /// finds the first view in `stacking_order` (front to back) that is
    /// *not* the pinned terminal, moves it to the back (top) of the
    /// stacking order, and focuses it, returning its id. Returns `None` if
    /// no such view exists — either `stacking_order` is empty, or the
    /// pinned terminal is the only registered view — leaving state
    /// unchanged either way.
    ///
    /// Code review follow-up (Story 1.5): the pinned terminal is
    /// permanently fixed at `stacking_order.front()` (`lower_view`, and
    /// `raise_view`'s own no-op guard for it), so a naive "always target
    /// `front()`" implementation would get permanently stuck returning the
    /// pinned terminal's id on every call once one exists, and
    /// `FocusNext` could never reach any other window again. Skipping over
    /// it here — rather than merely deprioritizing it — means it is never
    /// selected as a `FocusNext` target at all; it remains directly
    /// focusable via a click (`main.rs`'s click-to-focus path), but never
    /// via cycling. Since the chosen target is still moved to the back by
    /// `raise_view` and the pinned terminal is excluded from both that
    /// reordering and this selection, repeated calls naturally round-robin
    /// through the non-pinned views while the pinned terminal stays fixed
    /// at the bottom.
    ///
    /// `raise_view`/`set_focus` on the found target are guaranteed to
    /// succeed since it was just read from this same `WmCore`'s own
    /// stacking order, so their `Result`s are unwrapped rather than
    /// propagated (NFR2: safe-by-construction, not caller-facing).
    pub fn cycle_focus(&mut self) -> Option<ViewId> {
        let target = self.stacking_order.iter().copied().find(|&id| {
            self.views
                .get(&id)
                .is_some_and(|view| view.app_id != PINNED_TERM_APP_ID)
        })?;
        self.raise_view(target)
            .expect("target was just read from this WmCore's own stacking order");
        self.set_focus(target)
            .expect("target was just read from this WmCore's own stacking order");
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::{PINNED_TERM_APP_ID, WmCore, WmCoreError};
    use crate::wm_core::ids::{TagId, ViewId};
    use crate::wm_core::view::{DEFAULT_FLOATING_GEOMETRY, Geometry};

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
    fn default_floating_geometry_has_expected_literal_value() {
        assert_eq!(
            DEFAULT_FLOATING_GEOMETRY,
            Geometry {
                x: 100,
                y: 100,
                width: 800,
                height: 600,
            }
        );
    }

    #[test]
    fn default_floating_geometry_applies_independently_to_multiple_views() {
        let mut core = WmCore::new();
        let view_a = core.register_view("app-a");
        let view_b = core.register_view("app-b");
        core.set_view_geometry(view_a, DEFAULT_FLOATING_GEOMETRY)
            .unwrap();
        core.set_view_geometry(view_b, DEFAULT_FLOATING_GEOMETRY)
            .unwrap();

        let recorded_a = core.views.get(&view_a).unwrap();
        let recorded_b = core.views.get(&view_b).unwrap();
        assert_eq!(recorded_a.geometry, DEFAULT_FLOATING_GEOMETRY);
        assert_eq!(recorded_b.geometry, DEFAULT_FLOATING_GEOMETRY);
        assert!(recorded_a.floating);
        assert!(recorded_b.floating);
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
    fn cycle_tag_from_none_current_selects_first_tag_in_registry() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_a = core.create_tag("a").unwrap();
        let _tag_b = core.create_tag("b").unwrap();
        assert_eq!(core.cycle_tag(output_id), Ok(Some(tag_a)));
        assert_eq!(
            core.outputs.get(&output_id).unwrap().current_tag,
            Some(tag_a)
        );
    }

    #[test]
    fn cycle_tag_advances_to_next_tag_in_creation_order() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let _tag_a = core.create_tag("a").unwrap();
        let tag_b = core.create_tag("b").unwrap();
        core.cycle_tag(output_id).unwrap();
        assert_eq!(core.cycle_tag(output_id), Ok(Some(tag_b)));
    }

    #[test]
    fn cycle_tag_wraps_around_to_first_tag_after_last() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_a = core.create_tag("a").unwrap();
        let _tag_b = core.create_tag("b").unwrap();
        core.cycle_tag(output_id).unwrap();
        core.cycle_tag(output_id).unwrap();
        assert_eq!(core.cycle_tag(output_id), Ok(Some(tag_a)));
    }

    #[test]
    fn cycle_tag_returns_ok_none_when_no_tags_registered() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        assert_eq!(core.cycle_tag(output_id), Ok(None));
        assert_eq!(core.outputs.get(&output_id).unwrap().current_tag, None);
    }

    #[test]
    fn cycle_tag_unknown_output_returns_error_and_leaves_state_unchanged() {
        let mut core = WmCore::new();
        core.create_tag("a").unwrap();
        let bogus_output = crate::wm_core::ids::OutputId(999);
        let snapshot = core.clone();
        assert_eq!(
            core.cycle_tag(bogus_output),
            Err(WmCoreError::UnknownOutput)
        );
        assert_eq!(core, snapshot);
    }

    #[test]
    fn cycle_tag_reuses_switch_tag_enforcement_and_reroutes_away_from_other_output() {
        let mut core = WmCore::new();
        let o1 = core.register_output();
        let o2 = core.register_output();
        let tag_a = core.create_tag("a").unwrap();
        core.switch_tag(o1, tag_a).unwrap();

        assert_eq!(core.cycle_tag(o2), Ok(Some(tag_a)));
        assert_eq!(core.outputs.get(&o2).unwrap().current_tag, Some(tag_a));
        assert_eq!(
            core.outputs.get(&o1).unwrap().current_tag,
            None,
            "cycle_tag must reuse switch_tag's own reroute enforcement, not a separate write path"
        );
    }

    #[test]
    fn create_tag_with_generated_name_uses_tag_n_naming_convention() {
        let mut core = WmCore::new();
        let first = core.create_tag_with_generated_name().unwrap();
        assert_eq!(core.tags.get(first).unwrap().name, "tag0");
        let second = core.create_tag_with_generated_name().unwrap();
        assert_eq!(core.tags.get(second).unwrap().name, "tag1");
    }

    #[test]
    fn create_tag_with_generated_name_returns_fresh_ids_on_repeated_calls() {
        let mut core = WmCore::new();
        let first = core.create_tag_with_generated_name().unwrap();
        let second = core.create_tag_with_generated_name().unwrap();
        let third = core.create_tag_with_generated_name().unwrap();
        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_ne!(first, third);
    }

    #[test]
    fn create_tag_with_generated_name_fails_when_registry_full() {
        let mut core = WmCore::new();
        for i in 0..64 {
            core.create_tag(format!("tag{i}")).unwrap();
        }
        assert_eq!(
            core.create_tag_with_generated_name(),
            Err(WmCoreError::TagLimitReached)
        );
        assert_eq!(core.tags.count(), 64);
    }

    #[test]
    fn create_tag_with_generated_name_delegates_to_create_tag_no_new_ids_by_construction() {
        let mut core = WmCore::new();
        let id = core.create_tag_with_generated_name().unwrap();
        let tag = core.tags.get(id).expect("id must be a real, present tag");
        assert_eq!(tag.name, "tag0");
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
    fn lower_view_moves_view_to_front() {
        let mut core = WmCore::new();
        let a = core.register_view("app-one");
        let b = core.register_view("app-two");
        assert_eq!(core.stacking_order(), vec![a, b]);
        core.lower_view(b).unwrap();
        assert_eq!(core.stacking_order(), vec![b, a]);
    }

    #[test]
    fn lower_view_on_already_front_view_is_idempotent() {
        let mut core = WmCore::new();
        let a = core.register_view("app-one");
        let b = core.register_view("app-two");
        core.lower_view(a).unwrap();
        assert_eq!(core.stacking_order(), vec![a, b]);
    }

    #[test]
    fn lower_view_unknown_id_returns_error() {
        let mut core = WmCore::new();
        let bogus_view = ViewId(999);
        assert_eq!(core.lower_view(bogus_view), Err(WmCoreError::UnknownView));
    }

    #[test]
    fn claim_pinned_terminal_spawn_returns_session_name_first_time() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("web").unwrap();
        assert_eq!(
            core.claim_pinned_terminal_spawn(tag_id),
            Ok(Some("tag-web".to_string()))
        );
        assert!(core.tags.get(tag_id).unwrap().terminal_spawned);
    }

    #[test]
    fn claim_pinned_terminal_spawn_is_idempotent_returns_none_after_first_claim() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("web").unwrap();
        core.claim_pinned_terminal_spawn(tag_id).unwrap();
        assert_eq!(core.claim_pinned_terminal_spawn(tag_id), Ok(None));
    }

    #[test]
    fn claim_pinned_terminal_spawn_unknown_tag_returns_error() {
        let mut core = WmCore::new();
        let bogus_tag = TagId(63);
        assert_eq!(
            core.claim_pinned_terminal_spawn(bogus_tag),
            Err(WmCoreError::UnknownTag)
        );
    }

    #[test]
    fn claim_pinned_terminal_spawn_session_name_uses_tag_dash_prefix_convention() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("my-tag-name").unwrap();
        assert_eq!(
            core.claim_pinned_terminal_spawn(tag_id),
            Ok(Some("tag-my-tag-name".to_string()))
        );
    }

    #[test]
    fn claim_pinned_terminal_spawn_reachable_via_switch_tag_alone_without_any_keybind() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_id = core.create_tag("web").unwrap();
        core.switch_tag(output_id, tag_id).unwrap();
        assert_eq!(
            core.claim_pinned_terminal_spawn(tag_id),
            Ok(Some("tag-web".to_string()))
        );
    }

    /// Task 6 RED: `raise_view` (the primitive both `cycle_focus` and
    /// `main.rs`'s click-to-focus path use to reorder) must never move the
    /// pinned terminal off the bottom of the stacking order — a real FR4
    /// violation Story 1.4's `cycle_focus`/click-to-focus code didn't
    /// account for since the pinned terminal didn't exist yet.
    #[test]
    fn raise_view_is_a_no_op_for_the_pinned_terminal() {
        let mut core = WmCore::new();
        let pinned = core.register_view(PINNED_TERM_APP_ID);
        let other = core.register_view("app-one");
        assert_eq!(core.stacking_order(), vec![pinned, other]);
        core.raise_view(pinned).unwrap();
        assert_eq!(
            core.stacking_order(),
            vec![pinned, other],
            "raise_view must not move the pinned terminal off the bottom"
        );
    }

    /// Task 6 RED: the same guard exercised through `cycle_focus` (as
    /// `Action::FocusNext` does) rather than calling `raise_view` directly
    /// — cycling focus onto the pinned terminal must not move it off the
    /// bottom of the stacking order.
    #[test]
    fn cycle_focus_does_not_move_pinned_terminal_from_bottom_of_stacking_order() {
        let mut core = WmCore::new();
        let pinned = core.register_view(PINNED_TERM_APP_ID);
        let other = core.register_view("app-one");
        core.lower_view(pinned).unwrap();
        assert_eq!(core.stacking_order(), vec![pinned, other]);

        core.cycle_focus();
        assert_eq!(
            core.stacking_order().first(),
            Some(&pinned),
            "cycling focus onto the pinned terminal must not move it off the bottom"
        );
    }

    /// Code review follow-up (finding #1): `cycle_focus` must never select
    /// the pinned terminal as a target, even though it permanently
    /// occupies `stacking_order.front()` (Task 5/6's own lower-to-bottom
    /// invariant). Before this fix, `cycle_focus` unconditionally read
    /// `front()` and used it as the target, so once a pinned terminal
    /// existed, every `FocusNext` press would repeatedly return the pinned
    /// terminal's id and never advance — this test proves it round-robins
    /// through the two non-pinned views instead, never once returning the
    /// pinned terminal's id.
    #[test]
    fn cycle_focus_skips_pinned_terminal_and_round_robins_through_others() {
        let mut core = WmCore::new();
        let pinned = core.register_view(PINNED_TERM_APP_ID);
        let a = core.register_view("a");
        let b = core.register_view("b");
        core.lower_view(pinned).unwrap();
        assert_eq!(core.stacking_order(), vec![pinned, a, b]);

        for _ in 0..10 {
            let next = core.cycle_focus();
            assert_ne!(
                next,
                Some(pinned),
                "cycle_focus must never select the pinned terminal"
            );
            assert!(
                next == Some(a) || next == Some(b),
                "cycle_focus must only ever return one of the non-pinned views, got {next:?}"
            );
        }
        assert_eq!(
            core.stacking_order().first(),
            Some(&pinned),
            "the pinned terminal must remain fixed at the bottom of stacking_order"
        );
    }

    /// Code review follow-up (finding #1), edge case: when the pinned
    /// terminal is the *only* registered view, there is nothing valid to
    /// cycle to — `cycle_focus` must return `None` rather than falling back
    /// to selecting the pinned terminal itself.
    #[test]
    fn cycle_focus_returns_none_when_only_pinned_terminal_registered() {
        let mut core = WmCore::new();
        core.register_view(PINNED_TERM_APP_ID);
        assert_eq!(core.cycle_focus(), None);
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
            core.cycle_tag(bogus_output),
            Err(WmCoreError::UnknownOutput)
        );
        assert_eq!(core, snapshot);

        assert_eq!(
            core.mark_terminal_spawned(bogus_tag),
            Err(WmCoreError::UnknownTag)
        );
        assert_eq!(core, snapshot);

        assert_eq!(core.raise_view(bogus_view), Err(WmCoreError::UnknownView));
        assert_eq!(core, snapshot);

        assert_eq!(core.lower_view(bogus_view), Err(WmCoreError::UnknownView));
        assert_eq!(core, snapshot);
    }

    #[test]
    fn closable_focused_view_returns_focused_id_when_not_pinned() {
        let mut core = WmCore::new();
        let view_id = core.register_view("foot");
        core.set_focus(view_id).unwrap();
        assert_eq!(core.closable_focused_view(), Some(view_id));
    }

    #[test]
    fn closable_focused_view_returns_none_when_nothing_focused() {
        let mut core = WmCore::new();
        core.register_view("foot");
        assert_eq!(core.closable_focused_view(), None);
    }

    #[test]
    fn closable_focused_view_returns_none_for_pinned_terminal_app_id() {
        let mut core = WmCore::new();
        let view_id = core.register_view(PINNED_TERM_APP_ID);
        core.set_focus(view_id).unwrap();
        assert_eq!(core.closable_focused_view(), None);
    }

    #[test]
    fn closable_focused_view_is_a_pure_query_and_never_mutates_state() {
        let mut core = WmCore::new();
        let view_id = core.register_view("foot");
        core.set_focus(view_id).unwrap();
        let snapshot = core.clone();
        assert_eq!(core.closable_focused_view(), Some(view_id));
        assert_eq!(
            core, snapshot,
            "Some case: closable_focused_view must not mutate state"
        );

        let mut core = WmCore::new();
        let pinned = core.register_view(PINNED_TERM_APP_ID);
        core.set_focus(pinned).unwrap();
        let snapshot = core.clone();
        assert_eq!(core.closable_focused_view(), None);
        assert_eq!(
            core, snapshot,
            "None case: closable_focused_view must not mutate state"
        );
    }

    #[test]
    fn cycle_focus_returns_none_when_no_views_registered() {
        let mut core = WmCore::new();
        assert_eq!(core.cycle_focus(), None);
    }

    #[test]
    fn cycle_focus_moves_front_of_stacking_order_to_back_and_focuses_it() {
        let mut core = WmCore::new();
        let a = core.register_view("a");
        let b = core.register_view("b");
        let c = core.register_view("c");
        assert_eq!(core.stacking_order(), vec![a, b, c]);

        assert_eq!(core.cycle_focus(), Some(a));
        assert_eq!(core.stacking_order(), vec![b, c, a]);
        assert!(core.views.get(&a).unwrap().focused);
    }

    #[test]
    fn cycle_focus_repeated_calls_return_to_original_order_after_a_full_rotation() {
        let mut core = WmCore::new();
        let a = core.register_view("a");
        let b = core.register_view("b");
        let c = core.register_view("c");

        assert_eq!(core.cycle_focus(), Some(a));
        assert_eq!(core.stacking_order(), vec![b, c, a]);

        assert_eq!(core.cycle_focus(), Some(b));
        assert_eq!(core.stacking_order(), vec![c, a, b]);

        assert_eq!(core.cycle_focus(), Some(c));
        assert_eq!(core.stacking_order(), vec![a, b, c]);
    }

    #[test]
    fn cycle_focus_single_view_is_idempotent() {
        let mut core = WmCore::new();
        let id = core.register_view("only");
        assert_eq!(core.cycle_focus(), Some(id));
        assert_eq!(core.stacking_order(), vec![id]);
        assert!(core.views.get(&id).unwrap().focused);
    }

    /// Regression guard for the Story 1.4 code-review follow-up
    /// (`main.rs`'s click-to-focus path must call `raise_view` to keep
    /// `wm_core`'s `stacking_order` synchronized with real z-order — see
    /// `manage_seats`). Exercises the same `raise_view`-then-`cycle_focus`
    /// interaction in isolation, without any Wayland glue: a prior
    /// `raise_view` reorder (standing in for click-to-focus) must be what
    /// `cycle_focus` reads next, not a stale front.
    #[test]
    fn cycle_focus_reflects_a_prior_raise_view_reordering() {
        let mut core = WmCore::new();
        let a = core.register_view("a");
        let b = core.register_view("b");
        let c = core.register_view("c");
        assert_eq!(core.stacking_order(), vec![a, b, c]);

        // Simulate click-to-focus on `b`: raises it to the back, same as
        // `manage_seats`'s `interacted` handling now does.
        core.raise_view(b).unwrap();
        assert_eq!(core.stacking_order(), vec![a, c, b]);

        // cycle_focus must operate on the resulting order (front == a),
        // not any order predating the raise_view call.
        assert_eq!(core.cycle_focus(), Some(a));
        assert_eq!(core.stacking_order(), vec![c, b, a]);
        assert!(core.views.get(&a).unwrap().focused);
    }
}
