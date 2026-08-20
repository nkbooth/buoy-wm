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
