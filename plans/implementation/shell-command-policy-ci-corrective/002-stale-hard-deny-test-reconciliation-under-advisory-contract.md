# Shell Command Policy CI C002 — Stale Hard-deny Test Reconciliation Under the Advisory Contract

Status: **active**; narrowly scoped post-closure corrective, not a shell-policy redesign
Primary class: security regression corrective
Source: `plans/subsystems/shell-command-policy-ci-corrective-addendum.md`.
Canonical: `plans/000-long-term-specification.md` §§4,17,27,29; ADR-0004; ADR-0028; current permission and sandbox contract.
Predecessor: C001 (`plans/closure/shell-command-policy-ci-corrective/001-status.md`) closed the legacy hard-deny-vs-Ask rewrite; the later command-authorization 005 corrective then accepted an empty tool-layer default blocklist, which C001-era tests still contradict.

## Defect

Three `tests/tool_execution.rs` tests plus one `src/tool/terminal.rs` unit test
assert the superseded tool-layer default-deny contract, and two of them
**execute genuinely destructive commands** (`rm -rf /`, `mkfs`, `dd`,
`chmod -R 777 /`) through a live shell on every run — currently failing only
because the binaries are absent (exit 127), not because any guard holds:

- `test_bash_tool_denied_pattern` executes `rm -rf /` expecting a tool denial.
- `test_bash_tool_hard_deny_tier_blocks_catastrophic_commands` executes nine
  destructive spellings expecting pre-spawn denial.
- `test_bash_tool_hard_deny_tier_survives_a_permissive_allowlist` executes four
  destructive spellings under a permissive allowlist.
- `terminal_custom_restrictions_only_narrow_canonical_policy` expects an
  allowlisted `env rm -rf /` to be admitted.
- `test_bash_escalation_signals_are_named` /
  `test_bash_escalation_signals_reach_the_operator_instead_of_being_denied`
  pin the retired signal vocabulary (substitution/eval/exec/base64), which the
  accepted classifier no longer emits.

The accepted contract (command-auth 005, ADR-0028) is: empty tool-layer
default blocklist; destructive shapes classified Medium with a named Ask
signal and gated at approval/sandbox; explicit configured restrictions deny
pre-spawn with precedence. No nonempty default blocklist may be reintroduced
to satisfy the failing tests — that would contradict ADR-0028 and restore the
easily bypassed restrictions deliberately removed.

## Ordered work packages

**A — Replace unsafe invocations first.** Rewrite the three tests before any
further suite execution: destructive examples through pure classifiers
(`classify_command` risk + `find_ask_pattern` names, no shell); explicit
hard-deny precedence through harmless sentinel words plus temp-file markers
(absent marker proves pre-spawn denial), including explicit-deny-overrides-
permissive-allowlist with a positive allowlist control. Reconcile the
escalation table to the verified current vocabulary and fix the terminal
`env`-prefix expectation (prefix must not escape the allowlist).

**B — Reconcile prose.** Fix the stale `src/permission/mod.rs` comment
claiming catastrophic shapes stay hard-denied in the tool; update the
shell-policy addendum with a C002 section. Historical C001 evidence stays
untouched.

**C — Qualify managed execution.** Advisory Ask-class shell waits for an
explicit decision (existing `shell_escalation_ask`); add explicit-human-deny
prevents dispatch (marker absent, denial recorded) and
approval-respects-sandbox-ceiling (WorkspaceWrite approval against a
FullHost-configured tool refuses pre-execution, marker absent).

**D — Verify and close.** `cargo test` for `tool_execution`,
`agent_loop_harness` shell tests, `permission`, `approval_router`,
`command_routing_adversarial`, lib `tool::bash::policy` + `tool::terminal`;
`cargo fmt --check`; workspace Clippy; `verify.sh quick`; hosted CI on the
exact SHA. Write closure `002-status.md`; update addendum and registry.

## Acceptance

- No test in the suite spawns a shell for a destructive example.
- Explicit deny (tool restriction or human decision) is proven by absent
  markers; named Ask signals route to approval, never to tool denial.
- No default blocklist reintroduced; ADR-0028 and command-auth 005 stand.
