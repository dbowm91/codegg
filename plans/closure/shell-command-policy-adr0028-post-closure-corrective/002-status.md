# Shell Policy ADR-0028 Post-Closure C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/shell-command-policy-adr0028-post-closure-corrective/002-advisory-shell-test-safety-and-contract-closure.md`

Source subsystem roadmap:

- `plans/subsystems/shell-command-policy-adr0028-post-closure-corrective-addendum.md#C002`

Repository baseline reviewed: `8ffb3cce` (planning branch head before implementation)

Implementation commits:

- `b0392431` — test(shell,git): ADR-0028 advisory shell fixtures and typed git-clean denial (shell half + guard + agent-loop fix; this closure's evidence was gathered on this tree)

## 1. Executive finding

C002 is complete. All shell-policy regression tests are now consistent with
accepted ADR-0028 and safe to execute in CI: built-in catastrophic and
syntax-risk patterns are advisory, the default blocked-command set is empty
by design, and terminal authority is explicit operator configuration,
permission, and sandbox enforcement. No destructive shell command is executed
by any test to prove a textual denial. The stale permission-module comment
claiming catastrophic shapes stay hard-denied in the tool is corrected, a
static test-safety guard with positive/negative fixtures runs in local quick
and hosted CI, and historical C001 remains preserved as superseded evidence.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Zero host-destructive execution tests in shell-policy/terminal suites | `tests/tool_execution.rs` rewritten; `scripts/check_shell_policy_test_safety.py` + `--self-test` | pass | Three unsafe `BashTool::execute` sites removed (see §3); guard proves no function mixes destructive literals with dispatch |
| Pure risk-classifier fixtures cover formerly hard-denied table; default blocked set stays empty | `CATASTROPHIC_ADVISORY_FIXTURES` + `find_ask_pattern` (integration); `catastrophic_shapes_are_advisory_risk_signals` + new `default_blocked_commands_is_empty_by_design` (lib) | pass | 8 catastrophic spellings as inert data, never executed; lib proves `check_shell_security` with empty default accepts them |
| Harmless configured hard deny stays terminal under permissive allowlist with zero dispatch | `test_bash_tool_configured_blocked_commands_deny_without_dispatch`; `test_bash_tool_configured_deny_survives_permissive_allowlist`; TempDir markers | pass | `printf` sentinel + `blocked list` message assertion; marker absent in both cases |
| Concrete risk/privilege targets Ask; syntax-only shapes stay quiet; explicit Deny wins | `CONCRETE_RISK_ASK_FIXTURES` (6), `PRIVILEGE_CREDENTIAL_ASK_FIXTURES` (7), `SYNTAX_ONLY_QUIET_FIXTURES` (10); `test_bash_explicit_deny_wins_and_allow_excuses_heuristic` | pass | Default checker: Ask for first two tiers, Allow for syntax-only; Deny outranks all, explicit Allow excuses heuristic Ask |
| AgentLoop Ask→AllowOnce and Deny/sandbox-unavailable paths marker-backed | `shell_escalation_ask_waits_for_explicit_approval_before_dispatch` repaired with configured `bash_ask_patterns` + TempDir sentinel; existing `hard_deny_always_wins`, sandbox `rejected_seatbelt_profile_does_not_execute_the_target` | pass | Pending observed, marker absent pre-approval, present post-AllowOnce; no dangerous command executed |
| Guard runs in local quick and hosted CI; no new parser/blocklist/runtime policy | `scripts/verify.sh` quick + `.github/workflows/ci.yml` wiring; `architecture/tool.md:718`, `architecture/permission.md:177-183`, `docs/security-semantics.md:41-56` audited (already advisory) | pass | Only production comment changed is `src/permission/mod.rs:1216-1217`; no tool schema, config, or approval semantics changed |
| Focused tests pass; final-SHA evidence recorded honestly | §4 commands/results | pass | Hosted CI not run (see §4); whole-workspace CI red for separately owned Git auth tests is a qualified operational dependency, not a C002 failure |

## 3. Production implementation evidence

Removed unsafe execution sites (static list):

- `tests/tool_execution.rs::test_bash_tool_denied_pattern` — `BashTool::new().execute` with `rm -rf /`.
- `tests/tool_execution.rs::test_bash_tool_hard_deny_tier_blocks_catastrophic_commands` — loop over `HARD_DENY_COMMANDS` (`mkfs`, `dd if=/dev/zero`, `chmod -R 777 /`, `shutdown`, `reboot`, `rm -rf /`, `mkfs.ext4 /dev/sda1`, `dd if=/dev/zero of=/dev/sda`, `shutdown now`) through `BashTool::execute`.
- `tests/tool_execution.rs::test_bash_tool_hard_deny_tier_survives_a_permissive_allowlist` — same shapes with a permissive allowlist through `BashTool::execute`.
- `tests/tool_execution.rs::ESCALATION_SIGNALS` stale Ask expectations for six syntax-only shapes (`echo $(whoami)`, backticks, `eval echo hello`, `eval(echo hi)`, `exec ls`, `base64 -d`); replaced by the three-way contract tables.
- `tests/agent_loop_harness.rs::shell_escalation_ask_waits_for_explicit_approval_before_dispatch` — `echo $(touch marker)` no longer raises Ask under ADR-0028 (plain substitution is ordinary programming); replaced with a benign `printf` sentinel plus an explicitly configured `bash_ask_patterns` (`*shell-dispatched*`) rule writing only inside the fresh TempDir.

Landed changes (implementation `b0392431`):

- `tests/tool_execution.rs`: inert catastrophic fixtures, configured-deny zero-dispatch tests, benign allowlist/`deny_all` controls, three-way escalation tables, explicit Deny/Allow precedence test. No `BashTool::execute` call in this file receives a host-global destructive string.
- `src/tool/bash/policy.rs`: new `default_blocked_commands_is_empty_by_design` lib test pinning the ADR-0028 empty-default contract.
- `src/permission/mod.rs`: corrected the `Genuinely catastrophic shapes stay hard-denied` comment to state the ADR-0028 advisory truth (built-in risk never implies host safety; explicit deny/permission/sandbox are authoritative; FullHost leaves host-global risk).
- `tests/agent_loop_harness.rs`: approval-path regression now uses the configured-Ask sentinel described above.
- `scripts/check_shell_policy_test_safety.py` (new): bounded regex guard over the five audited test targets, stale-identifier rejection, reviewed-exception mechanism, documented AST/dataflow limits, in-memory positive/negative/stale/exception fixtures plus live-tree check via `--self-test`.
- `scripts/test_shell_policy_test_safety.py` (new): fixture runner required by the plan's verification commands.
- `scripts/verify.sh` quick + `.github/workflows/ci.yml`: both guard entry points wired before cargo tests.
- Bash/terminal authority confirmed same tier: `tool_category_for_name` maps `bash | terminal | test | python_script` to `ShellExec` (`src/permission/mod.rs:151-184`); no new execution path introduced.

Old-vs-new assertion matrix:

| Old (C001, superseded) | New (C002, ADR-0028) |
|---|---|
| `BashTool::new().execute("rm -rf /")` → `Permission` | Catastrophic strings never executed; `find_ask_pattern("rm -rf /")` → `Some("rm -rf /")` advisory; `check_shell_security` with empty default → `Ok` (lib) |
| `HARD_DENY_COMMANDS` loop expects built-in hard deny incl. under allowlist | `with_blocked_commands(["printf"])` + `with_allowlist(["printf","echo"])` on a benign sentinel → `Permission("blocked list")`, marker absent |
| All 11 `ESCALATION_SIGNALS` expect `Ask` incl. bare `$(...)`/backtick/`eval`/`exec`/`base64 -d` | 6 concrete-risk + 7 privilege/credential fixtures → `Ask`; 10 syntax-only shapes → `None`/`Allow`; explicit Deny wins, explicit Allow excuses heuristic |
| Agent-loop Ask via `echo $(touch marker)` syntax | Agent-loop Ask via explicitly configured `bash_ask_patterns` on a benign TempDir sentinel |

Historical `plans/closure/shell-command-policy-ci-corrective/001-status.md`
and `plans/closure/command-authorization-and-execution-boundaries/005-status.md`
are unchanged.

## 4. Verification executed

### Commands run

```bash
python3 scripts/check_shell_policy_test_safety.py
python3 scripts/test_shell_policy_test_safety.py
python3 scripts/check_shell_policy_test_safety.py --self-test
cargo fmt --all -- --check
cargo test -p codegg --lib -- tool::bash::policy
TMPDIR=/tmp cargo test -p codegg --test tool_execution
TMPDIR=/tmp cargo test -p codegg --test permission
TMPDIR=/tmp cargo test -p codegg --test approval_router
TMPDIR=/tmp cargo test -p codegg --test agent_loop_harness shell_escalation_ask
TMPDIR=/tmp cargo test -p codegg --test sandbox_containment
python3 scripts/check_approval_router.py
python3 scripts/check_policy_surface.py
TMPDIR=/tmp bash scripts/verify.sh quick
TMPDIR=/tmp cargo clippy --workspace --all-targets --locked -- -D warnings
```

### Results

- Guard: `check_shell_policy_test_safety: ok`; fixtures: pure-classifier accept + executable-destructive reject + stale-identifier reject + reviewed-exception honor + audited tree clean (`self-test ok`).
- `cargo fmt --check`: clean.
- Lib `tool::bash::policy`: 55 passed, 0 failed.
- `tool_execution`: 61 passed, 0 failed (includes 5 new/rewritten C002 tests).
- `permission`: 53 passed, 0 failed.
- `approval_router`: 17 passed, 0 failed.
- `agent_loop_harness shell_escalation_ask`: 1 passed.
- `sandbox_containment`: 15 passed.
- `check_approval_router.py`: passed. `check_policy_surface.py`: ok.
- `verify.sh quick`: passed (fmt, agent schema, core/client/desktop boundaries, sandbox, ADR, execution-ownership, TUI authority, route disposition, audit, scheduler-bypass, config-merge, provider wire/catalog/resilience, OpenAI endpoint, Eggwork routing, shell test-safety ×2, workspace `cargo check`).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
- Hosted Linux CI on the exact implementation SHA: **not run** (local-only verification; branch not pushed at evidence time). No hosted run is claimed. Whole-workspace nextest remains red for the separately owned Git authorization tests (see Git C001 closure); that is a qualified operational dependency, explicitly not C002 evidence.
- Environment note: `cargo test` on this macOS host required `TMPDIR=/tmp`; the default `TMPDIR` (`/var/folders/...`) failed temp-dir creation during compilation (`couldn't create a temp dir ... target/debug/deps/rmeta...`). All test counts above were gathered with `TMPDIR=/tmp`.

## 5. Invariant review

- Command syntax by itself never creates built-in terminal Deny: `default_blocked_commands` empty (new lib test), `check_shell_security` accepts catastrophic strings (existing lib test), integration asserts advisory `Some` without execution. Holds.
- No test invokes a fork bomb, root deletion/permission change, device wipe, or power transition on any OS: static guard + manual audit of every `execute` call in the five audited targets (all use `echo`/`printf` sentinels or pre-existing benign commands). Holds.
- Pure classifiers accept dangerous strings as data without a shell/process: fixtures call only `find_ask_pattern`. Real-tool tests use TempDir markers with zero-dispatch assertions. Holds.
- Explicit operator Deny and `bash_deny_patterns` override permissive grants in documented precedence: new precedence test proves Deny wins and Allow excuses heuristic Ask; sandbox/permission ceilings untouched. Holds.
- Managed execution requires ApprovalRouter/sandbox; direct tool calls prove no approval decision: agent-loop test proves Pending→AllowOnce→dispatch with marker evidence; `check_approval_router.py` passes. Holds.
- No widening of FullHost/Yolo or bypass via terminal/alternate interpreter/nested agent: terminal shares `ShellExec` tier; no new route added; FullHost risk explicitly documented. Holds.
- Historical records intact: C001 and command-auth closures unedited; supersession stated here and in-tests. Holds.

## 6. Failure and recovery review

- A rejected invocation has no dispatch, marker write, or unexpected approval: proven by marker-absent assertions on both configured-deny tests and the allowlist control.
- An allowed invocation is recorded by the same audit/authorization owner: agent-loop AllowOnce path dispatches and writes the marker; no alternate owner introduced.
- Required sandbox unavailable fails before launch even after Ask approval: covered by existing `rejected_seatbelt_profile_does_not_execute_the_target` (15/15 sandbox suite green); no change needed.
- Cancelled approval does not dispatch: existing ApprovalRouter semantics unchanged (`check_approval_router.py` green).
- FullHost remains explicitly unsafe under broad grants: documented in the corrected permission comment and `docs/security-semantics.md`; tests do not mask it with regex claims.
- Restart/saved approvals retain precedence/scope: untouched; precedence test confirms Deny still outranks remembered Allow.

## 7. Migration and compatibility review

No storage migration, approval receipt, permission mode, protocol, tool
schema, config key, or persistence change. `BashTool::new` defaults
unchanged (empty blocked set, no allowlist, no deny-all). Marker fixtures
use unique TempDir paths per test (nextest-parallel safe) and never touch
paths outside TempDir. No broad family grants for compounded commands.

## 8. Security review

No managed approval or sandbox bypass was found; had one emerged the plan
required stopping rather than masking with text denies. No new hardcoded
regex denial, ad hoc shell parser, unsafe FullHost fallback, or terminal/Git
authority change. Secrets: none touched; no logging of secrets. The guard is
defense-in-depth only and is documented as not replacing human review.

## 9. Documentation and operations

- Corrected: `src/permission/mod.rs` catastrophic-hard-deny comment (the one genuinely contradictory site).
- Rewrote: `tests/tool_execution.rs` contract header (ADR-0028 successor notice with C001 supersession link).
- Audited, no change needed (already advisory): `architecture/tool.md:345,718`, `architecture/permission.md:177-183`, `architecture/security.md:155,323,497`, `docs/security-semantics.md:37-56,58-78`, `src/tool/bash/policy.rs` module docs + risk tables, `plans/registry.md:28` (already states advisory/empty/superseded).
- Operations: `scripts/check_shell_policy_test_safety.py --self-test` is the diagnostic; reviewed exceptions require a `shell-test-safety-exception` comment with rationale.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted exact-SHA Linux CI not run for this closure | No hosted proof of the new guard/tests on CI runners | Future hosted `CI / verify` run on the closure head; local evidence is complete and honestly labeled |
| low | Whole-workspace nextest still red on separately owned Git authorization tests (reset/network stale positives) | Unrelated to shell scope; tracked in Git C001 closure | Owned by Linux-containment follow-up, not this milestone |

No critical/high/medium findings. No known high-severity defect remains.

## 11. Roadmap disposition

Milestone C002 closed. The `shell-command-policy-adr0028-post-closure-corrective`
workstream has no further milestone (its addendum defines C002 as the
terminal verification closure), so its roadmap moves to closed. No later
implementation plan is registered or newly unblocked by this closure.

## 12. Registry updates

- `plans/registry.md` Active subsystem roadmaps: Shell-policy ADR-0028 row → `closed`, current milestone `C002 closed at b0392431 (closure plans/closure/shell-command-policy-adr0028-post-closure-corrective/002-status.md)`, blockers `—`.
- `plans/registry.md` Dependency-ready plans: Shell-policy C002 row → `closed`, implementation `b0392431`.
- `plans/registry.md` execution-gate note (§Current execution order): the `Do not run tool_execution or full workspace tests on this baseline` warning is discharged for this branch head — the unsafe invocations are removed and the guard runs before tests. The Git-clean half of the gate is discharged by the sibling Git C001 closure in the same push.
- `plans/subsystems/shell-command-policy-adr0028-post-closure-corrective-addendum.md`: Status → `closed — C002 closed`; C002 plan link marked implemented/closed.
- `plans/implementation/shell-command-policy-adr0028-post-closure-corrective/002-advisory-shell-test-safety-and-contract-closure.md`: Status → `implemented; closed`.
- Unblock audit (per `plans/003-planning-process.md` §11): reviewed the registry Blocked work section and affected subsystem dependency graphs. No registered plan lists shell C002 as a hard/interface dependency; Linux C002/C003 remain blocked on ADR-0027 + native proof, Windows C045/C046/C048 on negative C043/C044, search M001/M002 on eggsearch parity, advisor M004 lines on negative qualifications. **Nothing newly unblocked — recorded explicitly.**
