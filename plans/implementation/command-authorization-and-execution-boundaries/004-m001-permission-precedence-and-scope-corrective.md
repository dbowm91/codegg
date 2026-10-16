# Command Authorization M001 C001 — Permission Precedence and Scoped Grant Corrective

Status: implemented
Class: invariant / correctness
Hard dependencies: accepted ADR-0034 (satisfied)
Next: M001 C002 and C003; M001 closure must not claim complete until all three correctives pass.
Closure: `plans/closure/command-authorization-and-execution-boundaries/004-status.md`.
Authority: accepted `plans/adrs/ADR-0034-command-syntax-and-execution-authority.md` and `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md`. Parent handoff: `plans/implementation/command-authorization-and-execution-boundaries/001-syntax-blocker-audit-and-classification.md`. Baseline inspected: `planning/linux-execution-isolation` @ `7b72582c4f01f8a53c7f20ed5dde1bc620f82965` (2026-10-13). No tests are represented as executed by this plan.

## 1. Objective and rationale

Repair permission-resolution correctness **before** making built-in command syntax less obstructive. An explicit Deny must never be masked by a catch-all Allow/Ask, a signed saved Allow, a safe-command list, or a reviewer; saved command-family grants must not automatically authorize a compound or opaque script with additional effects. This is authorization correctness, *not* an attempt to secure the shell through complete parsing.

Codegg currently makes ordinary commands frictionless, but `src/permission/mod.rs::config_ruleset` pushes Bash allow/deny/ask patterns **before** a catch-all `bash_patterns: None`; `find_tool_rule_with_args` searches in reverse and immediately matches the catch-all. Default mode and explicit `bash="ask"` can therefore shadow user-selected specific patterns. `PermissionStore::get_decision_scoped` and `check_with_args` first consult reusable signed decisions; `decision_scope_for_shell_command` skips wrappers/assignments and derives only the first executable basename (e.g., `cargo test` and `cargo test; ./other-script` both become `cmd:cargo`). SecurityService can still escalate downstream, so this is a scoped-approval mismatch/risk, not a claimed confirmed sandbox escape.

## 2. Invariants

- Deterministic tool-disabled, authenticated user/operator/session/agent/project Deny and parent/child ceilings are never converted to Allow by later rules, persistent Allow, `ApprovalMode::Yolo`, Automatic reviewer, literal command rewrites or `with_exec_mode`; honor established domain priority and explicitly document conflicts between scopes.
- Preserve existing `PermissionStore` signing and legacy decision storage, including old unscoped grants. Do not silently reinterpret or discard signed/unsiged valid user choices or change persisted format without compatibility tests.
- An explicit text rule only governs its matched *invocation*. It does not guarantee equivalent behavior cannot be achieved by a script, interpreter, alternate tool, or subprocess. All routed children retain exactly the originating enforced capability envelope.
- A reusable **narrow** family grant cannot cover compound shell, command substitution, shell functions, here-doc execution, wrappers that conceal a different command, or ambiguous/unparseable commands solely because their first token matches. Such invocations either receive an explicit whole-invocation scoped grant, a consciously broad user grant, or return Ask according to user policy. Lack of deterministic family **must not** silently promote to a broad automatic grant.
- Existing `ReadOnly`, `WorkspaceWrite`, `FullHost`, network enforcement and receipt semantics are unchanged. This corrective does not introduce a shell syntax security boundary or new Linux mechanism.

## 3. Owned source and production changes

A. Inventory actual matched rule ordering in `src/permission/mod.rs`: `config_ruleset`, `default_ruleset`, `ToolRule::matches_bash_command`, `find_tool_rule_with_args`, `effective_tool_rule_with_args`, persistent decision lookup, agent/session defaults, active modes, `allow_all_bash`, `with_exec_mode` and `PermissionChecker::check_bash`/`check_git`. Also inspect `src/agent/tool_batch.rs` permission + security precedence and `tests/policy_surface_m007.rs` compatibility.

B. Define a canonical and testable precedence contract, distinguishing **scope authority** (hard ceiling and explicit grants/denies), **matched invocation preference** (user configurable allow/deny/ask), **session stored decision** (signed and scope-bound), and **fallback default**. Within a given config/rule tier, specific matching Deny wins over a broad fallback and explicit Ask outranks only a generic Allow; preserve documented agent > session > config boundaries, subject to immutable parent ceilings. Resolve conflicting persisted decisions predictably with documented generation/time behavior rather than depending on incidental vector order. Avoid a global "Deny always wins" simplification that inadvertently lets a lower-priority local config override an explicitly authorized higher-priority scope.

C. Fix catch-all rule shadowing by changing evaluation or construction to treat fallback only after no matching specific rule, with explicit evidence for `bash_deny_patterns`, `bash_ask_patterns`, `bash_allow_patterns` in both configured and unconfigured Bash modes. Audit Git `default_ruleset` overlapping `branch` rules (read-only and mutating), plus tool-name path aliases, for analogous precedence drift; do not replace with broad name denylists.

D. Retain scope persistence signatures while restricting reuse of `cmd:<argv0>` to a provably single, unambiguous command shape. Use a conservative parser/lexical gate only to decide whether *a prefix-scoped cached approval is reusable*; never call it a sandbox. A command containing shell operators, redirection, expansion with executable semantics, nested shell evaluation, or ambiguous quoting must not reuse first-token family by default. For simple commands with normal options or quoted literal args, keep session-scoped approvals practical. If a parsing library is needed, compare to Codegg's existing `command_intent::shell_shape`; do not implement a second homegrown complete shell grammar. Explicitly broad user grants still work in their declared authority scope and are presented honestly.

E. Ensure `PersistentDecision.scope=None` compatibility is visible and does not silently bypass a currently configured explicit Deny; if legacy broad allows are retained, require a clear `legacy_broad` source label and boundary compatibility tests. An unknown command should not automatically be granted broad persistence. Add typed source tags to diagnostics without raw script content; avoid public DTO/migration unless necessary.

## 4. Acceptance tests — exact cases

| Fixture | Required behavior |
|---|---|
| `permission.default=allow`, `bash_deny_patterns=["git push*"]` | Matching `git push` denied; ordinary `git status` not denied; pattern limitation documented |
| `permission.bash=ask` with matching allow/deny/ask patterns | Every specific pattern wins over catch-all Ask, with expected decision |
| `allow_all_bash=true` and explicit higher-priority deny | Resolution follows documented explicit scope contract; no implicit bypass |
| Saved scoped `cmd:cargo` Allow and `cargo test --all` | Reuse allowed when shape is unambiguous and scope matches |
| Same grant and `cargo test; ./second.sh` or `cargo test && curl ...` | Not covered by narrow saved grant; permission evaluated without token-family auto-allow |
| `env VAR=1 cargo check`, `/usr/bin/cargo test` | Only approved simple-command canonicalization retained; no malformed alias escalation |
| `sh -c 'cargo test; ./second.sh'`, here-doc, pipes, substitution, script file | No first-token grant laundering; effect authority unchanged |
| Legacy persisted broad Allow and explicit current Deny | Explicit Deny still enforced; backward-compatible storage/reporting documented |
| Signed stored decision tampering, unscoped downgrade | Signature invalid/denied per existing signing semantics |
| Agent child ceiling, `Yolo`, `Automatic`, headless, plan mode | No ceiling widening; explicit Deny always terminal |
| Failing required sandbox after approval | Target marker absent; never auto-retry via FullHost |

Add tests next to `src/permission/mod.rs`, `tests/permission.rs`, `tests/policy_surface_m007.rs` and `tests/approval_router.rs` as needed. Use synthetic scripts and sandboxed disposable fixtures; do not execute destructive host commands.

## 5. Execution, compatibility and ownership

Make the minimal source changes first; run `cargo fmt --all -- --check`, `cargo test -p codegg --lib permission`, `cargo test -p codegg --test permission`, `cargo test -p codegg --test policy_surface_m007`, `cargo test -p codegg --test approval_router -- --test-threads=1`, `python3 scripts/check_approval_router.py`, `python3 scripts/check_policy_surface.py`, `scripts/verify.sh quick`. If Cargo target differs, record the actual target. Keep signed store backward compatibility and no new daemon/state owner. Update `architecture/permission.md`, `docs/security-semantics.md`, config examples and test guard as required.

## 6. Closure and stop criteria

C001 closes only with a decision-precedence truth table, before/after reproduction of catch-all shadowing, a scoped compound-command test, persistent signature/backward-read tests, exact commit and test evidence. A broader grant must be explicitly user-authorized and must not imply host/sandbox privileges. Stop if fixing correctness requires silently revoking an explicit user broad grant, broadening a parent's execution ceiling, converting `Ask` into an unconditional Deny, or duplicating existing approval-store/routing authority. Leave unresolved changes blocked with corrective evidence rather than counting a parser match as an OS guarantee.
