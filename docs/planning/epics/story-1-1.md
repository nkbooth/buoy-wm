# Story 1.1: Devcontainer & CI Scaffolding (project bootstrap)
Epic: 1 | Priority: H | Status: pending

## Description
Bootstraps the project from the `tinyrwm` Rust reference implementation and
stands up the devcontainer, pre-commit hooks, and CI workflow that every
subsequent story builds on. This is also buoy-wm's starter-template setup
story (ADR-008) — devcontainer/CI scaffolding and starter-template seeding
are one story, not two.

## Acceptance criteria
**Given** a fresh clone of `buoy-wm`
**When** the devcontainer is opened via devpod
**Then** `cargo build` succeeds inside it with no host-installed Rust toolchain
**And** `.pre-commit-config.yaml` exists and runs `cargo fmt --check` and `cargo clippy` on commit
**And** a CI workflow file exists that runs `cargo build` and `cargo test` on push
**And** the Cargo workspace is rooted at `buoy-wm/wm/` per ADR-008, seeded from the `tinyrwm` Rust reference implementation

- [ ] Tests pass (unit + integration where applicable)
- [ ] Code review: PASS

## Technical notes
- Build/test only inside a devcontainer via devpod — the Rust toolchain is never installed on the host.
- This story closes buoy-wm's implementation-readiness infrastructure gate (devcontainer, CI, pre-commit) — see `_bmad-output/planning-artifacts/implementation-readiness-report-2026-08-06.md`.
- Secrets infrastructure (`.env.template`, gitleaks) is waived for this project — no secrets in the architecture (`_bmad/custom/bmad-check-implementation-readiness.toml`).

## Test plan
TBD during dev loop.

## FR coverage
Bootstrap story — no functional requirement directly, unblocks all of Epic 1.
