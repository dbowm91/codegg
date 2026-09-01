# M004 Closure — Isolated Mutation and Structured Results

Status: closed

Implementation commit: `37b9cc9c9442fbca20fa63072581b4be1067deaf`

Repository baseline: `b08d33b7e52bde1bde1ddcddeeee3c7c157a4103`

Source plan: `plans/implementation/agent-run-worktree-concurrency/004-isolated-mutation-and-structured-results.md`

## 1. Executive finding

M004 is strictly closed. Mutation-capable durable delegated runs are classified
from resolved authority, allocated a durable managed worktree before model
execution, and bound to that root for file, shell, terminal, Git, test, and LSP
execution. Authorized children can make local commits only in their owned
worktree. Results are bounded and machine-readable, with Git-derived state and
scheduler validation evidence. Parent integration is an explicit typed
operation and completion does not mutate the parent branch.

No unresolved high or medium finding remains in the M004 scope.

## 2. Requirement-to-evidence matrix

| Requirement | Production evidence | Verification evidence |
|---|---|---|
| Automatic mutation classification and isolation | `SubAgentPool::requires_isolation`; `SubagentJobExecutor` allocates a `WorktreeService` lease in `Preparing` before `send_and_wait_scheduled` | `scheduler_contention`, `scheduler_cancellation`, `worktree` suites; `check_scheduler_bypass.py` |
| Read-only authority remains narrow | `execute_agent_task` applies the safety envelope; read-only agents retain hard denies for mutation/Git/shell tools | `isolated_child` tests; full default and feature-enabled suites |
| Same-root execution | Worktree path replaces child `allowed_paths`/`workspace_root`; tool registry configures root-scoped file tools, terminal, Git, commit, Bash, and test execution | `check_daemon_cwd_usage.py`, `check-core-boundary.sh`, Bash path-negative test, full verification |
| Scoped child commit | `ChildGitPolicy::LocalCommitOnly` permits only stage/unstage/commit under inherited `GitWrite`; network/history/destructive operations remain gated | Git isolated-child policy tests and `cargo test --lib git` |
| Bounded structured result | `codegg_core::run_result::AgentRunResult`, schema migration v40, SQLite/in-memory persistence, `bounded()`/`encode_bounded()` | `agent_run`, `run_result`, and focused root tests |
| Git-derived state and validation/artifact evidence | `collect_agent_run_result` reads `egggit::status_v2`; scheduler test jobs emit bounded markers extracted from tool-result events | `admission_tests::scheduler_validation_markers_become_structured_evidence`; focused checks |
| Explicit integration and conflict handling | `AgentRunIntegrationService` validates run/worktree/base/repository lineage and dispatches typed merge/cherry-pick/rebase operations | Git mutation integration/closure suites and integration precondition implementation review |
| Dirty/conflicted retention and recovery | Worktree refresh/release uses M003 cleanup policy; conflicts produce `RequiresRecovery` and a recovery hint; no automatic parent merge | `worktree`, `scheduler_cancellation`, `scheduler_restart_recovery`, and Git recovery suites |

## 3. Production evidence

- Durable preparation links the run to its attempt, transitions it through
  `Preparing`, creates and attaches the managed worktree, then transitions to
  `Running` before constructing the child request.
- Missing durable run identity, repository identity, workspace root, worktree
  service, allocation, or linkage fails preparation explicitly; it never falls
  back to concurrent shared mutation.
- Child authority is derived from the resolved agent permission surface and
  inherited denied tools. Unknown or writable surfaces select isolation
  conservatively.
- The result DTO records run/worktree identity, base/result commits, changed
  paths, repository state, retryability, recovery hints, findings, validation,
  and artifact references. The result is persisted in the durable run store and
  is not reconstructed from final prose.
- Test/build validation remains scheduler-owned. Test completion status and job
  identity are carried through a bounded internal marker and become typed
  validation/artifact entries in the durable result.
- `AgentRunIntegrationService` is parent-side and explicit. It checks the
  managed worktree base, repository root lineage, clean target state, and target
  head before invoking the canonical typed Git mutation operations.

## 4. Verification commands and results

The following exact focused checks passed on the implementation tree:

```text
rtk cargo check -p codegg-core -p codegg --locked                         PASS
rtk cargo test -p codegg-core agent_run --locked -- --test-threads=1       PASS (4)
rtk cargo test --lib admission_tests --locked -- --test-threads=1         PASS (7)
rtk cargo test --lib isolated_child --locked -- --test-threads=1           PASS (2)
rtk cargo test -p codegg-core worktree_service -- --test-threads=1        PASS (8)
rtk cargo test --test scheduler_contention -- --test-threads=1            PASS (14)
rtk cargo test --test scheduler_cancellation -- --test-threads=1          PASS (10)
rtk cargo test --test worktree -- --test-threads=1                         PASS (14)
rtk cargo test --lib git -- --test-threads=1                               PASS (296)
rtk cargo fmt --all                                                         PASS
rtk git diff --check                                                        PASS
rtk bash scripts/check-core-boundary.sh                                     PASS
rtk python3 scripts/check_daemon_cwd_usage.py                               PASS
rtk python3 scripts/check_execution_ownership.py                             PASS
rtk python3 scripts/check_scheduler_bypass.py                               PASS
rtk python3 scripts/check_git_forbidden_patterns.py                         PASS (0 findings)
rtk python3 scripts/generate_builtin_agents.py --check                      PASS
```

The repository’s capped broad verification also passed with default features
and with `server,plugins,lsp-test-support`: default workspace tests, Clippy,
doc tests, and the feature-enabled root suite all completed with zero failures.
The exact committed tree additionally passed the focused checks above after the
final validation-marker change. The only build diagnostic was the known
non-fatal macOS linker section-size warning.

## 5. Invariant review

- Concurrent mutation-capable delegated runs do not share the parent/sibling
  working tree or index.
- Allocation and durable ownership precede child loop construction.
- Read-only children do not gain write authority from workspace reuse.
- Worktree ownership narrows inherited authority and cannot widen it.
- Local child commits are restricted to an owned worktree plus inherited
  `GitWrite`; push, network, destructive history, remote/config, and broad
  cleanup remain independent policy decisions.
- Parent integration is explicit and typed; completion alone has no parent
  mutation side effect.
- Result state is machine-derived and bounded.
- Dirty and conflicted worktrees remain inspectable under M003 cleanup rules.
- Scheduler resource/exclusivity control remains responsible for shared caches,
  ports, databases, and validation jobs.

## 6. Failure, cancellation, and recovery

Preparation failures terminalize the durable run before model execution. Child
pool failures and cancellation persist a structured failed/cancelled result;
worktree refresh/release is attempted without force-removing dirty state.
Conflicted state maps to `RequiresRecovery` with an actionable hint. Existing
M001-M003 restart reconciliation remains authoritative, and the typed result
store upserts by run ID so result delivery is idempotent. No code path retries a
commit merely because result persistence or completion delivery was delayed.

Integration refuses stale base/target state and reports typed Git conflicts;
it does not silently merge or resolve them.

## 7. Migration and compatibility

Migration v40 adds the `agent_run_result` table and a bounded JSON check while
preserving existing run/task records and legacy `result_ref` behavior. The
storage layout version is 40. Existing constructors remain available, with the
new worktree-aware scheduler registration as an additive path. Existing
read-only delegation and manual worktrees remain compatible. `SubAgentReport`
continues as a presentation adapter; structured result persistence is the
machine authority.

## 8. Security review

The child root is installed in every relevant tool configuration and Bash and
terminal apply explicit child-root escape checks. File/Git path policies remain
independent of model claims. The child Git policy rejects push, remote/config,
reset/clean/history-rewrite and other non-local mutation families even when a
local commit is allowed. The core-boundary, Git forbidden-pattern,
execution-ownership, scheduler-bypass, and daemon-path guards all pass.

## 9. Documentation and operations

Updated:

- `architecture/agent.md`
- `architecture/git.md`
- `architecture/scheduler.md`
- `architecture/worktree.md`
- `assets/prompts/agents/general.md`
- `src/tool/task.rs` tool contract
- generated built-in agent assets

The plan, roadmap, registry, and this closure record are updated together in
the status-transition commit.

## 10. Unresolved findings

None at high, medium, or low severity within M004 scope. The child-shell token
check is intentionally defense-in-depth; canonical workdir/path policy,
structured Git policy, and the existing platform sandbox remain the primary
enforcement layers. This is documented behavior, not an open correctness
finding.

## 11. Roadmap disposition

M004 is closed. M005 — run groups and scheduler-backed background handles — is
now ready because its declared dependencies M002 and M004 are closed. M006
remains blocked because it additionally requires M005; no other registered
future plan became ready in this audit.

## 12. Registry update

`plans/registry.md` now records M004’s closure record and implementation commit,
advances the active subsystem roadmap to M005 ready, removes M005 from blocked
work, and retains M006 as blocked on M005. The source roadmap status table and
the M004 implementation plan agree with that disposition.
