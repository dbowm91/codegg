# Command Authorization M001 C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/command-authorization-and-execution-boundaries/005-m001-advisory-policy-and-preflight-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md#6-dependency-graph`

Repository baseline reviewed: `7b72582c4f01f8a53c7f20ed5dde1bc620f82965` (fresh plan baseline); implementation reviewed at `696e7a4d`

Implementation commit:

- `696e7a4d` — make built-in command classifiers advisory and retain explicit policy.

## 1. Executive finding

C002 is complete. Built-in Bash syntax patterns and default literal names now
provide bounded risk signals instead of tool-side terminal rejection. Ordinary
quoting, pipes, substitutions, `bash -c`, `python -c`, and environment wrappers
do not prompt solely because of their syntax. Concrete high-impact targets and
authority/credential signals may still produce review metadata. Operator-selected
text rules and typed authorization remain distinct: explicit Bash/security Deny,
permission Deny, required-enforcement failure, and deterministic typed-input
invalidity keep their existing terminal behavior.

The `deny_critical_commands` setting is presence-aware. An omitted value no
longer silently acts as an explicit prohibition; a configured `true` remains a
strict operator choice, and layered config inherits a lower-priority explicit
value when the higher-priority security section omits it. Command-preflight
`high risk` and textual `verdict:block` findings now warn and enter the existing
authorization route. Typed validators retain deterministic blocking semantics.
No Linux network or IPC containment is claimed: Landlock-only execution still
has unrestricted network/IPC in this baseline.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Built-in syntax heuristics cannot hard-deny an invocation | `src/tool/bash/policy.rs::check_shell_security`; `default_blocked_commands`; `find_risk_pattern`; Bash policy tests | pass | The default blocked literal set is empty. Explicit `with_blocked_commands` restrictions and allowlist policy remain available. |
| Routine shell composition does not trigger syntax-only Ask | Bash policy tests for wrappers, quotes, environment assignments, pipelines and substitutions; `tests/approval_router.rs` | pass | Review remains available for concrete target/authority risk or stricter configured policy. |
| Catastrophic-looking built-in strings retain classification visibility | `find_risk_pattern` and classification fixtures in `src/tool/bash/policy.rs` | pass | Root/home recursive deletion, formatting/device writes, fork-bomb shape, shutdown/reboot, and related signals remain risk metadata; no command is run by these fixtures. |
| Explicit text restrictions retain their invocation-level denial | `src/tool/bash/policy.rs` explicit blocked-list tests; `src/security/policy.rs` configured `denied_commands` tests | pass | These are clearly documented as text preferences, not equivalent-effect or kernel boundaries. |
| Critical-command default distinguishes omission from explicit strict policy | `SecurityGateConfig.deny_critical_commands: Option<bool>`; schema presence test; layered merge tests in `crates/codegg-config/src/paths.rs` | pass | Omitted is advisory by default; explicit `Some(true)` denies. Explicit higher-priority `Some(false)` overrides inherited true; omission inherits. |
| High-risk command preflight is advisory in `BlockOnDefinite` | `src/preflight/service.rs::parse_command_result`; preflight unit and integration tests | pass | `risk=high` and command-preflight text `verdict:block` yield Warn. |
| Deterministic typed preflight failures still block | `tests/preflight_integration.rs` and preflight service tests | pass | Malformed structured input, invalid typed arguments and text-replace contract failures retain deterministic outcomes. |
| Risk and preflight reasons reach one approval route | `src/agent/tool_batch.rs` command-preflight aggregation and single `ApprovalRouter` route; `python3 scripts/check_approval_router.py`; approval-router suite | pass | Permission/security terminal Deny is resolved before advisory review; reasons are combined into one logical request. No secondary broker was added. |
| Child workspace lexical checks remain conservative defense in depth | `src/tool/bash/policy.rs::validate_child_workspace_command` | pass | Retained and labeled as lexical preflight, not shell parsing or OS containment. |
| Human-owned shell behavior remains unchanged | `src/shell/policy.rs` unchanged; human shell confirmation tests in relevant suite | pass | Agent Bash UX changes were not imported into human shell policy. |
| Docs accurately separate text policy, review, and actual enforcement | `architecture/permission.md`, `architecture/security.md`, `architecture/tool.md`, `docs/security-semantics.md`, `codegg.example.jsonc`, `CHANGELOG.md` | pass | Documentation states that Landlock-only network and IPC remain unrestricted. |

## 3. Built-in syntax denial disposition

| Previous built-in denial or escalation | New disposition | Preserved boundary |
|---|---|---|
| `rm -rf /`, recursive deletion of root/home, and similar catastrophic target patterns | Risk signal; may be reviewed based on approval mode and current context | Explicit `bash_deny_patterns`, explicit security text denial, permission Deny and actual sandbox enforcement still apply. |
| `mkfs`, `dd` zeroing/device targets, raw-device redirection, `fdisk`/`parted`, `zcat` entropy targets | Risk signal; no built-in terminal syntax rejection | No claim that command text can enumerate every destructive effect. |
| Fork-bomb-looking text, root permission changes, `umount /`, init/telinit 0, shutdown/reboot/poweroff/halt, `kill -9 -1` | Risk signal and possible review | A matching string does not guarantee prevention of equivalent behavior. |
| `curl | sh`, `sudo`, credential/authority-sensitive targets | Contextual review signal | Network, credentials, OS privilege and execution capability still require their actual authority; no implicit privilege grant. |
| `bash -c`, `python -c`, wrappers, variable assignments, ordinary quotes, pipes, substitutions | No Ask solely from grammar | Tool authority and obtained filesystem sandbox continue to govern execution. |
| Explicit configured blocked literals/patterns and `security.denied_commands` | Explicit invocation-level Deny | Best-effort text matching only; remains configurable and documented as such. |

The policy collector is not a complete shell parser and is not presented as
such. Tests use classification or synthetic outcomes and do not execute
destructive strings on the host.

## 4. Production implementation evidence

- `src/tool/bash/policy.rs` replaced the misleading hard-block collector with
  `find_risk_pattern`; built-in risk names no longer populate the default
  blocked literal set. Ordinary shell composition no longer escalates only
  because it uses a wrapper or shell grammar. Explicit configured restrictions
  remain terminal invocation preferences.
- `SecurityGateConfig.deny_critical_commands` changed from a defaulted boolean
  to `Option<bool>`. The security policy denies only explicit `Some(true)`.
  Config merging carries a lower-priority explicit value through a higher
  security section that omitted it, while an explicit higher-priority value
  remains authoritative.
- `src/preflight/service.rs` treats command-preflight risk severity and textual
  block verdicts as warnings. Deterministic failures from typed validators
  remain blocks.
- Agent command preflight is made available to the existing authorization path
  and its advisory reasons are combined with security/sensitive-path reasons
  before the one `ApprovalRouter` decision. The Bash executor continues to
  emit preflight diagnostics without treating heuristic risk as containment.
- `validate_child_workspace_command` remains a conservative lexical check. It
  does not establish OS containment and does not replace required sandbox
  enforcement.
- `src/shell/policy.rs` was not changed; the human-owned shell’s confirmation
  contract remains separate.

## 5. Verification executed

Host: `Linux deadpool 6.8.0-146-generic #146-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 3 16:12:30 UTC 2026 x86_64`; Rust `1.99.0`, host `x86_64-unknown-linux-gnu`.

### Commands run

```bash
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib security
cargo test -p codegg --lib preflight
cargo test -p codegg --test preflight_integration
cargo test -p codegg --test approval_router -- --test-threads=1
cargo test -p codegg-config paths::tests::test_merge_configs_replaces_default_backed_sections_whole
cargo test -p codegg-config schema::tests::critical_command_deny_gate_preserves_presence
python3 scripts/check_approval_router.py
python3 scripts/check_policy_surface.py
python3 scripts/check_config_merge_coverage.py
cargo fmt --all -- --check
git diff --check
scripts/verify.sh quick
cargo run --bin codegg -- validate --config codegg.example.jsonc
```

### Results

- Bash policy: 111 passed, 0 failed.
- Security: 484 passed, 0 failed.
- Preflight: 40 passed, 0 failed.
- Preflight integration: 72 passed, 0 failed.
- Approval router: 17 passed, 0 failed.
- Config security merge case: 1 passed; critical gate presence case: 1 passed.
- Approval-router, policy-surface and config-merge guards passed.
- Formatting and `git diff --check` passed.
- `scripts/verify.sh quick` passed, including workspace all-target check and quick guards.
- `codegg validate --config codegg.example.jsonc` reported valid configuration.

All results are local verification; no hosted CI result is claimed. A concurrent
workspace test sweep was not used as closure evidence. Linux Landlock-only
network and Unix IPC remain unrestricted; no offline or equivalent-effect
containment claim is made here.

## 6. Invariant review

- Explicit permission/security Deny and required enforcement failures remain
  terminal; heuristic risk cannot auto-authorize FullHost or bypass a user’s
  configured restriction.
- Command risk is review metadata. It is not a proof of shell equivalence,
  containment, or effect denial.
- Approval mode, workspace scope, child ceilings and sandbox request remain
  independent. No filesystem roots, worktree leases, daemon authority or
  network rights were broadened.
- Typed validator correctness failures remain distinguishable from command
  risk. Scanner/preflight unavailability is not treated as a permission grant.
- The human shell’s explicit confirmation behavior is unchanged.

## 7. Failure and compatibility review

No process ownership, scheduler, durable approval receipt, or execution backend
changed. The config field remains deserializable from existing boolean config;
the new optional representation preserves whether a value was omitted. Layered
merge tests cover omitted, inherited explicit true, and explicit false
override. The example config validates. No config migration rewrites explicit
operator preferences.

## 8. Security review

The change removes misleading syntax-derived hard stops while retaining actual
authorization and required sandbox checks. Explicit text filters remain useful
workflow controls but can be bypassed by equivalent scripts or alternate tools.
Risk signals may request review but do not claim to stop destructive effects in
FullHost. No secret or raw command-body logging was added. Linux Landlock-only
execution continues to permit network and Unix IPC; those dimensions remain
explicitly unqualified for denial.

## 9. Documentation and operations

- Updated permission, security and tool architecture docs and user-facing
  security semantics to distinguish explicit text policy, risk review and
  obtained OS enforcement.
- Updated the example configuration and selector copy for presence-aware
  critical-command policy.
- Retained the existing approval-router and policy-surface guards; all passed.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Linux Landlock-only execution does not restrict network or Unix IPC | Allowed workspace code can still reach those host capabilities | Keep the limitation explicit. Qualify native isolation in the Linux execution-isolation line before claiming these dimensions. |
| low | Built-in command risk patterns are incomplete by nature | Alternate spelling, script, compiled code or another tool can produce equivalent effects | Treat them only as review signals; actual enforcement and explicit authorization remain authoritative. |

No high-severity finding remains for C002. The medium finding is a pre-existing
backend limitation and is not represented as resolved by this closure.

## 11. Roadmap disposition

C002 is closed. C003 is dependency-ready and may proceed with its exact-revision
workflow-friction qualification. Parent M001 remains incomplete until C003
closes and the original inventory criteria pass. M002 remains blocked on parent
M001. M003 remains blocked on M002 and on native evidence for any claimed
enforcement dimension. Linux network/IPC containment status is unchanged.

## 12. Registry updates

- Marked implementation plan 005 implemented and added this closure record.
- Removed C002 from active implementation rows and added it to recently closed
  work.
- Registered plan 006 as ready because C002 was its hard dependency.
- Kept parent M001 blocked on C003 and M002/M003 blocked as before.
- Updated the command-authorization roadmap milestone/dependency status.
