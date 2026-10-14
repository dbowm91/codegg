# Shell Command Policy CI — Legacy Hard-deny vs Ask Reconciliation Corrective

Status: **C001 closed; C002 closed (scoped)** (stale hard-deny test reconciliation under the accepted advisory contract; independent of Windows MXC admission).
Parent authority: `plans/003-planning-process.md`; canonical tool/permission, approval/sandbox and shell ownership contracts; relevant security ADR-0004.
Source failure: Linux CI run `38417097457` (SHA `3fbe6201`) and earlier `38405432003` failed `tests/tool_execution.rs::test_bash_tool_blocked_patterns` (`$(whoami)`) and `test_bash_tool_blocked_command_list` (`sudo su`). Tests were written for a historical regex hard-block expectation.

## Upstream implementation discovery (2026-10-13)

At `main` SHA `b9ba494c1b21638c2e2f660301ba207ce8859682`, commit `9d32c35b516315adef21e30eb2a9e436826231e0` has **already replaced** the two historic stale hard-block assertions with hard-deny-versus-Ask tests in `tests/tool_execution.rs`. The Windows work branch `631ae9488f251ac70e91b9c8299d2c1c3bff83fa` has not integrated those mainline commits. C001 is **not yet closed**: the mainline tests must be brought into the Windows branch by C040, the actual approval/no-bypass path must be verified, and focused plus hosted CI must pass at the integrated SHA. Do not recreate the upstream test fix or restore regex denial.

## Current reality and intended distinction

`src/tool/bash/policy.rs` distinguishes irreversible **hard-deny** command patterns and review-worthy **ask** patterns, while `src/permission/mod.rs` handles the approval decision. A direct `BashTool::new().execute` test without an approval context is not an end-to-end proof of an approval prompt. Restoring blanket string-spelling denials for `sudo` or `$(...)` would regress the intended structural-authority model and create obfuscation incentives; silently allowing them under a configured ask policy without an actual approval decision is also unacceptable.

Do not loosen canonical prohibitions for genuinely forbidden operations (e.g. destructive root mutations), do not hide CI failures by ignoring tests, and do not couple this repair to C036/C037 or MXC runtime support.

## Dependency

C001 `plans/implementation/shell-command-policy-ci-corrective/001-bash-hard-deny-vs-ask-test-reconciliation.md` is closed at exact integrated head `b89ba9e364bbf492d19048bed54803c4a8fd6d8e`; see `plans/closure/shell-command-policy-ci-corrective/001-status.md`. The corrected hard-deny/Ask matrix, actual approval/no-dispatch path, focused suites, and hosted Linux CI passed. Native MXC research remains independent.

## Acceptance

Correct and regression-test the three separate outcomes (hard-deny, policy ask requiring a decision, explicit authorized execution) through their canonical owners. No new command-regex security barrier, no silent bypass of permission mode, and no false positive from a direct model-facing tool call without approval context. Record closure, hosted test result and update the registry.

## C002 — Stale hard-deny test reconciliation under the advisory contract (active, 2026-10-14)

The later command-authorization 005 corrective accepted an empty tool-layer
default blocklist (ADR-0028), which C001-era tests still contradict — and two
of them execute genuinely destructive commands through a live shell on every
run, failing only on absent binaries. C002 rewrites the three affected
`tests/tool_execution.rs` tests (destructive examples through pure
classifiers with no shell; explicit hard-deny precedence through harmless
sentinels plus temp-file markers, including deny-over-allowlist),
reconciles the escalation-signals table to the verified current vocabulary,
fixes the terminal `env`-prefix allowlist expectation, reconciles the stale
`src/permission/mod.rs` hard-deny comment, and qualifies the managed path
(explicit human deny blocks dispatch; approval respects the sandbox ceiling).
No default blocklist is reintroduced. Plan:
`plans/implementation/shell-command-policy-ci-corrective/002-stale-hard-deny-test-reconciliation-under-advisory-contract.md`.
C001 evidence stays historical.
