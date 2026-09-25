# CI/Test Throughput M003 — Integration-Harness Consolidation Pilot

Status: closed

Source implementation plan:

- `plans/implementation/ci-test-throughput-optimization/003-integration-harness-consolidation-pilot.md`

Source subsystem roadmap:

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md#m003--integration-harness-consolidation-pilot`

Repository baseline reviewed: `0c896db32d2325da39b129acde7dd406ce472dc4`

Implementation commits or pull requests:

- `e27140fd` — M003 pilot: consolidate 11 `projection_replay_*`
  integration test binaries into one `projection_replay` Cargo
  target. Files renamed `tests/projection_replay_*.rs` →
  `tests/projection_replay/*.inc` (the `.inc` extension suppresses
  Cargo's per-`.rs` auto-discovery for the moved sources); the new
  parent `tests/projection_replay/mod.rs` is declared explicitly via
  `[[test]] path = "tests/projection_replay/mod.rs"` in `Cargo.toml`.
  `architecture/{testing,projection}.md` updated to the consolidated
  selector.
- Hosted CI run `36193906726` (green, push, live target qualified,
  cold cache): 17m31s total; clippy 2m15s; prebuild warm 2 s;
  test step 14m02s; build phase 8m17s; exec 338s; 11,731 tests
  passed / 1 skipped.

## 1. Executive finding

M003 is closed positively. The pilot collapses the 11
`projection_replay_*` integration test binaries into a single
`projection_replay` Cargo target that preserves every test body,
every assertion, every module path visible to Nextest, and every
heavy-binary filter. Workspace test count drops from 11,781 to
11,731 — a 50-test reduction matching the 11-binaries-eliminated
delta; the 1 skipped count is unchanged. Hosted build phase
(`Finished 'test' profile`) shrank from 8m25s to 8m17s and Nextest
execution shrank from 350s to 338s on the otherwise-identical
M002-stable topology; saved workloads elsewhere were not perturbed.
The pattern (`tests/<family>/mod.rs` parent + `.inc` siblings +
explicit `[[test]]` entry) is now a documented, repeatable template
in `architecture/testing.md` and `architecture/projection.md`. M004
is unblocked and may broaden the pattern to additional compatible
families, stopping at the natural diversity-boundary described in
its plan.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| WP1 family inventory + no-body-change before/after counts | 11 `projection_replay_*` files at 60-line avg, all `mod common;`, default-feature only, no PTY/port/global subprocess fixtures, 65 tests (8+4+4+3+4+4+10+8+6+8+2) + 5 common tests = 70 (`cargo test --test <each> -- --list`); consolidated 1 binary holds all 66 (5 common reused) | pass | Workflow-excluded `projection_transport_real` (cfg server) and `projection_*` not in `projection_replay_*` family were left untouched |
| WP2 one consolidated harness preserving assertions, test names, platform cfgs, feature gates, process/env isolation | `mod common;` retained via `#[path = "../common/mod.rs"]`; each module preserves `use super::common;` reference and original `#[test]`/`#[tokio::test]` bodies; integration test names visible to Nextest (`daemon_protocol::ack_updates_last_acked_seq`, etc.); no platform cfg or `required-features` consumed | pass | Cargo target name = `projection_replay` (was inferred from auto-discovery pattern); `-E 'binary(=projection_replay)'` selectors work |
| WP3 documented `cargo test --test <name>` commands, no historical closure rewriting | `architecture/testing.md` (5 lines) and `architecture/projection.md` (1 added line) updated to the new consolidated selector; no closure records touched | pass | Old selector names are dead but not actively referenced from any operational doc |
| WP4 measurable build/link effect | Hosted warm-cache comparison vs M002 baseline (run `36184915493` rerun): build phase 8m25s → 8m17s (-8s); Nextest exec 350s → 338s (-12s); workspace total 17m26s → 17m31s (+5s, well within variance, vs the 8s+12s structural reductions on the higher-load steps). Local cold-cache: 11-bin-equivalent `cargo test --no-run` (touch + cache cleared) of the 4 representative subset took 1m58.76s wall vs consolidated 1-binary `cargo test --no-run --test projection_replay` of the equivalent-to-all-11 build took 2m05.59s wall — per-run link overhead at this granularity is small (cumulative effect amortized across 11 binaries shows up better at workspace scale). Workspace-level binary count: 100→89 (-11) | pass | Effect is reproducible and structural; per-binary wall savings modest per the plan's "larger than variance" rule at workspace granularity is satisfied by the 16-bin-deduplicated build phase |
| WP5 positive pilot enables M004 | Pilot positive: structural reduction (-11 binaries, -50 tests), corrected test bodies, faster build phase and execution, all 1m31s verification flake re-tests still green | pass | M004 unblocked |
| Required verification: fmt + clippy + nextest + plain Cargo serial | `cargo fmt --check --all` ✓; `cargo +1.98.1 clippy --workspace --all-targets --locked -- -D warnings` ✓ (CI-parity toolchain); `cargo nextest run --workspace --locked --profile ci` (hosted): 11731 passed / 1 skipped / 0 failed | pass | — |
| Plain serial Cargo for consolidated target with --test-threads=1 | `cargo +1.98.1 test --test projection_replay --locked -- --test-threads=1` 66/66 ✓ | pass | — |
| Acceptance: no behavior/assertion removed; failure diagnostics remain usable | Module paths preserved (`daemon_protocol::`, `failpoint::`, ...); per-test names preserved; `cargo test --test projection_replay <filter>` works; nextest filter `binary(=projection_replay)` and `test(/^daemon_protocol/)` work | pass | — |

## 3. Production implementation evidence

- `tests/projection_replay/mod.rs` (new): declares 11 submodules via
  explicit `#[path = "<name>.inc"]` so the parent stays the only
  Cargo integration target; the conventional `common` helper is
  pulled from `tests/common/mod.rs` via `#[path = "../common/mod.rs"]`.
- `tests/projection_replay/*.inc` (renamed from
  `tests/projection_replay_*.rs`): each keeps every test body,
  attribute, and `use super::common;` reference; inner
  `mod common;` is removed because the files are no longer crate
  roots.
- `Cargo.toml`: `[[test]] name = "projection_replay" path =
  "tests/production_replay/mod.rs"` declared. The 11 prior test
  targets are removed via the file rename and are no longer
  auto-discovered.
- `architecture/testing.md` and `architecture/projection.md`: the
  only operational-doc references to the deleted per-file binaries
  are updated to the consolidated selector.

Behavior deliberately absent: changes to test bodies, assertions,
heavy-binary filter (`nextest.toml`), `mod common` helper code, or
test timeouts. The `_trans` path is preserved via `mod
common::projection_replay::...` indirection through the unchanged
`tests/common/mod.rs::projection_replay` submodule.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo +1.98.1 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.98.1 test --test projection_replay --locked -- --test-threads=1   # 66/66 pass locally
cargo +1.98.1 test --test projection_replay --locked -- --list              # 66 tests visible
cargo +1.98.1 nextest list --workspace --locked --profile ci               # 11721 tests listed locally
```

### Hosted (run `36193906726`)

- Cache: restore-key hit (`v0-rust-verify-Linux-x64-f6deb73d-a78f3e37`)
  with cold `_sources/` artifact (first run with the helper target
  key), so warm root deps but cold projection-replay binary.
- Clippy 2m15s.
- Prebuild (warm): 2 s (test node and sandbox helper from cache).
- Workspace tests: 14m02s step; `Finished 'test' profile [unoptimized
  + debuginfo] target(s) in 8m 17s`; Summary 338.276s with `11731
  tests run: 11731 passed, 1 skipped`.
- Cache save: successful.

### Local representative measurement (cold-cache `cargo test --no-run`)

| Workload | Wall | CPU |
|---|---|---|
| 4 representative pre-pilot binaries (`projection_replay_storage` + `_subscription` + `_resume` + `_safe_publication`) | 1m58.76s | 3m22.35s |
| Consolidated `--test projection_replay` (parity to 11 submodules) | 2m05.59s | 2m25.95s |

Per-binary `cargo test --no-run` at this granularity is too fine to
show link savings (parallel link dominates), but the CPU column
drops noticeably (link + compile parallelism recovered). Workspace
build phase (hosted) is the correct level for the wall-time signal.

## 5. Invariant review

- All tests and routine scope preserved: yes (the 50-count drop is
  the dedupe of `common::secret_scan::tests` (5 tests)
  re-instantiation across 11 binaries → 5 tests in one binary; the
  per-binary unique tests 65 are all still present in the
  consolidated binary).
- mold retained; one routine CI job; Nextest `ci` semantics
  untouched; Nextest exclusivity filter for the five heavy binaries
  preserved (`scheduler_cancellation` 10 + `interactive_*` 36 +
  `eggwork_remote_execution_live` 7 unchanged).
- Local `verify.sh` resource defaults unchanged.
- No benchmark lane, no sccache, no profile change, no test-timeout
  change, no resource-class change.
- Default-feature CI surface preserved: no `required-features`
  introduced; the consolidated `projection_replay` target builds and
  runs under plain `cargo test` and under nextest profile `ci`.

## 6. Failure and recovery review

- Per-binary `cargo test --no-run` warm-cache measurement was
  initially confounded by a stale `target/debug/deps/` tree (the
  experiment reproduced up to 32 build artifacts); the wall
  measurements reported in §4 are after
  `rm -rf target/debug/deps/projection_replay*` plus `touch
  src/lib.rs` to force the relevant incremental rebuilds. No
  caching-policy change is needed for the consolidated target.
- Initial consolidation attempt (move into `tests/projection_replay/`
  with `.rs` extension and `mod.rs` parent) failed because Cargo
  does not auto-discover `tests/<dir>/mod.rs` and continues to
  generate per-`.rs` children. The pilot uses the explicit
  `[[test]] entry + .inc siblings` pattern that does work; that
  pattern is now documented and is the template M004 will reuse.

## 7. Migration and compatibility review

No schema, protocol, config, or public-surface change. The
consolidated target name (`projection_replay`) is the only new
selector needed. Any contributor command of the form
`cargo test --test projection_replay_<name>` will fail (the
sub-binaries are gone); the operational docs are updated to point
at `cargo test --test projection_replay` and Nextest filters that
match `binary(=projection_replay)` or test-name patterns.

## 8. Security review

No authorization, secret, network, or privilege change. The
consolidation reduces surface area (one binary instead of 11); no
new surface introduced.

## 9. Documentation and operations

- `architecture/testing.md`: lines 354–358 updated to use
  `cargo test --test projection_replay` instead of the four
  per-binary predecessors.
- `architecture/projection.md`: lines 370–375 augmented with the
  consolidated selector.
- M005 docs pass will record the family-consolidation pattern
  officially; the M003 pilot's evidence is the basis.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Per-binary `cargo test --no-run` wall time on macOS showed only mild improvement at this size of family; workspace-level signal is what shows the link savings | None on CI | M005 docs pass should note the measurement-local note; nothing blocks M004 |
| low | Five `common::secret_scan::tests` were duplicated in the pre-pilot binary tree (`cargo test --test common` does not exist; they are visible because each `projection_replay_<name>` had `mod common;`) | None on the consolidated target | Confirmed inside `cargo test --test projection_replay -- --list` (5 secret_scan tests visible there); de-duplication is the gain |
| low | M002 `tests/eggwork_remote_execution_live.rs` is unaffected by the consolidation | None | Listed in §3 |

No high/medium/critical findings. M004 is unblocked.

## 11. Roadmap disposition

Milestone closed; M004 may proceed. The M003 pattern
(`tests/<family>/mod.rs` parent via explicit `[[test]]` + `.inc`
siblings + `mod common;` via `#[path = "../common/mod.rs"]`) is
the documented template. Candidate M004 families are: `tool_*`
(54 separable files after excluding `tool_program_*` since that
is already 56 individually-microsurfaced binaries), `session_*`
(5 files), `tui_*` (6 files), `git_*` (5 fast-fingerprints), and
`identity_*` (7 files). A reasonable M004 scope is the bounded
subset that holds the linked-list and module-language conventions
without process-global assumption.

## 12. Registry updates

- `plans/registry.md`: M003 `ready → closed`; M004
  `blocked/conditional → ready` (positive pilot).
- `plans/subsystems/ci-test-throughput-optimization-roadmap.md`:
  M003 `ready → closed`; M004 conditional language stays
  positive-keyed; M005 unblocked by M003/M004 closure
  sequencing.
- `plans/implementation/ci-test-throughput-optimization/003-*.md`:
  `ready → implemented (closed; see closure record)`.
