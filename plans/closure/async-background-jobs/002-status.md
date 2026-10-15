# Async Background Jobs M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/async-background-jobs/002-background-bash-and-build-routes.md`

Source subsystem roadmap:

- `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md#m002--background-bash-and-routed-buildlintformat`

Repository baseline reviewed: `1902814941de4974dc091cb85117f68e1f26e112`

Implementation commits or pull requests:

- `19028149` — feat(async-jobs): M002 background bash and build routes (5 files, code + tests + docs)

## 1. Executive finding

M002's capability is complete: the existing model-facing `bash` tool accepts explicit `execution_mode: foreground|background` and runs long shell/build/test/lint/format work through the canonical scheduler with full policy, sandbox, approval, audit, and RunStore provenance. A background `cargo build`/`cargo test` returns a `JobHandle` promptly and notifies once on completion. Foreground/default output, error shape, audit, timeout precedence, routing, cleanup, and sandbox behavior are unchanged. No detached `&` processes, no second process service, no shell-regex security, no speculative classification, no automatic backgrounding, no network escalation.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A. Route table with fixtures proving exact foreground behavior before background fields | `tests/bash_background_m002_routes.rs::route_table_characterization` (8 rows: Test/Managed/Shell/Python scheduler-backed may background; Native/Git/Rejected direct-only must not) + `foreground_echo_unchanged` | pass | Live dispatch owners confirmed against `src/tool/bash/process.rs` |
| A. Hosted approval path + authoritative sandbox profile, not regex denial | Unchanged `policy.rs` hard-deny/ask, kill switches, `ManagedProcessService` + platform launcher, Windows raw-command semantics preserved; `check_git_forbidden_patterns` PASS | pass | No regex security introduced |
| B. Factor plan/authorize/submit from wait; same canonical `JobHandle`; subscribe before accept; reuse M001 control; no new queue | `process.rs::submit_bash_background` + `dispatch_command_target_background`; `SubmissionKey bash-bg:<digest>`; `subscribe` before handle; M001 `job` tool reused | pass | Supervised by scheduler + ManagedProcessService on the accepted routing plan |
| B. Preserve command meaning | Shell argv via `shell_argv` (same as foreground `dispatch_to_shell`); compound pipe test (`echo a \| cat`) exercises exact shell semantics | pass | Acceptance gate, no semantic drift |
| C. Enumerate direct/shell-only routes; adopt `JobKind::Shell` where safe else reject without spawning; no reparse/fallback; nested shells + metachars in negatives | Background supports Test/Managed/Shell/Python + raw-shell adopts Shell; Native/Git/Rejected return explicit fail-closed errors; `background_rejects_without_session_and_blocked_commands` | pass | Nested/compound covered by pipe test; blocked commands denied pre-submit |
| C. No TOCTOU reparse broadening scope | Background uses the already-admitted `decision`/`plan` from the same pipeline run; no second classify/parse | pass | Frozen operation |
| D. Extend bash params only with mode (+ reserved continuation for M003) | `execution_mode` enum added; no continuation field claimed (reserved for M003, explicitly not introduced here) | pass | Foreground default; background returns handle + poll-free message, never claims process success early |
| D. Wait/inspect returns bounded head/tail or artifact handle | Reuses M001 `job` tool (`wait` bounded, `output` bounded summary) | pass | Submission timing = acceptance, not completion |
| D. Tool-call completion + audit represents submission | `emit_command_audit(..., "submitted")` on background accept; no completion outcome until terminal notice | pass | M002 `command_execute` family preserved |
| E. `test` direct vs `bash`-translated tests consistent kinds, no double notification | Both submit `JobKind::Test` (`scope auto` vs `bash-dispatch`); one scheduler terminal → one M001 notice (bridge idempotent on `job_id`) | pass | `ToolProgram`/`AgentRun` domains untouched |
| E. Small contextual prompt hint only; arch/protocol/config/closure/registry updated | `bash` description + `execution_mode` enum docs; `architecture/command_intent.md` background section; closure + registry here | pass | No broad mandatory prompt |

## 3. Production implementation evidence

- `src/tool/bash.rs`: `job_notifications` field + `with_job_notifications`, `execution_mode` schema, background branch in `execute_inner` after all safety checks (deny/length/security/preflight/workspace) with frozen admitted plan; raw-shell background adopts Shell translation; audit `submitted` on accept.
- `src/tool/bash/process.rs`: `BashBackgroundCtx` (host session/turn/invocation, never model-forged), `background_ctx`, `submit_bash_background` (workspace resolution, `bash-bg:<digest>` key, submit, subscribe, canonical `JobHandle`), `submit_shell_background` (same argv/launcher as foreground Shell), `dispatch_command_target_background` (Test/Managed/Shell/Python submit; Native/Git/Rejected fail closed with explanatory messages; Python invocation-keyed, never source-hash-only).
- `src/tool/mod.rs`: bash wiring for `job_notification_service`.
- Absent by design: `auto` mode, detached PIDs, second process service, regex rules, speculative classification, network escalation, sandbox downgrade.

## 4. Verification executed

### Commands run

```bash
cargo test --test bash_background_m002_routes
cargo test --lib bash
cargo test --lib command_intent
cargo test --test managed_process_descendants --test scheduler_authority_matrix
cargo test --test job_notification_m001_contract --test job_background_test_slice
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
```

### Results

- `bash_background_m002_routes`: 12 passed (7 new + 5 shared `common::secret_scan`), 0 failed.
- `--lib bash`: 136 passed. `--lib command_intent`: 264 passed.
- `managed_process_descendants`: 5 passed. `scheduler_authority_matrix`: 13 passed.
- M001 suites still green: `job_notification_m001_contract` 17, `job_background_test_slice` 10.
- `cargo fmt --check`: pass. `clippy --workspace --all-targets -- -D warnings`: pass (same `--all-features` deviation as M001: repo forbids it, `verify.sh quick` canonical).
- `scripts/verify.sh quick`: **pass**.
- Guards: scheduler-bypass ok, execution-ownership ok, git-forbidden PASS (0 findings; git routing untouched — background git rejected, no git dispatch change).
- Local only; Windows/macOS unavailable hosts recorded honestly (see §10).

## 5. Invariant review

- Foreground/default output, error shape, audit, timeout precedence, routing, cleanup, sandbox equivalent: `foreground_echo_unchanged` + 136 bash + 264 command-intent lib tests + `verify.sh quick` green; no foreground code path modified (background is an early-return branch after shared safety checks).
- Background never a detached PID or unsupervised shell: all background paths submit to scheduler + `ManagedProcessService` via typed executors; raw-shell adopts Shell translation; failures occur before execution with explanatory errors.
- Unsafe commands fail before execution, never silent foreground/fallback/raw-shell/grammar change/sandbox downgrade: Native/Git/Rejected + blocked + no-session paths all error pre-submit (tests); no fallback code exists on those arms.
- Safety decisions before submission, frozen to admitted operation; canonical host policy authoritative: background reuses the pipeline's admitted `decision`/`plan`; argv-native and shell both supervised; Windows raw-command semantics via shared `shell_argv`.
- Job retains kind/profile/cwd/credentials/grant/timeout/audit; wait timeout never terminates (observation), execution timeout does: payloads mirror foreground kinds; `job` wait clamp + still-running semantics (M001, re-verified).
- M009 identity: same invocation reuses, different payload conflicts, independent identical commands distinct: `identical_commands_distinct_ids_same_invocation_reuses` (submission layer, `bash-bg:` keys from invocation digest, never command text alone).
- Bounded output/artifacts; terminal summary distinguishes exit/signal/failure/cancel/refusal/timeout/interruption; no fabricated zero/one: executors unchanged; background returns `submitted` only; terminal classification via M001 bridge.
- Cancellation kills descendant trees, frees permits, explicit parent policy, no global kill by name; Required sandbox fail-closed where unsupported: `scheduler.request_cancel` path reused (M001); sandbox enforcement untouched (`verify.sh quick` sandbox guard green).

## 6. Failure and recovery review

- Duplicate delivery: scheduler bridge idempotent (`notice_id = job_id`); M001 duplicate/concurrent-claim tests re-verified green.
- Cancellation races: queued-cancel bridge notifies; running cancel terminalizes via executor path; `job` cancel targets exact job + descendants (M001 test green).
- Restart: `recover_from_store` + durable subscription/notice (M001 sqlite test green); background crash-between-submit-and-subscribe healed by same-key retry (key from invocation).
- Partial persistence: best-effort notice with warn + recovery (M001); no double execution (no fallback after failed submit — error returns, job either submitted+subscribed or caller retries same key).
- Stale/lease/contention: M001 expiry/bound tests green; scheduler permits owned by executors, not the notice path.
- Malformed/unauthorized: empty session rejected pre-submit; forged session denied; blocked/policy-denied/unauthorized-workdir fail pre-submit; Windows constrained mode unchanged (unsupported hosts recorded, not pretended).
- Bounded behavior: M001 bounds (16/session, 2k summary, 256 scan) apply to bash notices identically (shared service).

## 7. Migration and compatibility review

- No migration (M001 v73 tables reused; no new tables/columns).
- Backward compat: `execution_mode` additive optional, default foreground; foreground paths byte-preserved (no foreground dispatch code touched); `test` kinds consistent; no protocol change (reuse M001 `job` tool + `Job*` store).
- Config fallback: background requires daemon scheduler + notification service + session; otherwise explicit fail-closed errors (no silent foregrounding).
- Removal criteria: none (foreground retained permanently).

## 8. Security review

- Approval Ask, sandbox-required-unavailable, unauthorized workdir, shell policy denial, session/turn forgery, Windows constrained, timeout, cancel/restart, grandchildren: policy/preflight/workspace gates run before any background submit (shared with foreground); background adds session-required + service-required gates; Native/Git/direct background rejected; `job` exact-owner scope (M001); grandchildren cleanup via scheduler `cancel_descendants` (re-verified by `managed_process_descendants` 5/5 + `scheduler_authority_matrix` 13/13).
- No command-regex barrier introduced (`check_git_forbidden_patterns` PASS).
- Secrets: bounded summaries only (M001); no new logging of command text beyond existing foreground behavior.
- Sandbox: `Required` remains fail-closed where unsupported (untouched; `verify.sh quick` sandbox guard green).

## 9. Documentation and operations

- `architecture/command_intent.md`: "Background Bash (M002)" (routes, keys, adopted Shell, rejections, consistency, control tool).
- `architecture/jobs.md` / `scheduler.md` / `authorization.md`: M001 sections apply unchanged (bash reuses the same contract/bridge/owner rule; no new authority).
- Tool catalog/discovery: `bash` description + `execution_mode` enum (poll-free guidance); `job` tool unchanged.
- `docs/execution-ownership.toml`: unchanged (no new owner; bash stays `scheduler`, job stays `scheduler` observation/cancel). Guards green.
- Operator: background rejections are explicit actionable errors naming the foreground or scheduler-backed alternative; notices via M001 channel.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted CI / Desktop E2E not run; evidence local `verify.sh quick` + focused suites | No hosted sweep proof on this SHA | Future hosted run may reference; no rework unless falsified |
| low | Native macOS/Windows not exercised; Windows PowerShell/Cmd/Git Bash/WSL + constrained-mode background untested | Platform-specific shell launcher/sandbox background unverified | Recorded honestly; M003+ should include native evidence where available; no pretend guarantee |
| low | No live-provider background-bash trajectory | Safe-boundary delivery proven by unit/integration, not live model turn | M003 live-turn qualification should include one bash background end-to-end |

No critical/high/medium findings. No corrective pass required. Stop condition not triggered (scheduler-owned without policy downgrade or semantic drift proven).

## 11. Roadmap disposition

Milestone closed. Unblock audit: **nothing newly unblocked** — M003 was already ready via M001 closure; M004 remains blocked on M003. No downstream status change follows from M002.

## 12. Registry updates

- `plans/registry.md` Active subsystem roadmaps row: `active (M001 closed; M002–M003 ready; M004 blocked)` → `active (M001–M002 closed; M003 ready; M004 blocked)`; current milestone text updated.
- `plans/registry.md` Dependency-ready plans: M002 row `ready` → `closed` (closure `plans/closure/async-background-jobs/002-status.md`, implementation `19028149`).
- `plans/registry.md` Blocked work: unchanged (M004 still blocked on M003).
- `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md`: M002 `ready` → closed.
- `plans/implementation/async-background-jobs/002-*.md`: `ready for handoff` → `implemented`.
