# Command Authorization M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/command-authorization-and-execution-boundaries/001-syntax-blocker-audit-and-classification.md`

Source subsystem roadmap:

- `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md#6-dependency-graph`

Repository baseline reviewed: `7b72582c4f01f8a53c7f20ed5dde1bc620f82965`; aggregate implementation reviewed at `66778184`.

Implementation commits: `fc4520df` (precedence/scoped grants), `696e7a4d` (advisory classifier/preflight), `ee01106f` (low-friction qualification), and `66778184` (equivalent outside-write effects).

Child closure records: C001 `004-status.md`; C002 `005-status.md`; C003 `006-status.md`, all under `plans/closure/command-authorization-and-execution-boundaries/`.

## 1. Executive finding

Parent M001 is complete. Built-in command text and heuristic risk classifications
no longer claim authority to prevent equivalent execution. The original blocker
owners were inventoried and reconciled below. Explicit operator-selected text
preferences, permission Deny, invalid typed requests, disabled tools, protected
scope, typed Git capability outcomes and required sandbox failure remain
distinct. Risk/preflight findings reach the existing ApprovalRouter.

The 62-command routine corpus produced no Ask or Deny for an already-authorized
workspace/action. A separate Linux effect test attempted the same outside-root
write through a shell, Python, and compiled native executable under the
production `WorkspaceWrite` launch spec; Landlock denied all three. This
qualifies filesystem enforcement equivalence for these fixtures only. The
backend reports no network isolation; Unix IPC is not claimed as confined.
M002 may proceed. This closure does not promote a Linux network or IPC backend.

## 2. Requirement-to-evidence and blocker inventory

| Owner / blocker surface | Origin | Decision owner and disposition | Evidence / status |
|---|---|---|---|
| `src/tool/bash/policy.rs`: prior catastrophic regex/literal hard blocks | Built-in heuristic | `find_risk_pattern` supplies review metadata; default blocked literals are empty. Explicit caller-supplied blocked lists remain invocation restrictions. | C002 §3; Bash policy tests; destructive inputs were not executed. |
| Bash wrapper/interpreter/expansion/composition Ask patterns | Built-in heuristic | Only concrete harmful targets/authority signals escalate. Ordinary syntax alone does not. | C002 §2/§3; C003 62-command fixture. |
| `src/tool/destructive.rs::destructive_match` | Built-in catastrophic-risk heuristic | Produces Ask through permission policy; never claims kernel denial. | `tests/permission.rs::low_friction_corpus_keeps_risk_review_and_explicit_deny`; Bash policy tests. |
| `src/permission/mod.rs`: allow/ask/deny patterns, saved decisions, rule precedence | Explicit preference / authenticated persisted decision | Explicit Deny remains terminal; tier precedence remains; compound shell does not reuse a first-token grant. Text filters are best-effort. | C001 §2/§3; permission and policy-surface tests. |
| `src/security/policy.rs`: critical severity and `security.denied_commands` | Heuristic severity vs explicit user text policy | Critical risk is advisory by default; explicit `deny_critical_commands=true` and explicit `denied_commands` retain denial. Omitted config is presence-aware. | C002 §2/§4; security/schema/layered-merge tests. |
| `src/preflight/service.rs`, `BlockOnDefinite` | Heuristic risk vs typed tool correctness | Command risk/high score/text block warns and routes to review. Malformed structured input, invalid typed args and validator failures retain deterministic Block. | C002 §2; preflight unit/integration suites (40 + 72). |
| `src/agent/tool_batch.rs`, `src/agent/loop.rs`, `src/agent/coordinator.rs` | Permission/security/preflight/sensitive-path policy | Permission/security Deny precedes escalation. Advisory reasons aggregate into the existing ApprovalRouter request. | C002 §2; approval router 17/17; approval guard. |
| `src/agent/tool_inspect.rs`, terminal payload extraction | Route/input representation | Terminal composes `command` and `args` before permission classification; complex shell cannot be mislabeled as a safe typed family. | `tool_inspect` tests; command-routing suite 139/139. |
| `src/tool/bash.rs`, `src/tool/terminal.rs`: length, explicit blocked lists, child lexical check | Tool contract / defense in depth | Length and explicit restrictions remain terminal. Child lexical validation is preflight only; actual required sandbox launch remains authoritative. | Bash/terminal tests; sandbox-policy guard. |
| `src/shell/policy.rs`, human `!` shell | Human-owned confirmation policy | Separate confirmation/block/warn behavior is unchanged; agent Bash/Yolo semantics were not imported. | C002 §2/§8; source unchanged in C001–C003. |
| `src/command_intent/` and command routing | Route classification / typed invalidity | Shell grammar affects route confidence and backend choice, not security authorization; unsupported routes do not create authority. | `tests/command_routing_adversarial.rs`; C003 corpus. |
| `src/tool/git.rs`, `src/git_mutations.rs`, `crates/egggit/src/process.rs` | Typed Git operation and executable Git capabilities | Trusted non-executable operations remain available; executable filters/hooks/helpers require containment or return typed authorization/unavailable outcomes. No raw Bash fallback grants authority. | Linux C001 closure `plans/closure/linux-execution-isolation/009-status.md`; Git policy tests/guard. |
| ToolBroker, disabled tools, plugins/MCP, unknown tools | Typed contract/caller policy | Disabled/invalid calls stop at broker/schema/permission owner; unknown mutating surfaces remain Ask or unavailable, not command-text exceptions. | Broker/contract tests; execution-ownership and boundary guards. |
| Sandbox profile, child ceiling, workspace roots, required backend | Actual OS enforcement / explicit authority | Required confinement failure remains pre-spawn terminal. Approval cannot grant missing capability; child roots remain narrower. | `tests/sandbox_containment.rs` 14/14; policy-surface/sandbox guards. |
| Static policy/security guards and docs | Regression control / operator surface | Guards protect authority wiring and explicit limitations rather than treating spellings as a sandbox. | Git, approval, policy-surface, sandbox-wiring and execution-ownership guards. |

The inventory distinguishes heuristic signals, operator text preferences,
typed correctness/authority failures, and observed kernel enforcement. No
command string is treated as an effect boundary.

## 3. Production implementation evidence

- C001 corrected tool-rule precedence and saved shell grant scope. Current
  Deny cannot be masked by saved Allow; compound/ambiguous shell uses an exact
  command digest.
- C002 made built-in syntax and heuristic command-preflight risk advisory,
  preserved explicit Deny and typed invalidity, made the critical-command
  setting presence-aware, and routed advisory reasons through one existing
  ApprovalRouter decision.
- C003 qualified 62 Rust/Python/JavaScript/Go/shell/local-Git workflows under
  an already-authorized PermissionChecker context and updated operator docs.
- `66778184` extended the real Linux helper fixture. Shell, `/usr/bin/python3`,
  and a C executable compiled inside the temporary workspace attempted the
  same outside-root write. All reported enforced Landlock and left markers
  absent.
- No filesystem scope, FullHost behavior, network/credential grant, daemon
  authority, child lease or raw Git fallback was broadened.

## 4. Verification executed

Host: `Linux deadpool 6.8.0-146-generic #146-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 3 16:12:30 UTC 2026 x86_64`; Rust `1.99.0`, `x86_64-unknown-linux-gnu`. Helper status reports Landlock ABI 4, filesystem allowlist enforcement, `no_new_privs`, and `no network isolation`.

### Commands run

```bash
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib security
cargo test -p codegg --lib preflight
cargo test -p codegg --test preflight_integration
cargo test -p codegg --test permission
cargo test -p codegg --test command_routing_adversarial
cargo test -p codegg --test approval_router -- --test-threads=1
cargo test -p codegg --test policy_surface_m007
cargo test -p codegg --test sandbox_containment -- --test-threads=1
cargo test -p codegg-config paths::tests::test_merge_configs_replaces_default_backed_sections_whole
cargo test -p codegg-config schema::tests::critical_command_deny_gate_preserves_presence
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_approval_router.py
python3 scripts/check_approval_reviewer.py
python3 scripts/check_policy_surface.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
git diff --check
scripts/verify.sh quick
cargo run --bin codegg -- validate --config codegg.example.jsonc
```

Results: Bash 111/111; security 484/484; preflight 40/40; preflight integration
72/72; permission 53/53; command routing 139/139; approval router 17/17; policy
surface 14/14; sandbox containment 14/14. Both config presence/layering cases
passed 1/1 each. All listed guards, formatting, quick verification and example
config validation passed. The exact backend-status test observed Landlock ABI
4. All results are local; no hosted CI result is claimed.

The baseline C003 fixture at `7b72582c` observed five classifier review signals
in the 62-command corpus and zero hard blocks. Post-change policy results are
62 Allow, zero Ask and zero Deny for an already-authorized workspace/action.
These are different policy layers and do not establish a human-prompt
percentage; see C003 closure for measurement detail.

## 5. Invariant review

- Explicit Deny and child ceilings remain terminal and precede reviewer/Yolo
  handling. Approval does not change the captured sandbox profile.
- Built-in risk and a classifier miss do not grant authority. FullHost remains
  host-authorized and is not described as protected by syntax matching.
- Tool disablement, invalid input, out-of-scope paths and missing required
  confinement retain typed terminal outcomes.
- Shell, Python and native code received the same WorkspaceWrite filesystem
  boundary in the Linux fixture. This does not establish network, IPC,
  credential or all-syscall equivalence.
- Explicit text controls keep their behavior and are documented as
  invocation-level preferences. Signed scopes and legacy broad grants retain
  compatibility and documented limitations.
- Human `!` shell confirmation is unchanged.

## 6. Failure and recovery review

Scheduler ownership, cancellation, process reap, durable approvals and daemon
restart behavior did not change. Managed process routes remain owned by the
existing scheduler/service. Required sandbox setup failure remains pre-target.
Typed Git operations requiring unqualified executable features return
capability outcomes rather than spawning raw Git. Bounded output and secret
redaction are unchanged.

## 7. Migration and compatibility review

No database or permission-store migration was added. Saved scope signatures
and legacy unscoped rows retain existing formats. `deny_critical_commands`
deserializes as an optional boolean so omitted and explicit values remain
distinguishable; layered config preserves lower-priority explicit values when
higher layers omit the setting. Example config validation and signed-scope
compatibility tests pass.

## 8. Security review

Text filters are not equivalent-effect controls. The classifier is
intentionally incomplete and tests do not execute catastrophic commands.
Linux Landlock ABI 4 enforces the observed filesystem boundary; network stays
unrestricted, while Unix IPC and pre-opened descriptors are not claimed as
confined. Native macOS/Windows evidence is outside this closure.

## 9. Documentation and operations

Updated `architecture/permission.md`, `architecture/security.md`,
`architecture/tool.md`, `docs/security-semantics.md`, `docs/tools.md`,
`codegg.example.jsonc`, `CHANGELOG.md`, and TUI policy-selector copy. Planning
records and the registry describe explicit policy and native limitations.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Linux Landlock has no network isolation; Unix IPC confinement is not established | Workspace code can reach those host capabilities despite filesystem confinement | Keep those dimensions unrestricted/unavailable in claims until the Linux native plans close. |
| low | Native macOS and Windows command-friction/effect fixtures were not run | Platform-specific execution is not qualified by Linux evidence | Preserve platform-scoped claims; require native evidence before broadening guarantees. |
| low | The 62-command corpus measures policy outcomes rather than executing all commands in a full agent session | It proves no syntax-only Ask in the authorized fixture, not success for every runner | Qualify route-specific outcomes in M002/M003; do not claim all commands executed. |

No high-severity M001 finding remains. The medium limitation belongs to the
separate Linux execution-isolation workstream and is not resolved here.

## 11. Roadmap disposition

Parent M001 is closed. M002 approval/preflight/route convergence is ready.
M003 remains blocked on M002 and requires native evidence for each claimed
enforcement dimension. This closure promotes no Linux network or IPC guarantee.

## 12. Registry updates

- Marked parent implementation plan 001 closed and added this aggregate record.
- Moved M001 to recently closed; C001–C003 child closures remain as detailed
  evidence.
- Registered M002 as ready; kept M003 blocked on M002.
- Reconciled the subsystem roadmap milestone table and dependency graph.
