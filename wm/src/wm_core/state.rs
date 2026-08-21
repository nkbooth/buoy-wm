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

//! `WmCore`: the aggregator that owns the view/tag/output registries and
//! exposes `wm-core`'s public mutating API.

use std::collections::{HashMap, VecDeque};

use super::MAX_TAGS;
use super::ids::{OutputId, TagId, ViewId};
use super::output::Output;
use super::tag::{TagRegistry, TagRegistryError};
use super::tag_set::TagSet;
use super::view::{Geometry, View};

/// The `app_id` prefix reserved for the lazily-spawned pinned terminals.
/// Each tag's terminal is spawned with `pinned-term-<tag id>` (see
/// [`pinned_term_app_id`]), which is how a mapped window is recognised as a
/// pinned terminal *and* which tag it belongs to. It is also the test the
/// close keybind applies to the acting seat's own focused window, which is
/// what enforces the architectural constraint that the pinned terminal is
/// never closed via a routed keybind (`architectural-constraints.md`).
///
/// This is a convention, not an authenticated identity (audit finding
/// D-02): `app_id` arrives from the client via `xdg_toplevel.set_app_id`,
/// so any same-UID Wayland client can claim one of these strings and
/// receive the pinned terminal's treatment — unclosable by keybind,
/// force-lowered, geometry-managed. That is window-management confusion
/// inside the documented trust boundary, not an escalation, and the
/// protocol offers nothing better to check it against:
/// `river_window_v1.unreliable_pid` says in its own description that it
/// "must not be used for anything security sensitive".
pub const PINNED_TERM_APP_ID: &str = "pinned-term";

/// The `app_id` a pinned terminal for `tag_id` is spawned with.
///
/// The tag is encoded in the identity rather than correlated by spawn order
/// because the two spawn sites run on different threads and a window maps
/// whenever its process gets round to it — so position-based correlation
/// silently swapped two tags' terminals whenever the later spawn won the
/// race (audit finding D-01(b)).
pub fn pinned_term_app_id(tag_id: TagId) -> String {
    format!("{PINNED_TERM_APP_ID}-{}", tag_id.0)
}

/// The tag a pinned terminal's `app_id` names, or `None` if `app_id` is not
/// one this WM emits.
///
/// Accepts only the exact spelling [`pinned_term_app_id`] produces for a
/// registrable tag id: `+3`, `03` and `pinned-term-300` are all rejected
/// rather than folded onto tag 3, so one tag's terminal identity has
/// exactly one spelling.
pub fn tag_id_from_pinned_app_id(app_id: &str) -> Option<TagId> {
    let suffix = app_id.strip_prefix(PINNED_TERM_APP_ID)?.strip_prefix('-')?;
    let tag_id = TagId(suffix.parse::<u8>().ok()?);
    (tag_id.0 < MAX_TAGS && pinned_term_app_id(tag_id) == app_id).then_some(tag_id)
}

/// Whether `app_id` names a pinned terminal — the replacement for the
/// `app_id == PINNED_TERM_APP_ID` equality test every call site used while
/// every pinned terminal shared one `app_id`.
pub fn is_pinned_term_app_id(app_id: &str) -> bool {
    tag_id_from_pinned_app_id(app_id).is_some()
}

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
    // Constructible via a live path since Story 1.7 wired `switch_tag`
    // (through `cycle_tag`) into `main.rs`'s tag-cycle keybind.
    UnknownOutput,
    /// The tag registry already holds the maximum of 64 tags (ADR-006).
    // Constructible via a live path via `create_tag`, wired into
    // `buoy-tag-picker`'s IPC-driven tag-creation flow.
    TagLimitReached,
    /// The proposed tag name failed the registry's validation — see
    /// [`TagRegistry::create_tag`](super::tag::TagRegistry::create_tag)
    /// for the rules (audit finding E-01).
    InvalidTagName,
}

/// Journal text, deliberately *not* the wire text.
///
/// `ipc::dispatch::describe_wm_core_error` renders the same variants for
/// the socket, and the two must not be merged: the wire strings are a
/// stability contract another crate string-matches on (audit finding
/// E-06), whereas a log line is free to be reworded whenever it reads
/// badly. Every variant is a unit variant, so nothing here can carry
/// peer-controlled text into a journal line.
impl std::fmt::Display for WmCoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WmCoreError::UnknownView => f.write_str("no such view is registered"),
            WmCoreError::UnknownTag => f.write_str("no such tag is registered"),
            WmCoreError::UnknownOutput => f.write_str("no such output is registered"),
            WmCoreError::TagLimitReached => {
                write!(f, "the tag registry is already full at {MAX_TAGS} tags")
            }
            WmCoreError::InvalidTagName => f.write_str(
                "the tag name is empty, too long, or contains a path \
                 separator or a control character",
            ),
        }
    }
}

impl std::error::Error for WmCoreError {}

/// A snapshot of one registered tag: its id and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagSnapshot {
    pub id: TagId,
    pub name: String,
}

/// A snapshot of one registered view: its id, `app_id`, and current tag
/// membership (ascending, registry-creation order — see
/// [`WmCore::snapshot`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewSnapshot {
    pub id: ViewId,
    pub app_id: String,
    pub tags: Vec<TagId>,
}

/// A snapshot of one registered output: its id and currently-displayed tag,
/// if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSnapshot {
    pub id: OutputId,
    pub current_tag: Option<TagId>,
}

/// A full, point-in-time, read-only view of a [`WmCore`]'s state: every
/// registered tag/view/output plus the currently-focused view (if any).
/// Plain data, no `serde` derives — `wm-core` stays protocol-agnostic; the
/// IPC layer (`wm/src/ipc/`) owns converting this into wire types.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WmCoreSnapshot {
    pub tags: Vec<TagSnapshot>,
    pub views: Vec<ViewSnapshot>,
    pub outputs: Vec<OutputSnapshot>,
    pub focused_view: Option<ViewId>,
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
    /// The single WM-wide focused view, if any — one WM-wide focus, not a
    /// per-seat focus set (data-model.md). The sole representation of
    /// focus: `View` deliberately carries no flag that could disagree with
    /// it (audit finding K-02).
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
    ///
    /// Test-only: production goes through `WindowManager`'s
    /// `#[derive(Default)]`, which constructs its `wm_core` field via
    /// `WmCore::default()`. `#[cfg(test)]` rather than
    /// `#[allow(dead_code)]` so that stays compiler-enforced (audit finding
    /// J-09).
    ///
    /// `#[cfg(test)]` is per-crate, so this exists only for this crate's own
    /// unit tests — the binary's tests and anything under `tests/` see the
    /// library compiled without `cfg(test)` and want `WmCore::default()`,
    /// which is the same constructor without the carve-out.
    #[cfg(test)]
    pub fn new() -> Self {
        WmCore::default()
    }

    /// Creates a tag with the given `name` via the tag registry, or
    /// returns the existing [`TagId`] if a tag with that name is already
    /// registered (idempotent by name; see
    /// [`TagRegistry::create_tag`](super::tag::TagRegistry::create_tag)).
    /// Fails with [`WmCoreError::TagLimitReached`] if the registry already
    /// holds 64 tags, and with [`WmCoreError::InvalidTagName`] if the name
    /// fails the registry's validation.
    // Wired into `buoy-tag-picker`'s IPC-driven tag-creation flow (Epic 2).
    pub fn create_tag(&mut self, name: impl Into<String>) -> Result<TagId, WmCoreError> {
        let name = name.into();
        self.tags.create_tag(&name).map_err(|err| match err {
            TagRegistryError::Full => WmCoreError::TagLimitReached,
            TagRegistryError::UnknownTag => WmCoreError::UnknownTag,
            TagRegistryError::InvalidName => WmCoreError::InvalidTagName,
        })
    }

    /// Registers a new view for `app_id`, returning a fresh, unique
    /// [`ViewId`]. The new view starts with empty tags, `floating ==
    /// true`, and zeroed [`Geometry`]. It is not
    /// focused; focus lives only in [`WmCore::focused_view`].
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
    // Wired via `ipc::dispatch::handle_request`'s `Request::ToggleTag` arm
    // since Story 2.1 — reachable and real today even though no client
    // sends it in-process yet (Epic 2's assign-mode picker, Story 2.2+,
    // doesn't exist until later stories; that distinction matters for an
    // accurate comment).
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
        self.focused_view = Some(id);
        Ok(())
    }

    /// Clears whichever view is currently focused, if any. A no-op if no
    /// view is focused.
    pub fn clear_focus(&mut self) {
        self.focused_view = None;
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

    /// Removes a registered output entirely, returning the tag it was
    /// displaying (if any). Fails with [`WmCoreError::UnknownOutput`] if
    /// `output_id` is not registered, leaving state unchanged; never
    /// panics.
    ///
    /// Deliberately does **not** decide where an orphaned tag goes next —
    /// `wm_core` stays protocol-agnostic (see this module's own
    /// [`WmCore::is_view_visible`]-style boundary): it has no way to know
    /// which *other* output should inherit the tag, since that requires
    /// Wayland-derived, pointer-position-aware state (`main.rs`'s
    /// `active_output_id`) that `wm_core` has no access to. It only forgets
    /// the removed output and reports what it was showing, mirroring
    /// [`WmCore::unregister_view`]'s "report enough for the caller to act,
    /// don't decide for them" shape.
    // Closes the gap Story 1.7's own Dev Agent Record flagged at the time:
    // "`wm_core` has no `unregister_output` method ... A removed real
    // output leaves a stale, permanently-registered `wm_core` output
    // behind, eligible forever after to be selected by `active_output_id`."
    // Wired into `main.rs`'s `remove_outputs` since Story 2.8.
    pub fn unregister_output(&mut self, output_id: OutputId) -> Result<Option<TagId>, WmCoreError> {
        let output = self
            .outputs
            .remove(&output_id)
            .ok_or(WmCoreError::UnknownOutput)?;
        Ok(output.current_tag)
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
    /// than reimplementing it — the same function the IPC `switch-tag`
    /// handler calls, so there is only one place the "next tag" rule lives.
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
    /// should spawn the terminal with, as
    /// `<terminal> -a <pinned_term_app_id(tag_id)> zellij attach --create
    /// tag-<name>`. On every subsequent call for the same tag, returns
    /// `Ok(None)` without side effects. Deliberately bundled into one
    /// method (mirroring `cycle_focus`'s precedent of composing several
    /// `wm-core`-internal steps into one atomic call) rather than exposing
    /// the spawned-flag and the name as two separate queries, so no caller
    /// can accidentally check without claiming or claim twice. Fails with
    /// [`WmCoreError::UnknownTag`] for an unregistered id.
    ///
    /// The claim is necessarily committed before the process it claims for
    /// exists, so a caller whose spawn fails must undo it with
    /// [`WmCore::release_pinned_terminal_claim`].
    // Wired into `main.rs`'s tag-cycle/tag-create keybind path since Story
    // 1.7, via `ensure_pinned_terminal_spawned`; and into
    // `ipc::dispatch::handle_request`'s `SwitchTag` arm since Story 2.4.
    pub fn claim_pinned_terminal_spawn(
        &mut self,
        tag_id: TagId,
    ) -> Result<Option<String>, WmCoreError> {
        let tag = self.tags.get(tag_id).ok_or(WmCoreError::UnknownTag)?;
        if tag.terminal_spawned {
            return Ok(None);
        }
        // The `tag-` prefix is load-bearing security, not cosmetics: tag
        // names are unvalidated, and this unconditional prefix is the only
        // thing keeping a tag called `--layout` from reaching zellij's argv
        // as a flag-shaped token. Pinned by
        // `pinned_terminal_session_name_can_never_begin_with_a_dash`
        // (audit finding D-03).
        let session_name = format!("tag-{}", tag.name);
        self.mark_terminal_spawned(tag_id)
            .expect("tag_id was just confirmed registered above");
        Ok(Some(session_name))
    }

    /// Rolls back a [`WmCore::claim_pinned_terminal_spawn`] whose spawn
    /// then failed, so the tag can claim again.
    ///
    /// Without this a failed spawn is permanent (audit finding D-01): the
    /// tag never retries, because the claim is idempotent by design. Fails
    /// with [`WmCoreError::UnknownTag`] for an unregistered id.
    // Called from `main.rs`'s `spawn_pinned_terminal_or_release_claim`, the
    // one place that knows whether the spawn actually happened.
    pub fn release_pinned_terminal_claim(&mut self, tag_id: TagId) -> Result<(), WmCoreError> {
        self.tags
            .unmark_terminal_spawned(tag_id)
            .map_err(|_| WmCoreError::UnknownTag)
    }

    /// Returns the stacking/render order, front-to-back (front=bottom,
    /// back=top).
    /// Test-only. Story 1.4 code-review follow-up (efficiency):
    /// `cycle_focus` now reads `self.stacking_order.front()` directly
    /// (O(1), no allocation) instead of going through this O(n)-allocating
    /// accessor, leaving it with no production call site. Exercised
    /// extensively by tests, where whole-order assertions read far more
    /// naturally as a `Vec`. `#[cfg(test)]` rather than
    /// `#[allow(dead_code)]` so that stays compiler-enforced (audit finding
    /// J-09).
    #[cfg(test)]
    pub fn stacking_order(&self) -> Vec<ViewId> {
        self.stacking_order.iter().copied().collect()
    }

    /// Moves an already-registered view to the back (top) of the stacking
    /// order. A no-op success (state unchanged) for the pinned terminal
    /// (recognised by [`is_pinned_term_app_id`]): FR4 requires it always be
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
        if is_pinned_term_app_id(&view.app_id) {
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
    /// terminal (recognised by [`is_pinned_term_app_id`]) or nothing is focused. A
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
    // acting seat's own focused `Window.app_id` directly instead, leaving
    // this test-only; `#[cfg(test)]` rather than `#[allow(dead_code)]` so
    // that stays compiler-enforced (audit finding J-09).
    #[cfg(test)]
    pub fn closable_focused_view(&self) -> Option<ViewId> {
        let id = self.focused_view?;
        let view = self.views.get(&id)?;
        (!is_pinned_term_app_id(&view.app_id)).then_some(id)
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
    ///
    /// Code review follow-up (Story 2.7): also skips any view
    /// `is_view_visible` reports as hidden (not shown on any output's
    /// current tag) — without this, cycling focus could jump to a window
    /// on a different, currently-hidden tag, giving it real keyboard focus
    /// while nothing on screen shows it's focused. A view with an unknown
    /// id can't occur here (`id` is always read from this `WmCore`'s own
    /// `stacking_order`), so `unwrap_or(false)` on the `Result` is
    /// unreachable in practice, not a silently-wrong fallback.
    pub fn cycle_focus(&mut self) -> Option<ViewId> {
        let target = self.stacking_order.iter().copied().find(|&id| {
            self.views
                .get(&id)
                .is_some_and(|view| !is_pinned_term_app_id(&view.app_id))
                && self.is_view_visible(id).unwrap_or(false)
        })?;
        self.raise_view(target)
            .expect("target was just read from this WmCore's own stacking order");
        self.set_focus(target)
            .expect("target was just read from this WmCore's own stacking order");
        Some(target)
    }

    /// Returns the id of the tag named `name`, if one exists. Matching is
    /// exact: a named-tag keybind that matched loosely could switch to a
    /// near-miss tag and then never be able to create the one actually
    /// asked for. A pure query; never mutates `self`.
    ///
    /// Delegates to [`TagRegistry::id_by_name`]'s existing name index
    /// rather than scanning — the registry already keeps a
    /// `HashMap<String, TagId>` for `create_tag`'s own idempotency check
    /// (code-review follow-up: this was an allocating O(n) scan over
    /// `ids()`).
    pub fn tag_id_by_name(&self, name: &str) -> Option<TagId> {
        self.tags.id_by_name(name)
    }

    /// Like [`WmCore::topmost_visible_view`], but restricted to views on
    /// `output_id`'s currently-displayed tag.
    ///
    /// Code-review follow-up: the unscoped query is the wrong question for
    /// the tag-switch focus repair. `is_view_visible` is true if *any*
    /// output shows one of a view's tags, so on a multi-output session a
    /// window on another monitor's current tag could win the scan and take
    /// keyboard focus away from the output the user actually switched —
    /// the pointer stays on one screen while typing goes to another.
    ///
    /// Returns `None` for an output showing no tag, and for an unregistered
    /// `output_id` — both mean "nothing on this output to focus", which
    /// callers treat as clearing focus rather than falling back to a
    /// WM-wide answer.
    pub fn topmost_visible_view_on(&self, output_id: OutputId) -> Option<ViewId> {
        let current_tag = self.outputs.get(&output_id)?.current_tag?;
        self.stacking_order.iter().rev().copied().find(|id| {
            self.views
                .get(id)
                .is_some_and(|view| view.tags.contains(current_tag.0))
        })
    }

    /// Returns the topmost currently-visible view — the view keyboard focus
    /// should fall to when whatever held it stops being visible, which is
    /// what a tag switch does to every window on the outgoing tag. Scans
    /// `stacking_order` back-to-front (top-to-bottom) and returns the first
    /// view [`WmCore::is_view_visible`] accepts, so a floating window on the
    /// newly-shown tag always outranks that tag's pinned terminal, which
    /// [`WmCore::lower_view`] keeps at the front (bottom) of the order.
    ///
    /// Unlike [`WmCore::cycle_focus`], the pinned terminal is a legitimate
    /// result: `cycle_focus` skips it so repeated `FocusNext` presses can
    /// round-robin the real windows, whereas this query is the fallback that
    /// deliberately lands on the tag's terminal backdrop when nothing else
    /// on that tag is visible. Returns `None` only when no registered view
    /// is visible at all, which callers treat as "clear focus".
    ///
    /// A pure query; never mutates `self`. An id in `stacking_order` always
    /// has a matching entry in `views`, so `is_view_visible`'s
    /// `UnknownView` arm is unreachable here — `unwrap_or(false)` is a
    /// safe-by-construction guard (NFR2), not a silently-wrong fallback,
    /// matching `cycle_focus`'s precedent for the same call.
    pub fn topmost_visible_view(&self) -> Option<ViewId> {
        self.stacking_order
            .iter()
            .rev()
            .copied()
            .find(|&id| self.is_view_visible(id).unwrap_or(false))
    }

    /// A full, read-only snapshot of this `WmCore`'s current state — the
    /// one new query this story adds to `wm-core`'s public API (every other
    /// IPC-mutation handler reuses an existing mutator, no `wm-core`
    /// changes needed). Views and outputs are sorted ascending by id for
    /// deterministic output (`HashMap` iteration order is otherwise
    /// unspecified) — both for test assertions and for real clients. A
    /// pure query; never mutates `self`.
    pub fn snapshot(&self) -> WmCoreSnapshot {
        let all_tag_ids = self.tags.ids();

        let tags = all_tag_ids
            .iter()
            .map(|&id| TagSnapshot {
                id,
                name: self
                    .tags
                    .get(id)
                    .expect("id from ids() is always present")
                    .name
                    .clone(),
            })
            .collect();

        let mut views: Vec<ViewSnapshot> = self
            .views
            .values()
            .map(|view| ViewSnapshot {
                id: view.id,
                app_id: view.app_id.clone(),
                tags: all_tag_ids
                    .iter()
                    .filter(|id| view.tags.contains(id.0))
                    .copied()
                    .collect(),
            })
            .collect();
        views.sort_by_key(|v| v.id);

        let mut outputs: Vec<OutputSnapshot> = self
            .outputs
            .values()
            .map(|output| OutputSnapshot {
                id: output.id,
                current_tag: output.current_tag,
            })
            .collect();
        outputs.sort_by_key(|o| o.id);

        WmCoreSnapshot {
            tags,
            views,
            outputs,
            focused_view: self.focused_view,
        }
    }

    /// The pure visibility DECISION (Story 2.7 Task 1): a view is visible
    /// if any of its tags matches any registered output's `current_tag`
    /// (standard multi-output dwm semantics - a view can be simultaneously
    /// visible on more than one output). A view with no tags at all is
    /// visible by default (the one exception, for the bootstrap case
    /// before any tag exists yet - see this story's ACs). `main.rs` is the
    /// only caller that turns this into a real `river_window_v1.show()`/
    /// `hide()` request; this method itself has no knowledge of the
    /// Wayland protocol (`wm-core` stays protocol-agnostic). Fails with
    /// [`WmCoreError::UnknownView`] for an unregistered id. A pure query;
    /// never mutates `self`.
    pub fn is_view_visible(&self, view_id: ViewId) -> Result<bool, WmCoreError> {
        let view = self.views.get(&view_id).ok_or(WmCoreError::UnknownView)?;
        if view.tags == TagSet::default() {
            return Ok(true);
        }
        Ok(self.outputs.values().any(|output| {
            output
                .current_tag
                .is_some_and(|tag_id| view.tags.contains(tag_id.0))
        }))
    }

    /// Returns the id of the tag (if any) currently assigned to `view_id`,
    /// in tag-registry creation order (mirrors [`WmCore::snapshot`]'s
    /// ordering convention for a `ViewSnapshot`'s `tags` field). Story 2.7
    /// Task 5's `main.rs` fullscreen-recompute pass uses this to recover
    /// which tag a pinned terminal was associated with at mapping time
    /// (Task 2), since `wm-core`'s own tag membership - not the transient
    /// spawn queue - is the durable source of truth afterward. Fails with
    /// [`WmCoreError::UnknownView`] for an unregistered id. A pure query;
    /// never mutates `self`.
    pub fn view_tags(&self, view_id: ViewId) -> Result<Vec<TagId>, WmCoreError> {
        let view = self.views.get(&view_id).ok_or(WmCoreError::UnknownView)?;
        Ok(self
            .tags
            .ids()
            .into_iter()
            .filter(|id| view.tags.contains(id.0))
            .collect())
    }

    /// Returns the [`OutputId`] currently displaying `tag_id`, if any.
    /// ADR-005's one-tag-per-output invariant (enforced by
    /// [`WmCore::switch_tag`]'s reroute logic) guarantees at most one
    /// output ever matches, so the first match found is unambiguous.
    /// Returns `None` both for a tag no output currently shows and for an
    /// unregistered `tag_id` - Story 2.7 Task 5's only caller
    /// (`main.rs`'s pinned-terminal fullscreen resolution) treats both
    /// cases identically ("nowhere to fullscreen it, fall back to hidden"),
    /// so no separate error variant is needed here. A pure query; never
    /// mutates `self`.
    pub fn output_showing_tag(&self, tag_id: TagId) -> Option<OutputId> {
        self.outputs
            .values()
            .find(|output| output.current_tag == Some(tag_id))
            .map(|output| output.id)
    }

    /// Returns `output_id`'s current tag, or `None` if it has no current
    /// tag or is not registered. Story 2.7 Task 4's `main.rs` auto-tag-on-
    /// create wiring uses this to resolve the active output's current tag
    /// for a freshly registered non-pinned window; that call site only
    /// ever passes an id already known to be registered (from
    /// `active_output_id()`'s own live-output scan), so collapsing
    /// "unregistered" and "no tag" into one `None` costs that caller
    /// nothing. A pure query; never mutates `self`.
    pub fn output_current_tag(&self, output_id: OutputId) -> Option<TagId> {
        self.outputs.get(&output_id).and_then(|o| o.current_tag)
    }

    /// The number of tags currently registered. Thin delegation to
    /// [`TagRegistry::count`](super::tag::TagRegistry::count); exists so
    /// `main.rs` can detect "completely fresh, no tags at all yet" (Story
    /// 2.10's login-bootstrap check) without the heavier allocation of a
    /// full [`WmCore::snapshot`]. A pure query; never mutates `self`.
    pub fn tag_count(&self) -> usize {
        self.tags.count()
    }
}

#[cfg(test)]
mod tests {
    /// Audit finding F-04: ~17 log sites formatted this error with `{e:?}`,
    /// so the user read `TagLimitReached` in their journal. `Display` is
    /// what makes `{e}` say something at them instead.
    #[test]
    fn wm_core_error_reads_as_a_sentence_not_a_rust_identifier() {
        let rendered = super::WmCoreError::TagLimitReached.to_string();
        assert!(
            rendered.contains(&super::MAX_TAGS.to_string()),
            "{rendered}"
        );
        assert_ne!(
            rendered,
            format!("{:?}", super::WmCoreError::TagLimitReached)
        );
    }

    /// Without `std::error::Error`, `WmCoreError` cannot flow into `main`'s
    /// `Box<dyn std::error::Error>` at all, which is why every `wm_core`
    /// failure has to be hand-logged.
    #[test]
    fn wm_core_error_is_a_std_error() {
        let boxed: Box<dyn std::error::Error> = Box::new(super::WmCoreError::UnknownView);
        assert!(boxed.source().is_none());
    }

    use super::super::ids::OutputId;
    use super::{
        WmCore, WmCoreError, is_pinned_term_app_id, pinned_term_app_id, tag_id_from_pinned_app_id,
    };
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
        assert_eq!(view.geometry, Geometry::default());
        assert_eq!(core.focused_view, None);
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
    fn set_focus_records_the_target_view_as_focused() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        core.set_focus(view_id).unwrap();
        assert_eq!(core.focused_view, Some(view_id));
    }

    #[test]
    fn set_focus_on_second_view_clears_first() {
        let mut core = WmCore::new();
        let first = core.register_view("app-one");
        let second = core.register_view("app-two");
        core.set_focus(first).unwrap();
        core.set_focus(second).unwrap();
        assert_eq!(
            core.focused_view,
            Some(second),
            "at most one view is focused, and it is the last one focused"
        );
    }

    #[test]
    fn clear_focus_clears_currently_focused_view() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        core.set_focus(view_id).unwrap();
        core.clear_focus();
        assert_eq!(core.focused_view, None);
    }

    #[test]
    fn set_focus_unknown_id_returns_error_and_does_not_change_focus() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        core.set_focus(view_id).unwrap();
        let bogus_view = ViewId(999);
        assert_eq!(core.set_focus(bogus_view), Err(WmCoreError::UnknownView));
        assert_eq!(core.focused_view, Some(view_id));
    }

    #[test]
    fn register_output_returns_fresh_id_with_no_current_tag() {
        let mut core = WmCore::new();
        let first = core.register_output();
        let second = core.register_output();
        assert_ne!(first, second);
        assert_eq!(core.outputs.get(&first).unwrap().current_tag, None);
    }

    // Story 2.8 Task 4 RED: `unregister_output` doesn't exist yet - closes
    // the gap Story 1.7's Dev Agent Record flagged (removed real outputs
    // left a permanent ghost `wm_core` output behind).

    #[test]
    fn unregister_output_removes_the_output_and_returns_none_when_it_had_no_current_tag() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        assert_eq!(core.unregister_output(output_id), Ok(None));
        assert!(!core.outputs.contains_key(&output_id));
    }

    #[test]
    fn unregister_output_removes_the_output_and_returns_its_current_tag() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_id = core.create_tag("web").unwrap();
        core.switch_tag(output_id, tag_id).unwrap();
        assert_eq!(core.unregister_output(output_id), Ok(Some(tag_id)));
        assert!(!core.outputs.contains_key(&output_id));
    }

    #[test]
    fn unregister_output_unknown_id_returns_error_and_leaves_state_unchanged() {
        let mut core = WmCore::new();
        core.register_output();
        let bogus_output = crate::wm_core::ids::OutputId(9999);
        let snapshot = core.clone();
        assert_eq!(
            core.unregister_output(bogus_output),
            Err(WmCoreError::UnknownOutput)
        );
        assert_eq!(core, snapshot);
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
    fn create_tag_returns_fresh_id_via_public_api() {
        let mut core = WmCore::new();
        let id = core.create_tag("web").unwrap();
        assert_eq!(core.tags.get(id).unwrap().name, "web");
    }

    /// Audit finding E-01: the registry's name validation must surface as a
    /// distinct `WmCoreError`, not be folded into an existing one, because
    /// the IPC layer turns it into the message the picker shows.
    #[test]
    fn create_tag_maps_an_invalid_name_to_its_own_error() {
        let mut core = WmCore::new();
        assert_eq!(core.create_tag(""), Err(WmCoreError::InvalidTagName));
        assert_eq!(core.create_tag("web/dev"), Err(WmCoreError::InvalidTagName));
        assert_eq!(core.tag_count(), 0);
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
    fn tag_count_is_zero_on_a_fresh_registry() {
        let core = WmCore::new();
        assert_eq!(core.tag_count(), 0);
    }

    #[test]
    fn tag_count_reflects_created_tags() {
        let mut core = WmCore::new();
        core.create_tag("web").unwrap();
        core.create_tag("term").unwrap();
        assert_eq!(core.tag_count(), 2);
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
        let pinned = core.register_view(&pinned_term_app_id(TagId(0)));
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
        let pinned = core.register_view(&pinned_term_app_id(TagId(0)));
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
        let pinned = core.register_view(&pinned_term_app_id(TagId(0)));
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
        core.register_view(&pinned_term_app_id(TagId(0)));
        assert_eq!(core.cycle_focus(), None);
    }

    /// Code review follow-up (Story 2.7): before this fix, `cycle_focus`
    /// only excluded the pinned terminal, so it could select a view on a
    /// tag that isn't currently shown on any output — giving a hidden
    /// window real keyboard focus with nothing visible to receive it. A
    /// view whose tag isn't displayed anywhere must never be a cycle
    /// target, even though it's still in `stacking_order`.
    #[test]
    fn cycle_focus_skips_views_not_visible_on_any_output_current_tag() {
        let mut core = WmCore::new();
        let visible_tag = core.create_tag("web").unwrap();
        let hidden_tag = core.create_tag("term").unwrap();
        let output_id = core.register_output();
        core.switch_tag(output_id, visible_tag).unwrap();

        let shown = core.register_view("shown-app");
        core.toggle_view_tag(shown, visible_tag).unwrap();
        let hidden = core.register_view("hidden-app");
        core.toggle_view_tag(hidden, hidden_tag).unwrap();

        for _ in 0..10 {
            let next = core.cycle_focus();
            assert_ne!(
                next,
                Some(hidden),
                "cycle_focus must never select a view hidden on every output"
            );
            assert_eq!(next, Some(shown));
        }
    }

    /// Code review follow-up (Story 2.7), edge case: if every other
    /// registered view is hidden, `cycle_focus` must return `None` rather
    /// than falling back to a hidden view just because it's the only
    /// non-pinned candidate left.
    #[test]
    fn cycle_focus_returns_none_when_only_candidate_is_hidden() {
        let mut core = WmCore::new();
        let visible_tag = core.create_tag("web").unwrap();
        let hidden_tag = core.create_tag("term").unwrap();
        let output_id = core.register_output();
        core.switch_tag(output_id, visible_tag).unwrap();

        let pinned = core.register_view(&pinned_term_app_id(TagId(0)));
        core.toggle_view_tag(pinned, visible_tag).unwrap();
        core.lower_view(pinned).unwrap();
        let hidden = core.register_view("hidden-app");
        core.toggle_view_tag(hidden, hidden_tag).unwrap();

        assert_eq!(core.cycle_focus(), None);
    }

    /// Code-review follow-up: the WM-wide query is wrong for the
    /// tag-switch repair path. A view on a *different* output's current
    /// tag is still "visible" WM-wide, so an unscoped scan could pull
    /// keyboard focus onto another monitor after switching this one.
    #[test]
    fn topmost_visible_view_on_ignores_views_visible_only_on_another_output() {
        let mut core = WmCore::new();
        let here = core.create_tag("web").unwrap();
        let elsewhere = core.create_tag("email").unwrap();
        let this_output = core.register_output();
        let other_output = core.register_output();
        core.switch_tag(this_output, here).unwrap();
        core.switch_tag(other_output, elsewhere).unwrap();

        let mine = core.register_view("app-one");
        core.toggle_view_tag(mine, here).unwrap();
        // Registered last, so it sits at the top of the stacking order and
        // would win any WM-wide scan.
        let theirs = core.register_view("app-two");
        core.toggle_view_tag(theirs, elsewhere).unwrap();

        assert_eq!(core.topmost_visible_view(), Some(theirs));
        assert_eq!(core.topmost_visible_view_on(this_output), Some(mine));
        assert_eq!(core.topmost_visible_view_on(other_output), Some(theirs));
    }

    #[test]
    fn topmost_visible_view_on_returns_none_for_an_output_showing_nothing() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("web").unwrap();
        let output_id = core.register_output();
        let empty_output = core.register_output();
        core.switch_tag(output_id, tag_id).unwrap();

        let view_id = core.register_view("app-one");
        core.toggle_view_tag(view_id, tag_id).unwrap();

        // No current tag at all, and an unknown id, both resolve to None
        // rather than falling back to a WM-wide answer.
        assert_eq!(core.topmost_visible_view_on(empty_output), None);
        assert_eq!(core.topmost_visible_view_on(OutputId(9999)), None);
    }

    /// The pinned-terminal fallback has to survive the output scoping —
    /// it is the whole point of the query.
    #[test]
    fn topmost_visible_view_on_falls_back_to_that_outputs_pinned_terminal() {
        let mut core = WmCore::new();
        let here = core.create_tag("web").unwrap();
        let elsewhere = core.create_tag("email").unwrap();
        let this_output = core.register_output();
        let other_output = core.register_output();
        core.switch_tag(this_output, here).unwrap();
        core.switch_tag(other_output, elsewhere).unwrap();

        let pinned = core.register_view(&pinned_term_app_id(TagId(0)));
        core.toggle_view_tag(pinned, here).unwrap();
        core.lower_view(pinned).unwrap();
        let other = core.register_view("app-one");
        core.toggle_view_tag(other, elsewhere).unwrap();

        assert_eq!(core.topmost_visible_view_on(this_output), Some(pinned));
    }

    #[test]
    fn tag_id_by_name_finds_an_existing_tag() {
        let mut core = WmCore::new();
        let web = core.create_tag("web").unwrap();
        core.create_tag("term").unwrap();

        assert_eq!(core.tag_id_by_name("web"), Some(web));
    }

    #[test]
    fn tag_id_by_name_returns_none_for_an_unknown_name() {
        let mut core = WmCore::new();
        core.create_tag("web").unwrap();

        assert_eq!(core.tag_id_by_name("nope"), None);
        assert_eq!(core.tag_id_by_name(""), None);
    }

    /// Tag names are matched exactly — a named-tag keybind must not switch
    /// to a differently-cased tag and then be unable to create the one the
    /// user actually asked for.
    #[test]
    fn tag_id_by_name_is_case_sensitive() {
        let mut core = WmCore::new();
        core.create_tag("web").unwrap();

        assert_eq!(core.tag_id_by_name("Web"), None);
    }

    #[test]
    fn topmost_visible_view_returns_none_for_an_empty_core() {
        assert_eq!(WmCore::new().topmost_visible_view(), None);
    }

    #[test]
    fn topmost_visible_view_returns_the_back_of_stacking_order_when_visible() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("web").unwrap();
        let output_id = core.register_output();
        core.switch_tag(output_id, tag_id).unwrap();

        let lower = core.register_view("app-one");
        core.toggle_view_tag(lower, tag_id).unwrap();
        let upper = core.register_view("app-two");
        core.toggle_view_tag(upper, tag_id).unwrap();

        assert_eq!(core.topmost_visible_view(), Some(upper));
    }

    /// The tag-switch case this query exists for: the topmost view belongs
    /// to a tag no output shows any more, so focus must fall through to the
    /// highest view that *is* visible rather than stopping at the top.
    #[test]
    fn topmost_visible_view_skips_views_hidden_on_another_tag() {
        let mut core = WmCore::new();
        let visible_tag = core.create_tag("web").unwrap();
        let hidden_tag = core.create_tag("term").unwrap();
        let output_id = core.register_output();
        core.switch_tag(output_id, visible_tag).unwrap();

        let visible = core.register_view("app-one");
        core.toggle_view_tag(visible, visible_tag).unwrap();
        let hidden = core.register_view("app-two");
        core.toggle_view_tag(hidden, hidden_tag).unwrap();

        assert_eq!(core.topmost_visible_view(), Some(visible));
    }

    /// Unlike [`WmCore::cycle_focus`], the pinned terminal is a legitimate
    /// target here — it is the intended last resort when a tag switch leaves
    /// no floating window visible, and `lower_view` keeping it at the front
    /// (bottom) of the stacking order is exactly what makes a back-to-front
    /// scan reach it last.
    #[test]
    fn topmost_visible_view_falls_back_to_the_visible_pinned_terminal() {
        let mut core = WmCore::new();
        let visible_tag = core.create_tag("web").unwrap();
        let hidden_tag = core.create_tag("term").unwrap();
        let output_id = core.register_output();
        core.switch_tag(output_id, visible_tag).unwrap();

        let pinned = core.register_view(&pinned_term_app_id(TagId(0)));
        core.toggle_view_tag(pinned, visible_tag).unwrap();
        core.lower_view(pinned).unwrap();
        let hidden = core.register_view("app-one");
        core.toggle_view_tag(hidden, hidden_tag).unwrap();

        assert_eq!(core.topmost_visible_view(), Some(pinned));
    }

    /// A pinned terminal belonging to some *other* tag must not be picked
    /// either — every candidate goes through the same `is_view_visible`
    /// gate, so "no view is visible" stays `None` rather than resolving to
    /// an off-tag terminal.
    #[test]
    fn topmost_visible_view_returns_none_when_every_view_is_hidden() {
        let mut core = WmCore::new();
        let shown_tag = core.create_tag("web").unwrap();
        let hidden_tag = core.create_tag("term").unwrap();
        let output_id = core.register_output();
        core.switch_tag(output_id, shown_tag).unwrap();

        let pinned = core.register_view(&pinned_term_app_id(TagId(0)));
        core.toggle_view_tag(pinned, hidden_tag).unwrap();
        core.lower_view(pinned).unwrap();
        let hidden = core.register_view("app-one");
        core.toggle_view_tag(hidden, hidden_tag).unwrap();

        assert_eq!(core.topmost_visible_view(), None);
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
    // Cognitive complexity 27 against the 20 gate, and deliberately so:
    // this test's whole value is that it walks *every* mutator in one
    // place, so the branch count is the coverage. Splitting it per mutator
    // would let a newly added mutator be forgotten, which is the failure
    // it exists to catch (audit finding T-06).
    #[allow(clippy::cognitive_complexity)]
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

        // Story 2.8 Task 4.1: extend the guard for the new mutator.
        assert_eq!(
            core.unregister_output(bogus_output),
            Err(WmCoreError::UnknownOutput)
        );
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
        let view_id = core.register_view(&pinned_term_app_id(TagId(0)));
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
        let pinned = core.register_view(&pinned_term_app_id(TagId(0)));
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
        assert_eq!(core.focused_view, Some(a));
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
        assert_eq!(core.focused_view, Some(id));
    }

    /// Regression guard for the Story 1.4 code-review follow-up
    /// (`main.rs`'s click-to-focus path must call `raise_view` to keep
    /// `wm_core`'s `stacking_order` synchronized with real z-order — see
    /// `manage_seats`). Exercises the same `raise_view`-then-`cycle_focus`
    /// interaction in isolation, without any Wayland glue: a prior
    /// `raise_view` reorder (standing in for click-to-focus) must be what
    /// `cycle_focus` reads next, not a stale front.
    #[test]
    fn snapshot_of_empty_core_has_empty_collections_and_no_focus() {
        let core = WmCore::new();
        assert_eq!(
            core.snapshot(),
            super::WmCoreSnapshot {
                tags: vec![],
                views: vec![],
                outputs: vec![],
                focused_view: None,
            }
        );
    }

    #[test]
    fn snapshot_includes_all_registered_tags_in_creation_order() {
        let mut core = WmCore::new();
        let tag_a = core.create_tag("a").unwrap();
        let tag_b = core.create_tag("b").unwrap();
        assert_eq!(
            core.snapshot().tags,
            vec![
                super::TagSnapshot {
                    id: tag_a,
                    name: "a".into()
                },
                super::TagSnapshot {
                    id: tag_b,
                    name: "b".into()
                },
            ]
        );
    }

    #[test]
    fn snapshot_view_reports_its_own_tag_membership_as_sorted_tag_ids() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let tag_a = core.create_tag("a").unwrap();
        let tag_b = core.create_tag("b").unwrap();
        core.toggle_view_tag(view_id, tag_a).unwrap();
        core.toggle_view_tag(view_id, tag_b).unwrap();
        let snapshot = core.snapshot();
        let view = snapshot
            .views
            .iter()
            .find(|v| v.id == view_id)
            .expect("registered view must be present in snapshot");
        assert_eq!(view.tags, vec![tag_a, tag_b]);
    }

    #[test]
    fn snapshot_view_with_no_tags_has_empty_tags_vec() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let snapshot = core.snapshot();
        let view = snapshot
            .views
            .iter()
            .find(|v| v.id == view_id)
            .expect("registered view must be present in snapshot");
        assert_eq!(view.tags, Vec::new());
    }

    #[test]
    fn snapshot_outputs_report_current_tag_including_none() {
        let mut core = WmCore::new();
        let output_with_tag = core.register_output();
        let output_without_tag = core.register_output();
        let tag_id = core.create_tag("web").unwrap();
        core.switch_tag(output_with_tag, tag_id).unwrap();
        let snapshot = core.snapshot();
        let with_tag = snapshot
            .outputs
            .iter()
            .find(|o| o.id == output_with_tag)
            .expect("output_with_tag must be present in snapshot");
        let without_tag = snapshot
            .outputs
            .iter()
            .find(|o| o.id == output_without_tag)
            .expect("output_without_tag must be present in snapshot");
        assert_eq!(with_tag.current_tag, Some(tag_id));
        assert_eq!(without_tag.current_tag, None);
    }

    #[test]
    fn snapshot_reports_focused_view_when_one_is_focused() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        core.set_focus(view_id).unwrap();
        assert_eq!(core.snapshot().focused_view, Some(view_id));
    }

    #[test]
    fn snapshot_reports_none_when_nothing_focused() {
        let mut core = WmCore::new();
        core.register_view("app-one");
        assert_eq!(core.snapshot().focused_view, None);
    }

    #[test]
    fn snapshot_views_and_outputs_are_sorted_ascending_by_id() {
        let mut core = WmCore::new();
        // Register out of any special order so a naive HashMap-iteration
        // pass would not already happen to come back sorted.
        let view_c = core.register_view("c");
        let view_a = core.register_view("a");
        let view_b = core.register_view("b");
        let output_c = core.register_output();
        let output_a = core.register_output();
        let output_b = core.register_output();
        let snapshot = core.snapshot();
        assert_eq!(snapshot.views.iter().map(|v| v.id).collect::<Vec<_>>(), {
            let mut ids = vec![view_c, view_a, view_b];
            ids.sort();
            ids
        });
        assert_eq!(snapshot.outputs.iter().map(|o| o.id).collect::<Vec<_>>(), {
            let mut ids = vec![output_c, output_a, output_b];
            ids.sort();
            ids
        });
    }

    #[test]
    fn snapshot_is_a_pure_query_and_never_mutates_state() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let tag_id = core.create_tag("web").unwrap();
        core.toggle_view_tag(view_id, tag_id).unwrap();
        let output_id = core.register_output();
        core.switch_tag(output_id, tag_id).unwrap();
        core.set_focus(view_id).unwrap();
        let snapshot_before = core.clone();
        let _ = core.snapshot();
        assert_eq!(core, snapshot_before);
    }

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
        assert_eq!(core.focused_view, Some(a));
    }

    // Story 2.7 Task 1 RED: `is_view_visible` is the pure visibility
    // DECISION this story adds to `wm-core` (no Wayland types involved) —
    // `main.rs` reuses it to decide whether to call `river_window_v1`'s
    // `show()`/`hide()`. These tests are written before the method exists
    // (RED) and must fail to compile until Task 1.2's GREEN step adds it.

    #[test]
    fn is_view_visible_true_when_a_tag_matches_some_output_current_tag() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let tag_id = core.create_tag("web").unwrap();
        core.toggle_view_tag(view_id, tag_id).unwrap();
        let output_id = core.register_output();
        core.switch_tag(output_id, tag_id).unwrap();

        assert_eq!(core.is_view_visible(view_id), Ok(true));
    }

    #[test]
    fn is_view_visible_false_when_tags_exist_but_none_match_any_output() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let view_tag = core.create_tag("web").unwrap();
        core.toggle_view_tag(view_id, view_tag).unwrap();
        let output_id = core.register_output();
        let displayed_tag = core.create_tag("term").unwrap();
        core.switch_tag(output_id, displayed_tag).unwrap();

        assert_eq!(core.is_view_visible(view_id), Ok(false));
    }

    #[test]
    fn is_view_visible_false_when_view_has_tags_but_no_output_is_registered() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let tag_id = core.create_tag("web").unwrap();
        core.toggle_view_tag(view_id, tag_id).unwrap();

        assert_eq!(core.is_view_visible(view_id), Ok(false));
    }

    #[test]
    fn is_view_visible_true_when_view_has_no_tags_at_all_bootstrap_exception() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");

        assert_eq!(core.is_view_visible(view_id), Ok(true));
    }

    #[test]
    fn is_view_visible_true_when_untagged_even_with_tags_and_outputs_registered() {
        // The bootstrap exception is about *this view's own* tag
        // membership being empty, not about the overall registry being
        // empty - a freshly-created window on a desktop that already has
        // tags/outputs must still be visible until it's explicitly tagged
        // (Task 4 auto-tags it immediately in practice, but the pure
        // decision here must not assume that already happened).
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_id = core.create_tag("web").unwrap();
        core.switch_tag(output_id, tag_id).unwrap();
        let view_id = core.register_view("app-one");

        assert_eq!(core.is_view_visible(view_id), Ok(true));
    }

    #[test]
    fn is_view_visible_true_when_view_has_multiple_tags_and_any_one_matches() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let tag_a = core.create_tag("web").unwrap();
        let tag_b = core.create_tag("term").unwrap();
        core.toggle_view_tag(view_id, tag_a).unwrap();
        core.toggle_view_tag(view_id, tag_b).unwrap();
        let output_id = core.register_output();
        core.switch_tag(output_id, tag_b).unwrap();

        assert_eq!(core.is_view_visible(view_id), Ok(true));
    }

    #[test]
    fn is_view_visible_true_when_shown_simultaneously_on_two_outputs() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let tag_id = core.create_tag("web").unwrap();
        core.toggle_view_tag(view_id, tag_id).unwrap();
        let o1 = core.register_output();
        let o2 = core.register_output();
        core.switch_tag(o1, tag_id).unwrap();
        // Distinct tag on o2 so o1 keeps tag_id (switch_tag would otherwise
        // reroute it away, ADR-005) - o1 is the one that must still match.
        let other_tag = core.create_tag("term").unwrap();
        core.switch_tag(o2, other_tag).unwrap();

        assert_eq!(core.is_view_visible(view_id), Ok(true));
    }

    #[test]
    fn is_view_visible_unknown_view_returns_unknown_view_error() {
        let core = WmCore::new();
        let bogus_view = ViewId(999);
        assert_eq!(
            core.is_view_visible(bogus_view),
            Err(WmCoreError::UnknownView)
        );
    }

    #[test]
    fn is_view_visible_is_a_pure_query_and_never_mutates_state() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let tag_id = core.create_tag("web").unwrap();
        core.toggle_view_tag(view_id, tag_id).unwrap();
        let output_id = core.register_output();
        core.switch_tag(output_id, tag_id).unwrap();
        let snapshot = core.clone();

        let _ = core.is_view_visible(view_id);
        assert_eq!(core, snapshot);
    }

    // Story 2.7 Task 2/5 RED: `output_showing_tag` is the pure query
    // `main.rs`'s pinned-terminal fullscreen wiring (Task 5) uses to find
    // which real `river_output_v1` proxy a tag is currently displayed on -
    // `main.rs` maps the returned `OutputId` to its own `Output` struct's
    // proxy; this method itself stays protocol-agnostic.

    #[test]
    fn output_showing_tag_returns_none_when_no_output_shows_it() {
        let mut core = WmCore::new();
        core.register_output();
        let tag_id = core.create_tag("web").unwrap();
        assert_eq!(core.output_showing_tag(tag_id), None);
    }

    #[test]
    fn output_showing_tag_returns_the_output_currently_displaying_it() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_id = core.create_tag("web").unwrap();
        core.switch_tag(output_id, tag_id).unwrap();
        assert_eq!(core.output_showing_tag(tag_id), Some(output_id));
    }

    #[test]
    fn output_showing_tag_returns_none_for_a_registered_tag_no_output_shows() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let shown_tag = core.create_tag("web").unwrap();
        let unshown_tag = core.create_tag("term").unwrap();
        core.switch_tag(output_id, shown_tag).unwrap();
        assert_eq!(core.output_showing_tag(unshown_tag), None);
    }

    #[test]
    fn output_showing_tag_reflects_reroute_after_switch_tag_moves_it() {
        let mut core = WmCore::new();
        let o1 = core.register_output();
        let o2 = core.register_output();
        let tag_id = core.create_tag("web").unwrap();
        core.switch_tag(o1, tag_id).unwrap();
        core.switch_tag(o2, tag_id).unwrap();
        assert_eq!(
            core.output_showing_tag(tag_id),
            Some(o2),
            "after a reroute, the tag's old output must no longer be reported"
        );
    }

    // Story 2.7 Task 5 RED: `view_tags` is the pure query
    // `main.rs`'s pinned-terminal geometry-recompute pass uses to recover
    // which tag a pinned terminal was associated with (the window's own
    // `app_id` carries that association only at mapping time - after that,
    // `wm-core`'s own tag membership is the source of truth).

    #[test]
    fn view_tags_returns_empty_vec_for_untagged_view() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        assert_eq!(core.view_tags(view_id), Ok(Vec::new()));
    }

    #[test]
    fn view_tags_returns_assigned_tag() {
        let mut core = WmCore::new();
        let view_id = core.register_view("app-one");
        let tag_id = core.create_tag("web").unwrap();
        core.toggle_view_tag(view_id, tag_id).unwrap();
        assert_eq!(core.view_tags(view_id), Ok(vec![tag_id]));
    }

    #[test]
    fn view_tags_unknown_view_returns_error() {
        let core = WmCore::new();
        let bogus_view = ViewId(999);
        assert_eq!(core.view_tags(bogus_view), Err(WmCoreError::UnknownView));
    }

    // Story 2.7 Task 4 RED: `output_current_tag` is the pure lookup
    // `main.rs`'s auto-tag-on-create wiring uses to resolve the active
    // output's current tag before calling `toggle_view_tag` on a freshly
    // registered non-pinned window.

    #[test]
    fn output_current_tag_returns_none_when_output_has_no_tag() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        assert_eq!(core.output_current_tag(output_id), None);
    }

    #[test]
    fn output_current_tag_returns_the_displayed_tag() {
        let mut core = WmCore::new();
        let output_id = core.register_output();
        let tag_id = core.create_tag("web").unwrap();
        core.switch_tag(output_id, tag_id).unwrap();
        assert_eq!(core.output_current_tag(output_id), Some(tag_id));
    }

    #[test]
    fn output_current_tag_returns_none_for_unregistered_output() {
        let core = WmCore::new();
        let bogus_output = crate::wm_core::ids::OutputId(999);
        assert_eq!(core.output_current_tag(bogus_output), None);
    }

    /// Audit finding D-02/D-01(b): a pinned terminal's window carries its
    /// tag in its own `app_id`, so correlation no longer depends on the
    /// order two threads' spawns happen to map in.
    #[test]
    fn pinned_term_app_id_round_trips_through_tag_id_from_pinned_app_id() {
        for raw in [0u8, 1, 7, 63] {
            let tag_id = TagId(raw);
            let app_id = pinned_term_app_id(tag_id);
            assert_eq!(tag_id_from_pinned_app_id(&app_id), Some(tag_id));
            assert!(is_pinned_term_app_id(&app_id));
        }
    }

    #[test]
    fn pinned_term_app_ids_are_distinct_per_tag() {
        assert_ne!(pinned_term_app_id(TagId(0)), pinned_term_app_id(TagId(1)));
    }

    /// Only the exact form this WM emits is recognised: everything else is
    /// an ordinary window, including the bare prefix, a non-canonical
    /// spelling of a number, and an id no tag can ever have.
    #[test]
    fn tag_id_from_pinned_app_id_rejects_everything_but_the_canonical_form() {
        for app_id in [
            "foot",
            "pinned-term",
            "pinned-term-",
            "pinned-term-x",
            "pinned-term-03",
            "pinned-term-+3",
            "pinned-term-1-2",
            "pinned-term-64",
            "pinned-term-300",
            "Xpinned-term-1",
        ] {
            assert_eq!(
                tag_id_from_pinned_app_id(app_id),
                None,
                "{app_id:?} was accepted as a pinned terminal"
            );
            assert!(!is_pinned_term_app_id(app_id));
        }
    }

    /// Audit finding D-01: the claim is committed before any process
    /// exists, so a spawn that fails has to be undone or the tag never
    /// retries for the rest of the session.
    #[test]
    fn release_pinned_terminal_claim_lets_a_failed_spawn_be_retried() {
        let mut core = WmCore::new();
        let tag_id = core.create_tag("web").unwrap();
        core.claim_pinned_terminal_spawn(tag_id).unwrap();
        core.release_pinned_terminal_claim(tag_id).unwrap();
        assert_eq!(
            core.claim_pinned_terminal_spawn(tag_id),
            Ok(Some("tag-web".to_string()))
        );
    }

    #[test]
    fn release_pinned_terminal_claim_unknown_tag_returns_error() {
        let mut core = WmCore::new();
        let bogus_tag = TagId(7);
        assert_eq!(
            core.release_pinned_terminal_claim(bogus_tag),
            Err(WmCoreError::UnknownTag)
        );
    }

    /// Audit finding D-03: tag names are entirely unvalidated, so the
    /// `tag-` prefix `claim_pinned_terminal_spawn` builds the session name
    /// with is the only thing preventing a flag-shaped argv token reaching
    /// `zellij`. It is an incidental property of a `format!` in a
    /// bookkeeping function, so it needs a test of its own before someone
    /// reworks how session names are derived.
    #[test]
    fn pinned_terminal_session_name_can_never_begin_with_a_dash() {
        let mut core = WmCore::default();
        let tag_id = core.create_tag("--layout").unwrap();
        let session_name = core
            .claim_pinned_terminal_spawn(tag_id)
            .unwrap()
            .expect("a freshly created tag has not claimed its spawn yet");
        assert!(
            !session_name.starts_with('-'),
            "session name {session_name:?} is flag-shaped and would be \
             parsed as an option by zellij"
        );
    }
}
