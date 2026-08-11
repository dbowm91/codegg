# Agent Runtime Correctness, Autonomy, and Simplification M011 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/011-typed-tool-outcome-and-hosted-closure-corrective-pass.md`
Source subsystem addendum: `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md`
Historical predecessor: `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md`

Repository baseline: `7d863763f700d936687ad01005e6a0d19b74c991`
Implementation candidate: `e3b671adb9298e738b38f6196de79f164293b670`
Implementation commits: `eeef46268fa6ea0fc8612af7cb08fd39ed7fedff`, `e3b671adb9298e738b38f6196de79f164293b670`

## 1. Executive finding

M011 is strictly closed. The stale post-M010 bootstrap test was deleted, and
the ordinary agent execution path now preserves typed tool status alongside
model-facing text through recovery. Native permission and timeout failures are
authoritative typed outcomes; MCP and question branches carry explicit status
when the branch knows the cause. No rendered-text classifier remains in the
ordinary execution path.

The exact final candidate passed the existing hosted verification workflow:

- run `31525206176`, job `93891703941`, head `e3b671adb9298e738b38f6196de79f164293b670`;
- Workspace Clippy passed;
- Workspace tests passed at `2026-08-11T19:16:58Z`;
- all preceding schema, boundary, sandbox, ownership, and formatting steps passed.

The failed predecessor evidence remains explicit: run `31521674076`, job
`93879950640`, failed on the obsolete empty
`autonomy_bootstrap_is_explicitly_one_shot` test under Workspace Clippy.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Delete obsolete bootstrap test | Test removed from `src/agent/progress_recovery.rs`; M010 bootstrap state remains absent | pass |
| Preserve native typed status | `execute_tool_calls()` returns `ToolExecutionOutcome`; native `ToolError` maps before display rendering | pass |
| Permission reaches recovery as `Denied` | `ToolError::Permission` maps in `ToolExecutionOutcome::from_tool_error`; denial keeps original text | pass |
| Timeout reaches recovery as `Timeout` | Native timeout branch uses `ToolError::Timeout`; typed mapping and question/MCP timeout outcomes are explicit | pass |
| Ordinary failure remains non-success | NotFound, Execution, Disabled, Io, and Network map to `ToolError`; Format maps to `ProtocolError` | pass |
| Misleading success text remains success | `ToolExecutionOutcome::success("permission denied; timeout cancelled")` remains `Success` | pass |
| No ordinary legacy classifier | `ToolExecutionOutcome::legacy` and `tool_execution_status(rendered)` were removed | pass |
| Model-facing compatibility | Successful/error display strings remain separate in `model_text`; MCP and question strings retain prior wording | pass |
| M010 recovery invariants | Bootstrap/dead branches and repository-specific second continuation remain absent; one bounded continuation remains | pass |
| M009 authority/workspace invariants | Broker principal remains bound to grant issuer; explicit workspace identity remains unchanged | pass |
| Hosted strict gate | Run `31525206176` / job `93891703941` passed Clippy and Workspace tests on exact candidate | pass |

## 3. Production data-flow

Before M011, the native path held `Result<String, ToolError>` but reduced it
to `Error: ...` text before recovery rebuilt status from substrings.

After M011:

```text
native executor Result<String, ToolError>
        |
        +-- Ok(text) --------------> ToolExecutionOutcome { Success, text }
        +-- Err(typed error) ------> ToolExecutionOutcome { mapped status, "Error: ..." }
        |
        +-- model context/event ----> outcome.model_text
        +-- recovery/progress ------> outcome.status
```

The internal outcome is carried through parallel result collection and sorted
by the original tool-call index, so concurrency and ordering are unchanged.
Truncation changes only `model_text`, never the already-known status.

## 4. Typed status mapping

| Known result | Recovery status |
|---|---|
| `Ok(display)` | `Success` |
| `ToolError::Permission` | `Denied` |
| `ToolError::Timeout` | `Timeout` |
| `ToolError::NotFound` | `ToolError` |
| `ToolError::Execution` | `ToolError` |
| `ToolError::Format` | `ProtocolError` |
| `ToolError::Disabled` | `ToolError` |
| `ToolError::Io` / `Network` | `ToolError` |
| MCP timeout branch | `Timeout` |
| Question cancellation branch | `Cancelled` |
| Question timeout branch | `Timeout` |
| Opaque MCP error text | `ToolError`, with its existing display text preserved |

There is no remaining `ToolExecutionOutcome::legacy` or rendered-string status
classifier call to justify. Successful output containing `permission denied`,
`timeout`, or `cancelled` remains typed success.

## 5. Verification executed

- `cargo test -p codegg --lib agent::progress_recovery -- --nocapture` — passed; 9 tests.
- `cargo test -p codegg --lib agent::r#loop::tests -- --nocapture` — passed; 39 tests.
- `cargo test --test agent_loop_harness -- --test-threads=1` — passed.
- `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `CARGO_BUILD_JOBS=1 cargo check --workspace --all-targets --locked` — passed.
- `scripts/verify.sh quick` — passed through guards and workspace check.
- `git diff --check` — passed.
- Hosted `CI / verify` run `31525206176`, job `93891703941` — passed through Workspace tests on the exact final candidate.

## 6. Security and compatibility review

Typed permission denial is established before display conversion and cannot
restore the base palette. Textual tool-call repair remains bounded and
adapter-owned. Broker `principal_ref` remains the grant issuer principal, not a
decision/grant ID. Explicit workspace identity and path policy are unchanged.

No storage, provider, ACP, daemon, session, Tool Program, or external broker
protocol changed. Model-facing result text was intentionally preserved,
including native error formatting, MCP error text, and question completion,
cancellation, and timeout text.

## 7. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| — | None in M011 scope | No critical, high, or medium finding remains |

M010 remains historical conditional evidence and was not rewritten. The failed
run `31521674076` is retained as predecessor evidence, while the green run
above is the controlling strict closure evidence.

## 8. Planning disposition and unblock audit

The corrective addendum and agent-runtime correctness workstream move to
`closed`. M011 is removed from dependency-ready work and is recorded as the
controlling closed milestone.

The registry audit found no registered implementation plan blocked by M011.
Therefore no future plan was promoted to `ready` by this closure. Development
Verification and Release M006 remains blocked on its independently named
Provider M007 and Tool Programs M019 dependencies; Tool Programs M019 remains
ready and is not altered by this workstream closure.

Recommendation: `closed`.
