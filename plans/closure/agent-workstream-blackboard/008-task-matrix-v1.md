# M008 repeated task matrix v1

Status: frozen protocol; these contracts define the next repeated evaluation
set. They are not outcome evidence. Keep evaluator files and task text fixed
for all conditions and repetitions; record the fixture tree digest for every
trial. Existing retry-policy and project-summary fixtures are the first task
in their respective strata.

## Common protocol

- Conditions: S0 (one capable CodeGG worker), S1 (two isolated workers with no
  peer board), S2 (same with factual peer observations), and S3 (same with
  observations and atomic task claims), where the condition is available and
  safe. S4 is excluded pending M005.
- Freeze one delegated-worker ceiling `B` per task before its first trial and
  use exactly that same `B` for every condition and repetition of that task.
  `IM-1` uses `B = 24,000`, above the earlier S0 range of 14,784–14,921
  reported tokens. `SI-1` uses `B = 1,400,000`, above the largest complete
  prior S0–S3 aggregate of 1,033,105 worker tokens; a two-worker arm receives
  700,000 per child. The task tool accepts up to 2,000,000 tokens so S0 can
  receive this same aggregate ceiling. These ceilings are task-specific because the
  already-observed workloads differ by more than an order of magnitude. New
  contracts require a separate pilot and frozen ceiling before their
  comparison begins. S0 receives `max_model_tokens = B`; S1/S2/S3 receive
  `aggregate_model_tokens = B`. Coordinator and integration usage is
  additional and must be counted separately and in all-in totals.
- Use three repetitions for each task and available condition. Counterbalance
  condition order by rotating S0/S1/S2/S3 left by repetition index (0, 1, 2),
  then omit conditions declared unavailable before trials.
  Use the same provider, model, tool
  policy, workspace policy, approval mode, and wall-clock limit. Do not retry
  a failed run invisibly; retries are separate attempts and consume budget.
- Record provider-reported input, output, cached tokens, estimated cost, and
  whether the local pricing table has a matching rate per worker;
  coordinator and integration usage; wall time; evaluator and tool time;
  useful throughput (accepted contract checks passed per worker-token and per
  wall-clock minute); valid integrated result; conflicts, duplicate work,
  interventions, and over-budget stops. An unanswered provider attempt is
  unknown usage, not zero.

## Frozen task contracts

| ID | Stratum | Fixture and task | Required accepted result |
|---|---|---|---|
| IM-1 | Independent modules | `008-fixture/`; implement the retry-delay policy independently in the Rust and Python modules. | Both language suites pass; behavior matches the frozen capped exponential contract. |
| IM-2 | Independent modules | `008-shared-interface-fixture/`; implement the Rust event-summary producer and Python report consumer as separate deliverables, but do not alter the frozen schema or golden data. | Rust, Python, and end-to-end golden suites pass. This is scored as independent-module work only when the prompt explicitly assigns disjoint Rust/Python files and asks the coordinator to integrate sequentially. |
| SI-1 | Shared interface | `008-shared-interface-fixture/`; implement both sides of `project-summary-v1`, with producer and consumer split across workers for S1/S2/S3. | Both language suites and the exact cross-language golden output pass without changing the schema/evaluator. |
| SI-2 | Shared interface | `008-fixture/`; implement matching Rust and Python retry-delay behavior against the same frozen five-case contract, with the Rust crate acting as the reference producer and Python client consuming the published contract. | Both language suites pass and a frozen cross-language JSON-vector evaluator matches all outputs. The evaluator vector file is part of this matrix digest. |
| OA-1 | Overlapping API/schema | `008-shared-interface-fixture/`; change the event contract to add an optional `actor` field with frozen normalization and validation rules. Both workers may edit the shared schema, Rust parser, and Python validator. | Versioned v2 schema, Rust/Python validation parity, and golden report pass; no silent v1 contract break. |
| OA-2 | Overlapping API/schema | Same fixture; add a `source_revision` validation and escaping rule required by both the producer CLI and report renderer. Both workers are explicitly allowed to edit the same Rust/Python API and schema paths. | Frozen malformed-input and Markdown-escaping checks pass in both languages and integrated golden evaluation. |

IM-2 deliberately uses the existing cross-language fixture as a second
independent-module contract with a distinct assignment and scoring stratum;
its task prompt and trial record must make that distinction explicit. This
classification overlaps the repository files with SI-1 by design; the task
contract and worker assignment define whether the run measures independent
module work or interface coordination. SI-2's new vector evaluator must be
authored and hashed before any provider trial.
OA-1/OA-2 require new immutable evaluator inputs and explicit expected outputs
before they can enter provider trials. Until those files exist and their
baseline fails for the intended reason, mark those task/condition cells
unavailable rather than treating the contract prose as an evaluated task.

No task in this matrix has yet been run under this equal-total-budget
protocol. The previous pilots remain separate historical evidence.
