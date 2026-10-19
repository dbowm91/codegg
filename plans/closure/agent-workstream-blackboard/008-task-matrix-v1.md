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
- Freeze one total agent ceiling `B` per task before its first scored trial
  and use exactly that same `B` for every condition and repetition. S0 runs
  the single coding agent directly in the durable session with
  `headless --max-model-tokens B`. S1/S2/S3 reserve `C = 300,000` tokens for
  the parent coordinator and set the headless turn ceiling to `C`; their
  delegated workers share `B - C` through `aggregate_model_tokens`. A
  two-worker arm receives an equal split of `B - C`, with the deterministic
  remainder assigned to the first request. Do not transfer unused coordinator
  budget to workers or unused worker budget to the coordinator. The 300,000
  coordinator reservation is fixed across task strata and was selected above
  the 249,041 tokens observed in an uncapped setup trajectory; it is a ceiling,
  not a target. Use one coordinator turn per scored trajectory: if its group
  remains active when that turn reaches its ceiling, classify the trajectory
  incomplete instead of submitting another coordinator turn and multiplying
  the reserved budget.
- `IM-1` uses `B = 700,000`: S0 receives 700,000; S1/S2/S3 reserve 300,000
  for the coordinator and 400,000 for workers (200,000 per child).
  `SI-1` uses `B = 1,400,000`: S0 receives 1,400,000; S1/S2/S3 reserve
  300,000 for the coordinator and 1,100,000 for workers (550,000 per child).
  These ceilings are task-specific because the observed workloads differ by
  more than an order of magnitude. IM-2, SI-2, OA-1, and OA-2 remain unavailable
  until evaluator files are frozen, their baseline failure is confirmed, and
  a task-specific `B` is recorded here before provider trials begin.
- Budgets are provider-reported input plus output tokens. The in-flight
  provider response completes before each run can observe its usage and stop,
  so record any per-run overrun and classify a trial whose all-agent total
  exceeds `B` as over-budget; do not silently treat the requested ceiling as
  actual consumption. The fixed allocations bound the nominal coordinator plus
  worker budgets. Record each run's actual usage, cached tokens, and cost
  availability; coordinator usage is part of the agent-token total. Human
  integration time, evaluator/tool time, and resource cost remain separately
  reported and are included in all-in comparisons.
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
