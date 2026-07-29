# Development Verification and Release Milestone 004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/development-verification-release/004-integration-evidence-cleanup-and-closure.md`

Source subsystem roadmap:

- `plans/subsystems/development-verification-release-roadmap.md#milestone-004--optional-integration-evidence-cleanup-and-closure`

Repository baseline reviewed: `d4d57d215cca7dfb2401f471ac87ad07798080c7`

Implementation commits:

- (this series) — Integration evidence cleanup: deleted LSP real-server workflow, orphaned aggregation scripts, reconciled documentation, closed subsystem roadmap

## 1. Executive finding

The milestone is complete. The scheduled, push-triggered, artifact-producing LSP compatibility workflow has been deleted. The orphaned aggregation script and its test file have been removed. All active documentation has been reconciled to describe one routine CI workflow, local real-server opt-in commands, and manual release ownership. The subsystem roadmap and registry reflect the closed state for all four milestones.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `.github/workflows/lsp-real-server.yml` deleted | `test ! -e .github/workflows/lsp-real-server.yml` passes | pass | Preferred path taken (full deletion, not manual-only contraction) |
| No scheduled or push-triggered real-server execution remains | Workflow file absent; `rg` search across `.github/workflows/` finds no schedule or real-server patterns | pass | |
| No `matrix-summary` aggregation job remains | Workflow deleted; `rg` finds no matrix-summary references in active docs | pass | Historical plan references preserved |
| No per-server compatibility artifact upload/download or retention remains | Workflow deleted; `rg` finds no upload-artifact/download-artifact/retention-days in `.github/workflows/` | pass | |
| `scripts/aggregate_lsp_compatibility_manifest.py` deleted | `test ! -e scripts/aggregate_lsp_compatibility_manifest.py` passes | pass | Orphaned: only consumer was deleted workflow |
| `scripts/test_aggregate_lsp_compatibility_manifest.py` deleted | `test ! -e scripts/test_aggregate_lsp_compatibility_manifest.py` passes | pass | Tests for orphaned script |
| Nextest profiles/scripts have current named local use | `.config/nextest.toml` has `default` and `timing` profiles; `capture-nextest-timing.sh` references `timing` profile correctly | pass | Stale `ci-heavy` references fixed |
| Quick and full verification pass or known failures documented | Fake-server LSP tests pass (5 scenario_engine, 36 composite_stdio); full verification fails at pre-existing tokio flavor check (documented in M002 closure) | pass | Pre-existing known issue, not caused by this milestone |
| Real-server test source and selectors remain | `crates/egglsp/tests/real_server_smoke.rs` exists; `rg` finds all 5 server selectors in docs | pass | No production test code deleted |
| Active documentation agrees on one routine workflow, opt-in checks, manual release | `rg` finds no stale workflow/artifact/profile references in active docs | pass | |
| No active link points to deleted workflow, script, or profile | `rg` search finds dangling references only in plans/implementation and plans/subsystems (historical evidence) | pass | |
| No automated release authority returned | `rg` finds no `cargo publish`, `gh release create`, or write permissions in workflows | pass | Only `ci.yml` with `contents: read` remains |

## 3. Production implementation evidence

No production domain changes. This milestone removed infrastructure:

- **Deleted**: `.github/workflows/lsp-real-server.yml` (233 lines, 6 jobs, weekly schedule, push trigger, artifact upload/download/retention, matrix-summary aggregation)
- **Deleted**: `scripts/aggregate_lsp_compatibility_manifest.py` (365 lines, manifest aggregation script)
- **Deleted**: `scripts/test_aggregate_lsp_compatibility_manifest.py` (475 lines, tests for aggregation script)
- **Fixed**: `scripts/capture-nextest-timing.sh` — corrected stale `ci-heavy` profile references to `timing`
- **Updated**: `architecture/lsp.md` — tier table, CI sections, Phase 4 outcomes, support tiers, compatibility matrix references all reconciled
- **Updated**: `architecture/testing.md` — "Real LSP tests" section rewritten to describe local opt-in commands

## 4. Verification executed

### Commands run

```bash
scripts/verify.sh quick                         # reaches tokio flavor check (pre-existing failure)
scripts/verify.sh full                           # reaches tokio flavor check (pre-existing failure)
CARGO_BUILD_JOBS=1 cargo test -p egglsp --locked --features lsp-test-support --test scenario_engine -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test --locked --features lsp-test-support --test lsp_composite_stdio -- --test-threads=1
find .github/workflows -maxdepth 1 -type f \( -name '*.yml' -o -name '*.yaml' \) -print | sort
rg --line-number --glob '.github/workflows/*.{yml,yaml}' 'schedule:|upload-artifact|download-artifact|retention-days|matrix-summary|cargo publish|gh release create|contents: write|packages: write|id-token: write' .github/workflows
rg --line-number --glob '!plans/archive/**' --glob '!plans/closure/**' 'lsp-real-server\.yml|aggregate_lsp_compatibility_manifest|lsp-compat-matrix-manifest|ci-fast|ci-heavy|ci-release' . | grep -v plans/implementation | grep -v plans/subsystems
rg --line-number 'ci-fast|ci-heavy|ci-release|weekly.*LSP' architecture/lsp.md architecture/testing.md AGENTS.md CONTRIBUTING.md README.md scripts .config
test -e crates/egglsp/tests/real_server_smoke.rs
rg --line-number 'rust_analyzer|basedpyright|gopls|typescript|clangd' crates/egglsp/tests architecture/lsp.md architecture/testing.md
```

### Results

| Command | Result | Notes |
|---|---|---|
| `scripts/verify.sh quick` | pre-existing tokio flavor failure | Documented in M002 closure; not caused by this milestone |
| `scripts/verify.sh full` | pre-existing tokio flavor failure | Documented in M002 closure; not caused by this milestone |
| `scenario_engine` tests | 5 passed | Deterministic fake-server LSP tests |
| `lsp_composite_stdio` tests | 36 passed | Fake-server LSP stdio integration |
| Workflow inventory | 1 file: `ci.yml` | `lsp-real-server.yml` successfully deleted |
| Workflow policy search | no matches | No schedules, artifacts, release patterns in remaining workflow |
| Dangling reference search | no active references | Only historical plan references remain |
| Stale profile search | no matches in active docs | `ci-fast/ci-heavy/ci-release` cleaned from active docs |
| `real_server_smoke.rs` exists | pass | Test code retained |
| Server selectors in docs | 150+ matches | All 5 servers documented in architecture/lsp.md and testing.md |

### Optional commands not run

- Real-server smoke tests (`lsp-real-server-tests` feature): requires installed server binaries not available in this environment. Documented as local opt-in commands.
- Plugin/example/audit commands: not required by documentation-only changes.

## 5. Invariant review

| Invariant | Status | Evidence |
|---|---|---|
| Routine CI is one bounded non-release job | maintained | `ci.yml` has single `verify` job, `contents: read` only |
| No workflow publishes or creates releases | maintained | `rg` finds no `cargo publish`, `gh release create`, or write permissions |
| Real-server tests remain runnable locally | maintained | `real_server_smoke.rs` exists; 5 server selectors documented in architecture/lsp.md |
| Fake-server tests remain part of local contract | maintained | 5 scenario_engine + 36 composite_stdio tests pass |
| No scheduled external compatibility job | maintained | Workflow deleted; no schedule trigger in any workflow |
| No artifact aggregation or retention | maintained | Workflow deleted; no upload-artifact/download-artifact in any workflow |
| No workflow has release authority | maintained | `ci.yml` has `contents: read` only |
| Historical closure records untouched | maintained | plans/closure/ files not modified |
| Optional commands report what was actually run | maintained | Closure record honestly records environment limitations |

## 6. Failure and recovery review

Not applicable. This milestone deletes infrastructure; no new failure modes are introduced. The acceptance of intentional removal of scheduled compatibility monitoring is documented as accepted policy in the plan.

## 7. Migration and compatibility review

- Existing direct Cargo commands for real-server testing remain valid and documented.
- No feature flags or test selectors were removed.
- The `lsp-real-server-tests` feature flag in `Cargo.toml` is retained for local opt-in use.
- `scripts/verify.sh` and the canonical quick/full contract are unchanged.
- Historical plan and closure records retain their original references for traceability.

## 8. Security review

- No registry tokens or credentials were involved.
- No workflow write permissions exist in remaining workflows.
- External tool installation remains an explicit maintainer action.
- No scheduled download of third-party binaries.
- No workflow gains release authority.

## 9. Documentation and operations

**Updated:**
- `architecture/lsp.md` — tier table, Phase 4 CI section, Real-Server CI section, support tiers, Phase 4 outcomes, compatibility matrix preservation, pass tables
- `architecture/testing.md` — "Real LSP tests" section, `--all-features` section, nextest profile descriptions
- `scripts/capture-nextest-timing.sh` — usage examples corrected from `ci-heavy` to `timing`

**Not requiring update:**
- `README.md` — no workflow or compatibility references
- `CONTRIBUTING.md` — no workflow references
- `AGENTS.md` — `lsp-real-server-tests` feature flag reference is correct (local opt-in)
- `RELEASING.md` — no workflow references

**Preserved for traceability:**
- `plans/implementation/development-verification-release/004-integration-evidence-cleanup-and-closure.md` — historical implementation plan
- `plans/closure/development-verification-release/001-status.md` through `003-status.md` — historical closure records

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | 1062 bare `#[tokio::test]` annotations cause `check-tokio-test-flavors.py` to exit 1 | `scripts/verify.sh quick` and `full` fail at tokio check step | Pre-existing from M001; not caused by this milestone; address in dedicated maintenance pass |

## 11. Roadmap disposition

Milestone closed. All four milestones in the Development verification and release subsystem are closed. The subsystem roadmap is marked closed.

No downstream registered plan was blocked on M004. The Tool Programs subsystem's M017 closure is independent.

## 12. Registry updates

- Move M004 from `ready` to `closed` in `plans/registry.md`
- Move `Development verification and release` subsystem status from `active` to `closed`
- Update current milestone to "Milestone 004 closed"
- Add M004 to recently closed work table
- Remove M004 from dependency-ready implementation plans
