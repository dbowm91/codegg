# Tool-Selection Advisor Post-Closure Corrective M004 — Small-Model Trajectory Qualification and Closure

Status: blocked

Repository baseline: `c9087346620988a7c793fb2687732a62e481103a`

Source corrective:

- `plans/subsystems/tool-selection-advisor-post-closure-corrective-addendum.md#m004--live-small-model-trajectory-qualification-and-corrective-closure`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Hard dependencies: M001 + M002 + M003 accepted closure.

Primary class: evidence/closure corrective.

## 1. Objective

Determine whether the contextual advisor and true pre-turn promotion improve real tool use for smaller/tool-fragile primary models.

M004 must use held-out end-to-end coding/tool tasks and actual primary-model calls (explicitly configured by the operator), compare declared modes, measure quality/cost/latency/error behavior, and close the corrective honestly whether the result is positive or negative.

This milestone does **not** make the advisor default-on.

## 2. Why this milestone is blocked

The live study is meaningful only after:

- M001 freezes a credible held-out corpus/task split and consent semantics;
- M002 produces the contextual small model and resource evidence;
- M003 wires pre-turn disclosure at the correct provider-definition seam.

## 3. Current evidence gap

The predecessor M005 qualification uses four deterministic cases and explicitly states that no hosted/live small-primary-model trajectory improvement is claimed.

Offline ranking evidence is necessary but insufficient. The product question is whether making the right capability visible changes downstream coding behavior.

## 4. Invariants

- Qualification provider calls are explicit operator actions and are not part of ordinary CodeGG startup/use.
- Qualification tasks are held out from model training and threshold tuning.
- No user/private repository content is automatically uploaded as a benchmark.
- Advisor remains default-off regardless of result.
- Permission/sandbox/broker behavior is identical across experiment arms.
- Provider/model identity, model settings, tool surface fingerprint, advisor artifact, task fixture version, and random/temperature settings are recorded.
- Failed/aborted trajectories remain part of the result rather than being silently discarded.

## 5. Qualification suite

Build a held-out trajectory suite separate from the training corpus. Prefer disposable fixture repositories with deterministic setup/verification.

Use at least **32 task groups** unless closure documents why a larger pre-registered suite is required. Cover:

- specialized LSP/navigation/refactor discovery;
- exact text search vs semantic/repo search;
- Git/status/diff/history inspection;
- tests/verification vs shell fallback;
- structured data validation;
- research/web capability where provider/network testing is explicitly enabled;
- plugin/MCP-like unknown descriptor cases;
- multi-step tasks needing two tool families;
- no-tool tasks where promotion is undesirable.

Include tasks intentionally designed so a useful tool is deferred and not trivially named in the prompt.

Freeze suite fingerprint before live runs.

## 6. Primary-model matrix

Use at least:

- one smaller/tool-fragile primary model tier representative of the use case;
- one stronger reference tier to detect whether promotion helps weak models but distracts strong ones.

Exact providers/models are operator-configured because available models change over time. Closure records the exact model IDs, dates/config, temperature/reasoning settings, and provider.

Do not make a specific commercial model a repository dependency.

## 7. Experiment arms

At minimum compare:

1. **off** — current normal progressive disclosure;
2. **reactive rerank** — advisor may rerank `tool_search`, no pre-turn promotion;
3. **proactive promote** — contextual advisor may expose bounded high-confidence deferred tools pre-turn;
4. optionally **full/expanded tool palette** as an upper-context-cost control when safe and useful.

Keep model/provider/task configuration identical across arms.

The `hashed-linear-v1` baseline may be included to prove whether contextual learning adds value beyond wiring.

## 8. Metrics

### Primary downstream metrics

- task success/fixture verifier pass;
- correct specialized-tool use rate;
- missed-tool/discovery failure rate;
- unnecessary shell fallback rate;
- invalid/bad tool-call rate;
- `tool_search` calls/turns;
- unnecessary promotion rate/no-tool false-promotion rate.

### Cost/performance metrics

- input/tool-schema tokens;
- output tokens;
- wall-clock;
- advisor p50/p95 latency;
- total provider calls;
- provider cost when available;
- local CPU/RSS overhead.

### Safety/reliability metrics

- permission denials caused by inappropriate promotion;
- advisor fallback/error count;
- hidden/denied promotion count (must remain zero);
- telemetry network attempts when disabled (must remain zero).

## 9. Statistical/evidence discipline

Pre-register:

- task suite fingerprint;
- primary metrics;
- thresholds/margins used by the advisor;
- model matrix;
- stopping rule;
- retry/repeat policy.

Prefer deterministic/low-temperature settings where supported. If model nondeterminism is material, use repeated trials for a declared subset or all tasks and report uncertainty rather than cherry-picking best runs.

Do not tune advisor thresholds on final live test outcomes. Threshold selection belongs to M001/M002 development data.

## 10. Local model deployment qualification

Alongside live trajectories, record contextual advisor resource behavior on representative local classes where available:

- Apple Silicon developer machine;
- x86_64 Linux;
- ARM64/SBC class such as Raspberry Pi-class hardware.

At minimum record artifact/tokenizer size, cold load, RSS, score latency at realistic candidate counts, and whether the selected model is practical for an SBC-class CPU.

If a target is unavailable, closure states that limitation rather than extrapolating.

## 11. Ordered work packages

A. Freeze disposable held-out trajectory suite.
B. Implement/extend qualification harness to capture full tool trajectories and outcome verifiers.
C. Configure model matrix and pre-register experiment.
D. Run off/reactive/proactive arms.
E. Analyze metrics by model tier/task family.
F. Run local resource matrix.
G. Reconcile mode documentation/registry and close or register a narrower follow-up if a correctness defect appears.

## 12. Acceptance criteria

M004 may close in either of two dispositions.

### Positive qualification

A mode may be described as **qualified experimental opt-in** only if:

- the contextual model materially improves a predeclared primary downstream metric for the smaller/tool-fragile tier without an unacceptable regression in task success, false promotion, prompt cost, or latency;
- authority/safety invariants remain perfect;
- contextual encoder results justify its resource cost over `hashed-linear-v1`;
- resource measurements fit the intended deployment envelope.

Even then, default remains off.

### Negative or mixed qualification

M004 may still close if the study is complete and truthful. In that case:

- proactive/rerank remain observe/research-only or are narrowed/disabled as evidence dictates;
- the model is not bundled/defaulted merely to preserve the project direction;
- results and residual hypotheses are recorded.

## 13. Required tests/verification

Before live calls:

```bash
cargo test --workspace --locked
cargo test --locked --features tool-advisor
cargo test --locked --features tool-advisor-training
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Qualification harness must support a dry-run that validates fixtures/model/config without provider calls.

For live runs, record exact commands/config fingerprints without committing secrets.

## 14. Documentation updates

- final qualification report;
- operator guide for enabling/disabling advisor modes;
- model/resource compatibility;
- clear status of `hashed-linear-v1` vs contextual encoder;
- no claim that telemetry or training is enabled automatically.

## 15. Stop conditions

Stop and register a correctness corrective if:

- any denied/hidden/parent-ceiling tool is promoted;
- provider-facing tool authority differs between arms beyond intended visibility;
- telemetry transmits without explicit remote consent;
- task fixtures leak into training;
- the qualification harness cannot distinguish pre-turn promotion from later `tool_search` discovery.

## 16. Closure evidence required

- suite/model/config fingerprints;
- exact primary model IDs/settings/date;
- per-arm task success/tool-use/cost/latency tables;
- no-tool false-promotion and authority-negative results;
- linear-vs-contextual comparison;
- local resource matrix;
- failed/aborted trajectory accounting;
- exact verification commands/results;
- final experimental mode disposition;
- registry reconciliation.
