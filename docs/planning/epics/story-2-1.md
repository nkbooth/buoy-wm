---
baseline_commit: 085154a
---

# Story 2.1: IPC Server Foundation
Epic: 2 | Priority: H | Status: done

## Description
A Unix domain socket IPC server, running in-process inside the WM binary
(not a separate crate/binary — see Technical notes' "Scope boundary"),
exposing `wm-core` state as a `get-state` query and accepting three
mutation commands (`toggle-tag`, `create-tag`, `switch-tag`) that call the
same `wm-core` functions Story 1.7's raw keybinds already exercise
(`toggle_view_tag`, `create_tag`, `switch_tag`). This gives Epic 2's two
companion clients — the fuzzel-driven tag-picker (Stories 2.2/2.3/2.4) and
the status-bar (Story 2.5) — a real protocol to build against. **This story
builds the server only; no client code (picker or bar) is written here**
(see Technical notes' "Scope boundary").

Per the Epic 1 retrospective
(`_bmad-output/implementation-artifacts/epic-1-retro-2026-08-07.md`), this
is a named, expected-highest-risk category for Epic 2: IPC handlers become
a *third* call site onto `wm-core` mutators that already have two (Epic 1's
direct construction/tests, and Story 1.7's raw keybinds), the same
"reused/modified shared primitive touched from new call sites" pattern that
produced real bugs in Stories 1.4 and 1.5. The task breakdown below is
written with that in mind: the dispatch-to-`wm-core` mapping is isolated
into one small, purely-tested function (`ipc::dispatch::handle_request`) so
review can compare it line-by-line against Story 1.7's own keybind call
sites, rather than being interleaved with socket I/O.

**Four gaps neither the PRD, the ADRs, nor `components.md` resolve, closed
here rather than silently invented or deferred** (see Technical notes for
full rationale on each):

1. **Socket path/location.** No document specifies where the socket file
   lives. **Resolution:** `$XDG_RUNTIME_DIR/buoy-wm.sock` when
   `XDG_RUNTIME_DIR` is set (the real-deployment case —
   `architecture/deployment.md` confirms the WM runs under a real `river`
   login session, which always sets it); a deterministic `/tmp`-based
   fallback otherwise (devcontainer/test convenience only).
2. **Exact JSON message shapes.** ADR-007 fixes the *envelope* (newline-
   delimited JSON, one object per line, both directions) but not field
   names for `get-state`/`toggle-tag`/`create-tag`/`switch-tag` or their
   responses. **Resolution:** a concrete, fully-specified schema — see
   Technical notes' "Wire protocol."
3. **Threading/concurrency model.** `technology-stack.md` explicitly flags
   `tokio` vs. `smol` as "TBD at implementation time — not a v1
   architectural commitment." **Resolution:** neither — a dedicated
   `std::thread` accept loop plus `Arc<Mutex<WmCore>>`, no async runtime
   added. See Technical notes' "Concurrency model."
4. **Server-vs-client scope boundary for this story.** `components.md`
   lists `tag-picker` and `status-bar` explicitly as "(companion client,
   separate binary)" but does not tag `ipc-server` that way — confirmed by
   re-reading `components.md`'s `ipc-server` entry, which says only "Unix
   domain socket exposing `wm-core` state... accepting mutation commands,"
   with `wm-core` as its only dependency, no client-side entry. Combined
   with Stories 2.2-2.5 each independently depending on "Story 2.1 (IPC
   server)" as a prerequisite rather than being part of it, this confirms
   `ipc-server` is a module inside the existing `wm` binary (`wm/src/ipc/`,
   alongside `wm/src/wm_core/`), not a new Cargo workspace member. No
   `wm/Cargo.toml` workspace conversion happens in this story.

## Acceptance criteria
**Given** the WM is running
**When** `ipc-server` starts
**Then** it listens on a Unix domain socket at `$XDG_RUNTIME_DIR/buoy-wm.sock` (or the documented fallback — Technical notes) with owner-only permissions (mode `0600`; no auth layer — filesystem permissions only, per `architecture/authentication-authorization.md`)
**And** a stale socket file left behind by a previous WM exit/crash does not prevent startup — it is removed before binding
**And** it speaks newline-delimited JSON in both directions (ADR-007), one JSON object per line

**When** a client connects and sends `{"type":"get-state"}`
**Then** it receives one JSON line describing the full tag registry (id + name per tag), every registered view's id/app-id/tag-membership, every registered output's id/current-tag, and the currently-focused view's id (if any) — see Technical notes' "Wire protocol" for the exact shape

**When** a client sends `{"type":"toggle-tag","view_id":<u64>,"tag_id":<u8>}`, `{"type":"create-tag","name":<string>}`, or `{"type":"switch-tag","output_id":<u64>,"tag_id":<u8>}`
**Then** the WM applies the mutation via the same `wm-core` functions exercised by Story 1.7's raw keybinds (`toggle_view_tag`, `create_tag`, `switch_tag` respectively) and responds with a structured acknowledgement (`{"type":"ok"}`, or `{"type":"tag-created","tag_id":<u8>}` for `create-tag`)
**And** the connection remains open for further requests (a well-formed request that fails at the `wm-core` level — e.g. an unknown id, or the tag registry full — is a normal application-level outcome, not a protocol violation: it gets `{"type":"error","message":"..."}` and the connection stays open, not reset)

**When** a client sends a malformed, unparseable, non-JSON, invalid-UTF-8, or oversized (>64 KiB in one line) message
**Then** that client's connection is sent one `{"type":"error","message":"..."}` line and then closed/reset — never left half-open, never causing the WM process itself to panic or exit (NFR2)
**And** no other connected client, and no other part of the WM (window/output/seat handling), is affected by one client's malformed input or by that client disconnecting/crashing mid-message

**Given** the mutation applies to a real, in-memory `wm-core` call (no network hop, no disk I/O)
**Then** the round trip from receiving a request line to writing its response line is structurally bounded well under the 50ms budget (NFR1) — see Technical notes' "NFR1" for the structural argument (same style as every prior story's NFR1 note, not a live benchmark)

- [x] Tests pass (unit + integration where applicable)
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: `WmCore::snapshot()` — the read-only query `get-state` needs (AC: "receives... the full tag registry... view's... tag-membership... output's... current-tag... focused view")**
  - [x] 1.1 RED — Inside the devcontainer via devpod (`devpod ssh buoy-wm -- cargo test --manifest-path wm/Cargo.toml wm_core::state::`), add to `wm/src/wm_core/state.rs`'s test module: `snapshot_of_empty_core_has_empty_collections_and_no_focus` — a fresh `WmCore::new()`, assert `core.snapshot() == WmCoreSnapshot { tags: vec![], views: vec![], outputs: vec![], focused_view: None }`. `snapshot_includes_all_registered_tags_in_creation_order` — `create_tag("a")`/`create_tag("b")`, assert `snapshot().tags == vec![TagSnapshot{id: tag_a, name: "a".into()}, TagSnapshot{id: tag_b, name: "b".into()}]`. `snapshot_view_reports_its_own_tag_membership_as_sorted_tag_ids` — register a view, `create_tag` twice, `toggle_view_tag` both onto it, assert the matching `ViewSnapshot.tags` contains exactly both ids (order = registry creation order, reusing `TagRegistry::ids()`'s existing ordering guarantee — no new ordering rule invented). `snapshot_view_with_no_tags_has_empty_tags_vec`. `snapshot_outputs_report_current_tag_including_none` — one output with a tag switched on, one freshly registered with `current_tag == None`, assert both `OutputSnapshot`s. `snapshot_reports_focused_view_when_one_is_focused` and `snapshot_reports_none_when_nothing_focused`. `snapshot_views_and_outputs_are_sorted_ascending_by_id` — register views/outputs out of any special order, assert `snapshot().views`/`.outputs` come back sorted by id (deterministic for both test assertions and real clients — `HashMap` iteration order is otherwise unspecified). `snapshot_is_a_pure_query_and_never_mutates_state` — snapshot a populated core, clone it first, call `.snapshot()`, assert the core is unchanged (same `assert_eq!(core, snapshot)` pattern as `closable_focused_view`'s existing pure-query test). Confirm all fail to compile — `WmCoreSnapshot`/`TagSnapshot`/`ViewSnapshot`/`OutputSnapshot`/`snapshot()` don't exist yet.
  - [x] 1.2 GREEN — In `wm/src/wm_core/state.rs`, add four plain-data structs (no `serde` derives — see Technical notes' "wm-core stays protocol-agnostic"): `pub struct TagSnapshot { pub id: TagId, pub name: String }`, `pub struct ViewSnapshot { pub id: ViewId, pub app_id: String, pub tags: Vec<TagId> }`, `pub struct OutputSnapshot { pub id: OutputId, pub current_tag: Option<TagId> }` (all `#[derive(Debug, Clone, PartialEq, Eq)]`), `pub struct WmCoreSnapshot { pub tags: Vec<TagSnapshot>, pub views: Vec<ViewSnapshot>, pub outputs: Vec<OutputSnapshot>, pub focused_view: Option<ViewId> }` (`#[derive(Debug, Clone, PartialEq, Eq, Default)]`). Implement `pub fn snapshot(&self) -> WmCoreSnapshot`: `tags` from `self.tags.ids().into_iter().map(|id| TagSnapshot { id, name: self.tags.get(id).expect("id from ids() is always present").name.clone() }).collect()`; `views` from `self.views.values()` mapped to `ViewSnapshot` (per-view `tags` computed as `self.tags.ids().into_iter().filter(|id| view.tags.contains(id.0)).collect()` — reuses `TagSet::contains`, no new bitset-iteration API needed), then sorted by `id`; `outputs` from `self.outputs.values()` mapped 1:1 to `OutputSnapshot`, sorted by `id`; `focused_view: self.focused_view`. `///` doc comment: this is the one new read-only query this story adds to `wm-core`'s public API — every other IPC-mutation handler reuses an existing method (`toggle_view_tag`/`create_tag`/`switch_tag`) with no `wm-core` changes needed, but no prior story ever needed to *enumerate* all views/outputs/tags at once, so this is new. Confirm all 8 new tests pass; confirm no regression in the existing 91-test suite (99 total after this task).

- [x] **Task 2: `wm/src/ipc/mod.rs` scaffold + a generic poison-recovering lock helper (AC: NFR2 — "never causing the WM process itself to panic")**
  - [x] 2.1 RED — Add `wm/src/ipc/mod.rs` with a `#[cfg(test)] mod tests`. Test `lock_recovering_returns_the_guard_normally_when_not_poisoned` — a plain `Mutex::new(5)`, assert `*lock_recovering(&mutex) == 5`. Test `lock_recovering_recovers_the_inner_value_after_a_panic_while_locked` — `let mutex = Arc::new(Mutex::new(5));`, spawn a thread that locks it and panics while holding the guard (`std::thread::spawn(move || { let _g = mutex2.lock().unwrap(); panic!("simulated"); })`), `.join()` the thread (its `Result` is `Err` — expected, don't propagate it), then call `lock_recovering(&mutex)` on the *same* mutex from the test thread and assert it returns `5` rather than panicking — proving a poisoned lock from one thread's panic does not cascade into panicking every subsequent locker (the exact failure mode this story must avoid: one buggy IPC connection thread must never poison the mutex the Wayland thread also locks, per this story's Technical notes on "Mutex poisoning"). Confirm both fail to compile — `lock_recovering` doesn't exist yet.
  - [x] 2.2 GREEN — Implement `pub fn lock_recovering<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> { mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) }` in `wm/src/ipc/mod.rs`, with a `///` doc comment explaining the poisoning-cascade rationale (a panic inside one client connection's request handling — however unlikely, given `wm-core`'s own NFR2-safe design — must not turn into a WM-wide crash the next time the Wayland thread locks the same `wm_core`; recovering the poisoned guard's last-known-good inner value is strictly better than propagating the panic, since `wm-core`'s own mutators are already `Result`-based and never leave partial writes on their own `Err` paths). Add `pub mod protocol;`, `pub mod dispatch;`, `pub mod server;` (empty stub files for now, filled in by Tasks 3/4/6) and add `mod ipc;` to `wm/src/main.rs` alongside the existing `mod wm_core;`. Confirm both new tests pass; confirm `cargo build --manifest-path wm/Cargo.toml` still succeeds with the two empty submodules present.

- [x] **Task 3: `wm/src/ipc/protocol.rs` — wire types + `parse_request`/`serialize_response` (AC: "speaks newline-delimited JSON," malformed-input bullets)**
  - [x] 3.1 Add `serde = { version = "1", features = ["derive"] }` and `serde_json = "1"` to `wm/Cargo.toml`'s `[dependencies]` (technology-stack.md names both explicitly for "IPC messages" — this is the story that actually needs them for the first time). Run `cargo build --manifest-path wm/Cargo.toml` inside the devcontainer to confirm they resolve and the lockfile updates.
  - [x] 3.2 RED — In `wm/src/ipc/protocol.rs`'s `#[cfg(test)] mod tests`, add parse tests exercising every request variant round-trips: `parses_get_state`, `parses_toggle_tag`, `parses_create_tag`, `parses_switch_tag` (each: call `parse_request(r#"{"type":"toggle-tag","view_id":3,"tag_id":2}"#)`, assert `Ok(Request::ToggleTag { view_id: 3, tag_id: 2 })`, etc. — exact JSON literals matching Technical notes' "Wire protocol" table). Then the **malformed-input suite this story's NFR2 emphasis specifically calls for**, every case asserting `parse_request(...)` returns `Err(_)` (never panics, never `.unwrap()`s inside `parse_request` itself): `parse_request_rejects_empty_string`, `parse_request_rejects_non_json_garbage` (`"not json at all"`), `parse_request_rejects_valid_json_that_is_not_an_object` (`"[1,2,3]"`, `"42"`, `"\"a string\""`), `parse_request_rejects_object_with_unknown_type` (`r#"{"type":"delete-everything"}"#`), `parse_request_rejects_object_missing_type_field` (`"{}"`), `parse_request_rejects_toggle_tag_missing_required_fields` (`r#"{"type":"toggle-tag"}"#`, and `r#"{"type":"toggle-tag","view_id":3}"#` missing `tag_id`), `parse_request_rejects_wrong_field_types` (`r#"{"type":"toggle-tag","view_id":"not-a-number","tag_id":2}"#`), `parse_request_rejects_tag_id_out_of_u8_range` (`r#"{"type":"switch-tag","output_id":0,"tag_id":999}"#` — `tag_id` is `u8`, 999 overflows), `parse_request_rejects_deeply_nested_json` (a JSON value nested well past any of this protocol's real shapes, e.g. `r#"{"type":"get-state","extra":[[[[[1]]]]]}"#` — must not panic; also confirms unknown extra fields don't cause a different failure mode than expected — see 3.3's `deny_unknown_fields` decision), `parse_request_rejects_invalid_utf8_bytes` (construct from a `&[u8]` containing `0xFF 0xFE`, confirm the byte→`&str` conversion this function's caller will need is validated *before* reaching `serde_json` and produces `Err`, not a panic — see 3.3's exact signature). Add serialize tests: `serializes_state_response_shape`, `serializes_ok_response`, `serializes_tag_created_response`, `serializes_error_response` — each asserting the exact JSON string `serialize_response(&response)` produces, matching Technical notes' literal examples. Confirm all fail to compile — none of `Request`/`Response`/`parse_request`/`serialize_response` exist yet.
  - [x] 3.3 GREEN — Define `#[derive(Debug, Clone, PartialEq, Deserialize)] #[serde(tag = "type", rename_all = "kebab-case")] pub enum Request { GetState, ToggleTag { view_id: u64, tag_id: u8 }, CreateTag { name: String }, SwitchTag { output_id: u64, tag_id: u8 } }` — deliberately **not** `#[serde(deny_unknown_fields)]` (tolerating unknown extra fields is a forward-compatibility no-op today and costs nothing; rejecting them would just be one more way to fail closed for no behavioral benefit this story needs). Define the DTO structs `TagDto { id: u8, name: String }`, `ViewDto { id: u64, app_id: String, tags: Vec<u8> }`, `OutputDto { id: u64, current_tag: Option<u8> }` (`#[derive(Debug, Clone, PartialEq, Serialize)]`) and `#[derive(Debug, Clone, PartialEq, Serialize)] #[serde(tag = "type", rename_all = "kebab-case")] pub enum Response { State { tags: Vec<TagDto>, views: Vec<ViewDto>, outputs: Vec<OutputDto>, focused_view: Option<u64> }, Ok, TagCreated { tag_id: u8 }, Error { message: String } }`. Implement `pub fn parse_request(bytes: &[u8]) -> Result<Request, ParseError>` — a small `pub enum ParseError { InvalidUtf8, InvalidJson(String) }` (`Debug`, `Clone`, `PartialEq`); body: `let text = std::str::from_utf8(bytes).map_err(|_| ParseError::InvalidUtf8)?; serde_json::from_str(text).map_err(|e| ParseError::InvalidJson(e.to_string()))`. Implement `pub fn serialize_response(response: &Response) -> String` — `serde_json::to_string(response).expect("Response is a fully-controlled, always-serializable type — no NaN floats, no non-string map keys")` (the one `.expect()` this story adds; justified per NFR2's established "safe-by-construction, not caller-facing" precedent, same reasoning as `cycle_focus`'s internal unwraps in Story 1.5/1.7). Confirm all new tests pass; confirm no regression.

- [x] **Task 4: `wm/src/ipc/dispatch.rs` — `handle_request`, the pure `Request`→`wm-core`-call→`Response` mapping (AC: "applies the mutation via the same `wm-core` functions... the connection remains open... a well-formed request that fails at the `wm-core` level... gets `{"type":"error",...}` and the connection stays open")**
  - [x] 4.1 RED — In `wm/src/ipc/dispatch.rs`'s `#[cfg(test)] mod tests`, using a real `wm_core::state::WmCore` (no socket, no `Arc`/`Mutex` — this is the pure decision layer, same "test the decision, not the I/O" split as every prior story's `wm-core` vs. `main.rs` boundary): `get_state_returns_state_response_matching_snapshot` — populate a `WmCore` with a tag/view/output, call `handle_request(&mut core, Request::GetState)`, assert the result is `Response::State { .. }` with fields matching `core.snapshot()` 1:1 (field-by-field, proving `dispatch` is a thin mapping over `snapshot()`, not a second place that computes state). `toggle_tag_calls_wm_core_toggle_view_tag_and_returns_ok` — register a view + tag, `handle_request(&mut core, Request::ToggleTag { view_id: view.0, tag_id: tag.0 })`, assert `Response::Ok` **and** assert the view's tag membership actually flipped in `core` (proving the call really happened, not just that `Ok` was returned). `toggle_tag_unknown_view_returns_error_not_panic` — a bogus `view_id`, assert `Response::Error { message: "unknown view" }`, core unchanged. `toggle_tag_unknown_tag_returns_error` — bogus `tag_id`, assert `Response::Error { message: "unknown tag" }`. `create_tag_calls_wm_core_create_tag_and_returns_tag_created_with_real_id` — `handle_request(&mut core, Request::CreateTag { name: "web".into() })`, assert `Response::TagCreated { tag_id }` where `core.snapshot().tags` actually contains `{id: tag_id, name: "web"}`. `create_tag_at_registry_cap_returns_the_picker_facing_error_message` — pre-fill 64 tags (same setup as `create_tag_returns_tag_limit_reached_when_registry_full`), assert `Response::Error { message: "tag limit reached (64)" }` — the exact literal string Story 2.3's own AC specifies the picker renders verbatim ("tag limit reached (64)"), so this dispatch layer is the single place that string is defined, not duplicated in a future picker story. `switch_tag_calls_wm_core_switch_tag_and_returns_ok` — mirrors the toggle-tag structure. `switch_tag_unknown_output_returns_error`, `switch_tag_unknown_tag_returns_error`. `handle_request_never_panics_regardless_of_which_ids_are_bogus` — a small property-style sweep: for every one of the three mutation variants, call with every combination of (valid, bogus) ids and assert the call returns (doesn't panic) in every case — a direct, explicit regression guard for this story's NFR2 emphasis, mirroring `every_mutator_rejects_unknown_ids_without_mutating_state`'s existing `wm-core` precedent one layer up. Confirm all fail to compile — `handle_request` doesn't exist yet.
  - [x] 4.2 GREEN — Implement `pub fn handle_request(wm_core: &mut WmCore, request: Request) -> Response` in `wm/src/ipc/dispatch.rs`: `Request::GetState => Response::from(wm_core.snapshot())` (a small `impl From<WmCoreSnapshot> for Response` mapping each `TagSnapshot`/`ViewSnapshot`/`OutputSnapshot` 1:1 into `TagDto`/`ViewDto`/`OutputDto`, `TagId`/`ViewId`/`OutputId`'s inner integers unwrapped via `.0` — this is the one place `wm-core`'s ids get converted to bare JSON-friendly integers, kept out of `wm-core` itself per Technical notes); `Request::ToggleTag { view_id, tag_id } => match wm_core.toggle_view_tag(ViewId(view_id), TagId(tag_id)) { Ok(()) => Response::Ok, Err(e) => Response::Error { message: describe_wm_core_error(e) } }`; `Request::CreateTag { name } => match wm_core.create_tag(name) { Ok(id) => Response::TagCreated { tag_id: id.0 }, Err(e) => Response::Error { message: describe_wm_core_error(e) } }`; `Request::SwitchTag { output_id, tag_id } => match wm_core.switch_tag(OutputId(output_id), TagId(tag_id)) { Ok(()) => Response::Ok, Err(e) => Response::Error { message: describe_wm_core_error(e) } }`. Add a small private `fn describe_wm_core_error(e: WmCoreError) -> String` mapping `UnknownView => "unknown view"`, `UnknownTag => "unknown tag"`, `UnknownOutput => "unknown output"`, `TagLimitReached => "tag limit reached (64)"`. `///` doc comment on `handle_request`: this is the single, pure, unit-tested mapping every socket connection calls into — deliberately factored out of `server.rs` so it can be reviewed and tested with zero socket/thread machinery in the loop, and so it is trivially diffable against Story 1.7's keybind call sites for the "same functions, no duplicate logic path" AC. Confirm all new tests pass; confirm no regression (still 99 `wm_core` tests + this task's new `ipc::dispatch` tests, all green).

- [x] **Task 5: `resolve_socket_path` — pure socket-path resolution (AC: "listens on a Unix domain socket at `$XDG_RUNTIME_DIR/buoy-wm.sock` (or the documented fallback)")**
  - [x] 5.1 RED — In `wm/src/ipc/server.rs`'s `#[cfg(test)] mod tests`: `resolve_socket_path_uses_xdg_runtime_dir_when_set` — `resolve_socket_path(Some("/run/user/1000"), None, None)` (signature: `xdg_runtime_dir: Option<&str>, user: Option<&str>, logname: Option<&str>` — explicit parameters, not read from `std::env` directly inside the function, so this stays a pure, injectable function per this story's "test the decision, not the I/O" split) returns `PathBuf::from("/run/user/1000/buoy-wm.sock")`. `resolve_socket_path_falls_back_to_tmp_user_when_xdg_runtime_dir_unset` — `resolve_socket_path(None, Some("nick"), None)` returns `PathBuf::from("/tmp/buoy-wm-nick.sock")`. `resolve_socket_path_falls_back_to_logname_when_user_also_unset` — `resolve_socket_path(None, None, Some("nick"))` returns the same. `resolve_socket_path_falls_back_to_literal_unknown_when_nothing_is_set` — `resolve_socket_path(None, None, None)` returns `PathBuf::from("/tmp/buoy-wm-unknown.sock")`. Confirm all fail to compile.
  - [x] 5.2 GREEN — Implement `pub fn resolve_socket_path(xdg_runtime_dir: Option<&str>, user: Option<&str>, logname: Option<&str>) -> PathBuf`: if `xdg_runtime_dir` is `Some(dir)`, `PathBuf::from(dir).join("buoy-wm.sock")`; else `PathBuf::from("/tmp").join(format!("buoy-wm-{}.sock", user.or(logname).unwrap_or("unknown")))`. Add a thin, untested (I/O-reading, not logic) wrapper `pub fn default_socket_path() -> PathBuf { resolve_socket_path(std::env::var("XDG_RUNTIME_DIR").ok().as_deref(), std::env::var("USER").ok().as_deref(), std::env::var("LOGNAME").ok().as_deref()) }` for `main.rs`/Task 7 to call. `///` doc comments on both explaining the real-deployment-vs-sandbox-fallback rationale (Technical notes' gap #1). Confirm all new tests pass.

- [x] **Task 6: `wm/src/ipc/server.rs` — the socket accept loop + per-connection handling (AC: startup/permissions/stale-socket bullets, all malformed-input bullets, "no other connected client... is affected")**
  - [x] 6.1 RED — This task's tests use **real `UnixListener`/`UnixStream` sockets** against a temp path (`std::env::temp_dir().join(format!("buoy-wm-test-{}-{}.sock", std::process::id(), <a per-test unique suffix, e.g. the test function's own name or a `static AtomicU64` counter>))`) — unlike `main.rs`'s Wayland `Dispatch` glue, this I/O boundary has **no live-compositor dependency** and is fully exercisable in this sandbox; do not apply the Wayland-glue no-RED/GREEN carve-out here. Each test: call `server::spawn(Arc::new(Mutex::new(WmCore::new())), &socket_path)`, which must return only after the listener is actually bound (no `sleep`-based race — see 6.2's ordering requirement), connect a real `std::os::unix::net::UnixStream::connect(&socket_path)`, write a request line + `\n`, read a response line back. Tests: `get_state_round_trip_returns_state_response`. `toggle_tag_round_trip_mutates_shared_wm_core` — connect, send `create-tag`, then (same or a second connection) send `toggle-tag` against the just-created id, then `get-state`, assert the view's tags reflect it — proves the `Arc<Mutex<WmCore>>` is genuinely shared across connections/requests, not per-connection state. `malformed_json_gets_error_response_then_connection_closes` — send `"not json\n"`, read the `{"type":"error",...}` line, then confirm a subsequent read returns EOF (connection closed) rather than hanging or accepting a further line. `oversized_line_without_newline_is_rejected_not_grown_forever` — write >64 KiB of non-newline bytes, assert the connection is reset (error response or immediate close) rather than the server growing an unbounded read buffer — the literal DoS-shaped case this story's NFR2 emphasis calls out. `invalid_utf8_bytes_get_error_response_not_a_panic` — write raw non-UTF-8 bytes plus `\n`, assert an error response, not a hung/crashed server. `one_malformed_connection_does_not_affect_a_second_concurrent_good_connection` — open two connections, send malformed on one and a valid `get-state` on the other (interleaved), assert the good connection's response is unaffected and the server (and a subsequent third connection) is still alive afterward — the direct proof of "no other connected client... is affected." `stale_socket_file_from_a_previous_run_does_not_block_startup` — pre-create a regular file (not a socket) at the target path with dummy bytes, then call `server::spawn` against that same path, assert it succeeds (removes the stale file, binds fresh) rather than returning an `AddrInUse`/`AlreadyExists` error. `socket_file_has_owner_only_permissions_after_spawn` — after `spawn`, `std::fs::metadata(&socket_path).unwrap().permissions().mode() & 0o777 == 0o600`. `well_formed_request_referencing_unknown_ids_keeps_connection_open` — send `toggle-tag` with a bogus `view_id`, read the `{"type":"error",...}` line, then send a second, valid `get-state` on the **same** connection and confirm it still gets a proper response (proving the "reset only on unparseable input, not on `wm-core`-level errors" AC distinction is real, not just documented). Confirm all fail to compile/fail to connect — `server::spawn` doesn't exist yet.
  - [x] 6.2 GREEN — Implement in `wm/src/ipc/server.rs`: `pub fn spawn(wm_core: Arc<Mutex<WmCore>>, socket_path: &Path) -> std::io::Result<JoinHandle<()>>`. Body: `let _ = std::fs::remove_file(socket_path);` (best-effort stale-file cleanup — ignore the `Err` if nothing was there; see Technical notes' "Stale socket handling" for why blind removal is acceptable here), `let listener = UnixListener::bind(socket_path)?;` (bind happens **synchronously in the calling thread, before any thread is spawned** — this is what lets 6.1's tests connect immediately after `spawn()` returns with no race/sleep), then `std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))?;`, then `Ok(std::thread::spawn(move || { for stream in listener.incoming() { match stream { Ok(stream) => { let wm_core = Arc::clone(&wm_core); std::thread::spawn(move || handle_connection(stream, wm_core)); } Err(e) => eprintln!("ipc: accept error: {e}") } } }))`. Implement `fn handle_connection(stream: UnixStream, wm_core: Arc<Mutex<WmCore>>)`: wrap in `std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { ... }))` around the per-connection loop body (see Technical notes' "Defense in depth: per-connection panic containment" — a second, independent layer beneath Task 2's poison-recovering lock, so a bug that panics while handling one connection's request never even reaches the shared mutex), logging via `eprintln!` if `catch_unwind` returns `Err`. Inside: a `BufReader` over a cloned/split stream, read lines with a `MAX_LINE_BYTES: usize = 64 * 1024` cap — read byte-by-byte or via `take(MAX_LINE_BYTES as u64 + 1)` combined with `read_until(b'\n', ...)`, and if the resulting buffer exceeds the cap without having found a newline, treat it as a malformed-input case (write the error response, then `return` to close the connection) rather than continuing to read. For each complete line (newline stripped): call `protocol::parse_request(line_bytes)`; on `Err(_)`, write `serialize_response(&Response::Error { message: "malformed request".into() })` + `\n`, flush, then `return` (closes the connection — the AC's "rejected/reset" semantics); on `Ok(request)`, `let response = dispatch::handle_request(&mut *ipc::lock_recovering(&wm_core), request);` (lock held only for this one call, released immediately after — see Technical notes' "NFR1"), write `serialize_response(&response)` + `\n`, flush, and continue the loop (do **not** return — this is the "well-formed request, `wm-core`-level error, connection stays open" branch). On a read error/EOF from the stream itself, exit the loop normally (client disconnected; not an error to log). `///` doc comments throughout explaining the malformed-vs-application-error distinction and the panic-containment rationale. Confirm all Task 6.1 tests pass; confirm no regression in the growing suite.
  - [x] **No RED/GREEN for the `UnixListener::incoming()` accept-loop's own infinite `for` loop and its outer `thread::spawn` boilerplate specifically** (as opposed to `handle_connection`'s body, which *is* fully covered by 6.1's real-socket tests) — there is no way to assert "the outer loop keeps accepting forever" other than what 6.1's multi-connection tests already do implicitly by successfully opening several connections against one `spawn()` call. Verified via `cargo build`/`cargo clippy --all-targets -D warnings` plus structural review confirming the accept loop never `.unwrap()`s on a per-connection `accept()` error (logs and continues instead, so one failed accept can't kill the whole listener).

- [x] **Task 7: Wire `main.rs` — `Arc<Mutex<WmCore>>` + spawn the IPC server (AC: all — Wayland/process glue, no new `wm-core` decision logic of its own; HIGH-priority review scrutiny per the Epic 1 retro's action item #1, since this touches every existing `wm_core` call site, not just new ones)**
  - [x] 7.1 Change `WindowManager`'s field (`wm/src/main.rs:97-103`) from `wm_core: WmCore` to `wm_core: Arc<Mutex<WmCore>>` (still `#[derive(Default)]`-constructible unchanged: `Arc<Mutex<T>>` is `Default` whenever `T: Default`, which `WmCore` already is). Add `use std::sync::{Arc, Mutex};` and `use crate::ipc;` to `main.rs`'s imports.
  - [x] 7.2 Update every existing call site to lock via `ipc::lock_recovering` (never bare `.lock().unwrap()` — Task 2's poisoning rationale applies identically here, since these are the *other* thread sharing the same mutex) and rename the resulting local bindings from `wm_core`/`self.wm_core` exactly as today so downstream code (`do_action`, `focus_top`, which both already take `&mut WmCore` as a parameter and are otherwise untouched) needs no further changes. Enumerated, so review can check each one directly against this list rather than diffing the whole file:
    - `remove_windows` (`wm/src/main.rs:231-269`): `let wm_core = &mut self.wm_core;` → `let mut wm_core_guard = ipc::lock_recovering(&self.wm_core); let wm_core = &mut *wm_core_guard;`.
    - `init_new_windows` (`wm/src/main.rs:287-317`): four `self.wm_core.<method>(...)` calls → lock once at the top of the function (`let mut wm_core = ipc::lock_recovering(&self.wm_core);`) and change each call site to `wm_core.<method>(...)`.
    - `ensure_pinned_terminal_spawned` (`wm/src/main.rs:325-333`): `self.wm_core.claim_pinned_terminal_spawn(tag_id)` → `ipc::lock_recovering(&self.wm_core).claim_pinned_terminal_spawn(tag_id)`.
    - `manage_seats` (`wm/src/main.rs:393-483`): `let wm_core = &mut self.wm_core;` → `let mut wm_core_guard = ipc::lock_recovering(&self.wm_core); let wm_core = &mut *wm_core_guard;` (unchanged usage for the rest of the function — `do_action`/`focus_top` calls, `raise_view`/`set_focus` calls — since `wm_core` is still a `&mut WmCore` local of the same name).
    - `Dispatch<RiverWindowManagerV1, ()>`'s `Event::Output` arm (`wm/src/main.rs:917-920`): `state.wm.wm_core.register_output()` → `ipc::lock_recovering(&state.wm.wm_core).register_output()`.
    Confirm via a full-file re-read after this task that **no other call site** touches `wm_core`/`self.wm_core` (grep `wm_core` in `wm/src/main.rs` and account for every match) — this is the specific verification step the retro's action item #1 asks for on any story that touches a shared `wm-core` call site.
  - [x] 7.3 In `fn main()`, after the existing `river_wm`/`river_xkb` presence checks (`wm/src/main.rs:1097-1104`) and before the `loop { event_queue.blocking_dispatch(...) }`, add: `let socket_path = ipc::server::default_socket_path(); if let Err(e) = ipc::server::spawn(Arc::clone(&app_data.wm.wm_core), &socket_path) { eprintln!("Failed to start IPC server on {socket_path:?}: {e}"); }` — a failed IPC-server bind (e.g. permissions issue) is logged and does **not** abort WM startup (the WM's core job — managing windows — must not depend on the IPC server; this is a deliberate degrade-not-crash choice, consistent with NFR2's priority ordering, and mirrors this codebase's existing `log_wm_core_err`-style "log, don't abort" convention rather than `std::process::exit`, which this file otherwise reserves for unrecoverable Wayland-protocol-level failures only).
  - [x] **No RED/GREEN for this task** — same carve-out class as every prior story's `main.rs` wiring task (Story 1.7 Tasks 4-6): call-site composition with no new decision logic (every decision this task's code paths reach — `toggle_view_tag`/`create_tag`/`switch_tag`/`register_output`/etc. — is already unit-tested in `wm_core`'s own suite; `lock_recovering` is unit-tested in Task 2; the IPC server's own logic is unit-tested in Tasks 3/4/6). Verification: `cargo build --manifest-path wm/Cargo.toml` and `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings` succeeding, the full existing 91-test `wm_core` suite (now 99 after Task 1) still green with zero behavioral change to any pre-existing call site's arguments/ordering, plus the explicit 7.2 checklist above satisfied and reviewed.

- [x] **Task 8: Hygiene — retire `toggle_view_tag`'s now-inaccurate `#[allow(dead_code)]` (AC: none directly — closing out a marker this story makes stale, same pattern as Story 1.7's Task 7)**
  - [x] 8.1 In `wm/src/wm_core/state.rs`, remove `#[allow(dead_code)]` from `toggle_view_tag` (now reachable via `ipc::dispatch::handle_request`'s `Request::ToggleTag` arm) and update its doc comment's "deferred to Epic 2's assign-mode picker" note to say it is wired via this story's IPC dispatch layer specifically (not the picker itself, which doesn't exist until Story 2.2 — the IPC handler is reachable and real today even though no client sends it yet in-process; that distinction matters for an accurate comment).
  - [x] 8.2 In `wm/src/wm_core/tag_set.rs`, re-check whether the module-level `#![allow(dead_code)]` (currently justified by "`toggle_view_tag`, its only caller, stays unwired") is still accurate now that `toggle_view_tag` is wired. Run `cargo build --manifest-path wm/Cargo.toml` to check for a `dead_code` warning on `TagSet::empty`/`contains`/`insert`/`remove` with the module-level allow removed; if all four are now warning-free (expected — `toggle_view_tag` exercises `contains`/`insert`/`remove`, and `View`'s `Default::default()` already covered `TagSet`'s own `Default`, though `TagSet::empty()` specifically may remain unreached since `register_view` uses `Default::default()`, not `TagSet::empty()` — narrow the allow to just `TagSet::empty()` if so, mirroring `TagRegistry::new()`'s existing narrow-allow precedent from Story 1.7, rather than leaving a broader-than-necessary module-level allow). Update the module comment accordingly either way.
  - [x] **No RED/GREEN** — pure hygiene, no behavior change. Verified via `cargo build`/`cargo clippy --all-targets -D warnings` showing exactly the intended `dead_code` state (nothing broader than necessary, nothing missing).

- [x] **Task 9: Full in-container verification gate (AC: all)**
  - [x] 9.1 Run, inside the devcontainer via devpod (`devpod ssh buoy-wm` — sanity-check this transport works before starting, per the Epic 1 retro's action item #3; fall back to direct `podman exec` against the running container, as Story 1.7 did, if `devpod ssh` still fails, and record which was used): `cargo fmt --manifest-path wm/Cargo.toml --all -- --check`, `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings`, and `cargo test --manifest-path wm/Cargo.toml`. Confirm the full suite passes: 91 pre-existing + 8 (`WmCore::snapshot`, Task 1) + 2 (`lock_recovering`, Task 2) + ~16 (`ipc::protocol`, Task 3) + ~10 (`ipc::dispatch`, Task 4) + 4 (`resolve_socket_path`, Task 5) + ~10 (`ipc::server` real-socket integration tests, Task 6) — expect roughly 141 total (exact count depends on the property-style sweep in 4.1 and how many discrete `#[test]` fns the malformed-input suites in 3.2/6.1 end up as); zero clippy/fmt violations; the exact `dead_code` state Task 8 verifies. Run `pre-commit run --all-files` as a final sanity check (same hooks as every prior story).
  - [x] 9.2 Manual smoke-test note, **narrower gap than every prior story's** (Technical notes' "Wire protocol" point: unlike Wayland `Dispatch` glue, this socket is independently exercisable without a live `river` session) — if time allows, run the WM binary standalone (it will fail at the `river_window_manager_v1` global lookup and `std::process::exit(1)` without a real compositor — so this specifically means confirming the IPC server thread starts and binds *before* that exit path is hit is not fully exercisable this way either, since `main()` spawns the IPC server only after the Wayland global checks succeed per Task 7.3's placement). Record in the Dev Agent Record whether a live end-to-end (`river` running, `nc -U`/`socat` talking to the real socket while the WM also manages real windows) smoke test was performed or explicitly deferred — same "state it, don't silently assume it" convention as every prior story. This is a smaller gap than Epic 1's Wayland-glue gap specifically because Task 6's own tests already exercise the *real* socket I/O path end-to-end with a real (test-constructed) `WmCore`; what remains untested only in-sandbox is the *combination* with a live Wayland session, not the socket protocol itself.

## Technical notes

**Scope boundary: server only, no client code.** Confirmed by re-reading
`components.md` (only `tag-picker` and `status-bar` are tagged "companion
client, separate binary"; `ipc-server` is not) and Stories 2.2-2.5 (each
lists "Depends on Story 2.1" as a prerequisite, not as shared scope). This
story adds zero new binaries/targets to `wm/Cargo.toml` — `ipc-server`
lives at `wm/src/ipc/`, a sibling module to `wm/src/wm_core/`, inside the
existing single `wm` binary. `wm/Cargo.toml` does **not** become a
`[workspace]` in this story (Story 1.2's own note flagged this as "likely"
needed once `ipc-server` needed to be "separately-built/tested" — it
doesn't, since it isn't a separate binary; that conversion, if ever needed,
is deferred to whichever future story first adds `tag-picker`/`status-bar`
as actual separate Cargo targets).

**Wire protocol — the exact JSON shapes.** Requests (client → WM), one per
line:
```
{"type":"get-state"}
{"type":"toggle-tag","view_id":3,"tag_id":2}
{"type":"create-tag","name":"web"}
{"type":"switch-tag","output_id":0,"tag_id":2}
```
Responses (WM → client), one per line:
```
{"type":"state","tags":[{"id":0,"name":"web"}],"views":[{"id":3,"app_id":"foot","tags":[0,2]}],"outputs":[{"id":0,"current_tag":0}],"focused_view":3}
{"type":"ok"}
{"type":"tag-created","tag_id":5}
{"type":"error","message":"unknown tag"}
```
`view_id`/`output_id` are `u64` (matching `ViewId`/`OutputId`'s inner
type); `tag_id` is `u8` (matching `TagId`'s inner type, ADR-006's 64-tag
cap). Unknown extra JSON fields are tolerated, not rejected
(`#[serde(deny_unknown_fields)]` is deliberately not used — see Task 3.3).
No schema versioning (ADR-007's own stated consequence — both ends ship
from the same repo). `get-state`'s `focused_view` field is an addition
beyond the original draft AC's literal three-item list ("current tags,
per-view tag membership, per-output current tag") — `wm-core` already
tracks a WM-wide `focused_view` (`features-and-acceptance-criteria.md`
lists "focus" among the state the WM tracks), and Story 2.2's picker
cannot show "the focused window's current tags checked" without knowing
which view is focused, so this story exposes it now rather than leaving a
gap Story 2.2 would have to reopen this story to close. Flagged as an
assumption, not silently added.

`create-tag`'s IPC behavior is "register a tag, return its id" only — it
does **not** also assign the new tag to any view. Story 2.3's AC ("a new
tag is registered... and it's immediately applied to the focused window")
is `tag-picker` client behavior: compose `create-tag` then `toggle-tag`
client-side. This keeps each IPC command a single `wm-core` call
(matching the AC's own three-verb list) rather than inventing a fourth,
compound command not named anywhere in the architecture docs.

**Concurrency model — resolving `technology-stack.md`'s tokio/smol "TBD."**
No async runtime is added. The IPC accept loop runs on a dedicated
`std::thread`, spawning one further `std::thread` per accepted connection
(thread-per-connection is more than adequate for a single-user local
socket serving at most a handful of short-lived/long-lived companion
clients — no connection-count limit is built, YAGNI). `WmCore` is shared
between this thread family and the pre-existing Wayland-dispatch main
thread via `Arc<Mutex<WmCore>>`. The mutex is locked only for the duration
of one `dispatch::handle_request` call (or one existing `wm_core.<method>`
call on the main-thread side) — never across a socket read/write or a
Wayland round-trip — so lock contention is bounded by in-memory `wm-core`
work only (NFR1). This resolves the technology-stack.md TBD by choosing
neither `tokio` nor `smol`: the existing Wayland dispatch loop
(`event_queue.blocking_dispatch`) is untouched and stays fully
synchronous, and the IPC side doesn't need an async runtime to serve a
handful of blocking, thread-per-connection sockets. No new ADR — this is
implementation-decision gap-filling within an existing story's scope, the
same judgment call Story 1.7 made for its own two resolved ambiguities,
not a decision among architecturally significant alternatives.

**Mutex poisoning — why every lock site uses `lock_recovering`, not
`.lock().unwrap()`.** A panic inside IPC request handling (however
unlikely, given `wm-core`'s own `Result`-based, `.expect()`-light design)
would, with a bare `.lock().unwrap()`, poison the mutex and then panic
*every subsequent locker* — including the main Wayland thread, the next
time it touches `wm_core` for any reason. That would turn "a bug in
handling one malformed IPC request" into "the whole WM crashes on its next
keypress," directly violating this story's NFR2 emphasis ("malformed...
input... must never crash the WM"). `lock_recovering`
(`mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())`) recovers
the guard instead of panicking, on both the IPC side and the pre-existing
`main.rs` call sites (Task 7.2) — both share the same mutex, so both need
the same discipline.

**Defense in depth: per-connection panic containment.** Beyond
`lock_recovering`, Task 6.2 also wraps each connection's request-handling
loop in `std::panic::catch_unwind`, so a hypothetical panic inside
`parse_request`/`handle_request`/`serialize_response` for one malformed or
adversarial message is caught and logged, closing only that one
connection, without even reaching the shared mutex or affecting any other
connection or the Wayland thread. This is not required by any ADR or FR —
it is this story's own decision, made because constraint 3 in this
story's brief explicitly asks for extra scrutiny on this exact failure
mode ("this is the WM's first... attack surface... must never crash the
WM"), and the cost (one `catch_unwind` wrapper) is small relative to the
downside (a crashed desktop session) if some future edge case in the
parsing/dispatch layer turns out not to be as panic-free as Tasks 3/4's
test suites believe it to be.

**Stale socket handling.** `UnixListener::bind` fails with `AddrInUse` if
a file already exists at the target path — including a perfectly valid,
still-listening socket from a *second* WM instance, not just a stale file
from a crashed one. This story does not distinguish the two cases (no
PID-file/lock-file check is built) — `river_window_manager_v1`'s existing
`Event::Unavailable` handling (`wm/src/main.rs:901-904`) already prevents
two real WM instances from running against the same `river` session in
the overwhelmingly common case, so the residual risk (a genuine race
between two WM processes both starting at once) is narrow and, per this
project's YAGNI convention, not worth building connect-before-unlink
detection for in v1. Flagged as a known, accepted limitation, not hidden.

**`wm-core` stays protocol-agnostic.** `TagSnapshot`/`ViewSnapshot`/
`OutputSnapshot`/`WmCoreSnapshot` (Task 1, `wm/src/wm_core/state.rs`) are
plain Rust structs with **no `serde` derives** — `wm-core`'s `Cargo.toml`
dependency surface gains nothing from this story. All `serde`/JSON-facing
types (`Request`, `Response`, `TagDto`/`ViewDto`/`OutputDto`) live in
`wm/src/ipc/protocol.rs`, and the conversion between the two (Task 4.2's
`impl From<WmCoreSnapshot> for Response`) is the one place `wm-core`'s
`TagId`/`ViewId`/`OutputId` newtypes get unwrapped to bare integers for
the wire. This is the literal implementation of constraint 5 in this
story's brief ("no socket/JSON types should leak into `wm_core`") and
mirrors `components.md`'s own stated boundary (`wm-core`: "no external
interface of its own"; `ipc-server`: "Key dependencies: `wm-core`" —
one-directional).

**NFR1 (50ms budget).** No literal benchmark is added (same structural-
argument treatment as every prior story's NFR1 note — a sandboxed CI
timing assertion would be flaky, not diagnostic). The structural argument:
`dispatch::handle_request` does at most one existing `wm-core` mutator
call (already NFR1-budget-accepted by the stories that introduced them)
plus, for `get-state`, one `O(tags + views + outputs)` snapshot build (all
bounded — ≤64 tags per ADR-006, and view/output counts are inherently
small for a single-user desktop session) — no I/O, no unbounded work. The
mutex is held only for that one call, not across any socket
read/write. The socket read/write itself is local Unix-domain-socket I/O
(no network stack, no syscall round-trip comparable to a real network
hop) plus `serde_json`'s well-understood linear-time parse/serialize cost
on messages capped at 64 KiB.

**NFR2 (panic-free) — summary of this story's four independent layers.**
(1) `wm-core`'s own mutators are already `Result`-based and panic-free by
construction (Epic 1's existing invariant, unchanged here). (2)
`parse_request`/`handle_request`/`serialize_response` are unit-tested
against an explicit malformed-input suite (Tasks 3.2, 4.1) proving they
return `Err`/`Response::Error` rather than panicking. (3)
`lock_recovering` prevents one thread's hypothetical panic from cascading
into every other locker crashing (Task 2). (4) `catch_unwind` around each
connection's handling loop contains a hypothetical panic to that one
connection only (Task 6.2). No single layer is asked to be the *only*
thing standing between a malicious client and a WM crash.

No new ADR. Checked `adrs.md` — nothing governs socket path conventions,
exact IPC field-naming, threading model, or the malformed-vs-application-
error connection-handling distinction; this story's four resolved gaps
(Description, above) are implementation decisions filling documented gaps,
not decisions among architecturally significant alternatives an ADR would
need to record — consistent with how Stories 1.3/1.5/1.6/1.7 judged their
own similarly-scoped gap-filling decisions.

Build/test/lint only ever run **inside the devcontainer via devpod**
(`devpod ssh buoy-wm -- cargo ...`), falling back to direct `podman exec`
against the running container if `devpod ssh`'s transport is still broken
(Epic 1 retro action item #3 asks this be sanity-checked before this story
starts) — same constraint as every prior story.

## Test plan
This story's tests split into three tiers:

1. **`wm_core` decision logic** (Task 1) — Rust unit tests added to
   `wm/src/wm_core/state.rs`'s existing test module, run via `cargo test
   --manifest-path wm/Cargo.toml` inside the devcontainer: `WmCore::snapshot`
   (8 tests) — empty-core case; tag/view/output inclusion and correct
   per-view tag-membership computation; focused-view reporting (`Some`/
   `None`); deterministic ascending-id ordering for views/outputs (`HashMap`
   iteration order is otherwise unspecified); and a pure-query no-mutation
   guard, matching `closable_focused_view`'s existing precedent.
2. **`ipc` module pure logic** (Tasks 2-5) — Rust unit tests in each new
   module's own `#[cfg(test)] mod tests`, run the same way, with real
   `WmCore`/`Mutex` instances but **no sockets**:
   - `lock_recovering` (Task 2, 2 tests): normal-guard case; poisoned-guard
     recovery after a real induced panic on another thread — the direct
     proof this story's mutex-poisoning mitigation actually works, not just
     that the code compiles.
   - `ipc::protocol` (Task 3, ~16 tests): all four request variants parse
     correctly; all four response variants serialize to the exact
     documented JSON; and a deliberately broad malformed-input sweep —
     empty input, non-JSON garbage, valid-JSON-wrong-shape (array/scalar
     instead of object), unknown `type`, missing required fields, wrong
     field types, an out-of-`u8`-range `tag_id`, deeply nested/oversized
     JSON values, and invalid UTF-8 bytes — every case asserting `Err`,
     never a panic.
   - `ipc::dispatch` (Task 4, ~10 tests): each of the three mutation
     commands both succeeds (with the real `wm-core` side effect verified,
     not just the `Response` variant) and fails correctly for unknown ids;
     `create-tag`'s registry-cap case asserts the exact literal error
     string Story 2.3's picker will render; `get-state` is asserted to be a
     thin wrapper over `WmCore::snapshot()`; and an explicit
     never-panics sweep across every (valid, bogus) id combination for
     all three mutation variants.
   - `resolve_socket_path` (Task 5, 4 tests): `XDG_RUNTIME_DIR`-present
     case; the `USER`→`LOGNAME`→literal `"unknown"` fallback chain, each
     level exercised independently.
3. **`ipc::server` integration tests over real sockets** (Task 6, ~10
   tests) — **not** a build+clippy+manual-review-only carve-out, unlike
   `main.rs`'s Wayland `Dispatch` glue: this is the one I/O boundary in
   this story that has no live-compositor dependency and is fully
   exercisable with real `UnixListener`/`UnixStream` sockets against
   temp paths inside this sandbox. Covers: a full request/response round
   trip; a shared-state round trip proving the same `Arc<Mutex<WmCore>>`
   is genuinely shared across multiple connections/requests; the
   malformed-input-closes-connection behavior (garbage JSON, oversized
   unterminated line, invalid UTF-8); the well-formed-but-`wm-core`-error
   case keeping the connection open (the AC's explicit reset-vs-stays-open
   distinction); one malformed connection not affecting a concurrent good
   one; stale-socket-file cleanup on startup; and owner-only (`0600`)
   socket file permissions.
4. **`main.rs` wiring** (Task 7) — **not** covered by automated unit tests,
   same boundary every prior story's Wayland-glue tasks have hit (real
   `wayland_client`/`river-window-management-v1` `Dispatch` types, no live
   protocol connection in this devcontainer sandbox): the `Arc<Mutex<
   WmCore>>` field-type change and every one of its enumerated call-site
   updates (Task 7.2's explicit list), and `main()`'s IPC-server-spawn call
   (Task 7.3). Verified via `cargo build`/`cargo clippy --all-targets -D
   warnings` succeeding, the full pre-existing 91-test `wm_core` suite (99
   after Task 1) staying green with zero call-site behavioral changes, and
   structural code review against Task 7.2's explicit per-call-site
   checklist — the specific verification step the Epic 1 retrospective's
   action item #1 asks for on any story touching a shared `wm-core` call
   site, applied here even though these are pre-existing call sites being
   mechanically relocked, not new ones.
5. **Dead-code hygiene** (Task 8) — `cargo build`/`cargo clippy -D
   warnings` output itself is the test: `toggle_view_tag` and
   `tag_set.rs`'s allow markers must show exactly their new, accurate
   state, nothing broader.
6. **Tooling gate** (Task 9) — `cargo fmt --check`, `cargo clippy -D
   warnings`, and the full `cargo test` run (91 pre-existing + this
   story's ~50 new tests across Tasks 1-6, all green — roughly 141 total,
   exact count depends on the malformed-input suites' final test-fn
   granularity), all in-container; `pre-commit run --all-files` as a final
   sanity check.

Remaining, explicitly-flagged coverage gap: the *combination* of a live
`river` compositor session managing real windows **and** a real companion
client talking to the socket at the same time is not exercised in this
sandbox (no live `river` session available, same class of gap every prior
story has documented) — only the socket protocol in isolation (tier 3,
against a real but test-constructed `WmCore`) and the Wayland glue in
isolation (tier 4, build+clippy+review only) are covered separately. This
is a smaller gap than Epic 1's own Wayland-only gap, not a new one of the
same size, since the socket I/O itself — the actual new attack surface
this story introduces — has real integration-test coverage, not just a
manual-review carve-out.

## FR coverage
FR11, NFR1, NFR2

## Dev Agent Record

### Implementation Plan
Followed the 9-task breakdown exactly, RED before GREEN on every task that
specified it (Tasks 1-6), no RED/GREEN on Tasks 7-8 per the story's own
carve-out (call-site composition / pure hygiene, verified via build+clippy+
structural review instead). Build/test/lint all run in-container via
`podman exec` against the running `bold_vaughan` devcontainer — `devpod ssh
buoy-wm` failed with `Error tunneling to container: wait: remote command
exited without exit status or exit signal` (same failure class the Epic 1
retro's action item #3 asked to be sanity-checked up front), so this story
falls back to direct `podman exec`, same as Story 1.7. `pre-commit run
--all-files` additionally required running as the `vscode` user inside the
container (`podman exec -u vscode ...`), not the default `root` exec user
— root triggered git's "detected dubious ownership" guard against the
`vscode`-owned checkout.

Test count progression: 91 (baseline) → 100 (Task 1, +9 `WmCore::snapshot`
tests — the task text estimated 8, but its own enumerated test list
specifies 9; implemented exactly the enumerated list) → 102 (Task 2, +2
`lock_recovering`) → 120 (Task 3, +18 `ipc::protocol`) → 130 (Task 4, +10
`ipc::dispatch`) → 134 (Task 5, +4 `resolve_socket_path`) → 143 (Task 6, +9
`ipc::server` real-socket integration tests). Final: 143 passed, 0 failed,
across three repeated runs of the `ipc::server` module with no flakiness
observed.

One deviation caught during Task 7 review, not specified verbatim in the
story's per-call-site list: `manage_seats` holds a `MutexGuard` across its
seat loop, then (unchanged from the pre-Story-2.1 code) calls
`self.ensure_pinned_terminal_spawned(tag_id)` after the loop for any
tag-cycle/tag-create actions that occurred — that method now also calls
`ipc::lock_recovering(&self.wm_core)` internally. Left as originally
implemented (guard held until function end via NLL), this would deadlock
`std::sync::Mutex` (not reentrant) on the very first `TagCycle`/`TagCreate`
keypress. Fixed by adding an explicit `drop(wm_core_guard);` immediately
after the seat loop, before the `pending_terminal_spawns` loop. Flagged
here for code review per the Epic 1 retro's action item #1 (shared
`wm-core` call sites get extra scrutiny) — this is exactly the class of
bug that review should double-check independently.

### Completion Notes
- All 9 tasks complete; all subtasks checked off in this file.
- `wm_core::state::WmCore::snapshot()` added as the one new `wm-core`
  read-only query this story introduces (`wm/src/wm_core/state.rs`), with
  four new plain-data structs (`TagSnapshot`/`ViewSnapshot`/
  `OutputSnapshot`/`WmCoreSnapshot`), no `serde` derives.
- New `wm/src/ipc/` module: `mod.rs` (`lock_recovering`), `protocol.rs`
  (`Request`/`Response`/DTOs/`parse_request`/`serialize_response`),
  `dispatch.rs` (`handle_request`, the pure decision layer), `server.rs`
  (`resolve_socket_path`/`default_socket_path`/`spawn`/
  `handle_connection`).
- `main.rs` wired: `WindowManager.wm_core` is now `Arc<Mutex<WmCore>>`;
  every pre-existing call site relocked via `ipc::lock_recovering` per the
  story's explicit enumerated checklist (verified via a full `grep
  wm_core` re-read — every match accounted for); IPC server spawned in
  `main()` after the Wayland global checks, failure logged not fatal.
- Hygiene: `toggle_view_tag`'s `#[allow(dead_code)]` removed (now reached
  via `ipc::dispatch::handle_request`); `tag_set.rs`'s module-level
  `#![allow(dead_code)]` narrowed to just `TagSet::empty()` (the one
  constructor still genuinely unreached — `register_view` still uses
  `Default::default()`), matching `TagRegistry::new()`'s existing
  narrow-allow precedent.
- Malformed-input robustness (NFR2) exercised at three independent layers
  per the story's own accounting: `ipc::protocol`'s parse tests (empty
  input, non-JSON garbage, wrong-shape JSON, unknown type, missing/
  wrong-typed fields, out-of-range `tag_id`, deeply-nested extra fields,
  invalid UTF-8 — all `Err`, never panic); `ipc::dispatch`'s
  never-panics id-sweep; `ipc::server`'s real-socket tests (malformed
  JSON, oversized unterminated line, invalid UTF-8 bytes, one malformed
  connection not affecting a concurrent good one) — all passing against
  real `UnixListener`/`UnixStream` sockets, not mocked.
- Manual end-to-end smoke test (live `river` session + a real socket
  client talking to the WM while it manages real windows) was **not**
  performed — no live `river` compositor session is available in this
  sandbox, the same class of gap every prior Epic 1 story documented for
  its Wayland glue. Per the story's own framing this is a narrower gap
  than Epic 1's: the socket I/O itself (this story's actual new attack
  surface) has real integration-test coverage (Task 6, 9 tests against
  genuine `UnixListener`/`UnixStream` sockets); only the *combination*
  with a live compositor session remains unexercised.
- `cargo fmt --check` initially failed after Task 6/7's new code (long
  lines the RED-phase drafts hadn't been formatted yet); resolved by
  running `cargo fmt --all` once, then re-verifying `--check` clean. No
  logic changes from the formatting pass.

### File List
- `wm/Cargo.toml` (added `serde`, `serde_json` dependencies)
- `wm/Cargo.lock` (lockfile update from the above)
- `wm/src/main.rs` (`mod ipc;`, `Arc`/`Mutex` import, `WindowManager.wm_core`
  field type change, every enumerated call site relocked, IPC server spawn
  in `main()`)
- `wm/src/wm_core/state.rs` (`TagSnapshot`/`ViewSnapshot`/`OutputSnapshot`/
  `WmCoreSnapshot`, `WmCore::snapshot()`, `#[allow(dead_code)]` removed from
  `toggle_view_tag`, new snapshot tests)
- `wm/src/wm_core/tag_set.rs` (module-level `#![allow(dead_code)]` narrowed
  to `TagSet::empty()` only)
- `wm/src/ipc/mod.rs` (new — `lock_recovering`, module scaffold)
- `wm/src/ipc/protocol.rs` (new — `Request`/`Response`/DTOs/
  `parse_request`/`serialize_response`)
- `wm/src/ipc/dispatch.rs` (new — `handle_request`,
  `impl From<WmCoreSnapshot> for Response`, `describe_wm_core_error`)
- `wm/src/ipc/server.rs` (new — `resolve_socket_path`/
  `default_socket_path`/`spawn`/`handle_connection`/`write_response`)

## Change Log
- 2026-08-07: Implemented Story 2.1 (Tasks 1-9) — IPC server foundation:
  `WmCore::snapshot()`, the `wm/src/ipc/` module (protocol/dispatch/
  server), `main.rs` wiring to `Arc<Mutex<WmCore>>`, and dead-code hygiene.
  91 → 143 tests, all green; `cargo fmt --check`/`cargo clippy --all-targets
  -D warnings`/`pre-commit run --all-files` all clean.
