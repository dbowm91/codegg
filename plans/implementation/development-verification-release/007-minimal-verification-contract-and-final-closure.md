# Development Verification and Release Milestone 007 — Minimal Verification Contract and Final Closure

Status: closing — implementation and focused evidence complete; shared hosted evidence pending

Repository baseline reviewed:

- current `main`: `8f86dd2a13fd0418ca850d1a84548e5f78b76a6a`
- Provider M007 conditional review: `plans/closure/provider-connections/007-status.md`
- Tool Programs M019 review plan: `plans/implementation/tool-programs/019-independent-strict-closure-and-evidence-ratification.md`
- Agent Runtime M017 review plan: `plans/implementation/agent-runtime-model-adaptation-acp/017-corrective-integration-evidence-and-closure.md`
- prior DVR closure plan: `plans/implementation/development-verification-release/006-final-evidence-and-release-documentation-closure.md`

Target independent closure record:

- `plans/closure/development-verification-release/007-status.md`

Primary class: verification-contract correction and final evidence closure

Secondary class: cross-subsystem evidence deduplication

## 1. Objective

Finish the current Provider, Tool Programs, Agent Runtime, and Development Verification closure work with the smallest verification surface that can establish correctness.

M007 exists because the active plans accumulated overlapping command matrices, repeated full-workspace runs, package-by-package release checks, and subsystem-specific hosted evidence requirements. At this stage those requirements create more execution and bookkeeping risk than confidence.

M007 must establish one proportional verification contract:

1. correct the fail-open `codegg-core` boundary guard without adding a dependency, job, framework, or new general-purpose checker;
2. use focused tests only for mechanisms changed or questioned by the active closure milestone;
3. run `scripts/verify.sh quick` once on the final accepted executable revision;
4. obtain one successful existing GitHub Actions `verify` job for that same revision;
5. allow that single hosted result to satisfy Provider M007, Tool Programs M019, Agent Runtime M017, and DVR M007 when each reviewer confirms the relevant executable tree is present in the checkout;
6. avoid a second local full-workspace run when the hosted job already performs formatting, workspace check, Clippy, and workspace tests;
7. remove package inventory regeneration, per-crate packaging, crates.io probing, and publication dry-runs from closure requirements unless a release is actually being prepared;
8. preserve independent review ownership without requiring each reviewer to rerun the same commands.

This is a narrowing and correctness pass. It must not become another CI redesign or validation framework.

## 2. Authoritative supersession

This plan supersedes only the verification breadth and evidence-collection sections of:

- Agent Runtime M017;
- Tool Programs M019;
- Development Verification and Release M006;
- the hosted-evidence unblock language in Provider M007.

The production invariants, security boundaries, ownership requirements, and independent-review requirements in those plans remain binding.

Where an older plan requires:

- all predecessor test commands;
- repeated adjacent suites;
- a local full run plus an equivalent hosted run;
- separate hosted runs for each closure record;
- package-by-package `cargo package` or `cargo publish --dry-run` evidence;
- release-registry availability or propagation checks;

this M007 contract takes precedence.

DVR M006 remains historical implementation evidence. M007 becomes the final Development Verification and Release closure authority.

## 3. Minimal verification principles

### 3.1 Verify changed mechanisms, not milestone history

A closure reviewer must inspect predecessor closure records and production call sites, but must not replay every predecessor command.

Focused execution is required only when:

- the current accepted revision changed the mechanism;
- the closure question specifically concerns that mechanism;
- prior evidence is tied to an executable tree that no longer matches;
- a current failure requires minimal reproduction.

Documentation-only drift does not invalidate executable evidence.

### 3.2 One broad run is enough

The existing hosted `verify` job already runs:

- generated-agent checks;
- Tokio guard checks;
- the core-boundary guard;
- formatting;
- workspace check;
- workspace Clippy with warnings denied;
- workspace tests with one test thread.

A successful hosted run on the accepted SHA is the sole broad verification result required by this closure series.

Do not additionally require `scripts/verify.sh full` on the same tree unless:

- hosted CI cannot run;
- the hosted failure is environment-specific and local reproduction is necessary;
- a reviewer identifies a concrete gap between the hosted command and the mechanism under review.

### 3.3 Evidence may be shared

The same accepted SHA and hosted run may be cited by multiple closure records. Separate subsystem closure does not require duplicate execution.

Each reviewer must independently inspect the relevant production diff and focused result, but may reuse:

- the same `scripts/verify.sh quick` result;
- the same hosted `verify` run;
- the same formatting/check/Clippy/workspace-test result;
- earlier focused evidence when executable tree identity is demonstrated.

### 3.4 Release checks are release-time work

The repository is not performing a release in this closure pass. Therefore:

- do not query crates.io ownership or name availability;
- do not run every package through `cargo package`;
- do not run `cargo publish --dry-run` across the workspace;
- do not test index propagation;
- do not build a new package inventory artifact;
- do not add publication automation.

`RELEASING.md` may receive a static correctness review only when it is modified. Actual package and registry checks remain a manual pre-release responsibility.

## 4. Work package A — Correct the fail-open core-boundary guard

The current script masks both `rg` command failures with `|| true`. In hosted CI, missing `rg` can therefore produce an empty result followed by a false pass.

Apply the smallest correction to `scripts/check-core-boundary.sh`:

1. do not treat command failure as equivalent to no match;
2. do not add a CI installation step for ripgrep;
3. use tools already guaranteed by the runner, such as POSIX/GNU `grep`, or implement a small explicit fallback inside the existing script;
4. distinguish:
   - no forbidden match: success;
   - forbidden match: failure with matched lines;
   - matcher/runtime error: failure with a clear diagnostic;
5. keep the existing two checks and existing ownership boundary;
6. do not generalize this into a repository-wide static-analysis framework.

Required evidence is limited to:

```bash
./scripts/check-core-boundary.sh
```

plus one temporary negative fixture or equivalent shell-level invocation proving that a forbidden import produces nonzero status.

No permanent test harness is required unless the existing script cannot be tested directly.

## 5. Work package B — Tool Programs M019 minimal review

M019 remains an independent review-only milestone. Its minimal executable evidence is:

```bash
cargo test --test tool_program_runtime -- --test-threads=1
cargo test --test tool_program_runtime -- --test-threads=1
cargo test --test tool_program_m014_authority_pipeline -- --test-threads=1
```

The runtime target is intentionally run twice because repeated-run isolation is the specific unresolved question. This is the only repeated test requirement in this closure series.

The reviewer must also inspect:

- the M018 executable diff;
- `ProgramStore` ownership;
- source-store ownership;
- whether any persistent terminal-result path is used by the target;
- zero-call emit/cancellation behavior;
- frozen-contract and authority consistency.

Do not rerun every adjacent Tool Programs integration target. Run an additional target only when inspection identifies a concrete untested finding.

If the three commands pass and no high/medium finding remains, the independent reviewer may create `plans/closure/tool-programs/019-status.md` using the shared quick/hosted evidence from M007.

## 6. Work package C — Agent Runtime M017 minimal review

M017 remains an independent production-path audit, but its test budget is capped at one representative focused target per corrected domain.

Default focused set:

```bash
cargo test --test acp_stdio -- --test-threads=1
cargo test --test agent_loop_harness -- --test-threads=1
cargo test --test context_plan_convergence -- --test-threads=1
cargo test --test provider_transcripts -- --test-threads=1
cargo test --test subagent -- --test-threads=1
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_execution_ownership.py
```

These cover:

- ACP lifecycle and protocol purity;
- specialized runtime/finalizer integration;
- prompt/context/cache convergence;
- adapter-driven reasoning and alias/privacy behavior;
- descendant admission, cancellation, and workspace ownership;
- static process-cwd and execution-authority boundaries.

The reviewer must still trace representative production call sites. It must not rerun every command from M012–M016 closure records.

A missing or renamed target may be replaced with the nearest current target covering the same mechanism. Do not add a new test solely to preserve an obsolete command name.

If these focused checks pass and no high/medium production finding remains, the independent reviewer may create `plans/closure/agent-runtime-model-adaptation-acp/017-status.md` using the shared quick/hosted evidence from M007.

## 7. Work package D — One final local and hosted verification pass

After the boundary-guard correction and any narrowly required in-scope fix:

```bash
scripts/verify.sh quick
```

Run this once on the final accepted executable SHA.

Then obtain one successful existing GitHub Actions `verify` job for that exact SHA or a planning-only descendant with executable-tree identity demonstrated.

Do not:

- add another workflow or job;
- add matrices, artifacts, schedules, retries, or release steps;
- increase test concurrency or create a second broad test script;
- rerun hosted CI separately for each subsystem;
- require a local `full` run merely to duplicate the hosted commands.

The hosted run must fail if the boundary matcher is unavailable or errors. A printed success after matcher failure is not acceptable evidence.

## 8. Work package E — Closure sequence

After one green shared hosted run:

1. Provider M007 reviewer confirms provider/storage executable identity and upgrades `plans/closure/provider-connections/007-status.md` from conditional to strict closure without rerunning provider tests unless provider/storage code changed.
2. Tool Programs M019 reviewer completes the independent review and creates `plans/closure/tool-programs/019-status.md`.
3. Agent Runtime M017 reviewer completes its independent integration review and creates `plans/closure/agent-runtime-model-adaptation-acp/017-status.md`.
4. DVR M007 reviewer inspects the boundary-guard correction, minimal evidence contract, shared run, and closure records.
5. DVR M007 reviewer creates `plans/closure/development-verification-release/007-status.md` and marks DVR closed.

Steps 1–3 may proceed in parallel after the shared hosted run. Their reviewers may use the same evidence and must not require duplicate broad execution.

The implementation agent for the boundary guard may move M007 to `closing` but must not author the independent DVR M007 closure record.

## 9. Acceptance criteria

M007 is complete only when:

- the core-boundary script cannot pass when its matcher is missing or errors;
- the existing boundary checks still detect forbidden imports and dependencies;
- no new CI job, dependency installation, validation framework, or general-purpose scanner is added;
- Tool Programs runtime passes twice and its authority-pipeline target passes once;
- Agent Runtime uses no more than the representative focused set unless a concrete finding justifies one additional target;
- `scripts/verify.sh quick` passes once on the accepted executable SHA;
- one existing hosted `verify` job passes on that SHA;
- no local full-workspace run is required when the hosted job is green;
- no package-by-package or registry/release verification is performed;
- Provider M007, Tool Programs M019, and Agent Runtime M017 use the shared broad evidence rather than duplicating it;
- the registry identifies M007 as the final DVR closure owner;
- no unresolved high/medium correctness finding remains in the reviewed scopes.

## 10. Explicit non-goals

M007 must not:

- redesign CI;
- expand routine verification;
- add coverage targets simply because historical plans listed them;
- require live providers, editors, scanners, registries, or external services;
- clean up unrelated warnings or dead code solely for aesthetics;
- narrow or remove existing product tests;
- modify production Provider, Tool Programs, ACP, agent, scheduler, or release behavior unless a focused review demonstrates a real in-scope defect;
- add release automation or perform a release;
- create additional follow-up plans for low-severity evidence preferences.

## 11. Stop conditions

Stop and register one narrow corrective implementation plan only when:

- a focused test exposes a reproducible high/medium production defect;
- the hosted job fails after the boundary guard correction and the failure is not a simple in-scope verification-script issue;
- Tool Programs repeated-run execution demonstrates actual persistent replay contamination;
- Agent Runtime production tracing finds a second authority path, privacy leak, cross-lineage cancellation, or workspace escape.

Do not create a follow-up for:

- a desire for broader coverage;
- an optional external integration;
- a release-time package check;
- a low-severity documentation preference;
- lack of a second duplicate full run.

## 12. Required closure record contents

The M007 closure record should remain compact and include only:

- accepted SHA;
- boundary-guard change and negative proof;
- focused Tool Programs results;
- focused Agent Runtime results;
- one quick result;
- one hosted run ID/job ID and conclusion;
- linked Provider M007, Tool M019, and Agent M017 dispositions;
- unresolved findings by severity;
- confirmation that no CI/release/test framework expansion occurred.
