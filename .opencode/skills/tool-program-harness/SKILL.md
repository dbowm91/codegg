---
name: tool-program-harness
description: Reusable harness for evaluating, testing, and validating Tool Programs across deterministic, live-model, and ACP transport modes
version: 1.1.0
process: any
---

# Skill: Tool Program Harness

Reusable harness for evaluating, testing, and validating Tool Programs
across deterministic, live-model, and ACP transport modes. The
program domain, storage, and execution contracts live in
`architecture/tool_programs.md`; this skill covers the harness that
exercises them.

## When to Load

Load this skill when:

- Running tool program scenario tests (deterministic or live)
- Evaluating tool program correctness, performance, or chaos resilience
- Validating Eggpool model identity and behavior
- Capturing evidence for closure records
- Debugging tool program failures or convergence issues

## Quick Start

### Deterministic Mode (default)

```bash
# Run all M010 scenario tests
cargo test --test tool_program_scenarios
cargo test --test tool_program_chaos
cargo test --test tool_program_resource_convergence
cargo test --test tool_program_model_behavior

# Run the external harness
python3 scripts/e2e/tool_program_harness.py --mode scripted --scenario all

# Exercise the real-process production path (daemon failpoints, restart, replay)
python3 scripts/e2e/tool_program_harness.py --mode native
```

### Live Eggpool Mode

Requires `CODEGG_EGGPOOL_URL`, `CODEGG_EGGPOOL_API_KEY`, and
`CODEGG_EGGPOOL_CONNECTION_ID` environment variables.

```bash
python3 scripts/e2e/tool_program_harness.py --mode eggpool --model mimo-v2.5 --no-model-fallback
```

Without URL/key the mode SKIPs. With them but without
`--no-model-fallback` or without `CODEGG_EGGPOOL_CONNECTION_ID` it FAILS
— live identity must be exact, never a fallback.

### ACP Mode (placeholder)

```bash
python3 scripts/e2e/tool_program_harness.py --mode acp --scenario all
```

The harness `--mode acp` adapter (`run_acp_mode` in
`scripts/e2e/tool_program_harness.py`) is still a placeholder that reports
SKIPPED ("ACP adapter not yet available"). This is the *tool-program
harness transport*, not the `codegg acp` ACP v1 stdio frontend, which
exists separately (`src/acp.rs`, `architecture/acp.md`). The native
protocol remains the baseline headless transport for harness evidence.

## Native source and inspection artifacts

`--mode native` is a thin client: it shells out to
`cargo test -p codegg --test tool_program_m015_daemon_failpoints`
(`scripts/e2e/tool_program_harness.py:280`), the bounded real-process
production suite. `--mode scripted` runs that native step too, after the
four deterministic binaries. The older inline core-stdio client (SHA-256
source staging, `JobSubmit`, `ToolProgramList`/`ToolProgramInspect`/
`ToolProgramCallPage`) remains in the file only as transport
documentation after an unconditional `return`.

Those artifact paths are production, not harness-owned:
`src/tool/tool_program_source.rs` persists an immutable SHA-256 source
reference under `.codegg/tool_program_sources/` and the executor verifies
the digest before parsing; `src/tool/tool_program_ledger.rs:94` writes
only bounded redacted call summaries under `.codegg/tool_program_calls/`.
Raw source, arguments, and result bodies are not part of the public
inspection response (`ToolProgramDetail` in
`crates/codegg-protocol/src/projection/dto.rs`).

`architecture/tool_programs.md` covers the domain, storage, and execution
path but has no harness section; this skill is the harness's guide.

## Scenario Schema

Each scenario has:

- `name` — identifier
- `version` — schema version
- `source` — restricted-Python source
- `tools` — allowed tool names
- `expected_status` — terminal status
- `deadline` — max wall-clock time
- `max_steps` / `max_iterations` — runtime bounds
- `broker` — fault injection configuration
- `seed` — deterministic chaos seed

## Fault Injection Points

| Boundary | Injection | Test |
|----------|-----------|------|
| Broker transient failure | `FailOnNthCallBroker`, `SeededChaosBroker` | `tool_program_chaos` |
| Step budget exhaustion | `RuntimeLimits.max_steps` | `tool_program_chaos` |
| Iteration budget | `RuntimeLimits.max_iterations` | `tool_program_chaos` |
| Cancellation | `CancellationToken` | `tool_program_chaos` |
| Malformed output | `MalformedOutputBroker` | `tool_program_chaos` |
| Worker panic | `AlwaysPanicBroker` | `tool_program_chaos` |
| Rate limiting | `RateLimitedBroker` | `tool_program_scenarios` |

## Resource Convergence

Measured per scenario by `ResourceSnapshot`
(`tests/tool_program_resource_convergence.rs:19`):

- `tasks_spawned` — no leaked tasks
- `calls_completed` — should equal expected call count
- `bytes_used` — should be positive for programs with tool calls
- `steps_used` — should be positive for non-trivial programs
- `iterations_used` — should be positive for loop programs
- No leaked processes or permits

## Secret Handling

- Eggpool credentials are read from environment variables only
- Never print, log, or commit `CODEGG_EGGPOOL_URL`, `CODEGG_EGGPOOL_API_KEY`, or
  captured provider responses
- Redacted endpoint class recorded in evidence, not actual values
- The harness persists nothing outside `.codegg/`, which `.gitignore` excludes

## Evidence Capture

When running for closure evidence:

1. Record exact commands, seeds, and repetitions
2. Record pass/fail counts and durations
3. Record skipped tests with reasons
4. Distinguish local vs CI evidence
5. Record environment (OS, Rust version, date)

## Test Files

| File | Purpose |
|------|---------|
| `tests/tool_program_scenarios.rs` | Scenario schema, runner, and 13 unit tests |
| `tests/tool_program_chaos.rs` | Deterministic fault injection, 13 tests |
| `tests/tool_program_resource_convergence.rs` | Resource baseline/final probes, 10 tests |
| `tests/tool_program_model_behavior.rs` | Scripted model behavior and direct/programmatic metric validation, 14 tests |
| `tests/tool_program_m015_daemon_failpoints.rs` | Real-process production suite run by `--mode native` |
| `scripts/e2e/tool_program_harness.py` | External harness runner (scripted/native/eggpool/acp) |

## Source verification

Verified 2026-10-06 against `scripts/e2e/tool_program_harness.py`,
`tests/tool_program_scenarios.rs`, `tests/tool_program_chaos.rs`,
`tests/tool_program_resource_convergence.rs`,
`tests/tool_program_model_behavior.rs`,
`tests/tool_program_m015_daemon_failpoints.rs`,
`src/tool/tool_program_source.rs`, `src/tool/tool_program_ledger.rs`,
`crates/codegg-protocol/src/projection/dto.rs`, and
`architecture/tool_programs.md`. Confirmed correct as written: the four
CLI modes and their arguments, the `--no-model-fallback` and
`CODEGG_EGGPOOL_CONNECTION_ID` preconditions, the `run_acp_mode`
SKIPPED placeholder, the `Scenario` field list, all five broker names and
their test files, and the four test counts. Rewrote "Native source and
inspection artifacts": native mode is a `cargo test` wrapper for
`tool_program_m015_daemon_failpoints` and the old inline client is
unreachable after the `return` at
`scripts/e2e/tool_program_harness.py:293`. Removed the `completed_calls`
bullet (it is a test name, not a `ResourceSnapshot` field) and the
`.gitignore` "captured response files" claim (the harness writes nothing
outside `.codegg/`, which `.gitignore` already excludes wholesale).
