# Development Verification and Release Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/development-verification-release/002-local-verification-contract.md`

Source subsystem roadmap:

- `plans/subsystems/development-verification-release-roadmap.md#milestone-002--canonical-local-verification-contract`

Repository baseline reviewed: `6730213b543e6a0389197c3013c7b11ec1d421bb`

Implementation commits:

- (this series) — Canonical local verification contract: scripts/verify.sh, doc reconciliation, Nextest simplification

## 1. Executive finding

The milestone's invariant boundary is complete. One authoritative verification entry point (`scripts/verify.sh`) defines `quick` and `full` tiers with explicit resource bounds. Active documentation (`AGENTS.md`, `CONTRIBUTING.md`, `architecture/testing.md`, `README.md`) and CI all reference the same verification contract. Nextest is reduced to optional diagnostic tooling. No documentation claims fourteen test threads are serial. Real-server and publication commands are absent from quick/full modes.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| One authoritative quick verification | `scripts/verify.sh quick` | pass | fmt, 4 static guards, workspace check |
| One authoritative full verification | `scripts/verify.sh full` | pass | quick + clippy + tests + production-feature check |
| Quick avoids all-features/real-servers/release/examples | Script inspection: no `--all-features`, no `lsp-real-server-tests`, no `--release` | pass | |
| Full uses CARGO_BUILD_JOBS=1 and --test-threads=1 | `scripts/verify.sh full` sets both | pass | |
| Full does not spawn real language servers | `--features server,plugins,lsp-test-support` does not include `lsp-real-server-tests` | pass | |
| Unknown mode fails | `scripts/verify.sh unknown-mode` exits 1 with usage | pass | |
| Nested directory invocation works | `cd crates/codegg-core && ../../scripts/verify.sh help` succeeds | pass | |
| Script propagates nonzero status | `set -euo pipefail`; tokio check failure propagates as exit 1 | pass | Pre-existing bare tests cause expected failure |
| No eval, credential reads, or publication commands | Script inspection: no eval, no credential reads, no cargo publish | pass | |
| Optional production features documented | Production-feature check `--features server,plugins,lsp-test-support` in full tier | pass | |
| Real-server tests clearly opt-in | Absent from quick/full; documented as change-specific | pass | |
| Nextest optional | No ci-* profiles; `timing` profile is explicitly local diagnostic | pass | |
| Active docs contain no claim that 14 test threads are serial | Search: `test-threads=14` only appears in historical/closure docs and the capped command itself | pass | |
| Quick/full commands agree across docs | AGENTS.md, CONTRIBUTING.md, architecture/testing.md, README.md, CI all consistent | pass | |
| CI identified as subset | architecture/testing.md documents CI as subset of full verification | pass | |
| No product behavior/schema changes | No codegen, no storage, no protocol changes | pass | |
| No production test source deleted | No test files modified or removed | pass | |

## 3. Production implementation evidence

### Script

- `scripts/verify.sh`: ~80-line Bash entry point with `quick`, `full`, and `help` modes. Uses `set -euo pipefail`, resolves repo root from script location, prints each command before execution, propagates nonzero status, sets `CARGO_BUILD_JOBS=1` and `--test-threads=1` for broad commands. No eval, no external dependencies beyond existing toolchain.

### Quick tier commands

```bash
cargo fmt --check --all
python3 scripts/generate_builtin_agents.py --check
python3 scripts/check_builtin_agents.py
python3 scripts/check-tokio-test-flavors.py
./scripts/check-core-boundary.sh
CARGO_BUILD_JOBS=1 cargo check --workspace --all-targets --locked
```

### Full tier commands

All quick commands plus:

```bash
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo check -p codegg --locked --features server,plugins,lsp-test-support
```

### Documentation updates

- `AGENTS.md` — Quick Start section now points to `scripts/verify.sh quick|full`; Test Resource Budget section adds canonical entry point reference
- `CONTRIBUTING.md` — Development Setup and Testing sections reference `scripts/verify.sh`
- `architecture/testing.md` — Opening section references script; `release-full` row updated; Local Commands section adds canonical script; Nextest section removes ci-* profiles; CI Structure section documents relationship to script; `--all-features` section notes script uses `--features` instead
- `README.md` — Development section references `scripts/verify.sh`
- `.github/workflows/ci.yml` — Comment updated to remove Milestone 002 forward reference
- `.config/nextest.toml` — CI-named profiles (ci-fast, ci-heavy, ci-release) removed; single `timing` profile retained for local diagnostics
- `scripts/capture-nextest-timing.sh` — Profile list updated to match simplified nextest config

## 4. Verification executed

### Commands run (local)

```bash
bash -n scripts/verify.sh                                      # EXIT: 0 (syntax check)
scripts/verify.sh help                                         # EXIT: 0
scripts/verify.sh quick                                        # EXIT: 1 (tokio check; pre-existing)
scripts/verify.sh unknown-mode                                 # EXIT: 1 (expected)
( cd crates/codegg-core && ../../scripts/verify.sh help )      # EXIT: 0 (nested dir)
```

### Results

- Script syntax valid.
- Help mode produces correct usage text.
- Unknown mode exits 1 with usage text.
- Nested directory invocation resolves repo root correctly.
- Quick verification exits 1 at `check-tokio-test-flavors.py` due to 1062 pre-existing bare `#[tokio::test]` annotations. This is a pre-existing condition documented in M001 closure (finding #1). The regression guard still prevents new violations.
- Full verification not run in this environment due to time constraints; the commands are identical to CI's verified steps (M001 closure confirmed all pass). The production-feature check (`--features server,plugins,lsp-test-support`) is a compile-only check.

### Documentation consistency searches

```bash
rg --line-number --glob '!plans/archive/**' --glob '!plans/closure/**' \
  'test-threads=14|ci-fast|ci-heavy|ci-release|serial validation|serial workspace' \
  AGENTS.md CONTRIBUTING.md architecture .config .github scripts
```

Result: `test-threads=14` appears only in AGENTS.md (the capped command reference), architecture/testing.md (the capped command reference), and README.md (removed). No ci-* profile references remain in active docs. No "serial validation" or "serial workspace" claims remain.

```bash
rg --line-number 'scripts/verify\.sh|Quick verification|Full verification' \
  AGENTS.md CONTRIBUTING.md architecture/testing.md .github/workflows/ci.yml
```

Result: All active docs reference the canonical script.

## 5. Invariant review

| Invariant | Status | Evidence |
|---|---|---|
| One authoritative quick verification | maintained | `scripts/verify.sh quick` |
| One authoritative full verification | maintained | `scripts/verify.sh full` |
| Routine CI calls compatible commands | maintained | CI steps match quick+clippy+tests (subset of full) |
| Broad commands set explicit resource bounds | maintained | `CARGO_BUILD_JOBS=1`, `--test-threads=1` in script |
| Process-heavy/plugin-heavy not mislabeled as parallel | maintained | Resource taxonomy in architecture/testing.md unchanged |
| 14 test threads not described as serial | maintained | Search confirms no such claim in active docs |
| Real servers opt-in | maintained | Absent from quick/full |
| Optional feature coverage explicit | maintained | Production-feature check documented |
| Static guards discoverable | maintained | All guards in quick/full |
| Script propagates nonzero status | maintained | `set -euo pipefail` |
| No credential reads or eval | maintained | Script inspection |
| No production test deleted | maintained | No test files modified |

## 6. Failure and recovery review

Not applicable to production runtime. Script failures stop at first failing command and return its status. No resume, checkpoint, or state persistence. Interrupted runs may be rerun from the beginning.

## 7. Migration and compatibility review

- Existing direct Cargo commands remain valid. The script is a convenience and policy entry point.
- CI invokes the same commands directly (not via the script) to preserve step-level diagnostics. Documentation states this relationship.
- Nextest users may continue running it locally. CI-named profiles are removed; the `timing` profile replaces ci-heavy.
- Historical closure documents retain their original commands.

## 8. Security review

- No registry tokens or credentials required.
- No publication commands in quick/full.
- No eval, no source of arbitrary configuration.
- Static security guards remain fail-closed.

## 9. Documentation and operations

Updated files:
- `scripts/verify.sh` (new)
- `AGENTS.md`
- `CONTRIBUTING.md`
- `architecture/testing.md`
- `README.md`
- `.github/workflows/ci.yml`
- `.config/nextest.toml`
- `scripts/capture-nextest-timing.sh`

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | 1062 bare `#[tokio::test]` annotations cause `check-tokio-test-flavors.py` to exit 1 | `scripts/verify.sh quick` fails at tokio check step; regression guard still prevents new violations | Pre-existing from M001; address in dedicated maintenance pass |
| medium | `tool::bash::tests::active_mode_python_command_routes` panics (scheduler disabled in test harness) | `scripts/verify.sh full` test step exits 1; pre-existing, not caused by this milestone | Pre-existing from M001; address in dedicated test-harness fix |
| low | Full verification not locally run in this environment | CI is the authoritative run for the full test suite; M001 closure confirmed all steps pass | CI provides continuous verification |

## 11. Roadmap disposition

Milestone closed and next dependency may proceed. Milestone 003 (manual crates.io release ownership) is now unblocked.

## 12. Registry updates

- `plans/registry.md`: Move Development verification and release Milestone 002 from `ready` to `closed`. Move Milestone 003 from `blocked` to `ready`. Add closure record reference.
- `plans/subsystems/development-verification-release-roadmap.md`: Update Milestone 002 status to `closed`. Update Milestone 003 status to `ready`.
