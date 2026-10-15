# Shell Command Policy CI C001 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/shell-command-policy-ci-corrective/001-bash-hard-deny-vs-ask-test-reconciliation.md`
Source subsystem corrective: `plans/subsystems/shell-command-policy-ci-corrective-addendum.md`
Integrated feature head: `b89ba9e364bbf492d19048bed54803c4a8fd6d8e`
Mainline source of test rewrite: `9d32c35b516315adef21e30eb2a9e436826231e0`
Hosted Linux CI: [run 38435606749](https://github.com/dbowm91/codegg/actions/runs/38435606749)

## 1. Finding

C001 is closed. The stale expectations that `sudo su` and `$(whoami)` must be
rejected by direct `BashTool::execute` were replaced on mainline by an explicit
two-tier policy matrix and imported by C040. The actual agent-loop caller path
was also verified: an Ask-class shell command publishes a permission request
and does not dispatch before a human decision. No production permission code
or textual command blocker was added.

## 2. Corrected tests and controls

The historical failing assertions were
`test_bash_tool_blocked_patterns` (`$(whoami)`) and
`test_bash_tool_blocked_command_list` (`sudo su`). Mainline commit `9d32c35`
replaced their contract with tests including:

- `test_bash_tool_hard_deny_tier_blocks_catastrophic_commands` and
  `test_bash_tool_hard_deny_tier_survives_a_permissive_allowlist` for genuine hard-deny
  examples, including destructive root operations.
- `test_bash_escalation_signals_are_named` and
  `test_bash_escalation_signals_reach_the_operator_instead_of_being_denied`
  for named Ask signals such as `sudo su` and shell substitutions.
- `test_bash_escalation_signals_do_not_block_the_tool` and
  `test_bash_ordinary_commands_trip_no_escalation_signal` for classification
  and false-positive controls.
- `shell_escalation_ask_waits_for_explicit_approval_before_dispatch` in
  `tests/agent_loop_harness.rs` for the actual managed caller path. It uses a
  bounded `echo $(touch <temporary marker>)` command, observes
  `PermissionPending`, asserts the marker is absent before approval, responds
  with `AllowOnce`, and observes the marker only after dispatch.

The test does not execute `sudo su` or any destructive command. Existing
permission and approval-router suites exercise their canonical decision paths;
the configured sandbox ceiling and explicit-deny precedence remain unchanged.

## 3. Verification

Focused suites on the integration tree:

- `cargo test --locked --test agent_loop_harness shell_escalation_ask_waits_for_explicit_approval_before_dispatch` — passed (1 test).
- `cargo test --locked --test tool_execution` — passed (59 tests).
- `cargo test --locked --test permission` — passed (44 tests).
- `cargo test --locked --test approval_router` — passed (17 tests).
- `cargo test --locked --test command_routing_adversarial` — passed (139 tests).

Hosted [Linux CI run 38435606749](https://github.com/dbowm91/codegg/actions/runs/38435606749)
passed on the exact C040 integrated SHA `b89ba9e364bbf492d19048bed54803c4a8fd6d8e`,
including formatting, workspace Clippy, repository guards and workspace tests.

## 4. Security disposition

Hard-deny remains authoritative even when user rules are permissive. Ask-class
signals remain approval requests rather than direct-tool hard denials, and the
real agent-loop path demonstrably waits for explicit approval before dispatch.
The bounded approved test action occurs only after `AllowOnce`. No regex-only
security barrier, shell policy relaxation, process-ownership change, or
production sandbox change was made. No new bypass was found.

## 5. Registry and dependency disposition

C001 is closed and registered in `plans/registry.md` and the shell corrective
addendum. C040 is closed separately in
`plans/closure/windows-native-sandbox/040-status.md`. This closure does not
change Windows admission decisions or unblock any Windows security milestone.
