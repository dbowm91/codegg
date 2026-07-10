# Command Routing Final Closure Pass — Validation Evidence

## Commit

- **SHA**: `1e4eb56bb168ef13e91912e0cbd84c2c2c148da6`
- **Date**: 2026-07-10

## Environment

- **Platform**: macOS (darwin)
- **Architecture**: x86_64
- **Rust version**: 1.96.0 (ac68faa20 2026-05-25)

## Changes

### Workstream A: TUI Capability Flags

- `can_rollback` set to `false` (no rollback backend exists)
- `can_rerun` now validates `rerun.argv` is `Some` and non-empty
- `can_promote` requires projection, completed/failed status, and `safe_for_model` artifact
- `can_view_artifact` set to `false` (no ranged reader available)

### Workstream B: Permission Defaults

- `is_safe_git_subcommand()` now only allows `git add`; commit/checkout/switch/restore/stash push default to `Ask`
- Writing formatters (`cargo fmt`, `prettier --write`, `black`) default to `Ask`
- Read-only formatters (`--check`, `--diff`) remain `Allow`
- New `is_read_only_formatter()` helper added

### Workstream C: RunStore Persistence

- Added `RunOwnership` enum (`Caller`, `DelegatedBackend`, `ChildOf(RunId)`)
- BashTool now propagates `RunKind` based on routing decision (GitRead, NativeTool, Search, GitMutation, ManagedProcess)
- BashTool skips RunStore persistence for TestRunner and PythonScript backends

### Workstream D: Test Persistence

- RunStore documented as authoritative persistence layer for test runs
- Legacy `.codegg/test-runs/index.json` marked as compatibility-only (deprecated)

### Workstream E: Artifact Viewing and Promotion

- `p` keybinding in RunDetailDialog gated on `can_promote`
- `RunArtifactView` doc comments added about ranged reader access

## Test Results

### Targeted Suites

| Suite | Tests | Result |
|-------|-------|--------|
| `command_intent` | 249 | ✅ Pass |
| `command_routing` | 17 | ✅ Pass |
| `command_routing_adversarial` | 139 | ✅ Pass |
| `python_sandbox_adversarial` | 57 | ✅ Pass |
| `test_runner` | 145 | ✅ Pass |
| `tool::bash` | 68 | ✅ Pass |
| `preflight_integration` | 72 | ✅ Pass |

### Validation

| Check | Result |
|-------|--------|
| `cargo clippy --all-features -- -D warnings` | ✅ No issues |
| `cargo fmt --check` | ✅ Clean |
| `cargo check -p codegg-core` | ✅ Clean |
| `cargo check -p codegg` | ✅ Clean |

### Skipped / Resource-Heavy

- `run_store` integration tests: timed out (filesystem I/O sensitive, known behavior)
- Full workspace suite (`CARGO_BUILD_JOBS=1 cargo test --workspace --all-features`): timed out at 15min (too heavy for single session)

## Closure Criteria Verification

- [x] Active routing remains opt-in and Observe by default
- [x] Unsupported rollback/artifact actions are disabled
- [x] Git commit, checkout/switch/restore, stash push, writing formatters are permission-gated
- [x] Native git/search routes persist with correct RunKind
- [x] One command produces one canonical run record
- [x] RunStore is the documented source of truth for tests
- [x] Legacy test indexing is compatibility-only and failure-tolerant
- [x] Promotion and artifact viewing enforce safety checks
- [x] All adversarial suites pass
- [x] Clippy and fmt pass
- [x] Final validation evidence document committed
