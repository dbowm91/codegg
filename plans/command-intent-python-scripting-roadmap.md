# Command Intent, Projection, and First-Class Python Scripting Roadmap

## Purpose

This roadmap defines the implementation path for making agent-authored commands and scripts first-class, safe, projected execution artifacts in codegg. The core premise is that LLMs are highly fluent in shell and Python, and codegg should accommodate that natural interface without allowing shell/Python to become an opaque bypass around tools, permissions, snapshots, or context budgeting.

The target architecture is not to remove tools. The target architecture is to place a command intent and projection layer in front of executable actions so that shell commands, tests, git operations, searches, file operations, Python scripts, and future scripting backends can all be classified, permissioned, executed, stored, projected, and compressed consistently.

## Current repo substrate

The repo already has much of the necessary foundation:

- `src/shell/` contains the human shell module, shell runtime, bounded output store, policy, digest extraction, projection bridge, projector traits, native projectors, and RTK-related projection support.
- `architecture/human_shell.md` documents command origin, capture policies, shell events, bounded output storage, digest extraction, native projectors, RTK discovery/projection, and the shell-command run bridge.
- `src/test_runner/` contains a supervised test runner with command resolution, streaming process execution, structured failure parsing, previous-failure indexing, log capture, and model-facing bounded reports.
- `architecture/test_runner.md` documents the test runner contract, strict custom command validation, argv-prefix allowlisting, failure taxonomy, and `.codegg/test-runs/` artifact indexing.
- `src/tool/bash.rs` provides the current model-facing bash tool, including command length limits, blocked patterns, optional allowed paths, Landlock integration, environment clearing, timeout handling, and output truncation.
- `crates/codegg-core` already owns domain surfaces such as snapshots, worktree state, session storage, and resilience.
- `crates/egggit`, `crates/eggsentry`, and `crates/eggcontext` provide native git facts, security scanning, and token/context utilities that should be reused rather than duplicated.

The missing piece is a unified command substrate that can map natural model behavior onto native codegg backends. Python is also missing as a first-class execution family; it currently appears only as plugin examples/scripts, not as a structured agent execution subsystem.

## Architecture target

All executable activity should pass through this conceptual pipeline:

```text
CommandSource / ScriptSource
  -> CommandIntentClassifier
  -> CommandPlanner
  -> PermissionPlanner
  -> ExecutionBackend
  -> RawArtifactStore
  -> ProjectionPipeline
  -> RTK policy, if enabled and useful
  -> ContextPromotionPolicy
  -> TUI / protocol / model-facing result
```

The central invariant is that raw execution output remains durably available through handles, while the model normally receives a bounded, structured projection. RTK should optimize projections under context pressure; it must not replace exact raw artifacts.

## Design principles

1. Preserve model fluency. Models should still be able to write commands such as `cargo test`, `pytest`, `git diff`, `rg`, and Python scripts. codegg should understand and supervise those idioms instead of forcing every operation through an unnatural bespoke tool call.

2. Do not build a partial shell interpreter. Recognize simple argv-shaped commands conservatively. If a command uses pipes, redirection, command substitution, heredocs, shell control operators, or complex quoting, classify it as complex shell and fall back to raw shell with stricter policy and projection.

3. Prefer native backends for stable semantics. Read-only git facts should route through `egggit` where possible. Tests should route through `test_runner` where possible. File searches can route through native grep/glob/search facilities where possible. Raw shell remains a fallback, not the semantic center.

4. Treat Python as a first-class scripting substrate. Python should have explicit execution modes, risk analysis, sandbox/snapshot policy, script provenance, changed-file capture, diff projection, and output handles. It should not be hidden inside `bash`.

5. Capability-based policy beats denylist policy. Denylists remain defense-in-depth, but the core policy model should say exactly which capabilities an execution plan has: read workspace, write workspace, subprocess, network, env access, dependency installation, outside-workspace access, destructive file mutation, git mutation, and context promotion.

6. Projection is the context boundary. Raw stdout/stderr, test logs, diffs, and Python outputs should generally enter context only through projectors. Promotion should be explicit or policy-driven.

7. RTK is a projection backend. RTK should apply to large or repetitive outputs when enabled and when exact spans can be preserved. It should not be required for correctness.

## Roadmap milestones

### Phase 01: Command Intent Core Model

Create the command intent data model and a conservative classifier without changing execution behavior by default. Establish `CommandIntent`, `CommandSource`, `CommandOrigin`, `CommandIntentKind`, `IntentConfidence`, `RiskAssessment`, `ExecutionCapability`, `ContextPolicy`, and initial command fixtures.

This phase should produce a no-op classification path that can be attached to shell/test invocations for logging and tests, but should not yet reroute execution.

### Phase 02: Command Planner and Backend Routing Skeleton

Introduce `CommandPlan` and `ExecutionBackend` as the seam between classification and execution. Add backend routes for raw shell, managed argv process, native tool, test runner, Python placeholder, and rejected execution. Add permission request generation, but keep rerouting opt-in or test-only until validated.

This phase should make routing decisions inspectable and testable without destabilizing current behavior.

### Phase 03: Projection Pipeline Unification and RTK Policy

Generalize the existing shell projection machinery into a command-output projection contract that can serve shell, tests, git, search, and future Python runs. Add a common `ProjectionResult`, raw artifact handles, truncation reports, exact-span preservation, redaction hooks, and `RtkProjectionPolicy`.

This phase should make projection the explicit context boundary and make RTK an optional projection backend.

### Phase 04: Test/Git/Search Intent Routing MVP

Begin using the command planner for safe, high-confidence command families. Route supported test commands into `test_runner`, route read-only git commands into `egggit`/native projectors where practical, and route common search/listing commands through native or managed argv projectors. Keep complex shell and risky mutations as raw shell with existing policy.

This phase proves that natural commands can map to structured codegg subsystems while retaining shell fallback.

### Phase 05: Python Scripting MVP

Add a first-class Python scripting subsystem with `Analyze`, `Transform`, and `Verify` modes. Implement script materialization, minimal environment, timeout, output handles, pre/post snapshot integration for transforms, changed-file/diff projection, and basic static risk analysis. Route agent-authored Python commands/scripts into this subsystem rather than treating them as arbitrary bash.

This phase establishes Python as a safe, inspectable, reversible, context-efficient execution substrate.

### Later phases

After the first five phases, continue with hardening and expansion:

- capability-based Python permission profiles;
- stronger AST risk classification;
- Landlock/fallback sandbox integration per mode;
- persistent `.codegg/runs/` indexing for command and script runs;
- first-class TUI cells for command/Python runs;
- RTK-aware compression of Python stdout/stderr, diffs, and generated reports;
- broader command family coverage for package managers, linters, formatters, type checkers, and git mutations;
- adversarial tests for command smuggling, Python escapes, output poisoning, and context-promotion mistakes.

## Compatibility and migration strategy

The initial phases should avoid breaking existing model workflows. `bash` should remain available, `!` and `!!` shell behavior should retain the documented context-promotion semantics, and `/test` should continue to work through the existing test runner. The new system should first observe/classify, then plan, then selectively route only high-confidence safe commands.

The Python subsystem should eventually replace common `python -c`, heredoc, and temp-script patterns for agent-authored scripts. The bash tool should not simply unblock `python -c`; instead, it should detect Python intent and route to the Python subsystem where policy is explicit.

## Validation strategy

Each phase should include unit tests, fixture tests, and at least one integration-style path. The existing repo already documents targeted test commands for shell projection, RTK unit tests, test runner, custom command validation, and Python plugin SDK tests. New tests should extend those patterns.

Minimum validation matrix:

- command intent fixtures for common commands, risky commands, and complex shell fallback;
- planner fixtures proving backend selection and permission request generation;
- projection fixtures proving raw artifact preservation and bounded model-facing output;
- RTK eligibility fixtures that pass without requiring an installed RTK binary;
- test runner routing fixtures for `cargo test`, `cargo nextest`, `pytest`, `uv run pytest`, and npm/pnpm/yarn/bun test variants;
- Python analyze/transform/verify fixtures, including write capture, changed-file detection, subprocess detection, and denial of unsupported capabilities;
- regression tests proving raw shell fallback remains available.

## Non-goals for the first five phases

- Full POSIX shell parsing.
- Perfect Python sandboxing on every OS.
- Automatic dependency installation for Python scripts.
- Network-enabled Python scripts by default.
- Silent routing of destructive git or filesystem mutations.
- Replacing the existing tool registry.
- Replacing raw shell for complex commands.

## Success criteria

This line of work is successful when an agent can naturally issue common commands and Python scripts, while codegg can answer these questions deterministically:

- What did the agent intend to do?
- Which backend executed it?
- Which permissions were required and granted?
- What raw artifacts were produced?
- What exact files changed?
- What projection entered the model context?
- Was RTK used, and what exact spans were preserved?
- Can the user inspect, rerun, promote, or roll back the result?
