# Shell Command Policy CI C001 — Legacy Bash Hard-deny vs Ask Test Reconciliation

Status: **implemented; closed**; upstream test-policy rewrite and approval-path verification passed on the C040 integrated head
Primary class: security regression corrective
Verified integration head: `b89ba9e364bbf492d19048bed54803c4a8fd6d8e` on `work/windows-native-sandbox-foundation`; source test rewrite: mainline `9d32c35b516315adef21e30eb2a9e436826231e0`.
Source: `plans/subsystems/shell-command-policy-ci-corrective-addendum.md`.
Canonical: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`; ADR-0004; current permission and sandbox contract.
Predecessor: prior shell policy split into hard-deny and Ask (`src/tool/bash/policy.rs` and `src/permission/mod.rs`), no previous closure authorized tests that assume the old contract.
Evidence: <https://github.com/dbowm91/codegg/actions/runs/38417097457> exact SHA `3fbe6201` (two failing tests); earlier run <https://github.com/dbowm91/codegg/actions/runs/38405432003>. Independent of Windows C036/C037/C038.

## Newly observed upstream fix and revised handoff (2026-10-13)

The required *test rewrite has already landed on current `main`*, commit `9d32c35b516315adef21e30eb2a9e436826231e0` ("test(bash): rewrite the stale blocklist tests around the two-tier policy"). `main` head `b9ba494c1b21638c2e2f660301ba207ce8859682` contains revised `tests/tool_execution.rs` tests for (a) actual catastrophic hard denies, including permissive-allowlist regression, (b) named `find_ask_pattern` escalation signals, (c) actual `PermissionChecker.check_bash_legacy` returning `PermissionResult::Ask`, (d) ordinary benign syntax and signal false positives. The current Windows branch head `631ae9488f251ac70e91b9c8299d2c1c3bff83fa` **does not include these tests**; the two historical failing test names remain in its tree. The repair should be **imported with current `main`**, not re-created and cherry-picked independently.

**Precedence for this updated handoff:** execute original work packages as a verification matrix, but **skip any rewrite already implemented in commit `9d32c35`**. C040 is the owning integration/merge handoff; C001 verifies the upstream fix and catches any genuine command-approval bypass in Codegg's actual daemon/managed-process execution path. Do not claim C001 closed based solely on mainline success or direct `BashTool::execute` behavior. First integrate main or verify in an isolated C040 merge preview, then run focused and hosted CI on the resulting **exact branch SHA**. If a real bypass is found, register a separate owned security corrective rather than turning regex hard-blocking back on.

Mainline PR conflict check also found a *separate* Windows versus TUI ADR-0021/0022 identity collision, addressed in Windows C039. Keep C001 independent of the ADR renumbering decision; only its final exact-head verification depends on C040 integration.

## Defect and failure mechanism

The Linux workspace CI run failed:
- `tests/tool_execution.rs::test_bash_tool_blocked_patterns` asserted that direct tool execution of `$(whoami)` is an error.
- `tests/tool_execution.rs::test_bash_tool_blocked_command_list` asserted that direct execution of `sudo su` is an error.

But the current policy deliberately moved shell substitutions, wrapper/privilege escalation and review-worthy command shapes into an **Ask** family while keeping a smaller hard-deny floor. The approval evaluator's `PermissionResult::Ask` and tool-side `ToolError::Permission` are **different outcomes and owners**. Tests that expect all risky spellings to hard-deny on `BashTool::new().execute` conflate them. They may also conceal an integration gap: a raw/uncontextualized BashTool execution should not be mistaken for a daemon-authorized session execution path. Validate both claims before editing.

## Ordered work packages

**A — Baseline and true authorization trace.** Read `src/tool/bash/policy.rs` hard-deny/ask tables, `src/permission/mod.rs` `check_with_args`/approval mode semantics, `src/tool/bash.rs` and `src/tool/bash/process.rs` actual dispatch, `tests/tool_execution.rs`, `tests/permission.rs` and applicable command-routing tests. Freeze an explicit behavior matrix for `rm -rf /`, `mkfs`, `sudo su`, `$(whoami)`, escaped/quoted harmless variants, shell wrappers, and an explicitly allowed benign command. Evaluate classification, requested approval mode/limits, direct tool facade behavior, and whether process actually dispatched; identify any route that bypasses the required approval authority. No execution of genuinely destructive examples: use pure classifier or mocked/synthetic dispatch and no root privileges.

**B — Test the right owner, no new textual blocker.** Import the existing mainline `9d32c35` test rewrite through C040 rather than duplicating or reverting it. Only if a gap remains after integration, update stale direct-tool assertions in `tests/tool_execution.rs` so they demand error for **actual hard-deny** examples and don't mislabel Ask as a tool hard-denial. Add targeted tests of `find_ask_pattern` / actual `PermissionResult::Ask` through `src/permission/mod.rs` or its integration harness for `sudo su` and shell substitution; when permission config is explicit Allow/Deny, honor policy ceiling, explicit deny, parent/subagent ceiling and daemon authorization. Do not change the code to prohibit exact command strings solely to make old tests green. If a genuine bypass exists, fix at the canonical permission/managed-execution gate using a separately recorded focused security corrective rather than adding an obscurity-sensitive regex.
 
**C — Enforce negative and legitimate controls.** Verify dangerous hard-denies remain fail-closed, even under permissive mode; quoted/heredoc literal examples don't spuriously ask; script/alternate-shell forms are governed by the same approval/sandbox ceiling (not "safe because regex missed"). For Ask-class commands, show *no unauthorized dispatch before explicit decision* through the normal caller path. It must not accidentally execute `sudo su` in a test environment; use stubs to assert decision and test a benign representative command for actual permitted execution.
 
**D — CI/closure.** Reproduce the two previously failing tests on source HEAD, apply smallest coherent change, run `cargo test --locked --test tool_execution -- test_bash_tool_blocked` (or two full test IDs), `cargo test --locked --test permission`, `cargo test --locked --lib tool::bash::policy`, relevant command-routing adversarial suites, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` where feasible, `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_execution_ownership.py`, `bash scripts/verify.sh quick`, and hosted root CI on exact implementation SHA. Mark commands not run. Test count must be nonzero and both historical failures actually resolve. If unrelated tests fail, classify individually rather than claiming full branch green.

## Acceptance/security gates

- At the chosen policy authority, true hard-deny commands produce canonical denial **without** execution, ask-risk command forms produce actual required-approval semantics (not a fake ToolError), explicit Deny refuses, and explicit authorization plus OS sandbox ceiling permit only actions within scope.
- Neither syntactic regex evasion nor a direct uncontextualized tool invocation can be used as an authorization substitute for a daemon-managed run. No new release-time process bypass is introduced.
- Focused test suite and the two historical assertions pass as *corrected behavior*; no test removed, ignored or expectation inverted merely to silence CI; production code stays unchanged unless a real bypass is found and explained with independent security evidence.
- Closure `plans/closure/shell-command-policy-ci-corrective/001-status.md` cites exact SHA/runs, before/after test names, approval/deny controls, actual dispatch/no-dispatch evidence, and any general CI remaining failures. Registry and subsystem addendum updated.

## Compatibility and non-goals

No public tool schema, protocol, RunStore migration, Windows MXC backend, administrator privilege, process service, release installer or sandbox default change. This is **not** a reintroduction of regex-as-security. No changes to ADR-0021/0022/0023 or C033/C035 historical closure. General draft PR conflict/rebase should be treated separately and not papered over by this CI corrective.
