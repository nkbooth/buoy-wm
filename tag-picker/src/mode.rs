// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! Pure CLI-argument-to-mode decision (Story 2.4 Task 4). `tag-picker` is
//! invoked with zero args (`Mod4+A`'s existing "assign mode") or as
//! `tag-picker switch <output_id>` (`Mod4+S`'s new "switch mode") — this
//! module decides which, and rejects any other argument shape before
//! `main` ever connects to the socket, the same "fail closed on malformed
//! input" discipline `wm`'s own `parse_request` applies (NFR2-style,
//! extended to this client per Story 2.2/2.3 precedent).

/// The picker's operating mode, decided purely from argv (Task 1.1;
/// extended with an optional output connector name in Story 2.9 Task 4).
/// `Assign` is the Story 2.2/2.3 toggle-and-reopen mode; its zero-args
/// invocation is byte-for-byte unchanged, now with `output_name: None`.
/// `Switch` carries the `output_id` `wm` resolved once at spawn time via
/// its own `WindowManager::active_output_id` (Task 1.2) — `tag-picker`
/// treats it as an opaque argument, never re-deriving "the active output"
/// itself. Both variants' `output_name` is the real Wayland connector name
/// (e.g. `"eDP-1"`) `wm`'s `WindowManager::output_name` resolved for that
/// same active output (Story 2.9), threaded through to `run_fuzzel`'s
/// `--output=` flag (Task 5) — `None` whenever `wm` didn't yet know it
/// (Story 2.9 AC 2), in which case no `--output` flag is passed at all.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Assign {
        output_name: Option<String>,
    },
    Switch {
        output_id: u64,
        output_name: Option<String>,
    },
}

/// Decides [`Mode`] from the process's own argv (excluding `argv[0]`, the
/// executable path). Zero arguments is `Assign { output_name: None }` —
/// the existing, backward-compatible default. One argument is *always*
/// `Assign` with that argument as `output_name`, regardless of its value —
/// including the literal string `"switch"`. Two arguments, the literal
/// `"switch"` followed by a valid `u64`, is `Switch` with `output_name:
/// None`. Three arguments, `"switch"`, a valid `u64`, then a name, is
/// `Switch` with that name as `output_name`. Any other shape (wrong count,
/// wrong first argument, non-numeric second argument for switch mode) is a
/// startup error, returned as `Err` rather than panicking or silently
/// falling back to a mode — malformed invocation is a pure argument-shape
/// problem the caller should surface immediately (Task 1.1, extended
/// Story 2.9 Task 4).
///
/// Code review follow-up (Story 2.9): a one-argument invocation used to be
/// rejected when that argument was exactly `"switch"` (to give a clearer
/// error for a mistyped `tag-picker switch` missing its output id) — but
/// `wm` now always spawns assign mode with the active output's *real*
/// connector name as this single argument, and nothing stops a real
/// Wayland output from being named `"switch"`. That guard turned an
/// unlikely but real connector name into total assign-mode failure
/// (`std::process::exit(1)`, no picker at all) — a correctness bug, not
/// just a missed nicety. A one-argument invocation is unconditionally
/// `Assign` now; a bare `tag-picker switch` typed by hand is simply
/// assign mode targeting an output literally named "switch" (which,
/// finding none, `fuzzel` falls back to its own default placement for,
/// same as any other unknown/not-yet-connected output name).
pub fn parse_args(args: &[String]) -> Result<Mode, String> {
    match args {
        [] => Ok(Mode::Assign { output_name: None }),
        [name] => Ok(Mode::Assign {
            output_name: Some(name.clone()),
        }),
        [mode, id] if mode == "switch" => id
            .parse::<u64>()
            .map(|output_id| Mode::Switch {
                output_id,
                output_name: None,
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
            "usage: tag-picker [<output_name>] | [switch <output_id> [<output_name>]], got: {args:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_with_no_arguments_is_assign_mode_with_no_output_name() {
        assert_eq!(parse_args(&[]), Ok(Mode::Assign { output_name: None }));
    }

    #[test]
    fn parse_args_with_one_argument_is_assign_mode_with_output_name() {
        assert_eq!(
            parse_args(&["eDP-1".into()]),
            Ok(Mode::Assign {
                output_name: Some("eDP-1".into())
            })
        );
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

    /// Code review follow-up (Story 2.9): a bare `["switch"]` used to be a
    /// rejected "missing output id" error, but that guard is exactly what
    /// made a real output literally named "switch" fail to open assign
    /// mode at all. A single argument is unconditionally an assign-mode
    /// output name now, including this one.
    #[test]
    fn parse_args_with_single_argument_literally_switch_is_assign_mode_with_that_name() {
        assert_eq!(
            parse_args(&["switch".into()]),
            Ok(Mode::Assign {
                output_name: Some("switch".into())
            })
        );
    }

    #[test]
    fn parse_args_rejects_unknown_first_argument() {
        // "frobnicate" is a single argument, which the new grammar accepts
        // as an assign-mode output name (Task 4) - use two unrecognized
        // arguments instead, still rejected as too many for assign mode and
        // not "switch" for switch mode.
        assert!(parse_args(&["frobnicate".into(), "extra".into()]).is_err());
    }

    #[test]
    fn parse_args_rejects_too_many_arguments() {
        assert!(
            parse_args(&["switch".into(), "3".into(), "extra".into(), "extra2".into()]).is_err()
        );
    }
}
