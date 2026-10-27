# Agent Worktree Placement and Build Affinity A003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-worktree-placement-and-build-affinity/003-repository-scoped-build-admission.md`

Source subsystem roadmap:

- `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md#milestones-and-boundaries`

Repository baseline reviewed: `80171f45295bebb9ab0b521262c0b295acb93074`

Implementation commits or pull requests:

- PR #207, merged at `fed10ce13be7fd99a6d5aa5f8b6639ba98d007bc` — typed opt-in repository quota, canonical lineage binding, and bounded admission reporting.
- `6915ca956` — production Cargo measurement records GNU time resource reports, target growth, Cargo output, and observed build-directory lock waits.
- Hosted Linux qualification run `40592341646`, process job `122499711946` — production Cargo comparison passed on source commit `a76d9bd69`.

Closed: 2026-10-27

Primary class: infrastructure

## 1. Executive finding

A003 is strictly closed. The scheduler now binds eligible Build and Format jobs to an optional operator-configured quota keyed by daemon-resolved canonical repository identity, in addition to existing workspace admission. Sibling worktrees of one repository share that budget, while unrelated repositories remain independent. Waiting jobs hold no process slot and resume after the admitted process reaches terminal state. A hosted Linux comparison exercised actual Cargo checks through `JobSubmissionService`, `JobScheduler`, and `ManagedArgvExecutor` with quota disabled, enabled at one for sibling worktrees, and enabled at one for unrelated repositories. It recorded wall time, user/system CPU, peak RSS, target-directory growth, and Cargo lock-wait counts using separate fresh target directories.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Repository bucket is derived from daemon-owned canonical workspace lineage | `opt_in_repository_quota_is_bound_from_catalog_not_request_metadata`; source workspace and linked sibling workspace resolve to the same repository record | pass | Caller-supplied resource request is normalized at submission |
| Quota is optional and configuration-owned | `scheduler.quota.max_concurrent_builds_per_repository`, default disabled; config validation rejects zero and limits caller requests to operator policy | pass | Existing workspace quota remains independent |
| Same-repository sibling jobs share the bound | Real `JobSubmissionService` + `JobScheduler` integration and production Cargo quota=1 case | pass | Exactly one sibling is admitted; waiter resumes after first terminal |
| Unrelated repositories remain independent at the same quota | Production Cargo unrelated-repository control | pass | Both jobs dispatch together with per-repository limit one |
| Waiting jobs retain no permit; completion releases quota | Scheduler quota/admission/resource-profile/permit-lifecycle tests: 49 passed | pass | Production Cargo control also observes queued-to-running resumption |
| Resource and Cargo lock behavior is measured without claiming arbitrary-shell coverage | Hosted production Cargo comparison, three scenarios, fresh per-job target directories, per-job output capture | pass | Exact host and measured values are recorded in §3 |
| Unknown quota namespace and unsupported job classes do not silently bypass bounds | Typed quota validation, admission tests, `check_scheduler_bypass.py`, and execution-ownership guard | pass | Arbitrary Bash remains outside the automatic repository-build classification claim |

## 3. Production Cargo comparison

Hosted run: `40592341646`, process job `122499711946`, A003 step. Test command:

```bash
cargo test --locked -p codegg --lib repository_quota_controls_production_cargo_build_admission -- --ignored --nocapture --test-threads=1
```

The test creates two linked worktrees of a dependency-free Rust fixture containing 2,000 generated functions, plus a separate unrelated repository. Every invocation uses a fresh, distinct `CARGO_TARGET_DIR`, `CARGO_BUILD_JOBS=2`, and `--offline`; there is no shared writable target directory or dependency cache. `ManagedArgvExecutor` runs the Cargo check as a scheduled Build job. GNU `time` records wall, user, system, and maximum RSS; target bytes are measured after each scenario. Cargo stdout/stderr is captured per job and the test counts `Blocking waiting for file lock` lines.

Host and toolchain: Linux x86_64, kernel `6.17.0-1022-azure`, `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, Cargo `1.99.1 (5f94df478 2026-08-27)`. The fixture had no dependencies, ran offline, and used a fresh separate target directory per build.

| Scenario | Scheduler wall time | Per-build wall / user / system / max RSS | Target bytes | Cargo build-directory lock waits |
|---|---:|---|---|---:|
| Repository quota disabled, sibling worktrees | 331 ms | `0.15 / 0.11 / 0.04 / 206,956 KB`; `0.15 / 0.12 / 0.03 / 208,984 KB` | 335,281; 335,283 | 0; 0 |
| Repository quota 1, sibling worktrees | 486 ms | `0.14 / 0.11 / 0.03 / 208,988 KB`; `0.15 / 0.11 / 0.04 / 206,920 KB` | 335,271; 335,273 | 0; 0 |
| Repository quota 1, unrelated repositories | 277 ms | `0.15 / 0.11 / 0.03 / 208,996 KB`; `0.15 / 0.11 / 0.03 / 206,880 KB` | 335,285; 335,295 | 0; 0 |

Each per-build tuple is GNU `time` output in seconds for wall/user/system, followed by maximum RSS in KiB. Scheduler wall time covers both jobs in the pair. The zero lock counts come from captured Cargo output for these separate fresh target directories; they do not establish behavior for shared targets or dependency-cache contention.

Interpretation: the repository quota affects scheduler admission before process start. Cargo target directories are separate, so this comparison does not recommend shared target reuse. A zero Cargo lock-wait count is evidence only for these isolated-target fixture runs. No result here claims that arbitrary Bash, terminal, or external Cargo commands are automatically classified as repository builds.

## 4. Verification executed

### Commands run

```bash
CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=/tmp/codegg-linker-sdk/cc MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 rtk cargo nextest run -p codegg --test scheduler_jobs --locked --profile ci -E 'test(opt_in_repository_quota_is_bound_from_catalog_not_request_metadata) | test(build_quota_admission) | test(scheduler_resource_profiles) | test(scheduler_permit_lifecycle)'
rtk scripts/verify.sh quick
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
rtk cargo fmt --check
rtk git diff --check
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
```

### Results

- Focused scheduler admission, resource-profile, and permit-lifecycle filters: **49 passed, 320 skipped, 0 failed**.
- Hosted Linux production Cargo comparison: **passed** in run `40592341646`, process job `122499711946`.
- Run `40592341646` was canceled after the A001 and A003 steps and the restricted-userns lane passed, while its remaining M004 process/PTY/LSP lane was still running. That unrelated remainder is not counted as passed evidence.
- `scripts/verify.sh quick`: **passed**, including 357 harness tests (356 passed, 1 skipped), all quick guards, and workspace all-target check.
- Workspace Clippy with `-D warnings`: **passed; no issues found**.
- Scheduler-bypass and execution-ownership guards: passed.
- Formatting and `git diff --check`: passed.

## 5. Invariant review

- Repository identity comes from the daemon's project catalog and workspace binding, never a request path or caller-provided quota discriminator.
- Repository quota is typed, optional, disabled by default, and bounded by operator configuration.
- The existing workspace quota is retained; repository admission supplements it.
- Quota binding is unconditional for eligible Build/Format submissions, including requests with caller-supplied resource keys.
- Unknown quota namespaces and unsupported kinds refuse rather than silently dropping enforcement.
- Queue caps are not execution permits. A queued job holds no process/CPU slot; slots return only after terminal process completion.
- The experiment uses separate target directories and makes no shared-cache or arbitrary-shell admission claim.

## 6. Failure and recovery review

Scheduler tests cover same-workspace and same-repository contention, unrelated repositories, bounded waiting, permit lifecycle, and cancellation/resource release. The production Cargo trial proves one sibling waits at quota one and is admitted after the first Cargo process terminates; unrelated repositories dispatch concurrently. No quota state is persisted separately from the existing typed job resource request, so restart retains the existing job recovery contract. Unknown quota scope and unsupported requests fail closed. No new cache, process wrapper authority, or nested-quota acquisition path is added.

## 7. Migration and compatibility review

The new configuration field is optional and defaults disabled. Existing serialized resource requests remain compatible; the quota uses the current persisted request representation. No schema migration or job payload variant was introduced. Existing per-workspace admission behavior remains unchanged unless the operator explicitly enables the repository limit.

## 8. Security review

Canonical repository identity is resolved by the daemon at submission and replaces caller metadata. Quota diagnostics expose bounded admission state without repository paths, command arguments, or environment values. The quota is not treated as a writer permit or as authorization to share writable Cargo targets. Jobs remain under existing process and checkout authority; arbitrary process commands are not represented as governed unless they are a supported typed Build/Format submission.

## 9. Documentation and operations

`architecture/build_admission.md`, `architecture/scheduler.md`, configuration reference, and the merged implementation document the optional repository quota, canonical scope, supported job classes, defaults, and bounded diagnostics. The Linux workflow runs the opt-in production Cargo comparison after the A001 contained-checkout success step. Operators can leave the limit disabled or set `scheduler.quota.max_concurrent_builds_per_repository`; a value of zero is rejected.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | No A003 correctness or evidence finding remains | None | None |

## 11. Roadmap disposition

Milestone A003 is closed. The A004 cache-economics pilot may be registered as eligible for fresh planning from the production measurements. It must evaluate cache isolation and portability without enabling a shared writable target directory by assumption. A005 remains blocked on A002 and A004 decision/economics evidence.

## 12. Registry updates

- Mark A003 closed in its implementation plan, subsystem roadmap, and `plans/registry.md`; move it from active implementation work to recently closed work.
- Move A004 from blocked to eligible for fresh planning and record that strict A003 closure discharged its dependency.
- Keep A005 blocked on A002 plus A004 decision/economics evidence.
- A001's independent closure makes A002 eligible for fresh planning; see `plans/closure/agent-worktree-placement-and-build-affinity/001-status.md`.
