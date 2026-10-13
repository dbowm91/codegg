# Shell Command Policy CI — Legacy Hard-deny vs Ask Reconciliation Corrective

Status: **ready C001** (independent of Windows MXC native research; no architecture/API changes).
Parent authority: `plans/003-planning-process.md`; canonical tool/permission, approval/sandbox and shell ownership contracts; relevant security ADR-0004.
Source failure: Linux CI run `38417097457` (SHA `3fbe6201`) and earlier `38405432003` failed `tests/tool_execution.rs::test_bash_tool_blocked_patterns` (`$(whoami)`) and `test_bash_tool_blocked_command_list` (`sudo su`). Tests were written for a historical regex hard-block expectation.

## Current reality and intended distinction

`src/tool/bash/policy.rs` distinguishes irreversible **hard-deny** command patterns and review-worthy **ask** patterns, while `src/permission/mod.rs` handles the approval decision. A direct `BashTool::new().execute` test without an approval context is not an end-to-end proof of an approval prompt. Restoring blanket string-spelling denials for `sudo` or `$(...)` would regress the intended structural-authority model and create obfuscation incentives; silently allowing them under a configured ask policy without an actual approval decision is also unacceptable.

Do not loosen canonical prohibitions for genuinely forbidden operations (e.g. destructive root mutations), do not hide CI failures by ignoring tests, and do not couple this repair to C036/C037 or MXC runtime support.

## Dependency

C001 `plans/implementation/shell-command-policy-ci-corrective/001-bash-hard-deny-vs-ask-test-reconciliation.md` is ready. Native MXC research can run independently. Closing C001 requires exact-head general CI green **or** explicit truthful blocking evidence for failures outside C001. Draft PR merge conflicts are a separate integration/rebase issue and are not automatically solved by C001.

## Acceptance

Correct and regression-test the three separate outcomes (hard-deny, policy ask requiring a decision, explicit authorized execution) through their canonical owners. No new command-regex security barrier, no silent bypass of permission mode, and no false positive from a direct model-facing tool call without approval context. Record closure, hosted test result and update the registry.
