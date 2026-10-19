# Linux Trusted Git Cost Corrective C003 — Closure Status

Status: closed (negative backend decision; status-path evidence only)

Source implementation plan:

- `plans/archive/implementation/linux-trusted-git-cost-corrective/003-gix-versus-cli-benchmark.md`

Source subsystem roadmap:

- `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md#milestones-and-handoffs`

Repository baseline reviewed: `5c43a7fdfbe667f51387cbd4f8a429a510a0b599`; implementation and benchmark evidence landed in `eb15d21726bbff86a90988a89828f0195ac12d35`.

Implementation commits or pull requests:

- `eb15d21726bbff86a90988a89828f0195ac12d35` — add isolated optional-gix harness, raw samples, status-path comparison report, and rebase compile correction.
- `5c43a7f` — pre-existing branch baseline; no production Git route changed by C003.

## 1. Executive finding

C003 is closed as a negative backend-selection decision for the measured status workload. gix 0.89.0 was slower than direct native Git on both packed and fragmented fixtures and does not eliminate the object snapshot copy cost by itself. Defer gix and keep it out of CodeGG production dependencies. The immutable snapshot route remains the only trusted production route. The snapshot cold path exceeds the C010 1,000 ms comparison threshold by several times, so the result supports a separate contained-native-CLI corrective, not a live-object policy change.

This closure does not claim complete Git semantic parity or production security qualification for gix, nor does it admit contained live Git. Those wider matrices were stopped after the measured gix candidate failed the status-path latency gate. Any future gix proposal must begin with a new bounded plan covering the intended operations.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Isolated optional comparison harness | `research/linux-trusted-git-cost-corrective/gix-bench/`; gix is opt-in in a separate Cargo workspace | pass | No root manifest, root lockfile, or production route change. |
| Packed and >=4,000 loose-object / ~100 MiB fixture | Two deterministic 4,096-file fixtures, one packed and one loose; raw CSV in `research/linux-trusted-git-cost-corrective/results/` | pass | Object stores were about 100.9 MB. |
| Three route comparison | Native Git, current `egggit::GitEnvPolicy` immutable snapshot route, and gix 0.89.0 isolated status | pass | Status-entry-count parity was 2 for each route. |
| >=100 warm / >=30 fresh-process samples per route and shape | 100 warm and 30 fresh-process samples in each of six route/shape CSVs | pass | `POSIX_FADV_DONTNEED` was requested and recorded before fresh-process samples; it is advisory, not guaranteed eviction. |
| Latency, CPU, RSS, object bytes and files | Per-sample CSV and `research/linux-trusted-git-cost-corrective/c003-gix-vs-cli-benchmark.md` | partial | Cold per-process RSS is recorded; warm `ru_maxrss` is cumulative and not treated as per-operation. I/O counters, retained scratch bytes, startup-only cost, concurrency contention, and actual verified device-cold state were not measured. |
| Structured payload and broad Git feature matrix | Status-entry-count parity only | not run | No diff/log/worktree/rename/submodule/SHA-256/malformed-object parity claim. The gix candidate was rejected for status performance before expanding the matrix. |
| Dependency and binary cost | Standalone optimized harness: no-gix 842,440 bytes; gix 6,265,824 bytes; `cargo tree` unique package names 35 vs. 129 | pass | This is not a CodeGG release-binary delta. |
| Evidence-backed recommendation | C003 benchmark report | pass | `defer-gix`; maintain the immutable snapshot production route; evaluate a contained native CLI separately. |

## 3. Production implementation evidence

No production code path, dependency, API, DTO, storage, or default was changed for C003. The isolated research workspace contains the optional gix comparison and can be deleted without affecting CodeGG. gix is absent from CodeGG's dependency graph and lockfile. No live ODB backend was selected or enabled.

## 4. Verification executed

### Commands run

```bash
cargo check --manifest-path research/linux-trusted-git-cost-corrective/gix-bench/Cargo.toml --features with-gix --locked
cargo build --manifest-path research/linux-trusted-git-cost-corrective/gix-bench/Cargo.toml --release --no-default-features --locked
cargo build --manifest-path research/linux-trusted-git-cost-corrective/gix-bench/Cargo.toml --release --features with-gix --locked
cargo run --manifest-path research/linux-trusted-git-cost-corrective/gix-bench/Cargo.toml --release --features with-gix --locked -- bench research/linux-trusted-git-cost-corrective/results
cargo test --locked -p egggit trusted_git -- --test-threads=1
cargo test --locked -p egggit -- --test-threads=1
cargo fmt --all -- --check
cargo fmt --manifest-path research/linux-trusted-git-cost-corrective/gix-bench/Cargo.toml -- --check
git diff --check
python3 scripts/check_adr_integrity.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_sandbox_contract.py
scripts/verify.sh quick
```

### Results

- Harness check/builds and benchmark completed; each CSV contains 100 warm and 30 fresh-process rows.
- Targeted trusted-Git egggit tests: 20 passed.
- Full egggit serial suite: 108 passed, 1 ignored across 2 suites.
- An initial un-serialized `cargo test --locked -p egggit` run had two failures involving the snapshot cache and global Git hooks. The repository guidance requires serial execution for process-global state; both failures passed in the serial full suite.
- Formatting, diff check, ADR integrity, execution ownership, forbidden Git pattern, and sandbox contract guards passed.
- `scripts/verify.sh quick` passed on branch head `326a9f042f9d98c12175be5cee5a7cb5d595b787`.
- Hosted x86_64 macOS CodeGG target compile passed on the same branch head in run [39102146034](https://github.com/dbowm91/codegg/actions/runs/39102146034); C003 itself changes no cross-platform production route.
- Exact throughput values use nearest-rank p95 and the cache-eviction limitation is recorded in `research/linux-trusted-git-cost-corrective/c003-gix-vs-cli-benchmark.md`.

## 5. Invariant review

- The immutable snapshot remains the trusted production behavior; no change to its consistency guarantee was made.
- No host Git execution was admitted and no no-writer-lease claim was introduced.
- gix remains test-only and opt-in, outside production manifests and root lockfile.
- No silent FullHost fallback, credentials, network access, hooks, or public consistency receipt were added.
- Performance values are workload-specific and do not supersede C010 or C012 decisions.

## 6. Failure and recovery review

The benchmark uses short-lived test repositories under a temporary directory. A process error, nonzero Git status, or status-count mismatch fails the run. Temporary repositories are removed with the owning `TempDir`. There is no persistent state, scheduler job, retry path, concurrency, cancellation, or daemon restart behavior in this research harness.

## 7. Migration and compatibility review

No migrations, protocol changes, user configuration, Windows/macOS routes, or production dependencies changed. The harness is a separate Cargo workspace and does not participate in the root workspace. No rollout or rollback is required; removing the research directory reverts this work.

## 8. Security review

The gix route uses `gix::open::Options::isolated()` and runs only in the standalone benchmark process; no production daemon parses untrusted repositories through gix. The benchmark is not a security differential: malformed packs, alternates, replace refs, hook/filter behavior, and malicious config cases are not qualified. Since the candidate was rejected and no production wiring was added, these tests remain a mandatory precondition for any future gix adoption. The live native Git test remains C001 test-only evidence under the actual M004/M005 managed process boundary.

## 9. Documentation and operations

- Added `research/linux-trusted-git-cost-corrective/c003-gix-vs-cli-benchmark.md` with host, toolchain, commands, results, methodology, and limits.
- Retained raw route/shape CSV artifacts under `research/linux-trusted-git-cost-corrective/results/`.
- Passed `check_adr_integrity.py`, `check_execution_ownership.py`, `check_git_forbidden_patterns.py`, and `check_sandbox_contract.py`.
- The separate full `scripts/verify.sh quick` and exact-head hosted Linux integration run remain broader branch verification, not prerequisites for this standalone no-go decision.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | gix was compared only for status entry counts, without broad `GitPayload` parity or malformed-repository security probes | No future operation beyond status can rely on this benchmark | Any re-proposal must use a new plan with operation and adversarial matrices before production discussion. |
| medium | Cache drop was advisory; no 1/4/8 caller run, verified device-cold I/O, or setup-only timing was captured | Absolute cold latency may vary by host/cache state | Preserve raw data and repeat under a controlled runner if a route decision depends on those dimensions. |
| high | Snapshot cold p95 is 4.27–5.75 seconds in these fixtures, above C010's 1,000 ms comparator | Current route's cold routine-read cost remains unresolved | Continue the separate C001/C002 contained native CLI work; retain immutable semantics until independently admitted. |

## 11. Roadmap disposition

C003's backend-selection objective is complete as a negative result: defer gix for status and do not promote a gix dependency. The measurement does not select or admit a replacement. C001 remains in progress pending exact-source hosted contained-Git probes; C002 remains blocked on C001 and independent ADR/M005 gates; C004/C005 remain blocked by their stated safety and admission dependencies. C010's no-admission and C012's no-live-mount decision are unchanged.

## 12. Registry updates

- Mark C003 closed as a negative backend decision and link this closure and the archived plan.
- Keep the Linux Trusted Git Cost Corrective roadmap active with C001 test-only in progress and C002/C004/C005 blocked.
- Keep proposed ADR-0044 proposed; no production route or M003 admission changes follow from C003.
