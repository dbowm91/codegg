# Coding-Agent Tool Surface Corrective M007 — M003/M004 Closure Evidence Reconciliation

Status: closing

Repository baseline: `5447e3bc62114cf91992690e26f3a09b897802c5`

Source roadmap:

- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md#m007--m003m004-closure-evidence-reconciliation`

Predecessor implementation/closure records:

- `plans/implementation/coding-agent-tool-surface-corrective/003-compact-discovery-and-multiplexed-tool-ergonomics.md`
- `plans/closure/coding-agent-tool-surface-corrective/003-status.md`
- `plans/implementation/coding-agent-tool-surface-corrective/004-structured-verification-facade.md`
- `plans/closure/coding-agent-tool-surface-corrective/004-status.md`

Long-term requirements:

- `plans/003-planning-process.md`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`

Applicable ADRs: none.

Primary class: invariant

## 1. Objective

Restore strict planning truth for M003 and M004 by executing and recording the verification commands their implementation plans required but their closure records did not document.

This is an evidence-reconciliation milestone, not a production refactor. It must not retroactively rewrite the historical M003/M004 closure records to imply commands were run at their original implementation baselines when they were not recorded.

## 2. Why this milestone is ready

Both production implementations landed and have focused closure evidence, but the planning contract was stricter than the recorded closures.

M003's implementation plan required:

- `cargo test --test agent_run_tool`;
- workspace all-features Clippy with `-D warnings`;
- `scripts/verify.sh quick`;

in addition to the focused tests and formatting checks already recorded. Those three required checks are absent from `003-status.md`.

M004's implementation plan required:

- workspace all-features Clippy with `-D warnings`;
- `scripts/verify.sh quick`;

in addition to the focused tests/static guards already recorded. Those two required checks are absent from `004-status.md`.

The registry also continued to list M003, M004, and M005 as dependency-ready after M003/M004 had closed and M005 had been blocked. That registry defect is corrected when this plan is registered; M007 owns only the remaining evidence qualification.

## 3. Current implementation evidence

### M003

Recorded closure evidence already includes:

- tool-search unit tests;
- LSP/Git/Task unit tests;
- tool-surface minimization;
- Tool Program Git/LSP palette;
- formatting;
- `git diff --check`;
- schema-size and authority/discovery evidence.

Missing from the recorded required command list:

- `cargo test --test agent_run_tool`;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
- `scripts/verify.sh quick`.

### M004

Recorded closure evidence already includes:

- focused `verify` tests;
- command-intent/Bash/TestTool tests;
- adversarial routing;
- tool execution and minimization;
- execution-ownership and scheduler-bypass guards;
- formatting;
- `git diff --check`.

Missing from the recorded required command list:

- `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
- `scripts/verify.sh quick`.

## 4. Invariants that must not regress

- Historical closure records remain historical evidence; do not fabricate original-baseline command results.
- Strict closure means every required command is either run and recorded or replaced by a documented current equivalent.
- A failing qualification command is a product/test defect until explained; do not mark the milestone strictly closed around it.
- M007 must not weaken M003/M004 acceptance criteria.
- Production fixes discovered by qualification require a new bounded corrective implementation plan unless the issue is strictly a stale test/command name with an obvious equivalent.
- No new CI lane is added solely for this evidence pass.

## 5. Scope

### In scope

- Run the union of the M003 and M004 required verification suites at one explicit current repository baseline.
- Record exact commands, results, test counts where available, and local-vs-hosted provenance.
- Resolve only stale command/target names when an unambiguous current equivalent exists.
- Produce `plans/closure/coding-agent-tool-surface-corrective/007-status.md`.
- On full pass, restore M003 and M004 from conditionally closed to strict closed in the roadmap/registry and reference M007 as supplemental qualification.
- On failure, leave the affected milestone conditionally closed and register a separate corrective implementation plan for the defect.

### Explicitly out of scope

- Reimplementing M003 discovery/Git facade behavior.
- Reimplementing M004 verification behavior.
- Editing old closure records to insert unrecorded historical commands.
- Treating a failing product test as an evidence-only issue.
- Broad cleanup unrelated to a qualification failure.
- Adding hosted CI requirements not present in the original plans.

## 6. Required production changes

None expected.

If a required command fails due to a real production defect, stop this evidence pass after capturing reproducible evidence and create a new corrective implementation plan. Do not repair unrelated production behavior inside M007.

If a failure is only a stale test-target/command name, identify the current canonical equivalent and record the substitution in the M007 closure.

## 7. Ordered work packages

### Work package A — Freeze qualification baseline

Record the exact HEAD and confirm no uncommitted production changes are being mixed into the evidence pass.

Acceptance evidence: closure names one baseline commit.

### Work package B — M003 exact qualification

Run every command required by M003, including the previously unrecorded `agent_run_tool`, Clippy, and quick verification.

Acceptance evidence: command-by-command result table.

### Work package C — M004 exact qualification

Run every command required by M004, including focused verify tests, static guards, Clippy, and quick verification.

Acceptance evidence: command-by-command result table.

### Work package D — Failure classification

For each failure, classify it as:

- real production regression/defect;
- stale test target with a current equivalent;
- environment/operational evidence issue.

Acceptance evidence: no unexplained red command is ignored.

### Work package E — Planning reconciliation

If all required evidence is green, write M007 closure and restore strict M003/M004 closure status in the roadmap/registry.

If not, retain conditionally-closed status and register the smallest corrective plan that owns the failure.

## 8. Failure, cancellation, restart, and contention semantics

This plan does not introduce runtime behavior.

Verification should use repository-standard serialization/threads where existing plans specify them. If a test is flaky, rerunning until green is not sufficient evidence: capture the failure and determine whether the repository already documents a bounded retry/quarantine policy.

## 9. Compatibility and migration

No compatibility or migration change is expected.

Any qualification-discovered production fix belongs in a separate corrective plan with its own compatibility review.

## 10. Required tests

Run the union of the predecessor plans' required tests, at minimum:

```bash
cargo test -p codegg --lib tool::tool_search
cargo test -p codegg --lib tool::lsp
cargo test -p codegg --lib tool::git
cargo test -p codegg --lib tool::task
cargo test --test tool_surface_minimization -- --test-threads=1
cargo test --test tool_program_git_lsp_palette -- --test-threads=1
cargo test --test agent_run_tool -- --test-threads=1

cargo test -p codegg --lib tool::verify
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib tool::test
cargo test --test command_routing_adversarial -- --test-threads=1
cargo test --test tool_execution -- --test-threads=1

python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
```

Use current exact target names if they have moved and record any substitution.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Do not infer Clippy or quick-verification success from narrower test passes.

## 12. Documentation updates

Expected planning-only updates:

- M007 closure record;
- coding-agent tool-surface roadmap status;
- `plans/registry.md`.

Architecture docs should change only if qualification exposes a factual documentation defect.

## 13. Acceptance criteria

M007 closes when:

1. every M003 and M004 required command has an explicit current-baseline result or an explicitly justified current equivalent;
2. all required commands are green, or any non-green result has been converted into a separately registered corrective plan while the affected predecessor remains conditionally closed;
3. no historical closure record is rewritten to claim unrecorded historical evidence;
4. roadmap and registry statuses match the actual evidence state;
5. no production change is hidden inside the evidence pass.

## 14. Stop conditions

Stop and register a corrective implementation plan if:

- Clippy or quick verification finds a production defect;
- `agent_run_tool` or another required test fails for product behavior;
- satisfying a missing command requires weakening tests or flags;
- the current repository no longer has an unambiguous equivalent for a required target;
- hosted/external evidence becomes necessary for strict closure.

## 15. Closure evidence required

Include:

- qualification baseline SHA;
- complete command/result matrix;
- explicit list of the previously missing M003/M004 commands;
- substitutions, if any;
- failure classification, if any;
- roadmap/registry disposition;
- statement that historical predecessor closure records were not rewritten.

## 16. Handoff notes

The production work may already be correct. The defect here is that strict closure outran its recorded evidence. Treat this as a truth-maintenance pass: either prove the existing implementations under the promised verification contract or register a real corrective if the proof fails.
