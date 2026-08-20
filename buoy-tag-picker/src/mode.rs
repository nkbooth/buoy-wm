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

//! Pure CLI-argument-to-mode decision (Story 2.4 Task 4). `buoy-tag-picker` is
//! invoked with zero args (`Mod4+A`'s existing "assign mode") or as
//! `buoy-tag-picker switch <output_id>` (`Mod4+S`'s new "switch mode") — this
//! module decides which, and rejects any other argument shape before
//! `main` ever connects to the socket, the same "fail closed on malformed
//! input" discipline `wm`'s own `parse_request` applies (NFR2-style,
//! extended to this client per Story 2.2/2.3 precedent).

/// The picker's operating mode, decided purely from argv (Task 1.1;
/// extended with an optional output connector name in Story 2.9 Task 4;
/// assign mode's positional grammar extended again in Story 2.10 Task 4).
/// `Assign` is the Story 2.2/2.3 toggle-and-reopen mode, extended by Story
/// 2.10 to also cover the no-focused-window switch-instead-of-toggle
/// fallback (Task 5) — `output_id` is `wm`'s own `WindowManager::
/// active_output_id` resolution (`None` only in the startup-race edge case
/// where no output is registered yet), needed so that fallback can send a
/// `SwitchTag` request. Its zero-args invocation is still byte-for-byte
/// unchanged, now with `output_id: None, output_name: None`. `Switch`
/// carries the `output_id` `wm` resolved once at spawn time via its own
/// `WindowManager::active_output_id` (Task 1.2) — `buoy-tag-picker` treats it as
/// an opaque argument, never re-deriving "the active output" itself. Both
/// variants' `output_name` is the real Wayland connector name (e.g.
/// `"eDP-1"`) `wm`'s `WindowManager::output_name` resolved for that same
/// active output (Story 2.9), threaded through to `run_fuzzel`'s
/// `--output=` flag (Task 5) — `None` whenever `wm` didn't yet know it
/// (Story 2.9 AC 2), in which case no `--output` flag is passed at all.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Assign {
        output_id: Option<u64>,
        output_name: Option<String>,
    },
    Switch {
        output_id: u64,
        output_name: Option<String>,
    },
}

/// Decides [`Mode`] from the process's own argv (excluding `argv[0]`, the
/// executable path).
///
/// Zero arguments is `Assign { output_id: None, output_name: None }` — the
/// existing, backward-compatible default.
///
/// One argument is `Assign` with that argument parsed as a `u64`
/// `output_id` (`output_name: None`) — a controlled, self-consistent break
/// of Story 2.9's grammar (Story 2.10 Task 4): assign mode's single
/// positional argument used to mean an output *name* (a `String`, for
/// `fuzzel --output=`); it now means an output *id* (a `u64`, for the
/// `SwitchTag` IPC request Task 5's no-focused-window fallback needs to
/// send), with the name becoming a second, optional argument below. This is
/// safe because `buoy-tag-picker` has exactly one real caller — `wm`'s own spawn
/// command — updated in the same story (see this story's Technical notes);
/// contrast with `Request`/`Response` shapes in `wire.rs`, a stable-ish
/// contract between two independently-evolving binaries that only ever
/// gets additive changes. A non-numeric single argument is now a startup
/// error, not an output name.
///
/// Two arguments, a valid `u64` followed by any string and *not* literally
/// `"switch"` as the first argument, is `Assign` with both `output_id` and
/// `output_name` set.
///
/// Two arguments, the literal `"switch"` followed by a valid `u64`, is
/// `Switch` with `output_name: None` — unchanged from Story 2.9, and tried
/// before the generic two-argument `Assign` shape above so a real `switch
/// <id>` invocation is never misparsed as an assign-mode output id of
/// `"switch"` (which would fail to parse as a `u64` anyway, but the guard
/// keeps the intent explicit).
///
/// Three arguments, `"switch"`, a valid `u64`, then a name, is `Switch`
/// with that name as `output_name` — also unchanged from Story 2.9. Any
/// other shape (wrong count, wrong first argument, non-numeric id) is a
/// startup error, returned as `Err` rather than panicking or silently
/// falling back to a mode — malformed invocation is a pure argument-shape
/// problem the caller should surface immediately (Task 1.1, extended Story
/// 2.9 Task 4, extended again Story 2.10 Task 4).
///
/// Code review follow-up (Story 2.9): a one-argument invocation used to be
/// rejected when that argument was exactly `"switch"` (to give a clearer
/// error for a mistyped `buoy-tag-picker switch` missing its output id) — but
/// `wm` used to always spawn assign mode with the active output's *real*
/// connector name as this single argument, and nothing stopped a real
/// Wayland output from being named `"switch"`. Story 2.10's grammar change
/// makes this moot either way: a single argument is now always parsed as a
/// `u64` output id, so a bare `buoy-tag-picker switch` typed by hand simply
/// fails to parse `"switch"` as a number and is rejected — no longer a
/// special case, just the general non-numeric-id rejection path.
pub fn parse_args(args: &[String]) -> Result<Mode, String> {
    match args {
        [] => Ok(Mode::Assign {
            output_id: None,
            output_name: None,
        }),
        [id] => id
            .parse::<u64>()
            .map(|output_id| Mode::Assign {
                output_id: Some(output_id),
                output_name: None,
            })
            .map_err(|_| format!("invalid output id: {id}")),
        [mode, id] if mode == "switch" => id
            .parse::<u64>()
            .map(|output_id| Mode::Switch {
                output_id,
                output_name: None,
            })
            .map_err(|_| format!("invalid output id: {id}")),
        [id, name] => id
            .parse::<u64>()
            .map(|output_id| Mode::Assign {
                output_id: Some(output_id),
                output_name: Some(name.clone()),
            })
            .map_err(|_| format!("invalid output id: {id}")),
        [mode, id, name] if mode == "switch" => id
            .parse::<u64>()
            .map(|output_id| Mode::Switch {
                output_id,
                output_name: Some(name.clone()),
            })
            .map_err(|_| format!("invalid output id: {id}")),
        _ => Err(format!(
            "usage: buoy-tag-picker [<output_id> [<output_name>]] | [switch <output_id> [<output_name>]], got: {args:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_with_no_arguments_is_assign_mode_with_no_output_id_or_name() {
        assert_eq!(
            parse_args(&[]),
            Ok(Mode::Assign {
                output_id: None,
                output_name: None
            })
        );
    }

    #[test]
    fn parse_args_with_one_argument_is_assign_mode_with_output_id_only() {
        assert_eq!(
            parse_args(&["3".into()]),
            Ok(Mode::Assign {
                output_id: Some(3),
                output_name: None
            })
        );
    }

    #[test]
    fn parse_args_with_two_arguments_is_assign_mode_with_output_id_and_name() {
        assert_eq!(
            parse_args(&["3".into(), "eDP-1".into()]),
            Ok(Mode::Assign {
                output_id: Some(3),
                output_name: Some("eDP-1".into())
            })
        );
    }

    /// Story 2.10 Task 4: a single non-numeric argument used to be a valid
    /// assign-mode output *name* (Story 2.9's grammar). It is now always
    /// parsed as an output *id*, so this — including the literal `"switch"`
    /// that Story 2.9's own code-review follow-up specifically carved out —
    /// is rejected as an invalid output id, not accepted as a name.
    #[test]
    fn parse_args_rejects_single_non_numeric_argument_as_invalid_output_id() {
        assert!(parse_args(&["switch".into()]).is_err());
        assert!(parse_args(&["eDP-1".into()]).is_err());
    }

    #[test]
    fn parse_args_with_switch_and_valid_output_id_is_switch_mode_with_no_output_name() {
        assert_eq!(
            parse_args(&["switch".into(), "3".into()]),
            Ok(Mode::Switch {
                output_id: 3,
                output_name: None
            })
        );
    }

    #[test]
    fn parse_args_with_switch_valid_output_id_and_name_is_switch_mode_with_output_name() {
        assert_eq!(
            parse_args(&["switch".into(), "3".into(), "DP-2".into()]),
            Ok(Mode::Switch {
                output_id: 3,
                output_name: Some("DP-2".into())
            })
        );
    }

    #[test]
    fn parse_args_rejects_switch_with_non_numeric_output_id() {
        assert!(parse_args(&["switch".into(), "not-a-number".into()]).is_err());
    }

    /// Two arguments where the first isn't literally `"switch"` and isn't a
    /// valid `u64` output id either (Story 2.10's new assign-mode
    /// two-argument shape) is rejected as an invalid output id — not a
    /// mystery third mode.
    #[test]
    fn parse_args_rejects_two_argument_assign_with_non_numeric_output_id() {
        assert!(parse_args(&["frobnicate".into(), "extra".into()]).is_err());
    }

    /// Story 2.10 Task 4: assign mode's own accepted argument-count range
    /// widened by one (0-1 args to 0-2 args), but three arguments is still
    /// only valid when the first is literally `"switch"` (Switch mode's own
    /// unchanged three-argument shape) — three assign-shaped arguments is
    /// rejected, not silently truncated or accepted.
    #[test]
    fn parse_args_rejects_three_arguments_when_first_is_not_switch() {
        assert!(parse_args(&["1".into(), "2".into(), "3".into()]).is_err());
    }

    #[test]
    fn parse_args_rejects_too_many_arguments() {
        assert!(
            parse_args(&["switch".into(), "3".into(), "extra".into(), "extra2".into()]).is_err()
        );
    }
}
