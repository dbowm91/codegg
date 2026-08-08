# Post-Audit Correctness, Simplification, and Footprint Milestone 005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/post-audit-correctness-simplification/005-routine-ci-and-static-guard-simplification.md`

Source subsystem roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Implementation commit:

- `0993d953e777cad9a8997b5e62520fcf4648b62e` — simplify routine CI verification

## 1. Executive finding

M005 is complete and closed. Routine verification remains one bounded `verify`
job with formatting, Clippy, workspace tests, generated-agent validation, and
the high-value core-boundary, sandbox, and execution-ownership guards. The
obsolete Tokio flavor scanner/baseline, YAML parser ownership guard, duplicate
hosted workspace check, and unconditional guard self-tests were removed without
adding a lane, matrix, audit, release, artifact, coverage, or benchmark gate.

The one-line `Option::is_none_or` use introduced by the preceding TUI
milestone was also made Rust 1.81-compatible so the required Clippy gate is
green. This does not change runtime behavior.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Remove invalid Tokio flavor machinery coherently | Deleted `scripts/check-tokio-test-flavors.py`, `scripts/tokio-test-flavor-baseline.txt`, and its dedicated tests; no current-tree references remain | pass | Bare tests were not mechanically rewritten |
| Correct Tokio default documentation | `architecture/testing.md`, `AGENTS.md`, and `architecture/overview.md` now describe bare `#[tokio::test]` as current-thread by default | pass | Upstream evidence is the official [Tokio test macro documentation](https://docs.rs/tokio/latest/tokio/attr.test.html) |
| Remove duplicate hosted compile check | `.github/workflows/ci.yml` removes `cargo check --workspace --all-targets --locked`; Clippy retains the exact same workspace/all-targets/locked coverage | pass | `scripts/verify.sh quick` retains the standalone check |
| Remove unconditional guard self-tests | CI and `verify.sh quick` no longer invoke sandbox or execution-ownership `--self-test`; both implementations remain manually callable and pass | pass | Normal production-tree guards remain in CI and quick verification |
| Classify remaining static guards | Generated-agent, core-boundary, sandbox, and execution-ownership checks remain hosted; YAML parser guard is deleted | pass | YAML ownership is compiler/Cargo-constrained by `codegg-config`'s sole parser dependency and public facade |
| Preserve one-job/manual-release posture | Workflow has one `verify` job and no release, matrix, artifact, audit, coverage, benchmark, or new lane changes | pass | Manual release posture remains documented |
| Keep local quick verification functional | `scripts/verify.sh quick` | pass | Final run completed successfully |
| Required hosted-equivalent correctness gates are green | Clippy, focused messages tests, and capped workspace tests with CI stack environment | pass | Workspace result: 9,686 passed, 10 ignored across 189 suites |

## 3. Production implementation evidence

The production-facing implementation is limited to verification and
documentation surfaces:

- `.github/workflows/ci.yml` now runs generated-agent checks, the core-boundary
  guard, regular sandbox and execution-ownership guards, formatting, Clippy,
  and serial workspace tests in one job.
- `scripts/verify.sh quick` retains fast workspace compilation and the regular
  high-value guards, while no longer running obsolete or self-test-only checks.
- The Tokio scanner, its historical baseline, dedicated test module, and YAML
  parser boundary guard were removed.
- `architecture/testing.md`, `architecture/overview.md`, and `AGENTS.md` now
  describe the actual CI and test-runtime behavior.
- `src/tui/components/messages.rs` uses a Rust 1.81-compatible equivalent of
  `is_none_or` so the hosted Clippy gate does not reject the existing logic.

No storage, protocol, runtime authority, scheduler, feature, release, or
user-facing product behavior changed.

## 4. Verification executed

### Commands run

```bash
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --lib tui::components::messages
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_execution_ownership.py --self-test
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
RUST_MIN_STACK=33554432 CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
```

### Results

- `scripts/verify.sh quick`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `cargo test --lib tui::components::messages`: 28 passed, 4,137 filtered.
- Sandbox and execution-ownership self-tests: pass.
- The uncapped workspace test reproduced the known M006-owned daemon-socket
  stack failure (SIGABRT). This was not hidden or reclassified as success.
- The CI-equivalent workspace test with `RUST_MIN_STACK=33554432` passed with
  9,686 tests passed and 10 ignored across 189 suites in 545.34 seconds.
- `actionlint` was not installed, so no actionlint result is claimed; direct
  workflow inspection and the repository verification commands were used.

## 5. Invariant review

- Routine CI remains one bounded job for pull requests and pushes to `main`.
- Formatting, Clippy warnings/errors, and workspace test failures remain
  merge-visible.
- Generated builtin-agent source/schema drift remains hosted and locally
  checked.
- Core-boundary, sandbox-contract, and execution-ownership invariants remain
  hosted and locally checked.
- The local quick path retains standalone compilation for fast feedback.
- No new direct push expansion, matrix, release publication, audit schedule,
  artifact upload, coverage gate, or benchmark gate was introduced.
- Optional feature, plugin, example, LSP, audit, and cross-platform checks
  remain targeted/manual as documented.

## 6. Failure and recovery review

This milestone changes no runtime state machine, persistence, protocol, or
daemon lifecycle. Verification failures fail closed through shell/Cargo exit
statuses. The uncapped test failure was preserved as evidence of the existing
stack dependency; the passing CI-equivalent run confirms the routine workflow
with its current documented resource environment. No migration or recovery
path is applicable.

## 7. Migration and compatibility review

No migration, configuration, protocol, or user action is required. Deleting the
Tokio/YAML guards changes developer-maintenance tooling only. Explicit
multi-threaded Tokio annotations remain available for tests that need worker
threads; bare annotations retain Tokio's documented current-thread default.
The global `RUST_MIN_STACK` workaround remains intentionally unchanged for
M006 to investigate and remove at the root cause.

## 8. Security review

The sandbox and execution-ownership production-tree guards remain in routine
CI. Their self-tests were removed from routine execution but remain available
for guard maintenance. No permission, process-spawn, command-routing, path,
secret, network, or sandbox enforcement behavior was weakened. The YAML guard
deletion does not expose the parser: `serde_norway` remains owned by
`codegg-config`, and consumers use the typed `parse_yaml` boundary.

## 9. Documentation and operations

Updated:

- `.github/workflows/ci.yml`
- `scripts/verify.sh`
- `architecture/testing.md`
- `architecture/overview.md`
- `AGENTS.md`
- `plans/registry.md`
- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`

The manual maintenance commands remain:

```bash
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_execution_ownership.py --self-test
```

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The daemon-socket test path still requires global `RUST_MIN_STACK=33554432`; without it, the capped workspace run aborts with SIGABRT | Resource workaround remains broader than desired | M006 owns root-cause correction and removal; M005 does not duplicate or rename the workaround |
| low | `actionlint` was unavailable locally | No independent actionlint result | Existing workflow remains directly inspected; obtain hosted workflow evidence in M008 or CI |

No critical or high-severity finding remains for M005.

## 11. Roadmap disposition and future-plan audit

M005 is closed. M006 and M007 were already dependency-ready before this
closure and remain `ready`; M005's soft CI reconciliation dependency is now
satisfied. M008 is not unblocked: its remaining hard dependencies are closure
of M006 and M007, so it remains `blocked` in both the roadmap and registry.
No other registered blocked plan names M005 as a blocker, and no future plan
became newly ready from this closure.

The independent supported-Linux Landlock evidence condition in the runtime
safety workstream remains conditional and is unchanged.

## 12. Registry updates

- The implementation plan is marked implemented with this closure record.
- The subsystem roadmap marks M005 closed and M008 blocked only on M006-M007.
- `plans/registry.md` removes M005 from dependency-ready plans, records it in
  recently closed implementation plans, keeps M006/M007 ready, and leaves M008
  blocked.
