# Agent Workstream Blackboard M008 — S0/S1 Pilot Results

Evidence status: partial pilot only; this is not the M008 closure record.

Source plan: `plans/implementation/agent-workstream-blackboard/008-comparative-trajectory-qualification.md`
Qualified CodeGG source: `ed065038c8c51e39005eec8df3783ad83ec05865`
Experiment binary: CodeGG 0.1.0, built from that source with Rust 1.89 for aarch64-apple-darwin.
Fixture baseline: `5f0c82703085da1e7b4cf80cd884260ef1c6a0a2`
Provider/model: `opencode_go/gpt-6-luna`; agent: `build`.

## 1. Question and protocol

This pilot compared S0 (one agent implements both independent modules) with
S1 (two agents work concurrently in separate linked worktrees, one module per
agent, with no peer board or task claims). Two repetitions started from clean
worktrees at the same fixture baseline. Condition order was S0-A, S1-A,
S1-B, S0-B.

The fixed task added the same capped exponential `retry_delay_ms` contract to
a dependency-free Rust crate and Python package. The existing Rust and Python
tests were not changed. All runs used the same model, agent profile,
approval mode (`yolo`), workspace-write sandbox, task specifications and
300-second wall timeout. S1 child sessions ran concurrently. We measured
CodeGG-reported token use and duration, process wall time and peak RSS, then
committed each isolated result, merged the two S1 branches sequentially and
ran both test suites on the integrated tree.

CodeGG's `exec` input has no aggregate token-limit field. Per-invocation
defaults were the same, and observed token totals are reported below; this is
not a fixed-total-token-budget comparison. The run count is N=2 per condition,
there is no provider seed, and the output does not expose exact tool-call
counts (only distinct tool names). Interpret all differences as descriptive.

## 2. Results

| Repetition | Condition | Agent tokens | Agent wall time | Peak RSS | Merge + integrated tests | Total observed wall | Outcome |
|---|---|---:|---:|---:|---:|---:|---|
| A | S0, one agent | 9,895 | 20.29 s | 123,355,136 B | included in agent run | 20.29 s | Rust + Python tests passed |
| A | S1, two isolated agents | 18,472 | 24.13 s slower child (17.54 s other) | 248,102,912 B summed child peaks | 2.298 s | 26.43 s | Both child tests and both integrated tests passed; no merge conflict |
| B | S0, one agent | 13,592 | 51.66 s | 116,424,704 B | included in agent run | 51.66 s | Rust + Python tests passed |
| B | S1, two isolated agents | 19,159 | 23.45 s slower child (17.49 s other) | 232,898,560 B summed child peaks | 2.339 s | 25.79 s | Both child tests and both integrated tests passed; no merge conflict |

Each S0 agent reported both required test commands passing; the evaluator then
reran both commands in each S0 worktree. Each S1 child reported its language
test passing; the evaluator merged the independently committed Rust and
Python branches and reran both suites on each integrated tree. All eight
agent-run language tests and all eight evaluator-run language tests passed.
Both integrated S1 trees were clean after verification.

S1 used 86.7% more tokens in A and 41.0% more in B. Its end-to-end time was
30.3% slower in A and 50.1% faster in B. S0 varied from 20.29 to 51.66
seconds, while S1 varied from 25.79 to 26.43 seconds. The small sample points
in opposite directions for latency and does not establish a reliable speedup;
the higher observed token cost is consistent in both repetitions. Peak RSS is
the sum of the concurrent child processes' individual peaks, not a sampled
aggregate process-tree peak.

## 3. Trial traceability

The sanitized per-trial records are preserved in
`plans/closure/agent-workstream-blackboard/008-pilot-data.json`. Fixture
commits were kept outside the CodeGG product repository:

| Trial | Commit |
|---|---|
| S0-A | `673353a75bac073bf6f5e375ba7b4a1e93a5a862` |
| S0-B | `dfcfd9de3b8e56889a221c48f1f4fa2d972c7bf7` |
| S1-A Rust child | `bad6cfcf62ad6a0a3edf51984905487cd4c9e7d0` |
| S1-A Python child | `6e0e92542463ea9db697f2df6d1d7025876d6d05` |
| S1-A integrated merge | `ed50607` then `7564198` |
| S1-B Rust child | `b235747915c71194d6189c7e0031880a7bf4cb9a` |
| S1-B Python child | `5e835eccbdccfb086414c98f2484be054e031763` |
| S1-B integrated merge | `c379e95` then `25fb260` |

The two attempts made with the previously installed executable were excluded
before measurement: both failed on the provider's unsupported deferred-tool
metadata, before editing any source. The branch fix removes that metadata for
providers without tool-deferral support. Those setup attempts are not counted
as task failures or retries.

## 4. Findings and limits

- The isolated S1 workflow completed both tasks and sequential integration
  without conflict. This demonstrates coordination correctness for two
  non-overlapping files on this fixture.
- S1 did not show a repeatable latency improvement over S0 and consumed more
  model tokens in both trials. This is not evidence that peer observations or
  task claims improve effectiveness: neither feature was enabled in S1.
- The fixture has no heavy shared Rust dependencies, overlapping edits,
  prerequisite chain, recovery event or background build. It does not qualify
  Cargo contention, M005 shared-checkout behavior, S2/S3, or M008's broader
  economics and fault matrix.
- CodeGG `exec` runs the agent loop directly; this pilot does not exercise the
  daemon-backed `JobSubmissionService` path and is not M007's scheduler
  admission qualification.
- No supported shared-write arm was run. M005 remains separately gated.

M008 remains open for the broader S0/S1/S2/S3 matrix, realistic Rust workspace
contention, additional task strata and reproducible aggregate resource-cost
capture. This pilot alone does not unblock the broad M008 closure gate.
