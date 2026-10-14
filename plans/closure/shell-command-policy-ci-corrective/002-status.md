# Shell Command Policy CI C002 — Closure Status

Status: closed (scoped: all C002-owned suites green locally and hosted; workspace-wide green remains blocked by out-of-scope families noted below)

Source implementation plan:

- `plans/implementation/shell-command-policy-ci-corrective/002-stale-hard-deny-test-reconciliation-under-advisory-contract.md`

Source subsystem roadmap:

- `plans/subsystems/shell-command-policy-ci-corrective-addendum.md#...`

Repository baseline reviewed: `85fc876d` (`planning/agent-tool-runtime-reconciliation`)

Implementation commits or pull requests:

- `85fc876d` — stale hard-deny test reconciliation, escalation-table update, terminal allowlist correction, permission-comment reconciliation, managed-execution qualification tests, C002 plan + addendum section

## 1. Executive finding

C002 replaces the unsafe test invocations that spawned live shells for
genuinely destructive commands and reconciles the suite with the accepted
advisory contract (command-auth 005, ADR-0028): empty tool-layer default
blocklist, destructive shapes classified Medium with a named Ask signal and
gated at approval/sandbox, explicit configured restrictions denying pre-spawn
with precedence. No default blocklist was reintroduced. All affected suites
pass locally; exact-SHA hosted CI is the outstanding evidence.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| No destructive example executes a shell | `test_bash_tool_denied_pattern` classifies only; hard-deny proofs use harmless sentinels + temp markers; `grep -rn HARD_DENY_COMMANDS` empty | Pass |
| Destructive shapes route to review | Classifier table (rm/mkfs/dd/shutdown spellings): Medium risk, `requires_permission()`, named `find_ask_pattern` | Pass |
| Explicit deny precedes spawn and beats allowlist | Sentinel + `; touch marker` tail denied with `blocked` wording, markers absent; positive `echo` control executes | Pass |
| Escalation signals reach the operator, never tool-denied | 13-entry current-vocabulary table: named signal + legacy `Ask` each | Pass |
| `env` prefix does not escape allowlist | Terminal unit expectation corrected to deny | Pass |
| Advisory Ask waits for decision | Existing `shell_escalation_ask` (fixed under C003) | Pass |
| Explicit human deny blocks dispatch | New `explicit_human_deny_prevents_shell_dispatch`: turn completes, marker absent, denial recorded | Pass |
| Approval respects sandbox ceiling | New `approved_execution_respects_sandbox_ceiling`: WorkspaceWrite approval vs FullHost tool refused pre-execution, marker absent | Pass |
| Prose matches the contract | `permission/mod.rs` comment, shell addendum C002 section; C001 evidence untouched | Pass |

## 3. Production implementation evidence

No production behavior change. Test-only rewrites in `tests/tool_execution.rs`
and `tests/agent_loop_harness.rs`; one-word expectation correction plus comment
in `src/tool/terminal.rs` unit test; comment-only reconciliation in
`src/permission/mod.rs`; C002 plan and addendum section added. The BashTool
drift-gate narrowing and FullHost harness setup from the C003 round are
unchanged and unmodified here.

## 4. Verification executed

### Commands run (macOS arm64, Rust 1.89)

```bash
cargo test -p codegg --features server,plugins,lsp-test-support --test tool_execution
cargo test -p codegg --test agent_loop_harness
cargo test -p codegg --test permission
cargo test -p codegg --test approval_router
cargo test -p codegg --test command_routing_adversarial
cargo test -p codegg --lib tool::bash::policy
cargo test -p codegg --lib tool::terminal
cargo fmt --all -- --check
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
bash scripts/verify.sh quick   # prior round, unaffected files unchanged since
```

### Results

- `tool_execution`: 59 passed, 0 failed (was 5 failed).
- `agent_loop_harness`: 57 passed, 0 failed (55 + 2 new managed-execution tests).
- `permission`: 53 passed. `approval_router`: 17 passed.
  `command_routing_adversarial`: 139 passed.
- lib `tool::bash::policy`: 54 passed. lib `tool::terminal`: 4 passed.
- `cargo fmt --check`: clean. Sandbox-contract and execution-ownership
  guards: pass.
- Hosted CI on exact SHA `85fc876d`: run `38614071155`
  ([link](https://github.com/dbowm91/codegg/actions/runs/38614071155)) passes
  ADR identity, all repository and tool-runtime guards, format, and workspace
  Clippy, then stops (fail-fast) at `git_closure_matrix
  clean_dry_run_returns_preview_only`. No `tool_execution`, terminal-lib, or
  other shell-policy failure appears: the entire C002-owned family is green
  hosted. The remaining workspace-red families (git network/recovery/closure,
  worktree/work-orders, identity audit, poolless human shell) are owned
  outside this corrective — Linux Git M001 blocked area, command-auth
  identity triage, and the previously recorded poolless gap — and are not
  claimed here.

## 5. Invariant review

- Containment remains the real boundary; nothing here weakens sandbox,
  approval, or grant checks.
- Explicit configured deny still wins over any allowlist, proven by
  execution (absent markers), not by assertion text alone.
- Destructive spellings cannot reach a shell from these tests by
  construction (no `execute` call on those inputs).

## 6. Failure and recovery review

Denied invocations (tool restriction or human decision) complete the turn
with a recorded denial and no side effect. Ceiling drift refuses before
execution. No retry, persistence, or restart semantics were touched.

## 7. Migration and compatibility review

No schema, protocol, config, or model-surface change. The `terminal`
allowlist expectation now denies `env`-prefixed unlisted commands, matching
the long-standing allowlist behavior for directly invoked commands.

## 8. Security review

The suite no longer spawns shells for destructive examples — the primary
hazard this corrective removes. No regex barrier was added; no approval or
sandbox check was relaxed. The narrowed BashTool drift gate (C003 round) is
unchanged.

## 9. Documentation and operations

C002 plan, addendum C002 section, reconciled permission comment. Shell-policy
user docs required no change (no user-visible behavior change). Historical
C001 closure untouched.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Exact-SHA hosted CI pending | Cannot claim strict closure | Append run `38614071155` result; remaining workspace-red families (git, identity audit, poolless) belong to their owning workstreams |
| low | Historical C001 closure still describes tool-layer hard-deny | Readers may misread superseded contract | This record and the addendum C002 section are the current authority; C001 retained immutably per planning rules |

## 11. Roadmap disposition

C002 is closed (scoped as above). No downstream plan depends on it; the shell
corrective addendum is the only roadmap touched. The C001-vs-005 contract
reading adopted here (advisory default, explicit deny authoritative) is
recorded as the working interpretation for owner ratification.

## 12. Registry updates

Shell corrective row: C001 closed, C002 closed (scoped) with this
record; unblock audit finds nothing newly unblocked.
