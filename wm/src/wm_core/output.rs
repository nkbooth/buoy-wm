// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! `Output`: an in-memory record of a display's currently-visible tag.

use super::ids::{OutputId, TagId};

/// An in-memory record for a registered output (display). `current_tag`
/// is the raw field-level primitive only; the one-tag-per-output
/// enforcement/reject-or-reroute decision belongs to a later story's
/// `switch_tag` operation layered on top of this setter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Output {
    pub id: OutputId,
    pub current_tag: Option<TagId>,
}
