# Workspace Authority and Session Navigation Milestone W001 — Workspace-Root Execution Authority

Status: ready for handoff

Repository baseline: `b77c8156b594494b7f59e0348d93a73ca2c1fd1c`
(branch `feat/auto-20261016-9ebb3f33`)

Source roadmap: `plans/subsystems/workspace-authority-and-session-navigation-corrective-addendum.md#milestone-w001--workspace-root-execution-authority-fail-closed`

Long-term requirements: `plans/000-long-term-specification.md#17-tui-target-architecture`;
`plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`;
`plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs: `plans/adrs/ADR-0033-directory-tied-project-identity-and-client-working-directory.md`
(accepted 2026-10-16). This milestone is **conformance work against that accepted
decision** and introduces no new architecture.

Primary class: invariant

## 1. Objective

Remove every process-cwd fallback from daemon-owned tool execution so that a
tool's working directory is always the propagated
`ExecutionContext.workspace_root`, and make absence of a resolved root a typed,
fail-closed outcome instead of a silent substitution of the daemon's launch
directory.

## 2. Why this milestone is ready

ADR-0033 is **accepted**, and its §2 already states the rule this milestone
implements: "The working directory is client state per tab (`ExecutionContext`);
the daemon never chdirs. 'Switch cwd' in all plans means switching the tab's
execution context, tool workdir default, and displayed path." The contract is
stable, so no decision negotiation is pending.

The defect is conformance, not design. `scripts/check_daemon_cwd_usage.py`
states the same requirement in its own docstring — working directories must come
from `ExecutionContext`, not process-global cwd — and the existing fallbacks
contradict it. No other unclosed milestone is a dependency.

## 3. Current implementation evidence

At baseline:

- `BashTool` resolves `workdir` from the tool argument, else `self.workspace_root`,
  and otherwise leaves it unset. When unset, six sites in
  `src/tool/bash/process.rs` fall back to `std::env::current_dir()` — covering the
  managed-shell path, the direct path, the Python-script path, and the
  resolution error path.
- `BashTool::default()` sets both `workspace_root` and `child_workspace_ceiling`
  to `None`, so a default-constructed tool is exactly the case that reaches those
  fallbacks.
- `crates/codegg-client` spawns the daemon without an explicit working directory,
  so the daemon inherits the launching process's cwd and keeps it for its whole
  lifetime.
- The session's own root is **correct** at baseline: the launch directory flows
  through workspace registration, session creation, runtime binding, and per-turn
  `ExecutionContext` construction. `validate_path` / `validate_target_path` join
  relative paths against `allowed_root` first.
- Observed on the review baseline: a daemon whose process cwd is
  `/Users/davidbowman/projects/worktree2/codegg` was serving a TUI launched from
  `/Users/davidbowman/projects/i2pr-tc`.
- `scripts/check_daemon_cwd_usage.py` allowlists
  `ToolRegistryOptions::default()` on the stated premise that "the canonical
  daemon path populates them via ToolRegistryOptions workspace_root". That
  premise is the unproven part. Its `PROTECTED_GLOBS` also omits `src/main.rs`
  and `src/exec.rs` by omission, although the docstring names both as
  intentionally permitted bootstrap readers.
- `BashTool` has two independent roots — `workspace_root` and
  `child_workspace_ceiling`. The ceiling is armed only by child-Git policy and
  must never be derived from `workspace_root`.

## 4. Invariants that must not regress

- `BashTool` keeps two independent roots. `child_workspace_ceiling` is the
  isolated child-Git worktree ceiling and is never derived from
  `workspace_root`.
- A relative path in any tool is relative to the session's `workspace_root`,
  never to process cwd.
- Path validation continues to canonicalize, reject symlink escapes, and join
  `allowed_root` before resolving a relative request. Never walk the raw request.
- Daemon process cwd is never mutated. ADR-0033 Option B (daemon chdirs) was
  explicitly rejected and must not be reintroduced.
- `check_daemon_cwd_usage.py` continues to pass, and continues to reject new
  process-cwd call sites in daemon-owned execution modules.
- Standalone, subagent, and job construction paths must keep working — they must
  be given a root through context, never through a fallback.
- No secret or credential path may appear in a fail-closed error message.

## 5. Scope

In scope:

- `src/tool/bash.rs` and `src/tool/bash/process.rs`: workdir resolution and the
  six fallbacks.
- Any other execution tool that resolves a working directory, root, or workdir
  from process cwd.
- Construction paths that currently build tools without a workspace root:
  registry factory options, `Default` constructors, subagent and standalone
  tool construction.
- `scripts/check_daemon_cwd_usage.py`: guard coverage and assertions.

Out of scope:

- Daemon `chdir` or any daemon process-cwd mutation.
- Session-switch root rebinding (that is W002).
- The `/workspace` and `/sessions` surfaces (W002, W003).
- ADR-0033's 1:1 root↔project rule, project archive/destroy, repath.
- Sandbox ceiling semantics beyond preserving the two-root distinction.
- Cross-platform re-qualification.

## 6. Required production changes

### Fail-closed resolution

Replace each process-cwd fallback with a typed failure that names the missing
context. The error must distinguish "no workspace root was resolved for this
call" from "the resolved root does not exist". It must not guess a directory.

The public, user-visible consequence is that a tool can no longer silently run in
an unrelated tree. Where a legitimate caller previously relied on the fallback, it
must now supply a root.

### Give every legitimate caller a root

Enumerate and fix the construction paths that produce a root-less tool:

- the registry factory and `ToolRegistryOptions` default;
- `Default` constructors for execution tools;
- subagent and delegated-run tool construction;
- standalone / non-daemon invocation;
- any test or fixture that constructs tools directly.

This is the bulk of the work and the main source of churn. A caller that cannot be
given a root must fail at construction, not at execution.

### Guard coverage

- Add `src/main.rs` and `src/exec.rs` to the scanned set **as explicit documented
  bootstrap exemptions**, so the guard's stated policy is actually enforced
  rather than satisfied by omission. Bootstrap must remain permitted to read cwd
  once at startup.
- Add an assertion that fails when a production-constructed execution tool can
  lack a workspace root, so the allowlist's "canonical daemon path populates them"
  premise is checked rather than assumed.
- Do not widen the allowlist. New exemptions require justification in the PR, as
  the script already states.

### Documentation of the daemon fact

Record that the daemon inherits its launch directory and that after this
milestone it is not an input to tool execution.

## 7. Ordered work packages

**W1 — Inventory before editing.** Enumerate every process-cwd reference in
`src/tool/**` and `crates/codegg-client`, and every construction site that
produces a root-less tool. Record the count. This inventory is the milestone's
scope truth; a mismatch with the plan's six-fallback figure is a finding, not an
inconvenience.

**W2 — Establish failing evidence.** Add a test that runs a session whose
`workspace_root` differs from the daemon's process cwd and asserts resolved tool
paths land in the session's directory. Confirm it fails against the baseline.

**W3 — Thread the context.** Fix the construction paths so a root is always
available on the canonical path. Run focused tests; expect fallout here first.

**W4 — Fail closed.** Remove the six fallbacks and replace them with typed
errors. Re-run W2's test to green.

**W5 — Strengthen the guard.** Add the explicit bootstrap entries and the
root-availability assertion. Confirm the guard passes and that a deliberately
injected violation fails it.

**W6 — Sweep.** Run the guard, `scripts/verify.sh quick`, focused tool tests, and
the daemon session tests.

## 8. Failure, cancellation, restart, and contention semantics

- Fail-closed must not strand legitimate work. If a construction path cannot be
  given a root, that path must be found in W1 and fixed in W3 — never left to
  fall through to a default.
- A daemon restart re-resolves nothing process-global; after this milestone a
  restarted daemon and a long-lived daemon behave identically with respect to tool
  execution, because neither's launch directory is an input.
- No new contention: root resolution is read-only against immutable per-turn
  context.
- Cancellation and sandbox behaviour are unchanged.

## 9. Compatibility and migration

No storage migration and no protocol change. This is a deliberate behaviour
change for any caller that relied on the implicit fallback; that change is the
point of the milestone and must be called out in release notes.

## 10. Required tests

Focused:

- Each of the six fallback sites resolves a typed error when no root is present,
  and never a directory.
- A tool with a resolved root uses it, including when the daemon's process cwd
  differs.
- Construction paths: registry factory with and without `workspace_root`;
  subagent and standalone construction; job-run construction.
- `BashTool` keeps `workspace_root` and `child_workspace_ceiling` independent,
  including that the ceiling is not derived from the root.

Integration:

- A real `CoreDaemon` started from directory A, with a session created for
  directory B, asserting every tool's resolved path is under B. This is the test
  that reproduces the reported symptom and must be the milestone's headline
  evidence.
- A guard self-test: the guard fails on an injected violation and passes clean.

## 11. Required verification commands

```bash
cargo fmt
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_execution_ownership.py
cargo test -p codegg --lib tool::
cargo test -p codegg --lib core::
cargo clippy --all-targets -- -D warnings
scripts/verify.sh quick
```

`scripts/verify.sh quick` includes the sandbox and execution-ownership guards.
Do not use `--all-features`.

## 12. Documentation updates

- `docs/daemon.md`: state that the daemon inherits its launch directory, that it
  is a user-scoped singleton, and that after W001 the launch directory is not an
  input to tool execution. Currently none of these documents mentions the fact at
  all.
- `architecture/core.md` and `architecture/session.md`: record workspace-root
  resolution order, and correct the `daemon_refresh.rs` line citation if it has
  drifted.
- `architecture/tool-programs.md` / `architecture/agent.md` as affected by the
  construction-path change.
- `AGENTS.md` gotcha on relative paths: strengthen from "never process cwd" to
  the enforceable claim that no fallback remains.
- `.opencode/skills/core/SKILL.md`, `.opencode/skills/tool-execution/SKILL.md`,
  `.opencode/skills/session-storage/SKILL.md` as their module guides change.
- `docs/security-semantics.md` if the fail-closed outcome is presented as a
  containment property.

## 13. Acceptance criteria

Externally observable:

- A session whose directory differs from the daemon's launch directory executes
  every tool against the session's directory.
- No tool ever runs in a directory it was not given.
- A tool constructed without a workspace root returns a typed error naming the
  missing context, and the message contains no credential path.
- `python3 scripts/check_daemon_cwd_usage.py` and `scripts/verify.sh quick` pass.

## 14. Stop conditions

Report rather than improvise if:

- W1's inventory finds call sites materially outside the stated scope — in
  particular any fallback in a subsystem with different ownership.
- Making a construction path fail closed would break standalone, subagent, or job
  execution in a way that requires a new authority decision. That is an ADR
  question, not an implementation detail.
- Any proposal to fix this by having the daemon `chdir` appears. ADR-0033
  Option B was rejected; superseding it requires a new ADR.
- The `child_workspace_ceiling` and `workspace_root` distinction cannot be
  preserved under the new resolution.

## 15. Closure evidence required

- Closure record at `plans/closure/workspace-authority-and-session-navigation/001-status.md`
  with a requirement-to-evidence matrix.
- The W1 inventory, with the resolved count.
- The W2 test shown failing at baseline and passing after.
- The two-directory daemon integration test result.
- Guard results, `cargo clippy -D warnings`, and `scripts/verify.sh quick`,
  labelled local vs hosted truthfully. No hosted CI claim without a cited run.

## 16. Handoff notes

Start at W1 and do not begin editing before the inventory is recorded — the
churn is concentrated in construction paths, and the fallback sites are the easy,
visible half of the problem. The headline acceptance evidence is the
two-directory daemon test; anything short of that does not demonstrate the
reported symptom is fixed.