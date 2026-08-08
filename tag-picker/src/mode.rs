// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! Pure CLI-argument-to-mode decision (Story 2.4 Task 4). `tag-picker` is
//! invoked with zero args (`Mod4+A`'s existing "assign mode") or as
//! `tag-picker switch <output_id>` (`Mod4+S`'s new "switch mode") — this
//! module decides which, and rejects any other argument shape before
//! `main` ever connects to the socket, the same "fail closed on malformed
//! input" discipline `wm`'s own `parse_request` applies (NFR2-style,
//! extended to this client per Story 2.2/2.3 precedent).

/// The picker's operating mode, decided purely from argv (Task 1.1).
/// `Assign` is the zero-args, Story 2.2/2.3 toggle-and-reopen mode,
/// byte-for-byte unchanged by this story. `Switch` carries the
/// `output_id` `wm` resolved once at spawn time via its own
/// `WindowManager::active_output_id` (Task 1.2) — `tag-picker` treats it
/// as an opaque argument, never re-deriving "the active output" itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Assign,
    Switch { output_id: u64 },
}

/// Decides [`Mode`] from the process's own argv (excluding `argv[0]`, the
/// executable path). Zero arguments is `Assign` — the existing,
/// backward-compatible default. Exactly two arguments, the literal
/// `"switch"` followed by a valid `u64`, is `Switch`. Any other shape
/// (wrong count, wrong first argument, non-numeric second argument) is a
/// startup error, returned as `Err` rather than panicking or silently
/// falling back to a mode — malformed invocation is a pure argument-shape
/// problem the caller should surface immediately (Task 1.1).
pub fn parse_args(args: &[String]) -> Result<Mode, String> {
    match args {
        [] => Ok(Mode::Assign),
        [mode, id] if mode == "switch" => id
            .parse::<u64>()
            .map(|output_id| Mode::Switch { output_id })
            .map_err(|_| format!("invalid output id: {id}")),
        _ => Err(format!(
            "usage: tag-picker [switch <output_id>], got: {args:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_with_no_arguments_is_assign_mode() {
        assert_eq!(parse_args(&[]), Ok(Mode::Assign));
    }

    #[test]
    fn parse_args_with_switch_and_valid_output_id_is_switch_mode() {
        assert_eq!(
            parse_args(&["switch".into(), "3".into()]),
            Ok(Mode::Switch { output_id: 3 })
        );
    }

    #[test]
    fn parse_args_rejects_switch_with_non_numeric_output_id() {
        assert!(parse_args(&["switch".into(), "not-a-number".into()]).is_err());
    }

    #[test]
    fn parse_args_rejects_switch_with_missing_output_id() {
        assert!(parse_args(&["switch".into()]).is_err());
    }

    #[test]
    fn parse_args_rejects_unknown_first_argument() {
        assert!(parse_args(&["frobnicate".into()]).is_err());
    }

    #[test]
    fn parse_args_rejects_too_many_arguments() {
        assert!(parse_args(&["switch".into(), "3".into(), "extra".into()]).is_err());
    }
}
