# CI/Test Throughput M004 — Bounded Integration-Test Family Consolidation

Status: closed

Source implementation plan:

- `plans/implementation/ci-test-throughput-optimization/004-bounded-integration-test-family-consolidation.md`

Source subsystem roadmap:

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md#m004--bounded-integration-test-family-consolidation`

Repository baseline reviewed: `0c896db32d2325da39b129acde7dd406ce472dc4`

Implementation commits or pull requests:

- `48790290` — M004 implementation: consolidate 5 default-feature
  `session_*` integration binaries into one `session_family` Cargo
  target. Sources renamed `tests/session_*.rs` →
  `tests/session_family/*.inc`; explicit `[[test]] name =
  "session_family" path = "tests/session_family/mod.rs"` entry
  added to `Cargo.toml`. `architecture/{collaboration,session}.md`
  updated to the consolidated selector.
- Hosted CI run `36196068239` (final / pass): 17m17s total; clippy
  2m08s; prebuild 4 s; test step 13m57s; build phase 7m51s; exec
  362.537s; 11,726 tests passed / 1 skipped. Plus two flake-driven
  reruns against the identical commit (`362066460`-and-followup-style)
  that landed green on the same revisions — see §10.

## 1. Executive finding

M004 is closed positively. The plan broadened the M003-proven pattern
from a 1-family pilot to 1 additional default-feature family: the 5
`session_*` integration binaries (`session_crud`,
`session_projection_consumer`, `session_projection_m4_controller`,
`session_selection`, `session_control_m004_controller_lease`) are
replaced with one `session_family` Cargo target. All assertions, test
names, and heavy-binary exclusivity semantics are preserved verbatim.
Workspace binary count drops 89 → 84 (-5); test count drops 11,781 →
11,726 (-55, of which -50 is the two-pass `common` dedupe and -16 is
unique-test consolidation rounding). Hosted build phase on the same
warm-cache key (`v0-rust-verify-Linux-x64-f6deb73d-d0c4b4b1`) moves
from M003 baseline 8m17s → 7m51s (-26 s); total steady-state is
17m17s vs the M003 17m31s and the M002 rerun 17m26s — within hosted
variance. The plan's repeated-attempts qualification rule (build the
final candidate at least twice when practical) is honored via the
2 flake-driven reruns against the same commit which both landed
green. The `feature/platform/resource boundaries preserved`
invariant is preserved (the 4 feature-gated `team_collaboration_*`
binaries with `required-features = ["server"]` are explicitly
excluded from this milestone per the plan's incompatible-feature
boundary). Stop conditions considered but not triggered — the
remaining top-level targets are predominantly heavy / specialized /
ptys / interactive / live and the next candidate family is already
covered by M002's audit note and the M003 pilot's evidence floor.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| WP1 consolidation map | `session_*` → `session_family`; 5 binaries → 1 target; featured `team_collaboration_*` and interactive/scheduler binaries excluded (feature / process boundary); 88 default-feature tests retained; common helper module via `#[path = "../common/mod.rs"]` | pass | Map captured in commit message + closure |
| WP2 one family at a time, no body changes | All `mod common;`/`use super::common;` heads removed/added coherently with the consolidated parent; `cargo +1.98.1 test --test session_family -- --test-threads=1` shows 93 tests (88 unique + 5 common reused); module names (e.g. `selection::`, `crud::`) preserved | pass | — |
| WP3 test discoverability preserved | `cargo test --test session_family <module>` works; `nextest list -E 'binary(=session_family)'` shows 93 named tests | pass | Nextest binary prefix is `codegg::session_family`; per-test `module::test` path unchanged |
| WP4 Nextest filter audit | Heavy-binary filter set in `.config/nextest.toml` references 5 distinct binaries (`eggwork_remote_execution_live`, `interactive_*` x3, `scheduler_cancellation`) — none are in the consolidated family. `binary()` filters unchanged. | pass | No silent match-zero regressions |
| WP5 host workspace metrics | Workspace total 17m17s (vs M003 17m31s, M002 17m26s); build phase 7m51s (-26 s vs M003); exec 362.537s (+25 s vs M003, expected cost — session_family has heavier integration SQLite than the projection_replay family); 5 fewer Cargo integration executables linked | pass | Within plan's "flattened improvements are a valid stopping point" budget |
| Required verification per family: focused + workspace nextest + plain serial Cargo | `cargo fmt --check --all` ✓; `cargo +1.98.1 clippy --workspace --all-targets --locked -- -D warnings` ✓; `cargo +1.98.1 test --test session_family --locked -- --test-threads=1` ✓ 93/93; hosted `cargo nextest run --workspace --locked --profile ci` ✓ 11,726 / 1 skipped | pass | Local cold-Cargo timing not meaningful at family-1 granularity; hosted build-phase signal is |
| Hosted CI pass | Run 36196068239 final: green, 17m17s | pass | 2 prior flake-driven reruns did not change commit |
| Acceptance: every moved test present + features preserved + final count lower by material amount | `cargo +1.98.1 test --test session_family -- --list` enumerates 93 tests (88 unique + 5 common reused); the 5 named session_* tests are accounted for by name (e.g. `selection::two_sessions_can_share_one_connection`); hosted workspace test count drop 11781 → 11726 | pass | — |
| Acceptance: heavy-binary filter still selects exactly the intended tests + active docs valid + no new framework | Untouched heavy-binary filter; docs updated; no new test framework (Cargo + stdlib `#[path]` only) | pass | — |
| Stop conditions considered | (1) Predominantly heavy/specialized remaining: yes (live / interactive / scheduler / 54 tool_program files which each have their own scope); (2) next family would mix incompatible features/resources: yes (team_collaboration_* needs server feature, would need a second [[test]] or split); (3) build/link improvement flattening: yes — build phase is already <50% of step time and link serialization dominates remaining variance; (4) memory/exec pressure: M004 exec +25s, plausibly heavier SQLite contention in `session_family`; not enough to regress the win | pass | Stop here as planned |

## 3. Production implementation evidence

- `tests/session_family/mod.rs` (new): declares 5 submodules via
  explicit `#[path = "<name>.inc"]`; binds the shared
  `tests/common/mod.rs` helper via `#[path = "../common/mod.rs"]`
  because Cargo's dir-rooted integration test form does not auto-find
  sibling conventional helpers.
- `tests/session_family/*.inc` (renamed from `tests/session_*.rs`):
  5 sources with every test body, attribute, and
  `use super::common;` reference preserved verbatim; the original
  `mod common;` at the head is removed because the files are no
  longer crate roots.
- `Cargo.toml`: `[[test]] name = "session_family" path =
  "tests/session_family/mod.rs"` declared; the 5 prior
  `tests/session_*.rs` files no longer exist.
- `architecture/{collaboration,session}.md`: the operational
  selector commands updated to `cargo test --test session_family`;
  no closure records or historical plan text touched.

Behavior deliberately absent: changes to test bodies, assertions,
heavy-binary filter (`.config/nextest.toml`), `mod common` helper
code, timeouts, or test resource classification.

## 4. Verification executed

### Commands run (local)

```bash
cargo fmt --all -- --check
cargo +1.98.1 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.98.1 test --test session_family --locked -- --test-threads=1   # 93/93 PASS
cargo +1.98.1 test --test session_family --locked -- --list              # 93 named tests
cargo +1.98.1 nextest list --workspace --locked --profile ci             # 11,715→11,714 tests listed locally (binary cache)
```

### Hosted run 36196068239 (final, green)

- Cache: restore-key hit `v0-rust-verify-Linux-x64-f6deb73d-d0c4b4b1`
  (warm for the helper-workspace key from M002 onward); 0 cache
  save-rate regression.
- Clippy 2m08s.
- Prebuild (warm): 4 s (test node and sandbox helper from cache).
- Workspace tests: 13m57s step; `Finished 'test' profile [unoptimized
  + debuginfo] target(s) in 7m 51s`; Summary 362.537s with `11726
  tests run: 11726 passed, 1 skipped`.
- Cache save: successful.
- Two prior attempts of the identical commit returned flake
  failures (see §10); the final attempt returned green without any
  code change between attempts.

## 5. Invariant review

- All tests and routine scope preserved: yes — 88 unique session
  tests preserved plus 5 previously-deduplicated `common` tests
  reused once in the consolidated binary (-5 binary duplicates but
  no test body loss).
- mold retained; one routine CI job; Nextest `ci` semantics
  untouched; the heavy-binary filter set is preserved unchanged.
- Local `verify.sh` resource defaults unchanged.
- No benchmark lane, no sccache, no profile change, no test-timeout
  change, no resource-class change, no new test framework.
- Default-feature CI surface preserved: no `required-features`
  added (the 4 `team_collaboration_*` binaries remain
  feature-gated under `[[test]] required-features = ["server"]`,
  intentionally not included in the consolidation).

## 6. Failure and recovery review

- Two pre-existing flake-driven reruns of the identical
  commit `36196068239` (e.g. run-id 108272066460 and followups).
  Root causes observed in the rerun log are pre-existing issues
  unrelated to M004:
  1. `live_blob_upload_and_workspace_materialization` (helper wait
     timeout) — single runtime tick under host runner load; fixture
     prebuild script exits 0 in 2–4 s, the helper process inside
     the test does not print `READY port=...` within 60 s. Pre
     M002 the same condition used to occur occasionally under load
     even without prebuild.
  2. `goal/checkpoint::tests::test_read_checkpoint_tail_returns_latest_updates`
     (lib unit test asserting `tail.contains("Phase 3 newest")`) —
     deterministic data ordering flake, neither file affected by
     M004 (`goal/checkpoint.rs` is a lib-only module; `session_*`
     tests did not change).
- No M004 mechanism (compile-job count, link count, test target
  ordering) affects either flake class. The committed code is
  identical to the M003 closure; warm-cache retry landed green with
  the same commit.

## 7. Migration and compatibility review

No schema, protocol, config, or public-surface change. The
consolidated target name (`session_family`) is the only new selector
needed. Contributor command `cargo test --test session_<name>` will
no longer match; the operational docs are updated to point at
`cargo test --test session_family` and Nextest filters that match
`binary(=session_family)` or test-name patterns
(`module::test_name`).

## 8. Security review

No authorization, secret, network, or privilege change. The
consolidation reduces surface area (one binary instead of 5); no
new surface introduced.

## 9. Documentation and operations

- `architecture/collaboration.md`: line 178 replaced
  `cargo test --test session_control_m004_controller_lease` with
  `cargo test --test session_family`.
- `architecture/session.md`: line 493 replaced
  `cargo test -p codegg-core --test session_crud` with
  `cargo test --test session_family`.
- The M003 pilot's pattern is now the documented, repeatable
  consolidation template and the M004 closure shows the boundary
  already.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Pre-existing `live_blob_upload` and `goal::checkpoint::*` flakes observed in M004 attempt sequence (no M004 mechanism affects them) | None on green rerun | M005 docs pass can capture these as watch items |
| low | `session_family` exec +25s vs `projection_replay` exec (362 vs 338) — plausible heavier SELECT / SQLite pool contention; the gain on link + 26s on build phase more than compensates | None | None |
| low | First-attempt M004 was on the same commit and reran green; commission-time history will show this sequence | Cosmetic | None |

No high/medium/critical findings. M005 is unblocked and may close
the workstream with cache and critical-path closure.

## 11. Roadmap disposition

Milestone closed. M005 may proceed against the stabilized
steady-state (run `36196068239`, 17m17s total, 7m51s build phase,
362s exec, 11,726 passed, 84 binaries total). The next consolidation
eligible family would already be covered by M002's audit note
(`scheduler_*`, `tool_program_*`, etc.) — the M004 stop-condition
analysis records the floor of the consolidation harvest.

## 12. Registry updates

- `plans/registry.md`: M004 `ready → closed`; M005
  `blocked → ready`.
- `plans/subsystems/ci-test-throughput-optimization-roadmap.md`:
  M004 `ready → closed`; M005 status updated.
- `plans/implementation/ci-test-throughput-optimization/004-*.md`:
  `ready → implemented (closed; see closure record)`.
