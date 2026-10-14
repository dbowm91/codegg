# Shell Policy ADR-0028 Post-Closure C002 — Safe Advisory and Explicit-Deny Contract Tests

Status: implemented; closed (implementation `b0392431`; closure `plans/closure/shell-command-policy-adr0028-post-closure-corrective/002-status.md`)

Baseline: `39f149a15ef529d66787d1d11551872ea22ec384` (`main`; 2026-10-14).

Source roadmap: `plans/subsystems/shell-command-policy-adr0028-post-closure-corrective-addendum.md` (C002).
Original corrective and historical closure: `plans/implementation/shell-command-policy-ci-corrective/001-bash-hard-deny-vs-ask-test-reconciliation.md`; `plans/closure/shell-command-policy-ci-corrective/001-status.md`. Contradicting **newer** command-auth closure: `plans/closure/command-authorization-and-execution-boundaries/005-status.md`.

Authority: `plans/000-long-term-specification.md` §§4,17,27–29; `plans/001-terminology-and-domain-model.md` §8; `plans/003-planning-process.md`; **accepted ADR-0028**; accepted ADR-0004.
Primary class: **invariant**.

## Objective

Make all shell-policy regression tests consistent with ADR-0028 and safe to execute in CI. Built-in catastrophic and syntax-risk patterns are *advisory*, not a default terminal rejection. Prove that operator-configured Deny and true authority/sandbox Deny still prevent execution, without invoking genuinely destructive shell commands. Correct stale documentation, add a non-vacuous test-safety guard, and issue a successor closure without rewriting the previously closed historical record.

## Defects and evidence (why old closure missed them)

1. `src/tool/bash/policy.rs::default_blocked_commands` returns an empty set; `check_shell_security` enforces **only** explicit `blocked_commands` and configured allowlist. `find_risk_pattern`/`find_ask_pattern` return risk metadata; `BashTool::execute_inner` calls `check_command_security` and then may dispatch a subprocess. `BashTool::new` also has no required sandbox by default. An unconfigured direct tool cannot be relied on to reject harmful command text.
2. `tests/tool_execution.rs::test_bash_tool_denied_pattern` directly invokes `BashTool::new().execute` with `rm -rf /`. `test_bash_tool_hard_deny_tier_blocks_catastrophic_commands` loops over `mkfs`, `dd`, `shutdown`, `reboot`, `rm -rf /` and more, invoking the actual tool while assuming a prior hard-stop. `test_bash_tool_hard_deny_tier_survives_a_permissive_allowlist` repeats the problem. **Do not run these tests before replacing the dangerous invocations.** Even a test intended to assert failure may spawn a command before failing.
3. `tests/tool_execution.rs::ESCALATION_SIGNALS` still expects Ask for ordinary syntax alone (plain command substitutions/backticks/`eval`/`exec`/`base64 -d`), although current `find_ask_pattern` intentionally narrows review to concrete harmful targets, privilege/credential indicators and built-in risk. `test_bash_escalation_signals_are_named` and `...reach_the_operator...` are therefore stale beyond the catastrophic-denial assertions.
4. `src/permission/mod.rs` comment says catastrophic text stays hard-denied by BashTool, contradicting the runtime and ADR. Historical shell C001 closure asserts the obsolete test design had passed at an older merged SHA; accepted command-auth C002 subsequently changed the contract. Historic evidence is not a license to restore regex security.
5. General workspace CI may have stopped before these tests: at `main` `39f149a` run [38606475897](https://github.com/dbowm91/codegg/actions/runs/38606475897) failed at `git_network_integration::clean_removes_untracked_files` after 7027 passing tests, with 6614 unrun. No current full-suite green outcome proves shell tests safe.

## Invariants

- Command syntax by itself never creates built-in terminal Deny. Advisory risk classification may drive normal operator review; explicit, trusted configured deny/allowlist, scope, tool disablement, typed-invalid and obtained sandbox enforcement remain independent hard controls.
- No test invokes an actual fork bomb, filesystem-root deletion/permission change, device formatter/wipe, host power transition or similar global effect, on any machine/OS, even under the assumption that some earlier guard blocks it.
- A pure classifier may accept a dangerous **string** as data, without creating a shell or process. Tests using real `BashTool` may run only safe, deterministic commands against `tempfile` paths; an explicit deny must prevent the marker and leave an affirmative assertion of *zero dispatch*. Never use a literal host-global destructive command in an executable test.
- Explicit operator permission Deny and `bash_deny_patterns` override a more permissive policy/grant in the documented priority order; an explicit `blocked_commands` tool preference is best-effort text-only restriction, not equivalent-effect containment.
- Managed agent execution requires the **existing** ApprovalRouter/permission receipt and required sandbox. A direct tool-call unit test is not proof of an approval decision.
- No silent widening of FullHost/Yolo or bypass via `terminal`, alternate interpreter, Bash active command routing or nested agent.
- Historical plan/closure records remain intact; new C002 closure makes the supersession explicit.

## Bounded implementation scope

### A. Test matrix and safe fixture first

Target `tests/tool_execution.rs` **before any full/affected test execution**. Replace the three unsafe tests with:

- A pure risk classification table `find_risk_pattern`/`find_ask_pattern` over catastrophic strings as inert test data; `check_shell_security` with the empty default should report no synthetic text-denial. No `BashTool::execute` for those strings.
- A real explicit blocked-list test using `BashTool::new().with_blocked_commands(vec!["printf ..."])` or a scoped benign sentinel `echo` payload, and `with_allowlist` permitting that same benign prefix. Use a temporary-file marker and assert no creation for the denied invocation. Validate the exact error kind as `ToolError::Permission` and the configured rule source, not an assumed built-in.
- A benign `deny_all` control, a positive allowed benign command in a temporary workdir and a different denied allowed-list control, so both positive and negative behaviors are verified. Do not change `BashTool::new` defaults to make tests green.

Replace `ESCALATION_SIGNALS` with a **three-way** table: risk with concrete target (returns `find_risk_pattern`/Ask); privilege/credential or meaningful unsafe target (Ask); ordinary shell grammar, command substitution, inline interpreter/decoder, quote and pipeline shapes absent concrete danger (no syntax-only Ask). Assert exact `PermissionChecker::check_bash_legacy` outcomes for default checker and configured Deny/Allow conditions, accounting for precedence: explicitly authorized commands may Allow even when heuristic risk is present, but cannot bypass an effective current Deny/sandbox.

### B. Managed approval and enforcement

Extend existing `tests/agent_loop_harness.rs`, `tests/approval_router.rs`, `tests/permission.rs` and `tests/sandbox_containment.rs` *only where a missing assertion is found*. Use benign shell commands writing a marker **only inside a fresh TempDir**, simulate Ask with a controlled explicitly configured `bash_ask_patterns` or a risk fixture that cannot affect host-global resources. Assert `PermissionPending` and marker absence before `AllowOnce`; after grant, marker exists; `Deny` and required sandbox-unavailable keep marker absent. Do not execute a dangerous command to test the classifier. Verify same effective authority for Bash/terminal and any typed routing involved without introducing a new execution path.

### C. Guard against future unsafe test runners

Add `scripts/check_shell_policy_test_safety.py` (or extend existing canonical guard) detecting execution of host-global catastrophic test strings through `BashTool::execute`, `TerminalTool::execute`, raw subprocess `Command`, or loops over destructive literals in shell-policy test targets. Guard should be intentionally bounded/transparent: list its audited test locations, explain syntax/AST/static-analysis limits, and require a reviewed exception for any intentionally safe non-execution classifier test. Include **positive and negative fixtures** showing it does not reject pure classifiers but rejects representative executable destructive test. Make it run from `scripts/verify.sh quick` and `.github/workflows/ci.yml` before cargo tests. Guard complements, not replaces, a human review and safe fixture design.

### D. Production comments, docs, closure

Correct contradictory `src/permission/mod.rs` comment and any misleading shell hard-deny captions in `tests/tool_execution.rs`, `architecture/tool.md`, `architecture/permission.md`, `architecture/security.md`, `docs/security-semantics.md`, `src/tool/bash/policy.rs` and `plans/registry.md`. State explicitly: built-in syntax/risk does not imply host safety; explicit text rules are best-effort workflow controls; obtained enforcement and permission are authoritative, FullHost leaves host-global risk. Preserve earlier C001 closure as historical evidence, link successor C002.

## Ordered work packages

1. **Pre-test safety review:** statically enumerate hazardous `BashTool::execute` and `Command` invocations in tests, and enumerate stale `ESCALATION_SIGNALS` assertions. Record actual current default policy and accepted ADR. No test execution until the unsafe cases are eliminated.
2. **Replace fixtures:** inert catastrophic classifier table; explicit policy marker checks with benign command; replace stale syntax-only Ask expectations; add nearby positive controls.
3. **Real authority path:** verify existing agent-loop Ask/deny/AllowOnce and sandbox pre-spawn with markers; fill only missing assertions, no new model policy owner.
4. **Guard and docs:** implement pre-test guard with mutation negative fixtures; wire quick and CI; update contradictory comments, plans and architecture notes.
5. **Qualification:** run focused safe tests and exact-head hosted CI, record failures/known unrelated Git auth gate; produce closure.

## Test and verification commands

**Before any potentially executing test**, run code review and newly added safety guard/negative self-test:

```bash
python3 scripts/check_shell_policy_test_safety.py
python3 scripts/test_shell_policy_test_safety.py
```

Then, *only after confirming all destructive direct-test invocations were removed*:

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib -- tool::bash::policy
cargo test -p codegg --test tool_execution
cargo test -p codegg --test permission
cargo test -p codegg --test approval_router
cargo test -p codegg --test agent_loop_harness shell_escalation_ask
cargo test -p codegg --test sandbox_containment
python3 scripts/check_approval_router.py
python3 scripts/check_policy_surface.py
bash scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Check actual selector names and adjust with explicit report if changed. After the change, request hosted Linux CI against **the exact code SHA**. CI may still fail due to the independent Git `clean` permission-fixture issue; capture whether the new shell guard ran successfully and, where safe, run `tool_execution` as an early isolated CI target so it cannot be skipped by nextest fail-fast. Do not use `--no-fail-fast` on the unchanged unsafe baseline.

## Failure, cancellation, restart, security and compatibility

No storage migration, new approval receipt, permission mode or protocol change. Marker fixtures must not race across threads or touch paths outside TempDir. A rejected invocation must have no process dispatch, marker write, or unexpected approval; an allowed invocation must be recorded by the same audit/authorization owner. Required sandbox unavailable must fail before process launch even after Ask is approved. A cancelled approval must not dispatch. FullHost remains explicitly unsafe if a user grants broad host execution; tests must not mask that with claimed regex protection. Restart/saved approvals retain current precedence and scope behavior; no broad family grants for compounded commands.

## Acceptance criteria

1. **Zero host-destructive execution tests** in the shell-policy/terminal suites; demonstrate automated guard negative mutation and manual audit of every affected test.
2. Pure risk-classifier fixtures cover the formerly hard-denied catastrophic table; the default `blocked_commands` set remains empty by design.
3. `tests/tool_execution.rs` proves a harmless **configured** hard deny remains terminal under permissive tool allowlist, with a zero-dispatch/marker assertion.
4. Concrete high-risk target/privilege/credential cases remain advisory Ask where current policy requires; syntax-only substitutions/`eval`/`exec`/`base64` don't spuriously Ask; explicit operator Deny always wins under documented precedence.
5. End-to-end `AgentLoop` Ask→AllowOnce and Deny/sandbox-unavailable paths have marker-backed non-dispatch and positive authorized controls.
6. Guard runs in local quick and hosted CI; no new parser/blocklist/runtime policy silently introduced; architecture and registry explain historical C001 vs ADR-0028.
7. Focused tests pass and evidence on final SHA is recorded honestly. Whole-workspace CI red for separately owned Git auth test is a **qualified operational dependency**, not evidence that shell C002 failed.

## Stop conditions

Stop and escalate if a real *managed* approval or sandbox bypass emerges; do not hide it by adding text denies. Stop if a test framework requires executing an actual catastrophic command: redesign around pure functions, mock dispatch, or benign TempDir markers. Stop on unowned changes to terminal or Git execution authority, user approval semantics, or hardenings that alter accepted ADR-0028. No new ADR unless actual architectural authority allocation must change.

## Closure record

Create `plans/closure/shell-command-policy-adr0028-post-closure-corrective/002-status.md` with implementation SHA/PR, old-vs-new assertion matrix, static list of removed unsafe execution sites, guard fixtures, focused commands/results, managed approval/no-dispatch proof, selected OS/sandbox limitations, docs cross-reference, any CI failure classification and final recommendation. Keep historical C001 and command-auth C002 closures unchanged.
