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

//! The two picker flows: what gets sent, in what order, in response to what
//! the user picked.
//!
//! Both loops lived in `main.rs` holding a concrete `UnixStream` and
//! spawning `fuzzel` directly, and terminated the process from inside on
//! every failure path — so exercising any of them killed the test harness
//! and the whole file sat untested (audit finding T-01). Here they are
//! generic over any [`Write`]/[`BufRead`] pair and any
//! [`Launcher`], and they *return* their
//! failures. `std::process::exit` lives in `main` alone.
//!
//! `Err` means "tell the user and exit non-zero"; `Ok` means "this flow is
//! finished", which includes the flows that stop because the WM refused
//! something. That split is not new — it is exactly which paths used to call
//! `die_visibly` and which used to `break` the loop.

use std::io::{BufRead, Write};

use buoy_common::framing::{Line, MAX_LINE_BYTES, read_line_bounded};
use buoy_common::log_err;

use crate::launcher::{Launcher, LauncherRequest};
use crate::picker;
use crate::wire;

/// Assign mode's `fuzzel` placeholder text. Deliberately says nothing about
/// creating: as of Story 2.13 this mode only toggles membership in tags
/// that already exist.
const ASSIGN_PLACEHOLDER: &str = "type to filter";

/// Switch mode's `fuzzel` placeholder text. The contrast with
/// [`ASSIGN_PLACEHOLDER`] is the point — this is the one mode that turns a
/// typed, non-matching name into a new tag (Story 2.13), and the
/// placeholder is where that is discoverable.
const SWITCH_PLACEHOLDER: &str = "type to filter, or a new name to create";

/// One `get-state` snapshot, as this crate models it.
#[derive(Debug, Clone, PartialEq)]
pub struct StateSnapshot {
    pub tags: Vec<wire::TagDto>,
    pub views: Vec<wire::ViewDto>,
    pub focused_view: Option<u64>,
}

/// An open, already-authenticated conversation with `buoy-wm`: one request
/// out, one response back, repeatedly, over a connection somebody else
/// established.
///
/// Generic over the halves rather than holding a `UnixStream`, which is the
/// whole reason the flows below can be tested at all.
#[derive(Debug)]
pub struct Link<W: Write, R: BufRead> {
    writer: W,
    reader: R,
}

/// What a `toggle-tag` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Applied {
    /// The WM applied it.
    Yes,
    /// The WM answered, and the answer was not success. Already logged;
    /// the caller stops.
    No,
}

/// What a `create-tag` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Created {
    /// The tag exists (whether this request made it or it already did —
    /// `create-tag` is idempotent by name on the WM's side).
    Tag(u8),
    /// Refused because the 64-tag registry is full (ADR-006). The only
    /// outcome that neither performed the action nor was dismissed, and so
    /// the only one worth reopening the picker for.
    RejectedAtCap,
    /// The WM answered something else, or stopped answering. Already
    /// logged; the caller stops.
    Nothing,
}

impl<W: Write, R: BufRead> Link<W, R> {
    /// Wraps an established connection's write and read halves.
    pub fn new(writer: W, reader: R) -> Self {
        Self { writer, reader }
    }

    /// Writes `request` as one JSON line followed by `\n`, flushing
    /// afterward. Returns `false` if the write failed (peer gone).
    fn send(&mut self, request: &wire::Request) -> bool {
        let line = wire::serialize_request(request);
        self.writer.write_all(line.as_bytes()).is_ok()
            && self.writer.write_all(b"\n").is_ok()
            && self.writer.flush().is_ok()
    }

    /// Reads and parses one newline-delimited response line. `None` covers
    /// EOF (peer closed the connection), a line over the framing cap, a
    /// read that hit the socket deadline, and a response that fails to
    /// parse — all of them "nothing usable came back," and every call site
    /// treats them the same way (log and stop).
    ///
    /// The cap is not paranoia about our own WM: against a squatted socket
    /// path this is the only thing bounding what an unrelated process can
    /// make this process allocate (audit finding E-04).
    fn read(&mut self) -> Option<wire::Response> {
        match read_line_bounded(&mut self.reader) {
            Ok(Line::Complete(bytes)) => wire::parse_response(&bytes).ok(),
            Ok(Line::Oversize) => {
                log_err!("response line over the {MAX_LINE_BYTES}-byte cap");
                None
            }
            Ok(Line::Eof) | Err(_) => None,
        }
    }

    /// Sends `get-state` and returns the parsed snapshot. Every caller needs
    /// the state to do anything at all, so any failure here is fatal to the
    /// flow.
    pub fn get_state(&mut self) -> Result<StateSnapshot, String> {
        if !self.send(&wire::Request::GetState) {
            return Err("buoy-wm stopped listening before the state request was sent".to_string());
        }
        match self.read() {
            Some(wire::Response::State {
                tags,
                views,
                focused_view,
            }) => Ok(StateSnapshot {
                tags,
                views,
                focused_view,
            }),
            Some(other) => Err(format!(
                "buoy-wm answered the state request with something unexpected: {other:?}"
            )),
            None => Err("buoy-wm sent nothing usable in answer to the state request".to_string()),
        }
    }

    /// Sends `switch-tag` for `output_id`/`tag_id`.
    ///
    /// Every failure is an `Err`, unlike [`Self::toggle_tag`]: `switch-tag`
    /// is always the last thing any of its three call sites do (Code review
    /// follow-up, Story 2.10 — this exact send/read/report block was
    /// duplicated three times over), so there is no "carry on with a
    /// degraded flow" to fall back to.
    pub fn switch_tag(&mut self, output_id: u64, tag_id: u8) -> Result<(), String> {
        if !self.send(&wire::Request::SwitchTag { output_id, tag_id }) {
            return Err("buoy-wm stopped listening before the tag switch was sent".to_string());
        }
        match self.read() {
            Some(wire::Response::Ok) => Ok(()),
            Some(wire::Response::Error { message }) => {
                report_server_error(&message);
                Err("buoy-wm refused the tag switch; see the journal for its reason".to_string())
            }
            other => Err(format!(
                "buoy-wm answered the tag switch with something unexpected: {other:?}"
            )),
        }
    }

    /// Sends `toggle-tag` for `view_id`/`tag_id`.
    ///
    /// A refusal is [`Applied::No`] rather than an `Err`: assign mode is a
    /// loop the user is still inside, and the WM declining one toggle ends
    /// the loop without being a failure of the keybind.
    fn toggle_tag(&mut self, view_id: u64, tag_id: u8) -> Applied {
        if !self.send(&wire::Request::ToggleTag { view_id, tag_id }) {
            log_err!("failed to send toggle-tag request");
            return Applied::No;
        }
        match self.read() {
            Some(wire::Response::Ok) => Applied::Yes,
            Some(wire::Response::Error { message }) => {
                report_server_error(&message);
                Applied::No
            }
            other => {
                log_err!("unexpected response to toggle-tag: {other:?}");
                Applied::No
            }
        }
    }

    /// Sends `create-tag` for `name`.
    fn create_tag(&mut self, name: &str) -> Created {
        if !self.send(&wire::Request::CreateTag {
            name: name.to_string(),
        }) {
            log_err!("failed to send create-tag request");
            return Created::Nothing;
        }
        match self.read() {
            Some(wire::Response::TagCreated { tag_id }) => Created::Tag(tag_id),
            Some(wire::Response::Error { message }) => {
                // Compared by equality on purpose: this is the one
                // cross-crate string coupling the audit kept, and
                // `wm`'s side asserts the same literal (audit finding
                // E-06).
                if message == picker::REJECTION_MESSAGE {
                    Created::RejectedAtCap
                } else {
                    report_server_error(&message);
                    Created::Nothing
                }
            }
            other => {
                log_err!("unexpected response to create-tag: {other:?}");
                Created::Nothing
            }
        }
    }
}

/// Reports a `wire::Response::Error` message on stderr.
///
/// `{:?}`, never `{}`: `message` arrives from whatever is on the other end
/// of the socket, so `Display` would let a squatted socket path (or a
/// wm-side error that embeds peer text) write control characters and
/// forged lines into the terminal or journal.
fn report_server_error(message: &str) {
    log_err!("{message:?}");
}

/// Everything assign mode was opened with: one `get-state` snapshot plus
/// the WM's spawn-time answer to "where is the user looking".
///
/// A parameter object because five of the seven arguments this replaced were
/// one snapshot pulled apart, which is what the `#[allow]` that used to sit
/// on `run_assign_mode` was standing in for (finding J-10's parameter
/// ceiling).
#[derive(Debug, Clone, PartialEq)]
pub struct AssignContext {
    pub state: StateSnapshot,
    /// The view the picker was spawned for, not whatever is focused now:
    /// following focus mid-pick would retarget the toggle under the user's
    /// hand.
    pub view_id: Option<u64>,
    pub output_id: Option<u64>,
    pub output_name: Option<String>,
}

/// Assign mode's toggle-and-reopen loop (Story 2.2).
///
/// Story 2.10 Task 5: the picker opens with no view focused (`view_id:
/// None`) too, not just the original always-a-view case. With a view
/// focused, a pick toggles that tag's membership and the picker reopens.
/// With no view focused, "toggle this tag's membership" has no target, so
/// the pick falls back to switching the active output (`output_id`, `wm`'s
/// own spawn-time resolution — see `mode::Mode::Assign`) to the picked tag
/// instead, then stops — a switch is a one-shot terminal action, not a
/// toggle-and-reopen, exactly like [`run_switch_mode`].
///
/// Story 2.13: this mode does not create tags. A typed name that matches no
/// row parses as `Cancelled` (see `picker::parse_fuzzel_output`); creating
/// belongs to [`run_switch_mode`], because a new tag is a place you go, not
/// a label you attach. This mode never mutates the registry, only
/// membership.
///
/// Code review follow-up (Story 2.10): `picker::should_open_picker` is
/// checked once, up front, before any wire traffic at all. It also makes
/// every `output_id` unwrap below unnecessary rather than merely safe: once
/// past this guard, a `None` view guarantees a `Some` output.
pub fn run_assign_mode<W: Write, R: BufRead>(
    link: &mut Link<W, R>,
    launcher: &mut impl Launcher,
    context: AssignContext,
) -> Result<(), String> {
    if !picker::should_open_picker(context.view_id, context.output_id) {
        log_err!("no window focused and no output known");
        return Ok(());
    }

    // Both the registry and the focused view's membership are re-read at
    // the top of every pass rather than cached across the loop (audit
    // finding K-03). The picker's operation is a *toggle*, not an absolute
    // set, so a cache that has drifted from the server — a keybind or a
    // second client having changed membership between two picks — inverts
    // the user's next pick rather than merely losing it. The cost is one
    // round trip per reopen on a connection that is already open, against a
    // server that holds its mutex across a single request.
    let mut state = context.state;

    loop {
        let current_tags = picker::view_tag_membership(&state.views, context.view_id);
        let known_ids: Vec<u8> = state.tags.iter().map(|t| t.id).collect();
        let entries = picker::build_checklist_entries(&state.tags, &current_tags);
        let input = picker::render_fuzzel_input(&entries);
        let outcome = launcher.run(&LauncherRequest {
            input: &input,
            // No `initial_search` — that mechanism exists only to restore a
            // name rejected at the tag cap, which is a create-path concept
            // and therefore switch mode's now (Story 2.13).
            initial_search: None,
            output_name: context.output_name.as_deref(),
            placeholder: ASSIGN_PLACEHOLDER,
        })?;

        match picker::parse_fuzzel_output(outcome.accepted, &outcome.stdout, &known_ids) {
            picker::PickerAction::Cancelled => return Ok(()),
            picker::PickerAction::Toggled(tag_id) => match context.view_id {
                Some(view_id) => match link.toggle_tag(view_id, tag_id) {
                    Applied::Yes => state = link.get_state()?,
                    Applied::No => return Ok(()),
                },
                // Story 2.10 Task 5: no view is focused, so there is
                // nothing to toggle membership on — switch the active
                // output to `tag_id` instead, handled the same shape as
                // `run_switch_mode`'s own `Selected` arm, then stop.
                None => {
                    // `should_open_picker` already refused to run at all
                    // unless a view or an output was known, and this arm
                    // only runs when the view is `None` — so an absent
                    // output here is unreachable, and stating that as a
                    // total match rather than an `expect` keeps this file
                    // free of panic sites.
                    match context.output_id {
                        Some(output_id) => link.switch_tag(output_id, tag_id)?,
                        None => log_err!("no output to switch, though one was required to open"),
                    }
                    return Ok(());
                }
            },
        }
    }
}

/// Switch mode's flow (Story 2.4 Task 6.2): renders a plain, un-checkboxed
/// list of every registry tag and opens the launcher. Selecting a row sends
/// `switch-tag` and returns; dismissing returns with no request sent at all.
///
/// Story 2.13 adds the create path, moved here from assign mode: a typed
/// name matching no row is sent as `create-tag`, and the resulting tag id
/// is switched to immediately — creating a tag and going there are one
/// action, because creating one is how you start working somewhere new.
/// `create-tag` is idempotent by exact name on `wm`'s side, so typing a
/// name that already exists resolves to that tag's id and switches to it,
/// with no duplicate registry entry and no special case here.
///
/// The loop reopens for exactly one reason: a `create-tag` rejected at the
/// 64-tag registry cap (ADR-006), which is the only outcome that has
/// neither performed the action nor been dismissed, and whose reason is
/// invisible to a keybind-spawned process with no attached terminal — so
/// it is re-surfaced as a prepended row with the rejected name restored
/// into the input box. Every other outcome, create or not, still
/// terminates on the first pass: one output shows exactly one tag (FR2),
/// so a switch is terminal by definition.
pub fn run_switch_mode<W: Write, R: BufRead>(
    link: &mut Link<W, R>,
    launcher: &mut impl Launcher,
    tags: &[wire::TagDto],
    output_id: u64,
    output_name: Option<&str>,
) -> Result<(), String> {
    let known_ids: Vec<u8> = tags.iter().map(|t| t.id).collect();

    // `Some(name)` when the most recent `create-tag` attempt was rejected
    // for hitting the 64-tag cap: the next reopen prepends the rejection
    // row and restores `name` into the input box via `--search`
    // (Story 2.3 Task 4.2, moved here by Story 2.13).
    let mut pending_rejected_name: Option<String> = None;

    loop {
        let mut input = String::new();
        if pending_rejected_name.is_some() {
            input.push_str(&picker::render_rejection_row());
        }
        input.push_str(&picker::render_switch_list(tags));
        let outcome = launcher.run(&LauncherRequest {
            input: &input,
            initial_search: pending_rejected_name.as_deref(),
            output_name,
            placeholder: SWITCH_PLACEHOLDER,
        })?;

        match picker::parse_switch_selection(outcome.accepted, &outcome.stdout, &known_ids) {
            picker::SwitchAction::Cancelled => return Ok(()),
            picker::SwitchAction::Selected(tag_id) => return link.switch_tag(output_id, tag_id),
            picker::SwitchAction::CreateTag(name) => match link.create_tag(&name) {
                // A created tag that can't then be switched to is reported
                // and exits non-zero through `switch_tag`'s `Err` —
                // deliberately the same failure posture a *selected
                // existing* tag has had since Story 2.4, rather than a
                // second, different one for the same request.
                Created::Tag(tag_id) => return link.switch_tag(output_id, tag_id),
                Created::RejectedAtCap => pending_rejected_name = Some(name),
                Created::Nothing => return Ok(()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launcher::LauncherOutcome;
    use std::collections::VecDeque;
    use std::io::{BufReader, Cursor};

    /// What one launcher invocation was asked to show, kept owned so a test
    /// can assert on it after the flow has finished.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Shown {
        input: String,
        initial_search: Option<String>,
        output_name: Option<String>,
        placeholder: String,
    }

    /// A [`Launcher`] that answers from a script and records what it was
    /// shown. The whole reason the flows above are generic over the
    /// launcher: no `fuzzel`, no Wayland session, no process (audit finding
    /// T-01).
    struct ScriptedLauncher {
        answers: VecDeque<Result<LauncherOutcome, String>>,
        shown: Vec<Shown>,
    }

    impl ScriptedLauncher {
        fn new(answers: Vec<Result<LauncherOutcome, String>>) -> Self {
            Self {
                answers: answers.into(),
                shown: Vec::new(),
            }
        }
    }

    impl Launcher for ScriptedLauncher {
        fn run(&mut self, request: &LauncherRequest<'_>) -> Result<LauncherOutcome, String> {
            self.shown.push(Shown {
                input: request.input.to_string(),
                initial_search: request.initial_search.map(str::to_string),
                output_name: request.output_name.map(str::to_string),
                placeholder: request.placeholder.to_string(),
            });
            self.answers.pop_front().unwrap_or_else(|| {
                panic!("the launcher was opened more times than the test scripted")
            })
        }
    }

    /// The user accepted `row`, exactly as a launcher returns a whole
    /// accepted line.
    fn accepted(row: &str) -> Result<LauncherOutcome, String> {
        Ok(LauncherOutcome {
            accepted: true,
            stdout: format!("{row}\n"),
        })
    }

    /// The user pressed Escape.
    fn dismissed() -> Result<LauncherOutcome, String> {
        Ok(LauncherOutcome {
            accepted: false,
            stdout: String::new(),
        })
    }

    /// A [`Link`] whose reader replays `responses` in order and whose writer
    /// is `sent`, so a test can assert the exact request sequence.
    fn link_over<'a>(
        responses: &[&str],
        sent: &'a mut Vec<u8>,
    ) -> Link<&'a mut Vec<u8>, BufReader<Cursor<Vec<u8>>>> {
        let script = responses
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        Link::new(sent, BufReader::new(Cursor::new(script.into_bytes())))
    }

    /// The request lines a flow actually wrote, newline framing removed.
    fn requests(sent: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(sent)
            .lines()
            .map(str::to_string)
            .collect()
    }

    const STATE_TWO_TAGS: &str = r#"{"type":"state","tags":[{"id":0,"name":"web"},{"id":1,"name":"code"}],"views":[{"id":9,"tags":[0]}],"focused_view":9}"#;

    fn two_tags() -> Vec<wire::TagDto> {
        vec![
            wire::TagDto {
                id: 0,
                name: "web".to_string(),
            },
            wire::TagDto {
                id: 1,
                name: "code".to_string(),
            },
        ]
    }

    fn assign_context(view_id: Option<u64>, output_id: Option<u64>) -> AssignContext {
        AssignContext {
            state: StateSnapshot {
                tags: two_tags(),
                views: vec![wire::ViewDto {
                    id: 9,
                    tags: vec![0],
                }],
                focused_view: Some(9),
            },
            view_id,
            output_id,
            output_name: Some("DP-1".to_string()),
        }
    }

    #[test]
    fn assign_mode_toggles_the_picked_tag_then_reopens_with_refreshed_state() {
        let mut sent = Vec::new();
        let mut link = link_over(&[r#"{"type":"ok"}"#, STATE_TWO_TAGS], &mut sent);
        let mut launcher = ScriptedLauncher::new(vec![accepted("[ ] code\t1"), dismissed()]);

        run_assign_mode(&mut link, &mut launcher, assign_context(Some(9), Some(4)))
            .expect("a toggle the wm accepted is not a failure");

        assert_eq!(
            requests(&sent),
            vec![
                r#"{"type":"toggle-tag","view_id":9,"tag_id":1}"#.to_string(),
                r#"{"type":"get-state"}"#.to_string(),
            ],
            "a toggle must be followed by a fresh read, never by a cached mirror"
        );
        assert_eq!(launcher.shown.len(), 2, "the picker did not reopen");
        assert_eq!(launcher.shown[0].placeholder, ASSIGN_PLACEHOLDER);
        assert_eq!(launcher.shown[0].output_name.as_deref(), Some("DP-1"));
        assert_eq!(
            launcher.shown[0].initial_search, None,
            "assign mode has no create path, so nothing to restore"
        );
    }

    #[test]
    fn assign_mode_with_no_focused_view_switches_the_output_instead_of_toggling() {
        let mut sent = Vec::new();
        let mut link = link_over(&[r#"{"type":"ok"}"#], &mut sent);
        let mut launcher = ScriptedLauncher::new(vec![accepted("[ ] code\t1")]);

        run_assign_mode(&mut link, &mut launcher, assign_context(None, Some(4)))
            .expect("switching instead of toggling is a success");

        assert_eq!(
            requests(&sent),
            vec![r#"{"type":"switch-tag","output_id":4,"tag_id":1}"#.to_string()],
            "with nothing to toggle, the pick must become a switch of the active output"
        );
        assert_eq!(
            launcher.shown.len(),
            1,
            "a switch is terminal; the picker must not reopen"
        );
    }

    #[test]
    fn assign_mode_refuses_to_open_with_neither_a_view_nor_an_output() {
        let mut sent = Vec::new();
        let mut link = link_over(&[], &mut sent);
        let mut launcher = ScriptedLauncher::new(vec![]);

        run_assign_mode(&mut link, &mut launcher, assign_context(None, None))
            .expect("nothing to act on is not a failure of the keybind");

        assert!(
            sent.is_empty(),
            "the guard must run before any wire traffic"
        );
        assert!(launcher.shown.is_empty(), "no window should have opened");
    }

    #[test]
    fn assign_mode_stops_when_the_wm_refuses_a_toggle() {
        let mut sent = Vec::new();
        let mut link = link_over(&[r#"{"type":"error","message":"no such view"}"#], &mut sent);
        let mut launcher = ScriptedLauncher::new(vec![accepted("[ ] code\t1")]);

        run_assign_mode(&mut link, &mut launcher, assign_context(Some(9), Some(4)))
            .expect("a refused toggle ends the loop without failing the process");

        assert_eq!(launcher.shown.len(), 1, "the picker must not reopen");
    }

    #[test]
    fn assign_mode_reports_a_launcher_that_could_not_be_spawned() {
        let mut sent = Vec::new();
        let mut link = link_over(&[], &mut sent);
        let mut launcher =
            ScriptedLauncher::new(vec![Err("cannot run `fuzzel`: not found".to_string())]);

        let error = run_assign_mode(&mut link, &mut launcher, assign_context(Some(9), Some(4)))
            .expect_err("a missing launcher must not look like a dismissal");

        assert!(error.contains("fuzzel"), "{error}");
        assert!(sent.is_empty());
    }

    #[test]
    fn assign_mode_fails_when_the_refresh_after_a_toggle_brings_back_nothing() {
        let mut sent = Vec::new();
        // The `ok` for the toggle, then EOF where the fresh state should be.
        let mut link = link_over(&[r#"{"type":"ok"}"#], &mut sent);
        let mut launcher = ScriptedLauncher::new(vec![accepted("[ ] code\t1")]);

        let error = run_assign_mode(&mut link, &mut launcher, assign_context(Some(9), Some(4)))
            .expect_err("a toggle applied against state that can no longer be read is a failure");

        assert!(error.contains("state request"), "{error}");
    }

    #[test]
    fn switch_mode_sends_switch_tag_for_the_selected_row() {
        let mut sent = Vec::new();
        let mut link = link_over(&[r#"{"type":"ok"}"#], &mut sent);
        let mut launcher = ScriptedLauncher::new(vec![accepted("code\t1")]);

        run_switch_mode(&mut link, &mut launcher, &two_tags(), 4, Some("DP-1"))
            .expect("an accepted switch is a success");

        assert_eq!(
            requests(&sent),
            vec![r#"{"type":"switch-tag","output_id":4,"tag_id":1}"#.to_string()]
        );
        assert_eq!(launcher.shown[0].placeholder, SWITCH_PLACEHOLDER);
        assert_eq!(
            launcher.shown[0].input, "web\t0\ncode\t1\n",
            "switch mode renders plain rows, with no checkbox glyph"
        );
    }

    #[test]
    fn switch_mode_dismissed_sends_nothing_at_all() {
        let mut sent = Vec::new();
        let mut link = link_over(&[], &mut sent);
        let mut launcher = ScriptedLauncher::new(vec![dismissed()]);

        run_switch_mode(&mut link, &mut launcher, &two_tags(), 4, None)
            .expect("a dismissal is a success");

        assert!(sent.is_empty(), "Escape must not mutate anything");
    }

    #[test]
    fn switch_mode_creates_a_typed_name_and_switches_to_it_in_one_action() {
        let mut sent = Vec::new();
        let mut link = link_over(
            &[r#"{"type":"tag-created","tag_id":2}"#, r#"{"type":"ok"}"#],
            &mut sent,
        );
        let mut launcher = ScriptedLauncher::new(vec![accepted("notes")]);

        run_switch_mode(&mut link, &mut launcher, &two_tags(), 4, None)
            .expect("creating and going there is one successful action");

        assert_eq!(
            requests(&sent),
            vec![
                r#"{"type":"create-tag","name":"notes"}"#.to_string(),
                r#"{"type":"switch-tag","output_id":4,"tag_id":2}"#.to_string(),
            ],
            "a created tag must be switched to immediately, not merely created"
        );
    }

    #[test]
    fn switch_mode_reopens_with_the_rejected_name_restored_when_the_registry_is_full() {
        let mut sent = Vec::new();
        let rejection = format!(
            r#"{{"type":"error","message":"{}"}}"#,
            picker::REJECTION_MESSAGE
        );
        let mut link = link_over(&[&rejection], &mut sent);
        let mut launcher = ScriptedLauncher::new(vec![accepted("notes"), dismissed()]);

        run_switch_mode(&mut link, &mut launcher, &two_tags(), 4, None)
            .expect("a cap rejection is reported to the user, not exited on");

        assert_eq!(launcher.shown.len(), 2, "the picker must reopen");
        assert_eq!(
            launcher.shown[1].initial_search.as_deref(),
            Some("notes"),
            "the rejected name must come back into the input box"
        );
        assert!(
            launcher.shown[1]
                .input
                .starts_with(picker::REJECTION_MESSAGE),
            "the reason must be visible as a row: {:?}",
            launcher.shown[1].input
        );
    }

    #[test]
    fn switch_mode_stops_on_any_other_create_tag_refusal() {
        let mut sent = Vec::new();
        let mut link = link_over(
            &[r#"{"type":"error","message":"tag name is not valid"}"#],
            &mut sent,
        );
        let mut launcher = ScriptedLauncher::new(vec![accepted("  bad  name")]);

        run_switch_mode(&mut link, &mut launcher, &two_tags(), 4, None)
            .expect("a refusal that is not the cap ends the flow without exiting non-zero");

        assert_eq!(
            launcher.shown.len(),
            1,
            "only a cap rejection reopens the picker"
        );
    }

    #[test]
    fn a_refused_switch_tag_is_a_failure_of_the_keybind() {
        let mut sent = Vec::new();
        let mut link = link_over(&[r#"{"type":"error","message":"no such tag"}"#], &mut sent);
        let mut launcher = ScriptedLauncher::new(vec![accepted("code\t1")]);

        let error = run_switch_mode(&mut link, &mut launcher, &two_tags(), 4, None)
            .expect_err("a switch the wm refused is the whole point of the keybind failing");

        assert!(error.contains("refused the tag switch"), "{error}");
    }

    #[test]
    fn a_response_line_over_the_framing_cap_is_nothing_usable() {
        let mut sent = Vec::new();
        let oversized = "x".repeat(MAX_LINE_BYTES + 1);
        let mut link = link_over(&[&oversized], &mut sent);

        let error = link
            .get_state()
            .expect_err("a line over the cap must not be parsed");

        assert!(error.contains("nothing usable"), "{error}");
    }

    #[test]
    fn a_wm_that_answers_the_wrong_message_is_named_in_the_failure() {
        let mut sent = Vec::new();
        let mut link = link_over(&[r#"{"type":"ok"}"#], &mut sent);

        let error = link
            .get_state()
            .expect_err("`ok` is not an answer to get-state");

        assert!(error.contains("unexpected"), "{error}");
    }
}
