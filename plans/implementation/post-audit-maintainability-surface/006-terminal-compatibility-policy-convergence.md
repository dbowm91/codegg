# Post-Audit Maintainability and Surface M006 — Terminal Compatibility Policy Convergence

Status: implemented — closed; see `plans/closure/post-audit-maintainability-surface/006-status.md`

Repository baseline: `98bc89fa613f5a1390202a90b497f59d5732d431`

Source roadmap:

- `plans/subsystems/post-audit-maintainability-surface-corrective-addendum.md#7-milestones`

Original closed work corrected by this milestone:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md` M001/M002/M004
- `plans/closure/post-audit-maintainability-surface/001-status.md`
- `plans/closure/post-audit-maintainability-surface/002-status.md`
- `plans/closure/post-audit-maintainability-surface/004-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs: none. Bash/managed-process ownership is already established.

Primary class: invariant / polish

Closure record: `plans/closure/post-audit-maintainability-surface/006-status.md`

## 1. Objective

Make the retained historical `terminal` model tool a genuinely thin compatibility adapter by removing its independent dangerous-command policy/classifier and delegating equivalent shell-content risk decisions to the canonical Bash command policy. Preserve the terminal tool name, historical parameter contract, deferred disclosure, permission/risk compatibility, managed-process execution, output/timeout bounds, and stored-run/import readers.

The milestone must prove safety parity or stricter compatibility behavior; it must not widen execution merely to deduplicate code.

## 2. Why this milestone is ready

The prior maintainability work explicitly classified compatibility adapters as acceptable only when they delegate canonical execution behavior. `terminal` is already documented as overlapping/deferred, while `bash` is canonical. Bash policy was physically decomposed and has a canonical `policy.rs`; managed process execution is already shared. No new architecture is needed.

## 3. Current implementation evidence

Before editing, inventory side by side:

### `src/tool/terminal.rs`

- `BLOCKED_PATTERN` regex and every matched shell construct;
- `blocked_commands` default set and prefix semantics;
- optional allowlist behavior and wrapper-command skipping;
- environment variable name filtering;
- `allowed_root` validation via Bash child-workspace helper;
- command+args concatenation and `sh -c` construction;
- timeout/output line/byte truncation;
- Tool name/category/parameters/deferred loading;
- tests and external construction/customization methods.

### canonical Bash policy/execution

- `src/tool/bash/policy.rs` blocked-pattern classifier and command-intent policy;
- destructive/sensitive path checks and child-workspace validation;
- scheduler/managed-process dispatch path;
- permission/risk classification for `bash` and `terminal` names;
- tests that pin command-policy behavior.

Produce a differential matrix of equivalent commands, noting where terminal's separate `command` + `args` representation changes semantics relative to a Bash script string.

## 4. Invariants that must not regress

- `bash` remains the canonical model shell; `terminal` remains compatibility-only/deferred.
- A command blocked by either existing canonical Bash safety policy or a justified terminal input-contract restriction must not become allowed through convergence without explicit evidence/review.
- `terminal` continues to execute one finite noninteractive command through canonical managed-process ownership; it does not become an interactive PTY.
- Scheduler/managed process, timeout, cancellation/process-tree, output bounds, working-root validation, and environment sanitation remain unchanged.
- Historical `terminal` tool name/serde/transcript/import/permission/agent-deny readers remain compatible.
- No shell command is authorized merely because an alias delegates to Bash policy.
- Custom `with_blocked_commands`/`with_allowlist` public behavior is preserved or explicitly dispositioned with consumer evidence before removal.
- No second parser/security policy is introduced under a different module name.

## 5. Scope

### In scope

- One reusable canonical shell-safety evaluation seam factored from or exposed by Bash policy as needed.
- Migration of `TerminalTool::check_command_security` to that seam.
- Classification of terminal-only input/env/allowlist restrictions as adapter validation rather than general shell-risk authority.
- Removal of duplicate blocked-pattern regex/list logic where no distinct contract remains.
- Differential/table-driven parity tests and negative tests.
- Documentation describing canonical owner and retained compatibility reasons.

### Explicitly out of scope

- Removing/renaming `terminal`.
- Changing interactive terminal/PTY services.
- Reworking Bash command parsing, scheduler routing, or shell execution architecture.
- Generalizing all tool security policies into one framework.
- Changing permission/risk levels solely as cleanup.
- New sandbox technology.

## 6. Required production changes

### Canonical safety seam

Prefer exposing a narrow function/type from the existing Bash policy module that accepts the canonical script text plus the context required to evaluate currently shared safety rules. Avoid making `BashTool` itself the dependency if that would couple execution state to policy.

A possible shape is illustrative only:

```text
ShellSafetyInput { script, allowed_root?, origin/profile? }
ShellSafetyDecision { allow | block(reason) | requires-existing-permission-path }
```

Do not create a generalized policy engine. If existing pure functions such as `find_blocked_pattern` and workspace validators are sufficient, use them directly and keep the seam smaller.

### Terminal adapter

Normalize `command` + `args` into the exact script string that will be passed to `sh -c`, then evaluate canonical shell safety on that same effective content before execution. This avoids checking one representation and executing another.

Retain argument-level validation only where it is intrinsic to the terminal compatibility contract. Environment variable name validation may remain terminal-specific if Bash's environment path is structurally different, but dangerous environment names should reuse a shared environment policy if one already exists.

If configurable terminal allow/block lists have supported callers, compose them as additional restrictions after canonical policy; they may narrow authority but never bypass canonical rejection. If they are unused/dead pre-1.0 APIs, record evidence before any removal rather than silently deleting them.

### Storage/protocol/runtime

No storage/protocol change. Continue using `ManagedProcessService` and existing cwd/output/timeout handling unless the census proves those are also duplicate adapters over a canonical helper and movement is trivial. Do not turn this into a full terminal rewrite.

### Documentation

Update `architecture/tool.md` and relevant shell/permission docs to state that terminal's security decision delegates canonical shell policy and to distinguish model `terminal` from human interactive terminal.

## 7. Ordered work packages

### Work package A — Differential policy census

Build a table of terminal/Bash decisions for benign commands and every terminal-specific blocked regex/list family: substitutions, pipes to shell, redirects, interpreters with `-e/-c`, nohup/background, kill/chmod/chown, download-to-root, fork bomb, wrapper commands, environment variables, allowlists and allowed-root checks.

Acceptance evidence: closure identifies same, stricter, or divergent semantics for each family and explains intended disposition.

### Work package B — Canonical shared safety entrypoint

Expose/reuse the smallest Bash policy functions necessary for compatibility callers. Keep Bash tests green and add no alternate state machine.

Acceptance evidence: one implementation owns blocked-pattern/destructive shell classification.

### Work package C — Terminal adapter migration

Evaluate the final effective shell script through canonical policy, then apply any justified terminal-only narrowing. Delete duplicate regex/static lists/functions that no longer have a distinct contract.

Acceptance evidence: repository search shows no independent `terminal` blocked-pattern authority; terminal execution still uses the same ManagedProcessService path.

### Work package D — Differential regression suite

Create table-driven tests that feed semantically equivalent terminal and Bash inputs through the policy seam. Include UTF-8/quoting/args boundaries and adversarial cases that previously matched only one implementation.

Acceptance evidence: no previously blocked dangerous case becomes executable; documented adapter-only differences are tested.

### Work package E — Compatibility/docs review

Search stored-run readers, imports, agent profiles, permission/risk maps, tool registry/disclosure, examples and tests for literal `terminal`. Confirm the name remains retained/deferred and no migration is needed.

## 8. Failure, cancellation, restart, and contention semantics

Policy rejection occurs before managed-process spawn exactly as before. Execution timeout/cancellation/process-tree behavior remains ManagedProcessService-owned and unchanged. This tool is one-shot; no restart recovery state is introduced. Concurrent tool calls remain scheduler/managed-execution governed according to current path.

A policy-classification error must fail closed. Do not catch a canonical policy error and fall back to the old terminal classifier or direct execution.

## 9. Compatibility and migration

No persisted migration. Preserve terminal name/parameters and supported builder methods unless consumer census proves a private/dead surface and removal is explicitly documented. Stored historical runs only need rendering/read compatibility; they do not require the old policy implementation to survive.

## 10. Required tests

### Focused unit tests

- canonical safety classifier existing tests;
- terminal effective-script construction;
- terminal + Bash differential table;
- custom terminal allow/block list composition if retained;
- env variable restrictions and allowed-root behavior.

### Integration tests

- representative allowed terminal command executes through managed process;
- representative blocked command creates no process side effect;
- permission/risk/disclosure compatibility remains unchanged.

### Restart/recovery tests

Not materially applicable; verify no persistent execution state added.

### Contention/cancellation tests

Run existing managed-process timeout/cancellation coverage touched by refactor; do not create a new stress harness.

### Security/negative tests

Explicitly include command substitution, shell pipe/download, interpreter `-c/-e`, destructive filesystem paths, background/disown, environment injection and quoting/argument concatenation cases from the census.

## 11. Required verification commands

```bash
cargo test -p codegg --lib -- tool::bash
cargo test -p codegg --lib -- tool::terminal
cargo test --test tool_execution
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Use actual existing guard names/targets at implementation time. No new CI lane.

## 12. Documentation updates

- `architecture/tool.md`: canonical shell policy and terminal compatibility adapter.
- shell/managed-process architecture docs if they currently imply independent terminal policy.
- permission/tool-surface docs only if source references are stale.
- source comments in `terminal.rs` and Bash policy.

## 13. Acceptance criteria

M006 closes when equivalent Bash/terminal shell content is governed by one canonical destructive/blocked policy implementation; terminal retains only justified compatibility/input narrowing; no previously blocked dangerous case is widened; the historical tool remains discoverable only according to its existing deferred policy; and execution/permission/storage contracts are unchanged.

## 14. Stop conditions

Stop if:

- differential testing reveals a material intentional policy difference whose correct canonical owner is ambiguous;
- convergence would require changing Bash execution semantics or permission levels;
- removing terminal builder customization would break demonstrated supported consumers without a migration decision;
- a generic security-policy framework is proposed;
- current HEAD has already made terminal a thin delegate and no duplicate classifier remains.

## 15. Closure evidence required

Include implementation commits, full differential policy matrix, retained terminal-only restrictions with rationale, deleted duplicate classifier inventory, safety parity/negative test outcomes, execution-owner guard results, tool-name/disclosure/permission compatibility review, focused/broad verification run, and severity-classified residual findings.

## 16. Handoff notes

Treat canonicalization as monotonic: shared policy plus optional terminal-specific narrowing. The fastest safe implementation is likely to reuse Bash policy functions around the exact script that terminal will execute, not to route terminal through the entire `BashTool` object.
