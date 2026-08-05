# Runtime Safety, Resource Control, and Footprint Roadmap

Status: active

Current disposition: C001 is conditionally closed for production correctness pending one supported-Linux enforcement result; M001 and M002 retain linked conditional dispositions; M004 closed; M005/M006 hosted conditions are operational; M003 is closing and M007–M008 remain blocked.

Repository baseline reviewed: `4d540ce315c9ef2a1c07544cd42df0efc43708e1`

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`
- `architecture/scheduler.md`
- `architecture/jobs.md`
- `architecture/testing.md`

Related ADRs:

- None required for the initial correctness work. The existing daemon-owned execution and scheduler authority remain unchanged.
- Milestone 007 must create an ADR only if measurement supports a binary-boundary change that alters public invocation, installation layout, IPC compatibility, or ownership. A crate-level refactor or additional binary target that preserves those contracts does not by itself require an ADR.

## 1. Purpose and ownership boundary

This workstream corrects concrete runtime-safety and resource-control defects found during the post-closure repository audit, then reduces dependency and binary overhead without removing product capability.

It owns:

- Linux sandbox implementation and truthful sandbox-policy reporting;
- process spawning, bounded stdout/stderr capture, timeout, cancellation, and descendant cleanup;
- preservation of typed argument vectors through tool planning and execution;
- grep worker admission, blocking-task bounds, and context extraction efficiency;
- dependency feature selection and avoidable duplicate runtime stacks;
- replacement or containment of deprecated configuration parsers;
- measurement-led executable topology and binary-footprint reduction;
- compact planning, verification, and maintenance documentation for this workstream.

It consumes, but does not redefine:

- scheduler and daemon execution authority;
- tool policy and approval semantics;
- Tool Programs language and authority boundaries;
- provider, ACP, session, and projection contracts;
- manual release cadence and crates.io ownership;
- existing public CLI, daemon protocol, and configuration compatibility unless a plan explicitly defines a compatible migration.

The governing rule is:

> Every subprocess path must use one bounded and cancellable execution contract; every advertised sandbox must enforce the policy it reports; and footprint work must be justified by measurement rather than feature deletion.

## 2. Work classification

### Invariants

- The daemon and scheduler remain the authoritative owners of durable execution.
- Child process authority cannot exceed the originating tool or job authority.
- Explicit sandbox requests must not silently degrade to unsandboxed execution.
- Automatic or best-effort sandbox modes must report the enforcement actually obtained.
- Process output retained in memory must be bounded before accumulation, not truncated only after unbounded collection.
- Timeout, cancellation, and owner shutdown must terminate the entire managed process group or job tree where the platform supports it.
- Native execution must preserve an already parsed argument vector; it must not reconstruct argv with whitespace splitting.
- Blocking grep work must be admitted under a real concurrency bound and must not repeatedly reread an entire file for each match context.
- Dependency reduction must preserve user-visible behavior and supported feature gates.
- Routine verification remains proportional. This workstream must not restore broad matrices, release automation, evidence artifacts, or duplicated full-workspace runs.

### Capabilities

- Linux users receive a functional, maintainable filesystem sandbox when the host kernel supports it.
- Tool and Python execution remain responsive under large or adversarial output.
- Cancellation and timeout do not leave common descendant processes running.
- Quoted arguments and paths containing spaces reach native commands intact.
- Repository search remains bounded and predictable on large trees and files.
- Default installations avoid unnecessary TLS, image, executor, parser, and database feature weight.
- Maintainers can measure which executable components account for binary size and decide whether a daemon/TUI split is warranted.

### Infrastructure

- A maintained Landlock integration or a small audited helper with explicit availability and enforcement states.
- One canonical managed-process request/result contract used by shell, native, Python, scheduler, and compatible helper paths.
- Bounded streaming collectors for stdout and stderr.
- Typed command/argv plumbing through routing and execution.
- Bounded grep batching and single-pass context extraction.
- Explicit Cargo dependency features and feature ownership.
- Repeatable release-build size reports used only during the footprint milestone and release preparation.

### Polish

- Removal of obsolete custom syscall code, duplicate dependency declarations, dead routing branches, and stale documentation.
- Consistent diagnostics for sandbox fallback, output truncation, cancellation, and process-tree cleanup.
- Compact registry entries and closure records.
- Reconciled test-count and verification documentation.

## 3. Explicit non-goals

This roadmap must not:

- redesign the scheduler, daemon, Tool Programs runtime, ACP, or provider architecture;
- introduce containers, virtual machines, seccomp policy generation, eBPF enforcement, or a new cross-platform sandbox framework;
- promise strong sandboxing on platforms where CodeGG only provides portable process restrictions;
- turn raw shell execution into a transparent parser or attempt complete shell-language analysis;
- replace grep with a new indexing service or persistent search database;
- remove supported configuration formats without a compatibility window;
- remove plugins, LSP support, clipboard text support, server support, image support, or other documented features solely to reduce size;
- require Wasmtime in the default build;
- automate releases or change release cadence;
- add broad benchmark infrastructure, nightly CI, coverage gates, mutation testing, target matrices, or package-publication checks to routine CI;
- create repeated closure plans for low-severity documentation preferences.

## 4. Current-state summary

At the reviewed baseline:

- `src/security/sandbox.rs` contains a handwritten Landlock syscall implementation with hard-coded syscall numbers and access-right bit handling. Availability detection does not establish that enforcement can be applied, the implementation does not correctly establish all required process preconditions, rule-add failures may be tolerated, and explicit requests can degrade without a hard failure.
- `src/python_script/executor.rs` applies sandbox setup through a `pre_exec` closure that performs nontrivial Rust work after fork, reports modes that do not always match the requested Python behavior, and captures process output through an unbounded `output()` call.
- `src/tool/bash.rs` has raw-shell and native subprocess paths that collect all output before truncation. Native dispatch reparses commands with `split_whitespace()`, corrupting quoted or escaped arguments. These paths overlap the capabilities already present in `src/managed_process.rs`.
- `src/managed_process.rs` already provides a stronger execution foundation, including bounded output concepts and Unix process-session handling, but it is not the universal subprocess boundary.
- `src/tool/grep.rs` acquires a semaphore permit before queuing blocking work but drops the permit before the blocking task executes. Large searches can therefore enqueue substantially more blocking work than intended. Context extraction rereads a file repeatedly for matches in that file.
- workspace Cargo manifests enable avoidable default features or duplicate capability stacks, including a default-TLS path alongside explicit rustls in one crate, SQLx defaults beyond the required runtime/database surface, image-enabled clipboard defaults for text-only use, umbrella futures and grep crates where narrower crates are sufficient, and an MD5 dependency for a non-security namespace where SHA-256 is already available.
- YAML parsing depends on the deprecated `serde_yaml` 0.9 line and is used across configuration, agent, command, and skill ingestion paths.
- the root executable combines daemon and TUI dependency graphs. Optional large features are generally gated, but there is no current measured decision on whether separate binary targets would materially reduce ordinary installation footprint.
- routine CI has already been contracted to one bounded job and release cadence is manual. Remaining overengineering is concentrated in historical planning/closure bookkeeping and inconsistent testing documentation, not in a large active workflow matrix.

These findings establish the implementation order below. They do not reopen the previously closed agent, provider, Tool Programs, or verification roadmaps except where a concrete shared process path must be corrected.

## 5. Target architecture

### 5.1 Sandbox contract

Sandbox selection has three explicit outcomes:

```text
Enforced(policy, backend, abi)
Unavailable(reason, documented fallback)
Failed(reason)
```

An explicit security requirement must return `Failed` when enforcement cannot be established. A best-effort mode may continue only through a named fallback and must expose that fallback in the execution result and logs.

Linux Landlock support is implemented through a maintained crate or a narrowly audited helper. CodeGG does not keep a second handwritten syscall ABI unless the maintained implementation cannot satisfy a documented requirement and an ADR accepts the maintenance burden.

Child setup performs only operations safe for the platform spawn boundary. Policy construction, path canonicalization, logging, and allocation occur before spawn/pre-exec. The final child hook applies a prebuilt policy with minimal system calls.

### 5.2 Canonical managed process boundary

All subprocess consumers translate into one typed request containing at least:

- executable;
- argv;
- working directory;
- environment policy;
- stdin policy;
- timeout and cancellation owner;
- output byte limits and truncation policy;
- process-tree cleanup policy;
- sandbox request and obtained enforcement report;
- execution provenance/job identity where required.

Output is read incrementally. Once a configured limit is reached, CodeGG follows one documented policy: retain a bounded prefix/tail or terminate the child when continued execution has no value. It must never continue appending to an unbounded `Vec` merely to truncate later.

The raw shell remains an explicit high-risk route, but its shell process is still launched and supervised through the same managed-process machinery.

### 5.3 Search execution

Grep search planning batches files deterministically and admits each blocking batch only while a permit is held for the entire blocking operation. Results remain bounded and cancellation-aware. Files with multiple matches are decoded or line-indexed once per search batch so context windows can be produced without repeated whole-file reads.

### 5.4 Dependency and binary topology

Cargo manifests select only required features. Duplicate TLS and runtime stacks are removed. Text-only clipboard use disables image defaults. Narrow futures and grep crates replace umbrella dependencies when source use permits. Namespace hashing uses an existing SHA-256 implementation with a domain separator.

Binary topology is measurement-led:

1. record current default and representative feature build sizes;
2. identify dominant crates and duplicated feature activation;
3. apply dependency-level reductions first;
4. evaluate separate daemon and TUI binary targets only after those reductions;
5. implement a split only when it produces a material reduction without duplicating business logic or changing the single-daemon ownership model.

The roadmap does not prescribe an arbitrary percentage. Milestone 007 defines the decision threshold and requires the measured result to be recorded.

## 6. Dependency graph

```text
M001 Landlock and sandbox contract correction
    |
    v
M002 Canonical bounded process execution
    |
    v
M003 Typed argv and shell-routing convergence

M004 Grep resource correctness  -------------------+
                                                    |
M005 Dependency feature normalization -------------+--> M007 Binary topology and footprint
    |                                               |
    v                                               |
M006 Deprecated parser and dependency maintenance -+

M001-M007 --> M008 Planning, verification, and maintenance closure
```

Dependency classes:

- M001 has no hard predecessor. It is the highest-priority security/correctness plan.
- M002 has a hard dependency on M001 because the canonical process request/result must carry the corrected sandbox contract.
- M003 has a hard dependency on M002 because argv preservation must terminate at the canonical executor rather than another ad hoc spawn path.
- M004 has no hard dependency and may proceed in parallel with M001.
- M005 has no hard dependency and may proceed in parallel with M001; M004 is already closed.
- M006 has a soft dependency on M005 so dependency cleanup is not repeated, but it may begin after M005's manifest ownership decisions are stable.
- M007 has hard dependencies on M002, M003, M005, and M006. It has a soft dependency on M004 because final representative build measurements should include the completed source cleanup.
- M008 has hard dependencies on M001–M007. It is a compact reconciliation pass, not a new verification program.

## 7. Ordered milestones

### Milestone 001 — Maintained Landlock enforcement and sandbox-contract correction

Class: invariant and security correctness

Implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/001-landlock-and-sandbox-contract-correction.md`

Objective:

Replace the unreliable handwritten Landlock path, make enforcement/fallback states explicit, remove unsafe child-setup behavior, and verify read-only versus writable sandbox semantics with focused host-capability-aware tests.

Exit conditions:

- no hard-coded Landlock syscall table remains in CodeGG production code unless justified by an accepted ADR;
- explicit sandbox mode fails when enforcement cannot be established;
- best-effort fallback is truthful and observable;
- rule-construction failures cannot yield a partial policy reported as enforced;
- Python transform/read-only modes request the intended policy;
- focused Linux tests skip only for a recorded unsupported-kernel reason.

### Milestone 002 — Canonical bounded process execution and process-tree cleanup

Class: invariant and infrastructure

Implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/002-canonical-bounded-process-execution.md`

Objective:

Make `ManagedProcessService` or its extracted core the single subprocess supervision path for shell, native, Python, scheduler, and compatible helper execution, with pre-accumulation output bounds and descendant cleanup.

Exit conditions:

- reviewed production subprocess paths use the canonical executor or have an explicit documented exemption;
- stdout/stderr limits are applied during streaming collection;
- timeout and cancellation clean up process descendants on supported Unix hosts;
- result metadata distinguishes timeout, cancellation, output truncation, spawn failure, and sandbox failure;
- ownership guards prevent reintroduction of unbounded `Command::output()` in governed paths.

### Milestone 003 — Typed argv preservation and shell-routing convergence

Status: closing

Class: invariant and correctness

Implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/003-typed-argv-and-shell-routing-convergence.md`

Objective:

Preserve parsed argument vectors through command planning and native dispatch, reduce overlapping BashTool routing authority, and keep raw shell an explicit route rather than an accidental fallback.

Exit conditions:

- quoted arguments, empty arguments, escaped spaces, and non-UTF-8-compatible platform handling follow the typed contract;
- native dispatch does not use `split_whitespace()` or equivalent reparsing;
- the policy/risk decision is made once and carried into execution;
- raw shell remains available only through an explicit high-risk execution mode;
- focused tests cover representative argv and fallback cases.

### Milestone 004 — Grep concurrency and context-extraction correctness

Class: infrastructure and performance correctness

Implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/004-grep-concurrency-and-context-efficiency.md`

Objective:

Enforce real blocking-work bounds, avoid repeated file reads for context, preserve cancellation, and keep result ordering and limits deterministic.

Exit conditions:

- semaphore permits cover the complete blocking operation;
- large searches cannot enqueue an unbounded number of blocking tasks;
- a file is not reread from disk for every match context;
- search result and context byte limits remain explicit;
- focused stress tests demonstrate bounded concurrency without introducing long-running benchmarks.

### Milestone 005 — Dependency feature normalization and namespace cleanup

Status: conditionally closed

Class: infrastructure and footprint

Implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/005-dependency-feature-and-namespace-normalization.md`

Objective:

Remove avoidable default features, duplicate capability stacks, umbrella dependencies, and the MD5 namespace dependency while preserving default behavior and optional feature gates.

Exit conditions:

- reqwest/TLS feature ownership is explicit across workspace crates;
- SQLx features are exact and defaults are disabled where compatible;
- clipboard remains text-capable without image-data defaults unless image clipboard behavior is actually required and tested;
- futures and grep dependencies are narrowed where source use permits;
- namespace hashing uses existing SHA-256 with a domain separator and compatibility is addressed;
- `cargo tree -e features` demonstrates the intended reductions.

### Milestone 006 — Deprecated parser and dependency maintenance

Status: conditionally closed

Class: compatibility and maintenance

Implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/006-deprecated-parser-and-dependency-maintenance.md`

Objective:

Remove or contain deprecated YAML parsing, preserve supported configuration and skill inputs, and establish a small dependency-maintenance contract without continuous update automation.

Exit conditions:

- no active write path emits a format through deprecated `serde_yaml` APIs;
- existing YAML inputs either remain readable through a maintained parser or are handled by a documented compatibility importer;
- parser behavior has fixture coverage for currently supported structures and diagnostics;
- dependency maintenance remains a periodic/manual maintainer action, not a new CI lane.

### Milestone 007 — Measurement-led binary topology and footprint reduction

Class: polish and infrastructure

Implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/007-binary-topology-and-footprint-reduction.md`

Objective:

Measure release binary composition after dependency cleanup, apply safe source/build reductions, and implement a daemon/TUI binary separation only when it materially improves ordinary deployment without changing single-daemon ownership or feature availability.

Exit conditions:

- baseline and post-cleanup measurements are recorded with exact build features and target;
- dominant contributors are identified with `cargo bloat`, Cargo feature trees, or equivalent local tools;
- no feature is removed solely for size;
- a split decision is recorded with quantitative evidence;
- if implemented, shared business logic remains in libraries, IPC compatibility remains stable, installation and invocation are documented, and aggregate duplication is considered;
- if not implemented, the plan closes with a measured no-split decision rather than forcing architecture churn.

### Milestone 008 — Planning, verification, and maintenance closure

Class: polish and governance

Implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/008-planning-verification-and-maintenance-closure.md`

Objective:

Reconcile documentation, static guards, CI invocation, and active planning state after the production milestones without expanding verification or creating another evidence-heavy closure chain.

Exit conditions:

- `plans/registry.md` contains only active/recent control-surface information;
- each milestone has one compact closure record with exact commands and residual findings;
- testing documentation uses one consistent test-count source or avoids fragile counts;
- routine CI remains one bounded job and does not gain release, matrix, audit, artifact, or benchmark work;
- redundant compile work is removed only when the resulting command contract is still clear;
- manual release ownership remains unchanged.

## 8. Cross-cutting requirements

### Security

- Security-sensitive fallback must be explicit. A warning followed by unsandboxed execution is not acceptable for an explicit enforcement request.
- Output limits must apply independently to stdout and stderr and must not permit memory amplification through interleaved streams.
- Environment inheritance and working-directory ownership must remain governed by existing execution policy.
- Raw shell remains an explicit capability with existing approval/risk treatment.
- New dependencies in the sandbox or parser boundary must be narrowly scoped and reviewed for maintenance status and unsafe code exposure.

### Concurrency, cancellation, and recovery

- Every blocking or child-process operation must have a clear owner and cancellation path.
- Semaphore permits must remain live for the resource use they claim to limit.
- Process termination must define graceful wait, forced kill, descendant cleanup, and reap behavior.
- Cancellation must not be reported as ordinary command failure.
- Interrupted searches and process runs must release permits, file handles, temp scripts, and snapshots.

### Storage and migration

- No database migration is expected.
- Namespace-hash changes must not orphan durable memory. Use a compatibility lookup/migration or demonstrate that the namespace is non-durable/reconstructible before changing it.
- Configuration parser changes must preserve existing user files or provide an explicit one-time migration/import path.

### Protocol and compatibility

- Daemon protocol and frontend projections are unchanged by M001–M006.
- M007 must preserve IPC compatibility unless an ADR and migration plan explicitly authorize a change.
- Tool result schemas may gain backward-compatible diagnostics for sandbox state, truncation, timeout, or cancellation. Breaking protocol changes are out of scope.

### Observability

Execution results and logs should expose:

- selected execution route;
- sandbox requested and obtained;
- output truncation per stream;
- timeout versus cancellation;
- process-tree cleanup failures;
- grep cancellation/limit termination;
- binary measurement target and feature set during M007.

Do not add a telemetry backend or durable event schema solely for this roadmap.

### Performance and footprint

- Correctness comes before micro-optimization.
- Avoid buffering entire outputs or repeatedly decoding files.
- Avoid creating one blocking task per file when a bounded batch suffices.
- Record release-mode size measurements, not debug artifacts.
- Distinguish on-disk binary size, stripped size, and runtime resident memory; do not claim one as another.

## 9. Verification strategy

Verification follows the existing minimal contract:

1. focused tests for the mechanism changed by the milestone;
2. relevant static guard where it prevents a concrete regression;
3. `scripts/verify.sh quick` once on the accepted milestone revision;
4. the existing hosted `verify` job when the change is merged or submitted;
5. no duplicate local full-workspace run when hosted `verify` already covers the same executable tree, unless a concrete failure requires reproduction.

Additional rules:

- M001 may require one Linux host-capability test run. Unsupported kernels must produce a recorded skip reason; they must not be treated as enforcement evidence.
- M002 may use short-lived child-process fixtures that deliberately emit large output or spawn a descendant. Tests must have strict timeouts.
- M004 stress coverage must remain bounded and deterministic; do not add long wall-clock benchmarks to CI.
- M005 and M007 use local Cargo feature/size inspection. `cargo bloat` or equivalent is a maintainer/developer tool, not a new required CI dependency.
- M006 uses parser fixtures, not a broad configuration corpus or fuzzing framework.
- M008 must not invent additional closure commands.

## 10. Closure and registry policy

Each milestone receives one compact closure record under:

- `plans/closure/runtime-safety-resource-footprint/NNN-status.md`

The record includes:

- accepted commit or PR;
- focused commands and outcomes;
- one quick/hosted verification reference where applicable;
- requirement-to-evidence summary;
- compatibility or migration outcome;
- unresolved findings by severity;
- recommendation: closed, conditionally closed, blocked, or corrective pass required.

Independent review is required for M001 because it changes a security boundary. M002 should receive a second-person or second-agent correctness review of process-tree and output-limit behavior. M003–M008 do not require independent closure unless their implementation exposes a high/medium unresolved finding.

Do not create a separate closure roadmap, ratification addendum, or evidence-transfer milestone for ordinary success. A reproducible high/medium defect receives one narrow corrective plan linked to the affected milestone.

## 11. Risks and deferred work

Known risks:

- Landlock behavior varies by kernel ABI and filesystem layout. Tests must distinguish unsupported hosts from implementation failure.
- Process-tree cleanup differs on Windows. This roadmap requires correct existing supported behavior and Unix process groups; a complete Windows Job Object implementation may be deferred if Windows process execution is not yet a supported production target, but the limitation must be explicit.
- Consolidating executors can reveal implicit differences in environment inheritance, output formatting, or cancellation semantics. Preserve documented behavior and migrate intentionally.
- Dependency feature changes can affect transitive TLS roots, SQLx macros/migrations, clipboard backends, or parser diagnostics. Use focused compile/tests for actual feature consumers.
- Separate binaries can reduce one executable while increasing aggregate installed bytes or packaging complexity. M007 must measure both.

Deferred unless new evidence warrants registration:

- seccomp or namespace sandboxing;
- remote/container execution isolation;
- persistent search indexing;
- whole-repository dependency replacement campaigns;
- Windows-specific binary topology;
- plugin ABI redesign;
- automatic dependency-update bots or release automation;
- continuous binary-size gates.

## 12. Handoff order

The recommended execution order is:

1. M001 immediately.
2. M004 is closed; M005 may execute independently in parallel with M001.
3. M002 production implementation is complete, with strict milestone promotion after M001 closes.
4. M003 after M002 reaches strict closure.
5. M006 after M005 establishes dependency ownership.
6. M007 after M002, M003, M005, and M006; include M004 in final measurements when available.
7. M008 after all production milestones close.

Agents should implement one milestone plan at a time, inspect current code before editing, preserve unrelated changes, and stop rather than redesign adjacent subsystems when a plan's stated boundary is insufficient.
