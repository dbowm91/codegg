# Command Authorization and Execution Boundaries M001 — Syntax Blocker Inventory and Advisory Conversion

Status: closing
Primary class: invariant
Hard dependencies: accepted ADR-0024 only.
Target closure: `plans/closure/command-authorization-and-execution-boundaries/001-status.md`.
Status and acceptance are planning declarations, not execution evidence. Source roadmap: `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md`. Repository baseline: `main` 2026-10-13 and Linux branch `c7c18181862d67c9e2a649399bbda9665869edd8`. Long-term: `plans/000-long-term-specification.md` §§4,17,27–29 and `plans/001-terminology-and-domain-model.md` §8. Accepted: ADR-0001/0002/0004/0016/0024; Linux mechanism ADR-0023 proposed. Closure belongs under `plans/closure/command-authorization-and-execution-boundaries/`.

## 1. Objective

Remove **built-in syntax-only terminal rejections** and replace them with advisory risk signals routed through existing permission/approval authority, without modifying effective filesystem/network/credential rights, elevating agents, or deleting explicit operator/user deny preferences. Preserve tool disablement and typed-invalid request denial. Produce a complete blocker inventory rather than only fixing the visible Bash regex.

## 2. Why ready; current implementation evidence

- `src/tool/bash/policy.rs` has `HARD_DENY_PATTERNS`, `default_blocked_commands()`, `check_shell_security()`, `ASK_PATTERNS`; called from `src/tool/bash.rs::execute_inner` before the permission-aware execution route. `src/permission/mod.rs` already supports `find_ask_pattern` and user `bash_allow_patterns`, `bash_deny_patterns`, `bash_ask_patterns`.
- `src/tool/destructive.rs` separately flags destructive shell invocations; `src/security/policy.rs` classifies `SecurityAction::{Observe,Ask,Deny}` and can hard-deny critical classifications or `denied_commands` strings.
- `src/preflight/service.rs` supports `BlockOnDefinite` (valid for actual tool contract failures, not unproven syntax danger). `src/command_intent/`, `src/agent/tool_inspect.rs`, `src/shell/policy.rs`, compatibility `terminal` and plugin/tool routing may introduce other denials; enumerate actual callers.
- Production already distinguishes requested `SandboxProfile`, `ApprovalMode`, explicit FullHost, and obtained `FilesystemEnforcement`/`NetworkEnforcement`. Do not weaken any of these.

## 3. Security invariants

- An explicit scoped operator/user/project/session/agent Deny and parent/child ceiling always takes precedence over default allow, YOLO and automatic reviewer. Syntax classification cannot weaken an explicit deny, and an approved command never overrides required sandbox failure.
- A builtin text pattern is only evidence of possible risk; a negative match does not imply safety. Broadening user grants, disabling tools, or granting host privileges requires an authenticated authority change.
- Preserve configurable `bash_deny_patterns` as *best-effort invocation-text preferences* with existing matching and precedence until an explicit migration; do not claim they are executable behavior restrictions. Keep `bash_allow_patterns` / `bash_ask_patterns` similarly classified.
- `deny_all`, invalid request JSON, length/resource limits, wrong worktree, explicitly forbidden paths/permissions, credential access, required sandbox failure and real kernel denials still fail.
- Strict/special security modes and FullHost/YOLO must display honest warnings: no code-based guarantee that a destructive command can be prevented without actual filesystem containment.

## 4. Scope

In: inventory and classify all syntax-only denial owners, convert default catastrophic regex/literal table to advisory `Ask`/risk findings; remove duplicated hard-pattern calls; preserve optional user text filters and typed hard authorization; reconcile docs/test expectations and audit fields. Out: fully analyzing shell semantics, replacing `ApprovalRouter`, implementing OS isolation or Git route (separate Linux C001), removing user configuration, changing default mode/FullHost, unreviewed product-wide security score changes.

## 5. Required production changes

A. Build a machine-checkable inventory table of all command-blocking paths: file/function, origin (built-in heuristic vs explicit operator preference vs typed requirement vs OS enforcement), decision owner, chosen disposition, and end-to-end execution route. Include `src/tool/bash/policy.rs`, `src/permission/mod.rs`, `src/tool/destructive.rs`, `src/security/policy.rs`, `src/preflight/service.rs`, `src/shell/policy.rs`, `src/command_intent/`, `src/agent/policy.rs`, `src/tool/git.rs`, `crates/egggit/src/process.rs`, tools/plugins, static guards, and user-supplied config. Explicitly inventory historical terminal blockers, not just active code.

B. Introduce minimal typed internal provenance for `RiskSignal` vs `AuthorizationDenial` vs `InvalidRequest` vs `MissingEnforcement` (or use an equivalent existing type). Avoid storing secret commands in audit metadata. If public schema changes, version properly. Keep exact optional user pattern matching as user workflow preference; never silently convert an explicit deny to allow. Distinguish a *requested operator text filter denial* from a built-in risk flag.

C. Convert default catastrophic Bash `HARD_DENY_PATTERNS` and `default_blocked_commands()` into advisory signals merged with `ASK_PATTERNS` and `destructive_match`, so the effective pre-authorization risk is visible. Retire builtin regex terminal denial in `check_shell_security`, retain `deny_all`, text preference allowlist and explicit permission constraints. Confirm classification precedes automatic safe-prefix approval where needed, without superseding explicit scoped Deny or inventing extra user approval in a previously granted caller context.

D. Review `SecurityService` critical classifications and `PreflightService::BlockOnDefinite` callers: distinguish proven invalid actions/tool correctness from merely "looks like destructive command". Convert heuristic-only terminal actions to review signals; retain deterministic typed invalid/capability denials. Preserve accepted reviewer-mode semantics.

E. Update operator/API/CLI UX and `architecture/permission.md`, `architecture/security.md`, `architecture/jobs.md`, `docs/cli.md`, config docs/tests, `scripts/check_policy_surface.py`, `scripts/check_approval_router.py` and static shell policy guard. Do not delete historical tests silently: convert obsolete text-denial assertions into advisory + obtained-enforcement assertions.

## 6. Ordered packages

1. Baseline audit: enumerate actual rules and all tools invoking them. Lock snapshot of explicit user-deny/persisted behavior and test disabled tool, parent-child scope, FullHost warning.
2. Normalize typed decision source for risk signal, actual authorization failure and missing required backend; no duplicated permission source across tool-specific facades.
3. Replace built-in terminal shell spelling rules with advisory inputs; validate approval-before-execution ordering and no silent fallback.
4. Reconcile security/preflight/compatibility and operator text-rule semantics. Preserve independently authored Deny and policy preferences.
5. Run effect-equivalence representative pairs (see below), docs/guard changes and closure. M001 does not claim an OS containment upgrade.

## 7. Failure, cancellation, restart and contention

On explicit Deny/tool-disabled/invalid scope/required sandbox failure, reject before target and do not re-route. Ask that is unanswered follows current timeout/defer rules; automatic reviewer cannot grant beyond ceiling. Preserve exact receipt scope, no double approval on shell->typed routing, bounded tracing, timeout and descendant cleanup. Concurrent project requests do not share ephemeral rule or authorized grant state. No config/state migration; if old user text rules gain a display label, keep matching behavior.

## 8. Test matrix and commands

- `rm -rf /`, `mkfs`, fork bomb spelling, shutdown, `sudo`, `bash -c`, `curl | sh` generate appropriate risk/approval rather than builtin terminal regex errors; do not execute catastrophic tests on host—use classifier fixtures.
- Same effect via `sh -c`, shell script, Python, native binary and tool routing has same sandbox ceiling regardless of risk match. A default `cargo test` / `git status` may execute arbitrary build scripts/config, so test that auto-allow never implicitly raises actual rights.
- Explicit operator text Deny still rejects matching invocation; bypassable alternate spellings demonstrate *the documented limitation* and may not exceed OS authority.
- Invalid JSON, disabled shell, out-of-workspace root, child worktree ceiling and no backend required launch fail closed. Approval modes Interactive/Automatic/Yolo and FullHost warnings remain true; risk signal cannot override explicit Deny.
- Focused commands: `cargo fmt --all -- --check`; `cargo test -p codegg --lib tool::bash`; `cargo test -p codegg --lib permission`; `cargo test -p codegg --test approval_router -- --test-threads=1`; `python3 scripts/check_approval_router.py`; `python3 scripts/check_policy_surface.py`; `python3 scripts/check_sandbox_policy_wiring.py`; `scripts/verify.sh quick`. Add narrow policy regression targets if existing tests insufficient; record actual invocations.

## 9. Acceptance

All built-in syntax hard denials in the inventory either become advisory or are justified as *real typed authority/invalidity* denials. No default risk match alone emits a terminal security guarantee. Explicit configured Deny and OS limits retain enforcement; operator text preferences are present and labeled text-only. Equivalence fixtures demonstrate same obtained authority under alternate shell/typed shapes; FullHost limitations are explicit. Existing safe workflows no longer fail solely due literal spelling. Closure maps changed rules, accepted exceptions and exact test results.

## 10. Stop conditions

Stop for loss of explicit Deny precedence, unconditional host execution from missing sandbox, unreviewed migration of persisted user preferences, unverified approval ordering or a classifier-specific policy path that can grant authority. Do not "fix" a failed test by changing the workspace sandbox profile.

## 11. 2026-10-13 research addendum — ordinary coding workflows as the default UX

The implementation is now split into three registered correctives while retaining this parent M001 invariant:

1. **C001 permission precedence and saved-grant scope:** `004-m001-permission-precedence-and-scope-corrective.md` (**ready**). Fix the `config_ruleset` specific Bash pattern vs catch-all order: `find_tool_rule_with_args` currently visits the catch-all first. Prevent a saved `cmd:cargo` Allow from silently covering a compound `cargo test; other-script`. Preserve explicit user Deny and signed persisted decision compatibility.
2. **C002 builtin classification/preflight:** `005-m001-advisory-policy-and-preflight-corrective.md` (hard: C001). Reconcile Bash hard regex/literal denials, `SecurityService` critical-risk heuristic denial and Eggsact `command_preflight` high-risk block. Explicit user-selected text denials and actual invalid tool contracts remain enforceable as their appropriate narrow controls. Do not change independent human shell semantics without separate approval.
3. **C003 normal developer friction qualification:** `006-m001-developer-workflow-friction-qualification.md` (hard: C001/C002). Validate everyday Rust/Python/JS/Go/build/test/format/lint, shell wrappers and local workspace cleanup. In an already-authorized workspace session, shell grammar alone does not trigger repeated approvals. New network, credential, host scope, protected Git or unsupported backend authority remains an actual separate request.

**Default-product norm:** permission to perform routine work derives from an actual authorized workspace and approval context, not from command-name safety claims or a vast static allowlist. Do not auto-approve everything simply because `cargo`, `python`, `make` or `npm` is argv0: build/test scripts can run arbitrary repository code. Avoid moving a synthetic blocklist into a different parser and calling that sandboxing. An opted-in strict operator preference may intentionally prompt more; preserve it and report that choice honestly.

**Required closure evidence:** explicit Deny-vs-catchall and persisted compound grant tests, default-vs-explicit `deny_critical_commands` compatibility assessment, ordinary command prompt-count corpus, one logical review per tool invocation, human-shell nonregression, FullHost/YOLO warnings, and actual Linux Landlock-only limitations. Parent M001 closure is blocked until C001–C003 close; M002 routing is not independently promoted as a workaround. No production implementation or tests were performed in the planning pass.
