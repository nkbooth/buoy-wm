---
baseline_commit: 085154ac0b1892b622a57d1c930ac0d587d4f3c9
---

# Story 1.1: Devcontainer & CI Scaffolding (project bootstrap)
Epic: 1 | Priority: H | Status: done

## Description
Bootstraps the project from the `tinyrwm` Rust reference implementation and
stands up the devcontainer, pre-commit hooks, and CI workflow that every
subsequent story builds on. This is also buoy-wm's starter-template setup
story (ADR-008) — devcontainer/CI scaffolding and starter-template seeding
are one story, not two. Closes the implementation-readiness infrastructure
gate (see `_bmad-output/planning-artifacts/implementation-readiness-report-2026-08-06.md`)
— no other story in Epic 1 can start until this one is done, since none of
them have a Rust toolchain to build against otherwise.

## Acceptance criteria
**Given** a fresh clone of `buoy-wm`
**When** the devcontainer is opened via devpod
**Then** `cargo build` succeeds inside it with no host-installed Rust toolchain
**And** `.pre-commit-config.yaml` exists and runs `cargo fmt --check` and `cargo clippy` on commit
**And** a CI workflow file exists that runs `cargo build` and `cargo test` on push
**And** the Cargo workspace is rooted at `buoy-wm/wm/` per ADR-008, seeded from the `tinyrwm` Rust reference implementation

- [x] Tests pass (unit + integration where applicable) — no unit tests added (pure scaffolding, per Test plan); all 4 tooling/process verifications in the Test plan pass.
- [x] Code review: PASS

## Tasks / Subtasks

- [x] **Task 1: Seed the Cargo project at `wm/` from tinyrwm's Rust reference implementation (AC: 1, 4)**
  - [x] 1.1 RED — Inside the devcontainer via devpod (`devpod ssh buoy-wm -- cargo build --manifest-path wm/Cargo.toml`), confirm the build fails because `wm/Cargo.toml` does not exist yet. Record the failure (missing manifest) as the starting red state.
  - [x] 1.2 GREEN — Vendor `codeberg.org/river/tinyrwm`'s `rust/` directory (`Cargo.toml`, `src/`, `protocol/`, `.gitignore`) into `wm/` at a pinned commit (record the commit hash in the PR/commit message for provenance). Rename the package from `tinyrwm` to `buoy-wm` in `wm/Cargo.toml`. Keep the upstream 0BSD SPDX headers on carried-over files — 0BSD imposes no attribution obligation but preserving the notice keeps provenance honest. Re-run `cargo build --manifest-path wm/Cargo.toml` inside the devcontainer via devpod; confirm it now succeeds.

- [x] **Task 2: Devcontainer scaffolding, opened via devpod (AC: 1)**
  - [x] 2.1 RED — Run `devpod up .` (or `devpod ssh buoy-wm`) against the repo before `.devcontainer/devcontainer.json` exists; confirm devpod reports no devcontainer configuration found.
  - [x] 2.2 GREEN — Add `.devcontainer/devcontainer.json` (Rust base image + a `postCreateCommand`/Dockerfile layer installing `libwayland-dev` and `pkg-config`, required to link `wayland-client`/`wayland-backend`). Run `devpod up .` and confirm it succeeds; from inside (`devpod ssh buoy-wm`) confirm `cargo build --manifest-path wm/Cargo.toml` succeeds. Confirm the host has no working `cargo` (e.g. `which cargo` fails or is absent on the host shell) to validate the "no host toolchain" clause of the AC.

- [x] **Task 3: Pre-commit hooks — `cargo fmt --check` and `cargo clippy` (AC: 2)**
  - [x] 3.1 RED — Add `.pre-commit-config.yaml` with local hooks running `cargo fmt --manifest-path wm/Cargo.toml --all -- --check` and `cargo clippy --manifest-path wm/Cargo.toml --all-targets -- -D warnings`. Introduce a deliberate formatting violation and a deliberate clippy violation (e.g. a redundant `.clone()`) in a throwaway edit inside `wm/src`. Run `pre-commit run --all-files` inside the devcontainer via devpod; confirm both hooks fail (red).
  - [x] 3.2 GREEN — Revert the deliberately-bad edit. Re-run `pre-commit run --all-files` inside the devcontainer; confirm both hooks pass (green) against the real seeded code. Note in the config file (as a header comment, matching the pattern used elsewhere in this user's personal repos) that these hooks are in-container only — `pre-commit install` is deliberately *not* run on the host, since neither `pre-commit` nor `cargo` exist there and the git-hook shim would fail closed.

- [x] **Task 4: CI workflow — `cargo build` and `cargo test` on push (AC: 3)**
  - [x] 4.1 RED — Author `.github/workflows/ci.yml` (GitHub Actions — see Technical notes) with a job that checks out the repo and runs `cargo build --manifest-path wm/Cargo.toml` and `cargo test --manifest-path wm/Cargo.toml`. Using `act` inside the devcontainer, run the workflow against a throwaway commit carrying the same deliberately-bad formatting/clippy edit from Task 3.1; confirm the job fails (red).
  - [x] 4.2 GREEN — Run `act` against the reverted/clean commit; confirm the job passes (green).

- [x] **Task 5: Fresh-clone end-to-end verification (AC: 1, 2, 3, 4)**
  - [x] 5.1 Clone the repo to a scratch directory, open it via `devpod up .`, and confirm `cargo build --manifest-path wm/Cargo.toml` succeeds inside the container with no host Rust toolchain involved.
  - [x] 5.2 Confirm `.pre-commit-config.yaml` and the CI workflow file both reference `wm/Cargo.toml` (not a stale/renamed path) and that the workspace root matches ADR-008 (`buoy-wm/wm/`).
  - [x] 5.3 Re-check the infrastructure gate table in `_bmad-output/planning-artifacts/implementation-readiness-report-2026-08-06.md` — devcontainer, CI workflow, and pre-commit rows should now read present (informational confirmation only; no separate code change).

## Technical notes
- Build/test/lint only ever run **inside the devcontainer via devpod** — `cargo build`, `cargo test`, `cargo fmt`, `cargo clippy` are never assumed to run on the bare host, and no task step in this story invokes them outside that boundary. Every RED/GREEN verification above is phrased as an in-container command for this reason.
- Secrets infrastructure (`.env.template`, gitleaks) is waived for this project — no secrets in the architecture (`_bmad/custom/bmad-check-implementation-readiness.toml`). Do not add hooks or CI jobs for either.
- This story closes buoy-wm's implementation-readiness infrastructure gate (devcontainer, CI, pre-commit) — see `_bmad-output/planning-artifacts/implementation-readiness-report-2026-08-06.md`. No other Epic 1 story can be implemented until it lands, since `wm/` (the Cargo project every later story edits) doesn't exist before Task 1.
- `wm/Cargo.toml` starts as a single package (not yet a multi-member `[workspace]` manifest) — the `wm-core` / `protocol-client` / `placement-engine` / `ipc-server` / `tag-picker` / `status-bar` split described in `docs/planning/architecture/components.md` begins in Story 1.2 onward. Converting `wm/Cargo.toml` to a `[workspace]` with members is that later story's job, not this one's (YAGNI — don't build the multi-crate layout before a second crate exists).
- tinyrwm's `rust/Cargo.toml` declares `edition = "2024"`; `docs/planning/architecture/technology-stack.md` says "Rust (2021 edition)". Keep the seeded `2024` edition as-is (matching upstream reduces day-one merge friction and 2024 is the current default for new Cargo projects) rather than downgrading it — this is a doc-drift note for a human to reconcile in `technology-stack.md`, not a story blocker.
- tinyrwm's Rust implementation currently declares these dependencies (as of the pinned commit vendored in Task 1.2): `bitflags`, `wayland-backend`, `wayland-client`, `wayland-scanner`. Carry them over unmodified; do not add unrelated dependencies in this story.
- CI provider: **GitHub Actions** (`.github/workflows/ci.yml`), confirmed by the user 2026-08-07 — no GitHub remote exists yet, and creating/pushing to one is out of scope for this story (a separate, deliberate action). CI RED/GREEN verification in Task 4 runs the workflow locally via `act` inside the devcontainer instead of a real push; a real remote can be wired up later without changing the workflow file.

## Test plan
No unit tests are added by this story — it is pure scaffolding, and the seeded `tinyrwm` binary carries no application logic of buoy-wm's own yet (state-model logic and its first unit tests begin in Story 1.2's `wm-core`). Verification here is at the tooling/process level, and every check below runs only inside the devcontainer via devpod:

1. **Devcontainer smoke test** — `devpod up .` then `cargo build --manifest-path wm/Cargo.toml` on a fresh clone; must succeed with zero Rust toolchain installed on the host.
2. **Pre-commit hook verification** — `pre-commit run --all-files`, exercised against the RED/GREEN pair from Task 3 (deliberately-bad edit fails the hook; reverted code passes it).
3. **CI verification** — the CI workflow run against the same RED/GREEN pair from Task 4 (bad commit fails the `cargo build`/`cargo test` job; clean commit passes it).
4. **`cargo test --manifest-path wm/Cargo.toml`** inside the devcontainer — tinyrwm ships no test suite today, so this is expected to report zero tests and exit 0. Note this explicitly in the story so a future reader doesn't mistake "0 tests ran" for a broken CI step; a real regression here is a non-zero exit code, not a zero test count.

## FR coverage
Bootstrap story — no functional requirement directly, unblocks all of Epic 1.

## Dev Agent Record

### Debug Log

- **Task ordering (Task 1 vs Task 2).** Task 1.1's RED step calls for `devpod ssh buoy-wm -- cargo build ...`, but no devcontainer exists until Task 2. Resolved per the story's own Technical notes allowance: Task 1.1's RED was confirmed via the filesystem fact (`wm/Cargo.toml` absent) plus an actual `devpod ssh buoy-wm` attempt, which failed with `workspace buoy-wm doesn't exist` — consistent evidence devpod cannot be used yet. Task 1.2's file authoring (vendor + rename) was done host-side with no toolchain invocation; its in-container `cargo build` GREEN check was performed once Task 2.2 built the devcontainer (same command satisfies both).
- **Task 2.1 RED didn't match the literal prediction.** `devpod up .` before `.devcontainer/devcontainer.json` exists did not error with "no devcontainer configuration found" as the story predicted — devpod auto-detected a fallback Python devcontainer and succeeded (exit 0). Substituted RED evidence: `cargo` was absent in that wrong auto-detected workspace (`cargo: command not found`, exit 127), which is the real failure the AC cares about. The wrong workspace was deleted (`devpod delete buoy-wm --force`) before building the real one.
- **Pre-existing lint/fmt issues in vendored tinyrwm code.** Under this devcontainer's toolchain (rustc/clippy 1.97.1), the vendored `wm/src/main.rs` (untouched, pinned commit `04a3f9f5b17cbe3851e56d2a1b962426a9d0a850`) fails both `cargo fmt --check` (one import-order diff) and `cargo clippy -- -D warnings` (one `collapsible_if`). Since Task 3.2's GREEN state requires both hooks to pass "against the real seeded code," fixed both: ran `cargo fmt` once to normalize the import order, and added a narrow `#[allow(clippy::collapsible_if)]` with a comment at the one call site. No logic changed.
- **Task 4.1's RED fixture needed to differ from Task 3.1's.** CI (`.github/workflows/ci.yml`) only runs `cargo build` + `cargo test`, not fmt/clippy (that's Task 3's pre-commit job) — so reusing Task 3.1's fmt/clippy-only violation would not fail the CI job as written. Used a genuine compile-breaking throwaway edit instead (a call to an undefined function) to produce a real RED for *this* job, then reverted it for GREEN.
- **`act` required real infrastructure work beyond the story's Technical notes.** This host runs rootless Podman (not Docker Engine) as its container runtime; `docker` on the host is a Podman compat shim. Docker-in-Docker inside the devcontainer failed outright (`/sys/kernel/security` mount denied under the nested user namespace). The `docker-outside-of-docker` devcontainer feature also failed twice: once because its `moby` option isn't packaged for Debian trixie (fixed with `"moby": false`), and again because its default source-socket mount is the host's `/var/run/docker.sock`, which on this host is a root-owned symlink to the *rootful* Podman socket (`statfs: permission denied` for an unprivileged user) — the feature ignores a top-level `"mounts"` override for this. Resolved by bypassing the feature: bind-mount the host's own rootless-Podman user socket (`/run/user/1000/podman/podman.sock`) into the container and point `DOCKER_HOST` at it directly, installing just `docker-ce-cli` (no daemon) via apt. `act` itself isn't packaged for Debian; installed via nektos/act's official pinned-version install script (`v0.2.89`). Added `.actrc` to pin the runner image (avoids an interactive first-run prompt over a non-TTY SSH exec) and to pass `--container-daemon-socket -` (act's default job-container socket bind-mount otherwise collides with the same nested-mount problem, since our workflow's steps never need Docker themselves).
- **Operational caveat, not a repo fix:** `devpod ssh <workspace> --command ...` re-injects `~/.docker/config.json` with `"credsStore": "devpod"` on every invocation (devpod's own credential-forwarding behavior, unrelated to devcontainer.json). That credential helper isn't reachable in this non-interactive exec context, which breaks anonymous image pulls. Workaround used throughout for `act` runs: `echo '{"auths":{}}' > ~/.docker/config.json && act ...` in the same SSH command. This is devpod-session-specific, not something devcontainer.json can fix, so it's not encoded as a repo file — noted here for whoever runs `act` next.
- **Host has a pre-existing `cargo`.** `which cargo` on the host resolves to `/home/linuxbrew/.linuxbrew/bin/cargo` (a Homebrew install, pre-existing, unrelated to this story — not installed by any task here). This means Task 2.2's literal validation step ("confirm the host has no working cargo") does not hold true on this machine. The AC's actual intent — that `cargo build` succeeds inside the devcontainer *without depending on* a host toolchain — was verified directly instead: every build in this story ran exclusively via `devpod ssh`/`cargo build --manifest-path wm/Cargo.toml`, never touching the host's cargo. Flagging this as a fact for the record, not a story blocker — nothing in this story installed or relies on the host cargo, and removing an unrelated pre-existing host tool would be out of scope and destructive to other projects on this machine.

### Completion Notes

- Vendored `codeberg.org/river/tinyrwm`'s `rust/` directory into `wm/` at pinned commit `04a3f9f5b17cbe3851e56d2a1b962426a9d0a850`; renamed the package `tinyrwm` → `buoy-wm` in `wm/Cargo.toml` only, per the task's explicit scope (left `wm/README.md`'s prose/binary-name references as-vendored — out of scope for this story's rename instruction).
- Devcontainer: `mcr.microsoft.com/devcontainers/rust:1` + `libwayland-dev`/`pkg-config` (link deps) + `pre-commit` + `docker-ce-cli` + `act` (pinned `v0.2.89`), with the host's rootless-Podman socket bind-mounted in for `act`'s use (see Debug Log).
- `.pre-commit-config.yaml`: local hooks for `cargo fmt --check` and `cargo clippy -D warnings`, in-container only (no `pre-commit install` on the host), header comment styled after this user's other personal repos (`n1cck-netlogger`'s pattern).
- `.github/workflows/ci.yml`: GitHub Actions, `cargo build` + `cargo test` on push/PR, verified locally via `act` (no GitHub remote exists — out of scope per the story).
- All RED/GREEN pairs for Tasks 1–4 confirmed with actual command output (see final report to the user for the full transcript summary). Task 5 fresh-clone verification performed against a separate local clone in the scratch directory, opened as its own devpod workspace (`buoy-wm-freshclone`), then torn down after verification.
- Two one-line, behavior-preserving lint fixes were made to the otherwise-unmodified vendored `wm/src/main.rs` (import order via `cargo fmt`, and an `#[allow(clippy::collapsible_if)]`) — required to make Task 3.2's explicit "both hooks pass against the real seeded code" GREEN state achievable; see Debug Log for why.
- Working tree left uncommitted (staged) per this user's commit-hygiene preference (commit only when explicitly asked) — Nick can review and commit directly.

### Code Review Follow-up (2026-08-07)

Four code-review findings addressed:

- **`wm/.gitignore` ignored `Cargo.lock`.** `buoy-wm` is a binary crate (Cargo's own guidance: commit the lockfile for binaries), and this story's whole point is reproducible builds. Removed the `Cargo.lock` line (inherited verbatim from vendored tinyrwm). Regenerated `wm/Cargo.lock` via `cargo build --manifest-path wm/Cargo.toml` inside the devcontainer and staged it.
- **`wm/README.md` had stale `tinyrwm` references post-rename.** `wm/Cargo.toml` renames the package to `buoy-wm` with no `[[bin]]` override, so the release binary is `./target/release/buoy-wm`. Updated the title (`# tinyrwm.rust` → `# buoy-wm`) and the `river -c ...` example path to match.
- **`.pre-commit-config.yaml` hooks only triggered on `\.rs$`.** A manifest-only change (e.g. a dependency bump) never ran `cargo fmt`/`cargo clippy` locally, and CI doesn't run them either — zero lint safety net for `Cargo.toml`/`Cargo.lock`-only commits. Broadened both hooks' `files:` pattern to `^wm/(src/.*\.rs|Cargo\.(toml|lock))$`, with a one-line comment explaining why.
- **`.devcontainer/devcontainer.json`'s `postCreateCommand` hardcoded Debian suite `trixie`** for the Docker apt repo, coupling it to the current `rust:1` base image's OS release. Replaced the hardcoded `trixie` with `$VERSION_CODENAME` sourced from `. /etc/os-release`, so a future base-image retag to a different Debian release doesn't 404 `apt-get update`.

Re-verified inside the devcontainer via `devpod ssh buoy-wm`: `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `pre-commit run --all-files`, and `act push -j build-and-test` all pass. `act`'s first attempt failed at the image-pull step (`Error retrieving credentials: ... dial tcp [::1]:12049: connect: connection refused`) — the same pre-existing `credsStore: devpod` issue already documented above under "Operational caveat, not a repo fix" (unreachable over a non-interactive `devpod ssh --command` exec, unrelated to these fixes). Applying the documented workaround (`echo '{"auths":{}}' > ~/.docker/config.json`) before the `act` invocation made it pass clean, confirming `buoy-wm v0.1.0` builds, `cargo test` reports 0 tests/exit 0 as expected, and the job succeeds end-to-end.

## File List

- `.devcontainer/devcontainer.json` (new)
- `.pre-commit-config.yaml` (new)
- `.github/workflows/ci.yml` (new)
- `.actrc` (new)
- `wm/Cargo.toml` (new — vendored from tinyrwm, package renamed to `buoy-wm`)
- `wm/Cargo.lock` (new — generated in-container; code review follow-up, see Dev Agent Record)
- `wm/.gitignore` (new — vendored, code review follow-up: `Cargo.lock` ignore rule removed)
- `wm/README.md` (new — vendored, code review follow-up: `tinyrwm` → `buoy-wm` title and binary path)
- `wm/protocol/river-window-management-v1.xml` (new — vendored, unmodified)
- `wm/protocol/river-xkb-bindings-v1.xml` (new — vendored, unmodified)
- `wm/src/main.rs` (new — vendored, two behavior-preserving lint fixes: import order, `#[allow(clippy::collapsible_if)]`)
- `.pre-commit-config.yaml` (code review follow-up: `files:` filter broadened to include `Cargo.toml`/`Cargo.lock`)
- `.devcontainer/devcontainer.json` (code review follow-up: Debian suite derived from `/etc/os-release` instead of hardcoded `trixie`)
- `docs/planning/epics/story-1-1.md` (modified — frontmatter `baseline_commit`, task checkboxes, Dev Agent Record, File List, Change Log, Status)

## Change Log

- 2026-08-07: Story implemented end-to-end (Tasks 1–5) via `bmad-dev-story`. Devcontainer, pre-commit, and CI infrastructure gate closed; `wm/` seeded from tinyrwm. See Dev Agent Record for RED/GREEN details and environment-specific deviations from the story's literal Technical notes (all within the story's pre-authorized scope).
- 2026-08-07: Code review follow-up — 4 findings addressed (`Cargo.lock` now tracked, `wm/README.md` de-`tinyrwm`'d, pre-commit `files:` filter broadened to catch manifest-only changes, devcontainer Docker-repo suite made base-image-drift-safe). See Dev Agent Record → Code Review Follow-up.
