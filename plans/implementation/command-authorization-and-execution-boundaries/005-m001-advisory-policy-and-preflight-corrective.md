# Command Authorization M001 C002 — Advisory Classification and Preflight Semantics

Status: closed
Class: invariant / compatibility
Closure: `plans/closure/command-authorization-and-execution-boundaries/005-status.md`.
Authority: accepted `plans/adrs/ADR-0034-command-syntax-and-execution-authority.md` and `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md`. Parent handoff: `plans/implementation/command-authorization-and-execution-boundaries/001-syntax-blocker-audit-and-classification.md`. Baseline inspected: `planning/linux-execution-isolation` @ `7b72582c4f01f8a53c7f20ed5dde1bc620f82965` (2026-10-13). No tests are represented as executed by this plan.

## 1. Objective

Reconcile the remaining built-in Bash literal/regex hard blocks, security critical-risk command denials, and Eggsact `command_preflight` high-risk `Block` with the accepted rule: command syntax is a risk/review signal, never kernel authority. Preserve genuinely deterministic tool-contract errors, authenticated explicit denials, child worktree ceilings, and human-owned terminal behavior. Prevent duplicate review prompts and log false "blocked by sandbox" diagnostics.

## 2. Specific source findings

- `src/tool/bash/policy.rs::HARD_DENY_PATTERNS`, `default_blocked_commands`, `find_blocked_pattern`, `check_shell_security` currently reject catastrophic matching strings before actual execution; `ASK_PATTERNS` also elevates harmless wrappers, substitutions and common shell programming forms.
- `src/security/policy.rs::action_for_command` and `action_for_findings` convert a classification of `Critical` into `SecurityAction::Deny` when `SecurityGateConfig.deny_critical_commands` defaults to true in `crates/codegg-config/src/schema.rs`. An explicit `security.denied_commands` list is user-selected text policy and must be retained, labeled best-effort.
- `src/agent/tool_batch.rs` integrates `PermissionChecker` and `SecurityService` into the sole `ApprovalRouter`. `src/tool/bash.rs::execute_inner` independently calls `check_command_security` and `PreflightService::check_command` before running a command. `src/preflight/service.rs::parse_command_result` marks `risk=high` or textual `verdict:block` as `PreflightSeverity::Block`; `BlockOnDefinite` can then become an unreviewable stop.
- `src/tool/bash/policy.rs::validate_child_workspace_command` rejects lexical `cd`, `..`, and paths beyond the worktree (a bypassable defense in depth). `src/shell/policy.rs` serves the explicitly human-controlled shell, separate from agent Bash. It currently has its own Block syntax rules and confirmation UI. **Do not modify human shell user-facing security UX silently in this milestone.**

## 3. Hard authorization vs heuristic review — required contract

Only genuine invalid/tool-disabled/unavailable or authenticated scoped Deny/child-ceiling decisions may stop without review. Misconfigured executable ownership, required sandbox failure, out-of-scope lease and protected-host resources remain terminal. Built-in regexes and high/critical *risk* classifications become advisory metadata; they may trigger Ask under operator-selected approval mode and current context, but cannot guarantee no equivalent behavior exists. Explicit user-selected text restrictions (`permission.bash_deny_patterns`, `security.denied_commands`, opt-in critical blocking) retain best-effort invocation denial and receive a truthful scope/limitation label.

The `deny_critical_commands=true` default creates a migration issue: distinguish an omitted inherited default from an **explicit** operator-selected strict denial. Inspect deserialization/config precedence, and use presence-aware compatibility or a deliberate versioned migration/deprecation warning. Do not unilaterally change an explicit persisted/admin denial or rewrite it into Ask without consent. If presence cannot be distinguished safely, stop and document a minimal configuration migration design before rollout.

For command preflight, a finding that a command looks dangerous is *advisory*. Retain `BlockOnDefinite` for provable correctness: invalid patch target, malformed JSON/TOML supplied to a typed validator, invalid typed arguments, or unsupported tool contract. A risk classifier returning a high score is not proof of a contract failure. Runtime scanner outage leaves risk classification unknown (not permission granted); required executable containment still fails closed.

## 4. Production changes

1. Add a bounded, typed classification disposition (`RiskSignal`, `ExplicitTextPreferenceDeny`, `DeterministicInvalid`, `MissingRequiredEnforcement`) reusing existing `SecurityDecisionHint`, `PermissionResult`, `PreflightDecision` and `ApprovalRouter` source labels. Keep a **single logical approval request** even where Bash, Eggsact and SecurityService all flag the same underlying request. No new agent loop or secondary approval broker.
2. Move `HARD_DENY_PATTERNS` and builtin literal catastrophic names into a consolidated, deduplicated advisory risk-signal collector. Retain operator-selectable Ask escalation, but do not automatically prompt on merely using `bash -c`, `python -c`, environment variables, pipes, regular quotation or command substitution in an otherwise-authorized workspace context. Distinguish explicit harm signals from ordinary shell composition.
3. Refactor `SecurityAction::Deny` only where derived from builtin heuristic risk/severity; explicitly configured `security.denied_commands` and verified typed invalidity continue to deny. Update `SecurityService` tests that currently expect `rm -rf /` and `curl | sh` to return Deny in default mode; assert review metadata and final OS/approval outcome separately.
4. Refactor `PreflightService::parse_command_result`: `high risk` and text `verdict:block` from `command_preflight` are not deterministic contract failures; use Warn/Ask through the canonical router in `BlockOnDefinite` mode. Verify JSON/TOML, text-replace and strongly typed validations retain their actual failure semantics. Update `src/tool/bash.rs` so preflight advisory findings are merged before decision and not dropped after an early route return.
5. Keep `validate_child_workspace_command` until an equally enforced child workspace boundary is demonstrated; annotate it as conservative advisory/preflight defense only, not an enforceable sandbox. Do **not** loosen `SandboxProfile::ReadOnly`, child scope, granted file roots or worktree leases because a regex has been removed. If conservative token checks block a legitimate contained case, expose a typed *capability unavailable* until a safe backend is qualified—do not incorrectly label lexer success as containment.
6. Inventory agent terminal compatibility, explicit human shell and `src/core/daemon_human_shell.rs`. For human shell, record a separate future decision if needed; do not import agent YOLO into human-initiated command confirmation.
7. Revise misleading docs in `architecture/tool.md`, `architecture/security.md`, `docs/security-semantics.md`, `architecture/permission.md`, `crates/codegg-config/src/schema.rs` comments, `codegg.example.jsonc` and guard tests to distinguish "explicit text filter" from actual kernel denial. No broad log of command bodies.

## 5. Tests and negative cases

- For strings `rm -rf /`, `mkfs /dev/xxx`, fork-bomb-looking text, `shutdown`, `curl | sh`, `sudo`, shell wrappers and inline interpreters: classification-only fixtures return risk metadata, not tool-side terminal syntax error. Do not execute them on a host; tests may use disposable sandbox fixtures only for benign substitutes.
- Explicit user `security.denied_commands`, `bash_deny_patterns`, `deny_all`, parent ceiling and confirmed backend failure still return correct typed denial with zero target marker.
- Normal `bash -c 'cargo check'`, `env RUSTFLAGS=... cargo test`, `python -c`, pipeline and substitution don't automatically generate repeat Ask solely from their syntax in a granted workspace context.
- `PreflightMode::BlockOnDefinite` high-risk `command_preflight` produces advisory outcome; malformed config/ambiguous text replace continue deterministic failure as before; structured errors cannot be laundered through a different tool.
- `ApprovalMode` interactive/automatic/YOLO, FullHost, explicit strict user settings, stored Deny, child plan-only, and Linux Landlock-only network reporting all retain truthful constraints.
- Human shell `HumanShellPolicyDecision` behavior and confirmation semantics stay unchanged and tests pass, with dedicated follow-up only if independently approved.

Run `cargo fmt --all -- --check`; `cargo test -p codegg --lib tool::bash`; `cargo test -p codegg --lib security`; `cargo test -p codegg --lib preflight`; `cargo test -p codegg --test preflight_integration`; `cargo test -p codegg --test approval_router`; `python3 scripts/check_approval_router.py`; `python3 scripts/check_policy_surface.py`; `scripts/verify.sh quick`. Add specific fixture targets as needed. Record actual host/mode, baseline expected changes and exact test outcomes.

## 6. Closure and stop

Close only after a cross-file table of every builtin syntax denial and its new disposition, demonstrably preserved explicit Deny, approval metadata seen exactly once, correct preflight distinction, unchanged human-shell and worktree ceiling, and updated help/docs. Stop if a previous explicit operator prohibition is silently dropped; a classifier exception auto-authorizes FullHost; config default-vs-explicit distinction cannot be safely migrated; or an unenforced path boundary is advertised as OS protection.
