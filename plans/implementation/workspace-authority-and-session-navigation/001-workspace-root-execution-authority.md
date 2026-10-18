# Workspace Authority and Session Navigation Milestone W001 — Workspace-Root Execution Authority

Status: implemented — W1, W1b, W2, W3, W4, W5, W7 complete; W6 partially
retired; W8 verification in progress. **Reachability premise corrected by §19**:
the live defect is a hard-fail, not a silent retarget. Closure record:
`plans/closure/workspace-authority-and-session-navigation/001-status.md`.

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
  daemon path populates them via ToolRegistryOptions workspace_root". Its
  `PROTECTED_GLOBS` also omits `src/main.rs` and `src/exec.rs` by omission,
  although the docstring names both as intentionally permitted bootstrap readers.
  **Corrected by the §18 reachability pass: the premise is not merely unproven,
  it is disproven.** The guard passes (exit 0) with all 28 sites present, because
  its `ALLOWLIST` suppresses them by line pattern. "No new violations" is
  therefore not evidence of conformance here.
- `ExecutionContext.workspace_root` is a `PathBuf`, **not** an `Option<PathBuf>`, and
  the canonical factory `build_session_tool_registry` always populates it. A root
  therefore always exists on the daemon path; the `Option` in
  `ToolRegistryOptions` exists only to serve non-canonical callers. See §18.1.
- `ToolRegistry::with_options` already resolves each tool as
  `Some(root) => tool.with_allowed_root(root)`, `None => tool.default()`. The
  `None` arms are the production risk; the `Some` arms are already correct. See §18.2.
- `SubAgentRequest.workspace_root` **is** `Option<PathBuf>`, and two production
  constructors can leave it `None` — `src/job_dispatcher.rs:75` and
  `src/scheduler/executors.rs:717`, both `allowed_paths.first().map(..)`. An empty
  `allowed_paths` yields `None`, which `worker.rs:1273` forwards into the registry.
  `worker.rs:1148` already branches on that `None` to deny `commit`, confirming the
  case is reachable. This is the reported symptom's live path. See §18.3.
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

In scope, **ordered by the §18 reachability pass** (see §6 for the full
rationale; this list is scope, not sequence):

- The two live `workspace_root: None` producers: `src/job_dispatcher.rs:75` and
  `src/scheduler/executors.rs:717`. This is the reported symptom's path.
- `ToolRegistry::with_options`'s `None` arms (`src/tool/mod.rs` and the ~15
  sibling registrations), converted to fail-closed.
- `build_session_tool_registry_legacy` (`src/tool/factory.rs:313-324`), for
  **deletion** — zero callers.
- `src/tool/bash.rs` and `src/tool/bash/process.rs`: workdir resolution and the
  six fallbacks.
- Any other execution tool that resolves a working directory, root, or workdir
  from process cwd — including the 18 cwd-defaulting `Default`/`new()`
  constructors, which §6.3 reaches by closing the registry `None` arms first.
- Construction paths that currently build tools without a workspace root:
  registry factory options, `Default` constructors, subagent and standalone
  tool construction.
- `scripts/check_daemon_cwd_usage.py`: guard coverage and assertions. It passes
  today with all 28 sites present, so it detects nothing as written.

Out of scope:

- Daemon `chdir` or any daemon process-cwd mutation.
- Session-switch root rebinding (that is W002).
- The `/workspace` and `/sessions` surfaces (W002, W003).
- ADR-0033's 1:1 root↔project rule, project archive/destroy, repath.
- Sandbox ceiling semantics beyond preserving the two-root distinction.
- Cross-platform re-qualification.

## 6. Required production changes

**Revised by the §18 reachability pass.** The original order — patch 28 fallbacks,
then fix construction paths — is inverted. The count of `std::env::current_dir()`
sites is not the cost driver: the canonical path already threads a non-optional
root, and the 18 cwd-defaulting constructors are the *default value that one
`None` arm exposes*, not 18 independent defects. Work is therefore ordered by
**reachability**, cheapest-and-highest-value first. §18 is the evidence for every
claim in this section.

### 6.1 Close the live `None` producers (fixes the reported symptom)

`SubAgentRequest.workspace_root` is genuinely optional and is forwarded into
`ToolRegistryOptions` at `src/agent/worker.rs:1273`. Two production constructors
can produce `None`:

- `src/job_dispatcher.rs:75` — `allowed_paths.first().map(PathBuf::from)`.
- `src/scheduler/executors.rs:717` — the same shape, only overwritten at line
  886 when an isolated worktree is leased.

Each must resolve a real root from daemon-owned workspace state, or fail the
dispatch closed with a typed error. Neither may substitute process cwd. This is
the smallest diff that removes the reported symptom end to end.

### 6.2 Delete dead code rather than migrate it

`build_session_tool_registry_legacy` (`src/tool/factory.rs:313-324`) has **zero
callers repo-wide**. It reads process cwd to synthesise a fake `WorkspaceRecord`.
Delete it. §9 forbids retaining unused shims, and migrating a dead cwd read
would leave the very pattern this milestone exists to remove.

### 6.3 Fail closed in the registry's `None` arms

`ToolRegistry::with_options` already resolves the `Some` arms correctly. Convert
its `None` arms (~15 sites, e.g. `src/tool/mod.rs:598-620`) from
`Tool::default()` to **fail-closed** behaviour.

This is the pivotal change: it removes the *reachability* of all 18
cwd-defaulting constructors from the daemon path in one edit, rather than 18
separate ones. A registry built without a root must not silently produce tools
whose `allowed_root` is the daemon's launch directory.

Only **after** this lands, decide per constructor whether it is still needed:
delete those reachable only from tests, convert the remainder to typed errors.

### 6.4 Fail-closed resolution

Replace each remaining process-cwd fallback with a typed failure that names the
missing context. The error must distinguish "no workspace root was resolved for
this call" from "the resolved root does not exist". It must not guess a
directory, and must not fall back to `"."`.

`ToolError` (`crates/codegg-core/src/error.rs:141`) has no variant that
expresses missing execution context; add one rather than overloading
`ToolError::Execution`, which is non-retryable but does not distinguish this
case. `edit.rs` and `commit.rs` currently **panic** on an unreadable cwd and
`replace.rs` degrades to `"."` — inside a user-scoped daemon a panic takes down a
process serving other projects, so these are the highest-severity sites even
though they are not on the confirmed live path.

The public, user-visible consequence is that a tool can no longer silently run in
an unrelated tree. Where a legitimate caller previously relied on the fallback, it
must now supply a root.

### 6.5 Guard coverage

The guard currently **passes** while all 28 sites are present (§18.5), so it
detects nothing today. Rewriting it is part of the fix, not cleanup:

- Stop pattern-matching the production cwd lines the allowlist currently excuses.
  Those exemptions were written for pre-migration code and now suppress the
  defect.
- Add `src/main.rs` and `src/exec.rs` to the scanned set **as explicit documented
  bootstrap exemptions**, so the guard's stated policy is enforced rather than
  satisfied by omission. Bootstrap must remain permitted to read cwd once at
  startup.
- Add an assertion that fails when a production-constructed execution tool can
  lack a workspace root — i.e. assert no `None` arm reaches a workspace tool in
  `with_options`.
- Verify the guard **fails on an injected violation**, not merely that it passes.
  A green guard that suppresses the pattern proves nothing.
- Do not widen the allowlist. New exemptions require justification in the PR, as
  the script already states.

### 6.6 Documentation of the daemon fact

Record that the daemon inherits its launch directory and that after this
milestone it is not an input to tool execution.

## 7. Ordered work packages

**Reordered by the §18 reachability pass.** The original sequence treated the
fallback sites as the main work and the construction paths as "the bulk of the
churn"; §18 showed the reverse is true. W1 is complete; W2 below therefore
establishes failing evidence for the **confirmed live path**
(`SubAgentRequest.workspace_root: None`), not for `bash/process.rs`.

**W1 — Inventory before editing.** Enumerate every process-cwd reference in
`src/tool/**` and `crates/codegg-client`, and every construction site that
produces a root-less tool. Record the count. This inventory is the milestone's
scope truth; a mismatch with the plan's six-fallback figure is a finding, not an
inconvenience. **Done — 28 sites, §17.**

**W1b — Reachability pass.** Trace each inventoried site to the construction path
that can actually reach it, so the work is sized by live paths rather than by
textual count. **Done — §18.** This changed the plan's §6 and §7.

**W2 — Establish failing evidence on the live path.** Add a test that drives a
durable subagent dispatch with an **empty `allowed_paths`** (so
`job_dispatcher.rs:75` yields `None`) and asserts the resolved tool root is not
the daemon's process cwd. Confirm it fails at baseline.

Do **not** substitute `bash/process.rs` for this. Those six fallbacks need
`BashTool.workspace_root == None`, which the canonical factory never produces, so
a test aimed at them may pass for the wrong reason — or fail for a reason the fix
does not address. The two-directory daemon test remains the milestone's headline
integration evidence (§10), but W2's unit-level failing evidence should target the
path §18.3 confirmed.

**W3 — Close the live `None` producers.** Fix `job_dispatcher.rs:75` and
`executors.rs:717` per §6.1 so a durable subagent receives a real root or fails
closed. Re-run W2's test to green. This is the smallest diff that fixes the
reported symptom; expect the least fallout here.

**W4 — Delete the dead legacy factory.** Remove
`build_session_tool_registry_legacy` (`factory.rs:313-324`), zero callers, per
§6.2. Do not migrate it.

**W5 — Fail closed in the registry `None` arms.** Convert the ~15 `None` arms in
`ToolRegistry::with_options` per §6.3. This is the highest-leverage single edit:
it removes the daemon-path reachability of all 18 cwd-defaulting constructors at
once. Run focused tool tests; fallout belongs here.

**W6 — Retire the remaining fallbacks.** Only now decide per constructor: delete
those reachable solely from tests, convert the rest to typed errors, and fix the
remaining resolve-time fallbacks (`bash/output.rs`, `terminal.rs`,
`formatter.rs`, `tool_program.rs`) and the panicking/degrading ones (`edit.rs`,
`commit.rs`, `replace.rs`) per §6.4. Most of this may prove to be deleting code
that W5 already made unreachable — that is a success, not leftover work.

**W7 — Rewrite the guard.** Per §6.5: stop excusing the production sites, add the
explicit `src/main.rs`/`src/exec.rs` bootstrap exemptions, add the
root-availability assertion, and **prove the guard fails on an injected
violation**. The guard currently passes while the defect is present, so "it
passes" is not the acceptance bar.

**W8 — Sweep.** Run the guard, `scripts/verify.sh quick`, focused tool tests, and
the daemon session tests.

## 8. Failure, cancellation, restart, and contention semantics

- Fail-closed must not strand legitimate work. If a construction path cannot be
  given a root, that path must be found in the inventory (§17) and fixed in W3,
  W5, or W6 — never left to fall through to a default. W3 in particular covers the
  durable-subagent dispatch path, so an empty `allowed_paths` must produce a typed
  refusal rather than a root-less registry.
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

- A durable subagent dispatch with an empty `allowed_paths` resolves a real
  workspace root, or fails closed — never the daemon's process cwd. This is the
  §18.3 confirmed path and W2's failing evidence.
- A registry built without `workspace_root` does **not** yield workspace tools
  whose `allowed_root` is the process cwd (§6.3). This covers all 18
  cwd-defaulting constructors by reachability rather than one test each.
- Each *remaining* process-cwd fallback resolves a typed error when no root is
  present, and never a directory. Assert the panic sites (`edit.rs`,
  `commit.rs`) return an error rather than unwinding, and that `replace.rs` no
  longer degrades to `"."`.
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
- A guard self-test: the guard **fails on an injected violation** and passes
  clean. Because the guard currently passes while all 28 sites are present, the
  failing half of this test is the one that carries the evidence.

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
- `python3 scripts/check_daemon_cwd_usage.py` and `scripts/verify.sh quick` pass,
  **and** the guard fails when a process-cwd call is injected into a protected
  module. A guard that merely passes is not evidence: it passes today with all 28
  sites present (§18.5).
- `build_session_tool_registry_legacy` no longer exists.

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
- The W1 inventory, with the resolved count, and the W1b reachability table.
- The W2 test shown **failing at baseline** and passing after, with the failure
  reason stated — and confirmation that it targets the `SubAgentRequest.workspace_root:
  None` path, not `bash/process.rs` (§7 W2).
- Evidence for W4: `build_session_tool_registry_legacy` deleted with zero callers
  confirmed before deletion.
- Evidence for W7: the guard **failing on an injected violation**, not only
  passing clean.
- The two-directory daemon integration test result.
- Guard results, `cargo clippy -D warnings`, and `scripts/verify.sh quick`,
  labelled local vs hosted truthfully. No hosted CI claim without a cited run.

## 16. Handoff notes

Start at **W2**, not W1: the inventory (§17) and the reachability pass (§18) are
both recorded and complete.

Do not size this milestone by its 28-site count. The canonical daemon path always
has a non-optional root, and the 18 cwd-defaulting constructors are the default
value one `None` arm exposes — so W5's single edit to `ToolRegistry::with_options`
removes their daemon-path reachability at once. Effort belongs on the two live
`None` producers (W3), the registry `None` arms (W5), and the guard (W7).

Two traps, both already paid for once:

- **The guard is not currently a check.** It passes with all 28 sites present
  because its allowlist suppresses them. Never accept "the guard passes" as
  evidence; require that it *fails* on an injected violation.
- **Do not aim W2's failing evidence at `bash/process.rs`.** Those six fallbacks
  require `BashTool.workspace_root == None`, which the canonical factory never
  produces. A test aimed there can fail for a reason the fix does not address, or
  pass for the wrong reason. Target the `SubAgentRequest.workspace_root: None`
  path that §18.3 confirmed live.

The headline acceptance evidence is the two-directory daemon test; anything short
of that does not demonstrate the reported symptom is fixed.

## 17. Execution log — W1 inventory (recorded 2026-10-17)

Executed before any edit, as §7 W1 requires. **The plan's "six fallback sites"
figure is wrong by a factor of nearly five**, and §7 anticipates exactly this:
"a mismatch with the plan's six-fallback figure is a finding, not an
inconvenience."

Every `std::env::current_dir()` textual match under `src/tool/**` — 32 in total —
classified. All are outside `#[cfg(test)]`, i.e. all compile into production.

### Class A — cwd-defaulting constructors (18 sites)

These set the tool's `allowed_root` / `workdir` to the daemon's launch directory
**at construction**, which is the "root-less tool reaches the fallback" case the
plan's §6 "Give every legitimate caller a root" exists to close.

`read.rs:148`, `glob.rs:25`, `grep.rs:38`, `edit.rs:25`, `replace.rs:25`,
`write.rs:18`, `write.rs:161` (a second `Default`), `apply_patch.rs:32`,
`diff.rs:45`, `lsp.rs:664`, `lsp_read.rs:95`, `git.rs:44`, `git_read.rs:158`,
`commit.rs:31`, `review.rs:27`, `research.rs:50`, `backend.rs:174`,
`skill.rs:77`.

Three of them fail differently, which matters for the fail-closed work:

- `edit.rs:26` and `commit.rs:32` **panic** if cwd is unreadable.
- `replace.rs:26` logs a warning and silently degrades to `"."`.

A panic is arguably worse than the silent substitution the milestone targets:
inside a user-scoped daemon it takes down a process that is serving other
projects' sessions. Both must become typed errors, not panics.

### Class B — execution-path fallbacks (10 sites)

`bash/process.rs` — 368, 658, 734, 781, 978, 1098 (the plan's six, confirmed
exact), plus `bash/output.rs:173`, `terminal.rs:276`, `formatter.rs:53`,
`tool_program.rs:418`.

`bash/process.rs:781` is a two-step fallback
(`.or_else(current_dir).unwrap_or_else(|| request.cwd.clone())`), so it has two
directories it can silently pick and needs both removed together.
`bash/process.rs:978` is the git path and carries an explicit documented
precedence list ending in "4. Process cwd." — that list needs rewriting, not just
the last line.

### Class C — documented legacy shim (1 site)

`factory.rs:324` — `build_session_tool_registry_legacy`, whose own docstring says
"New production code MUST NOT call this" and which synthesizes an
`ExecutionContext` rooted at cwd for tests. Keep the shim, keep its cwd read,
but it must be provably unreachable from production — which is exactly the
`check_daemon_cwd_usage.py` root-availability assertion W5 is to add.

### Class D — doc comments only (3 sites)

`mod.rs:352`, `mod.rs:1960`, `test.rs:29`. Not call sites.

### Finding: there is already an in-repo precedent for the fix

`test.rs:29-31` documents that `TestTool` was given exactly this treatment
already — "Without it the tool fell back to `std::env::current_dir()`, which in a
daemon is the directory the daemon was launched from — so a session rooted at one
repository ran that repository's tests inside whatever…" and the tool now
carries an explicit base for a relative `workdir`. W001 should follow that
established pattern rather than invent one, and `TestTool` is the worked example
to copy the shape from.

### Stop-condition assessment

§14's first stop condition is "W1's inventory finds call sites materially
outside the stated scope — **in particular any fallback in a subsystem with
different ownership**." Every site is under `src/tool/`, the same subsystem and
the same ownership as the sites the plan already named, so the specific trigger
does not fire. §6 already puts "any other execution tool that resolves a working
directory, root, or workdir from process cwd" in scope, and calls the
construction-path work "the bulk of the work and the main source of churn."

**Decision: proceed, with the inventory above carried into the closure record and
the estimate revised from six sites to twenty-eight.** The volume is the finding;
it is not grounds to stop, because the scope is the one the plan already chose.

## 18. Research log — reachability of the 28 sites (recorded 2026-10-17)

The §17 inventory counted *textual* occurrences. Before choosing an approach, the
28 sites were traced to their **construction paths** to establish which can
actually reach process cwd in production. This changes the recommended shape of
the work: the count is not the cost driver.

### 18.1 The root is already non-optional on the daemon path

`codegg_core::workspace::ExecutionContext.workspace_root` is a **`PathBuf`, not an
`Option<PathBuf>`** (`crates/codegg-core/src/workspace.rs:657`). The canonical
factory `build_session_tool_registry` takes `Arc<ExecutionContext>` and sets
`workspace_root: Some(execution.workspace_root.clone())` (`src/tool/factory.rs:190`).
So on the canonical daemon path a root **always exists**; the `Option` in
`ToolRegistryOptions.workspace_root` (`src/tool/mod.rs:355`) exists only to serve
non-canonical callers.

This means §6's "typed, fail-closed outcome" is **not** about a root that can be
absent in normal operation. It is about closing the specific `None` arms that let
a root-less construction reach execution.

### 18.2 The registry already threads the root; only its `None` arm is dangerous

`ToolRegistry::with_options` already resolves each tool as
`Some(root) => Tool::default().with_allowed_root(root), None => Tool::default()`
(`src/tool/mod.rs:598-620` and ~15 more). The `Some` arms are correct. **The
`None` arms are the entire production risk**, because `Tool::default()` runs the
constructor from §17 Class A, which reads `std::env::current_dir()`.

This reframes the 18 Class A constructors: they are not 18 independent bugs to
hand-patch. They are the *default value* that a single `None` arm exposes.

### 18.3 Confirmed live production path to `workspace_root: None`

`SubAgentRequest.workspace_root` is genuinely `Option<PathBuf>`
(`src/agent/worker.rs:96`), and `worker.rs:1273` forwards it into
`ToolRegistryOptions`. Two production constructors can leave it `None`:

- `src/job_dispatcher.rs:75` — `allowed_paths.first().map(PathBuf::from)`. An
  empty `allowed_paths` yields `None`, so a durable subagent job builds a
  registry with no root and every workspace tool silently falls back to the
  daemon's launch directory. **This is the reported symptom's live path.**
- `src/scheduler/executors.rs:717` — same
  `allowed_paths.first().map(...)` shape; only overwritten with
  `Some(worktree.path)` later at line 886 when an isolated worktree is leased.

`worker.rs:1148` already branches on `request.workspace_root.is_none()` to deny
`commit`, which independently confirms `None` is reachable rather than
theoretical. The other three production constructors
(`specialized_runtime.rs:166`, `snapshot_capture.rs:162`, `task.rs:2053`) pass
`Some(..)`.

### 18.4 Dead and doc-only sites

- `src/tool/factory.rs:324` `build_session_tool_registry_legacy` has **zero
  callers repo-wide** (verified outside its own definition). Its docstring
  already says "New production code MUST NOT call this." It is dead code and
  should be deleted, not migrated — §6 forbids keeping unused shims.
- `src/tool/mod.rs:352,1960` and `src/tool/test.rs:29` are doc comments.

### 18.5 The existing guard does not currently detect these sites

`scripts/check_daemon_cwd_usage.py` **passes today** (exit 0) with all 28 sites
present, because its `ALLOWLIST` suppresses them by line pattern. Reproduced
locally: `allowed_root: std::env::current_dir` (11 sites),
`std::env::current_dir().` (6), `or_else(|| std::env::current_dir().ok())` (6),
`workdir: std::env::current_dir` (4), `let cwd = std::env::current_dir` (2); the
only 4 unmatched lines are the doc comments from §18.4.

So the guard's stated policy ("new calls are rejected") is currently satisfied by
**suppression of every existing call**, not by absence. §6's "do not widen the
allowlist" cannot be executed meaningfully until the allowlist stops covering the
production sites it was written to excuse. This is the same class of defect the
AGENTS.md `check_daemon_cwd_usage.py` gotcha warns about: the allowlist exempts
the `Default::new()` constructors, so "no new violations" is not proof the
default is right.

### 18.6 Recommended approach

Ordered cheapest-and-highest-value first, rather than by file:

1. **Close the live `None` paths** (`job_dispatcher.rs:75`,
   `executors.rs:717`) so a durable subagent either receives a real root or fails
   closed. This alone fixes the reported symptom and is a small, targeted diff.
2. **Delete the dead legacy factory** (`factory.rs:313-324`) rather than migrate it.
3. **Convert the `None` arms in `ToolRegistry::with_options`** into fail-closed
   behavior, which removes the *reachability* of all 18 Class A constructors from
   the daemon path.
4. **Then** decide per-constructor whether it is still needed, delete the ones that
   are only reachable from tests, and convert the rest to typed errors.
5. **Rewrite the guard** to (a) scan `src/main.rs` / `src/exec.rs` as explicit
   documented bootstrap exemptions, (b) assert no production `None` arm exists,
   and (c) stop pattern-matching the production cwd lines it currently excuses.

Steps 1-3 are the actual fix and are far smaller than 28 sites. Steps 4-5 are the
durable hardening. Reporting the count as the cost driver would overstate the work
by roughly an order of magnitude and would push effort into patching constructors
whose only remaining caller is a test.
## 19. Execution log — premise correction (recorded 2026-10-17)

Executing W2 invalidated one load-bearing claim in §18. Recording it here
rather than quietly editing §18, because §18 is the dated research record and
the correction is the interesting part.

### 19.1 `job_dispatcher.rs:75` is not a live path — it is dead code

§18.3 called `src/job_dispatcher.rs:75` "the reported symptom's live path".
It is not:

```
$ grep -rn "SubAgentJobDispatcher" --include=*.rs .   # excluding its own file
(no output)
```

`JobDispatcher`, `SubAgentJobDispatcher` and `NullJobDispatcher` have zero
references outside the file that declares them. `docs/execution-ownership.toml`
already records the dispatcher as a legacy bridge "superseded by
SubagentJobExecutor". The live producer is `src/scheduler/executors.rs:717`
and only that one. §5's scope item for `job_dispatcher.rs` is therefore a
deletion, not a fix.

### 19.2 `workspace_root: None` does not silently retarget — it hard-fails

§18.3 also predicted that `SubAgentRequest.workspace_root: None` would reach
`ToolRegistry::with_options`, whose `None` arm builds tools whose
`allowed_root` is the daemon's process cwd. The registry *is* built that way
(`worker.rs:1272`), but the child loop never starts:

```rust
// src/agent/worker.rs:1494
let workspace_root = request
    .workspace_root
    .clone()
    .ok_or_else(|| "subagent execution requires an explicit workspace root".to_string())?;
```

This is *before* `AgentLoop::new`, so the cwd-defaulted tools are constructed
and then never invoked. W2's test caught this as its baseline failure:

```
assertion `left == right` failed: child run failed before the read was attempted:
  Subagent task failed: subagent execution requires an explicit workspace root
  left: Failed
 right: Completed
```

So the live defect is a **liveness** defect, not a cross-workspace authority
breach: a legitimate daemon-originated delegated job (empty `allowed_paths`,
which `daemon.rs`'s chat-collaboration action and the schedule materializer
both emit) is refused even though the daemon was holding the correct root.

### 19.3 No production path executes a cwd-rooted tool

Sweeping every non-test caller of the registry constructors:

| Site | Status | Effect |
|---|---|---|
| `tool/factory.rs` `build_session_tool_registry` | binds `Some(execution.workspace_root)` | correct |
| `scheduler/tool_program_executor.rs:195` `job_tool_context` | binds `ctx.workspace_root` | correct |
| `scheduler/tool_program_executor.rs:162` `catalog()` | reached only from `validate_runtime`'s manifest resolution | schema only, no execution |
| `agent/worker.rs:1272` | constructed, then refused at `worker.rs:1494` | inert |
| `job_dispatcher.rs` | no callers | dead |

Three sibling executors already bind the scheduler lease root and say so in
their own doc comments: `PythonJobExecutor` ("never fall back to process CWD,
which is wherever the daemon was launched rather than the job's workspace"),
`ToolProgramExecutor::job_tool_context`, and `TestTool`. `SubagentJobExecutor`
was the only one of the family that did not, which is a much sharper
statement of the defect than §18 managed.

### 19.4 Consequence for W5/W6

W5 and W6 are therefore **not** fixing an observed breach; they make the
invariant structural so that the next caller cannot trip it. That is still
worth doing — it is the only way the guard (W7) becomes a real check, and it
removes a landmine whose trigger is a one-line `ToolRegistry::default()` in
some future caller. But the honest justification is defence in depth, and the
closure record states it as such rather than claiming a breach was fixed.

W3 remains the fix for the actual live defect, and it is the change W2's test
demonstrates.
