---
name: tool-program-harness
description: Reusable harness for evaluating, testing, and validating Tool Programs across deterministic, live-model, and ACP transport modes
version: 1.0.0
process: any
---

# Skill: Tool Program Harness

Reusable harness for evaluating, testing, and validating Tool Programs across deterministic, live-model, and ACP transport modes.

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

# Exercise the production scheduler/executor and public inspection protocol
python3 scripts/e2e/tool_program_harness.py --mode native
```

### Live Eggpool Mode

Requires `CODEGG_EGGPOOL_URL`, `CODEGG_EGGPOOL_API_KEY`, and
`CODEGG_EGGPOOL_CONNECTION_ID` environment variables.

```bash
python3 scripts/e2e/tool_program_harness.py --mode eggpool --model mimo-v2.5 --no-model-fallback
```

### ACP Mode (when available)

```bash
python3 scripts/e2e/tool_program_harness.py --mode acp --scenario all
```

ACP is reported as skipped until a production ACP adapter is scheduled; the
native protocol remains the baseline headless transport.

## Native source and inspection artifacts

The native harness persists an immutable SHA-256 source reference under
`.codegg/tool_program_sources/`, submits it through `JobSubmit`, waits through
the scheduler, and verifies `ToolProgramList`, `ToolProgramInspect`, and
`ToolProgramCallPage`. The executor writes only bounded redacted call
summaries under `.codegg/tool_program_calls/`; raw source, arguments, and
result bodies are not part of the public inspection response.

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

Measured per scenario:

- `calls_completed` — should equal expected call count
- `completed_calls` vec — should be bounded by `calls_completed`
- `bytes_used` — should be positive for programs with tool calls
- `steps_used` — should be positive for non-trivial programs
- `iterations_used` — should be positive for loop programs
- No leaked tasks, processes, or permits

## Secret Handling

- Eggpool credentials are read from environment variables only
- Never print, log, or commit `CODEGG_EGGPOOL_URL`, `CODEGG_EGGPOOL_API_KEY`, or
  captured provider responses
- Redacted endpoint class recorded in evidence, not actual values
- `.gitignore` excludes any captured response files

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
| `tests/tool_program_scenarios.rs` | Scenario schema, runner, and 12 unit tests |
| `tests/tool_program_chaos.rs` | Deterministic fault injection, 14 tests |
| `tests/tool_program_resource_convergence.rs` | Resource baseline/final probes, 10 tests |
| `tests/tool_program_model_behavior.rs` | Scripted model behavior and direct/programmatic metric validation, 14 tests |
| `scripts/e2e/tool_program_harness.py` | External harness runner (scripted/eggpool/acp) |
