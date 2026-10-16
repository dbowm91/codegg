# Shell Command Policy — ADR-0028 Post-Closure Corrective Addendum

Status: closed — C002 closed (implementation `b0392431`; closure `plans/closure/shell-command-policy-adr0028-post-closure-corrective/002-status.md`)

Planning baseline: `39f149a15ef529d66787d1d11551872ea22ec384` (main, 2026-10-14).

Canonical references:
- `plans/000-long-term-specification.md` §§4, 17, 27–29;
- `plans/001-terminology-and-domain-model.md` §8;
- `plans/003-planning-process.md` §§2, 4, 6–10;
- **accepted** `plans/adrs/ADR-0034-command-syntax-and-execution-authority.md` (governs this contradiction; ADR-0028 is a historical alias archived by the identity migration);
- accepted ADR-0004 for approval vs obtained sandbox separation.

Historical ownership, retained without retrospective rewrites:
- `plans/subsystems/shell-command-policy-ci-corrective-addendum.md` and `plans/closure/shell-command-policy-ci-corrective/001-status.md` (C001 closed under an obsolete default hard-deny expectation);
- `plans/closure/command-authorization-and-execution-boundaries/005-status.md` (M001 C002 closed: default command blocklist empty, built-in risk advisory);
- `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md` (M001–M003 closed);
- `architecture/permission.md`, `architecture/security.md`, `docs/security-semantics.md`, `architecture/tool.md`.

## Purpose and evidence

Correct a contradictory *test and historical-closure contract* without reintroducing syntax-based command rejection. `src/tool/bash/policy.rs::default_blocked_commands()` intentionally returns an empty set; `find_risk_pattern` and `find_ask_pattern` classify risk as advisory. `src/tool/bash.rs::execute_inner` checks explicit `blocked_commands` and allowlist then may spawn when directly invoked. `tests/tool_execution.rs` instead asserts that `BashTool::new().execute` hard-denies catastrophic command text; its `HARD_DENY_COMMANDS` cases include `rm -rf /`, `mkfs`, `shutdown` and `reboot`. These tests are unsafe to execute unchanged on a host. Separate escalation tests still demand syntax-only Ask for `$(whoami)`, backticks, `eval`, `exec`, and Base64 decoding, contradicting current intentional classifier behavior. The current permission-module comment falsely asserts that catastrophic strings remain rejected by BashTool itself. Historical C001's evidence describes an earlier two-tier promise that is no longer true.

The accepted ADR-0028 is the authority; preserve its best-effort explicit text Deny, true permission/capability/sandbox Deny, and advisory/default-low-friction boundary. Do not roll back the accepted change merely to make stale tests pass.

## Ownership and invariants

- **Tool policy** (`src/tool/bash/policy.rs`): bounded built-in risk metadata; explicit configured `blocked_commands` and allowlist are invocation-level best-effort workflow restrictions, never OS containment.
- **Approval** (`src/permission/mod.rs` + ApprovalRouter): explicit operator/agent/session Deny terminal; concrete destructive/privilege/credential risks may Ask, subject to documented precedence and permission mode; benign shell grammar alone is not a reason to Ask/Deny.
- **Execution** (`ToolBroker`, managed process, `SandboxProfile`): actual authorization and obtained environment control; required unavailable sandbox fails closed before spawn; no duplicate prompt or shell bypass.
- **Tests**: dangerous command strings only in **pure nonexecuting** classifier/policy functions; real process tests use benign, uniquely named files under `tempfile` with no host-global effects, never assume a regex prevents an unsafe command from running.
- **History**: preserve `plans/closure/shell-command-policy-ci-corrective/001-status.md` and `plans/closure/command-authorization-and-execution-boundaries/005-status.md`; a separate corrective closure supersedes conflicting conclusions.
- **Compatibility**: no new tool schema, config setting, shell keyword blacklist, broad permission policy rewrite, persistence migration or effectful default-deny override.

## Dependencies / milestones

```text
Accepted ADR-0028 + closed command-auth C002
                +
Historically closed shell CI C001 with contradicted test expectations
                |
                v
C002 Shell policy test safety + contract reconciliation [ready]
                |
                v
C002 exact-SHA verification and separate closure; no other new milestone
```

### C002 — Make shell regression tests safe and truthful

Primary class: **invariant** (test harness must never dispatch destructive host commands merely to assert a textual hard stop).

Plan: `plans/implementation/shell-command-policy-adr0028-post-closure-corrective/002-advisory-shell-test-safety-and-contract-closure.md`.

Hard dependencies: accepted ADR-0028, command-auth M001 C002, original C001 closure; all satisfied. Operational: clean exact-head CI may remain red due to an unrelated typed Git-clean test (separate roadmap/plan). Implementation may proceed independently of that issue.

Acceptance: remove every direct `BashTool::execute` potentially catastrophic test, prove advisory classifications without spawning, prove explicit text/policy Deny overrides permissive local rules with harmless fixtures, prove real agent approval/no-dispatch and required-sandbox fail-closed with marker fixtures, run a static guard that prevents reintroduction, update commentary/docs, and publish `plans/closure/shell-command-policy-adr0028-post-closure-corrective/002-status.md` with precise outcomes and upstream-independent CI disposition.

## Verification and security expectations

Target `tests/tool_execution.rs`, `tests/permission.rs`, `tests/agent_loop_harness.rs`, `src/tool/bash/policy.rs` and `src/permission/mod.rs` in one bounded pass. Add a guard to `scripts/verify.sh quick` and the canonical `.github/workflows/ci.yml` as appropriate; negative mutation fixture proves guard actually rejects a dangerous direct-execution test. CI failure unrelated to the changed scope is logged explicitly, never hidden with `#[ignore]` or an invented `ToolError::Permission`. The branch may advance when focused tests and guard pass; strict closure requires exact test evidence and a truthful hosted outcome, with separate Git corrective dependency only for whole-workspace green.

Do not execute `cargo test --workspace` against the unchanged baseline before removing the dangerous tests; the current historical test suite assumes built-in hard-denies that no longer exist.

## Non-goals and stop conditions

No new hardcoded regex denial, no ad hoc command parser as security boundary, no unsafe `FullHost` fallback, no rewriting closed C001 evidence or accepted ADR-0028. Stop if an actual authorization bypass is found in managed shell routes: register a separately owned security corrective for the canonical ApprovalRouter/ManagedProcess boundary, not a silent text blacklist. Stop if tests require executing a catastrophic host-global command to demonstrate their proposition; use purely parsed or mocked evidence.
