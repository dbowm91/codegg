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
  `OA-1` and `OA-2` each use `B = 700,000`: S0 receives 700,000; S1/S2/S3
  reserve 300,000 for the coordinator and 400,000 for workers (200,000 per
  child). These ceilings are task-specific because the observed workloads
  differ by more than an order of magnitude. IM-2 and SI-2 remain unavailable
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
| OA-1 | Overlapping API/schema | `008-overlapping-api-fixtures/oa1/`; add the frozen optional actor field. Trim ASCII edge whitespace, preserve case, and require 1–64 ASCII bytes matching `[A-Za-z0-9][A-Za-z0-9._@-]{0,63}`. Both workers may edit the shared schema, Rust parser, and Python validator. | Versioned v2 schema, Rust/Python validation parity, frozen v2 golden report, and unchanged v1 behavior pass. |
| OA-2 | Overlapping API/schema | `008-overlapping-api-fixtures/oa2/`; require a 1–128 Unicode scalar revision with no controls or edge whitespace; use a Markdown code-span delimiter longer than its longest backtick run. Both workers may edit the same producer and consumer API paths. | Rust/Python validation parity, unchanged v1 reports for ordinary revisions, frozen malformed-input checks, and exact cross-language report bytes pass. |

IM-2 deliberately uses the existing cross-language fixture as a second
independent-module contract with a distinct assignment and scoring stratum;
its task prompt and trial record must make that distinction explicit. This
classification overlaps the repository files with SI-1 by design; the task
contract and worker assignment define whether the run measures independent
module work or interface coordination. SI-2's new vector evaluator must be
authored and hashed before any provider trial.
OA-1 evaluator baseline `2b47e6b8edff74e2c50578f65ecc2461111f425352c3038fbb3fe7420e24780d`
is frozen in `008-overlapping-api-fixtures/oa1/` (Git tree
`17fe5b0dbe7a233d25ac842bea5011f1e9e36c20`). Its Rust, Python, and golden
checks fail for missing actor-v2 support. OA-2 baseline
`8c0ac4b26b6a042843d629455d9fb689a6c684121026919118c2f966cdb6f436` is frozen
in `008-overlapping-api-fixtures/oa2/` (Git tree
`edaa7d38feb37c8cc829f46daad88a24dd663c7e`). Its Rust and Python suites fail
on invalid revision acceptance and the unsafe code span; its golden evaluator
fails on that report mismatch. Both overlapping-API tasks and their budgets
are frozen before scored trials.

Three under-budget cells have been completed: IM-1 S0 and S1 at repetition 1,
and SI-1 S0 at repetition 1. OA-1 S0 and OA-2 S0 repetition-1 attempts are
recorded as over-budget; OA-1 produced no candidate and OA-2 passed evaluators
but is excluded from equal-budget effectiveness comparison. The SI-1 fixture digest is
`93c0eadb64f4ec9bc423599085863a5018800322`. OA-1 is frozen with input tree
`17fe5b0dbe7a233d25ac842bea5011f1e9e36c20` and source baseline
`9260fda525284b821beda2782de23f6888d37b56`; results are recorded in
`008-isolated-results.json` and `008-isolated-results.md`. All other
task/condition/repetition cells remain unrun; previous pilots remain separate
historical evidence.
