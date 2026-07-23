# Tool Programs Milestone 010 — Harness, Eggpool, Chaos, Performance, and Closure

Status: blocked pending Milestone 008 closure; Milestone 009 is a soft dependency for hosted/native equivalence

Repository baseline: `2f715941516a1d49be578fdef56714ad3ddfe8bf` (`main`)

Source roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-10--harness-eggpool-chaos-performance-and-closure`

Long-term requirements:

- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/002-long-term-roadmap.md#phase-13--acp-adapter`
- `plans/001-terminology-and-domain-model.md` — provider connection, session, turn, job, attempt, run, artifact

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Primary class: capability verification / polish / final closure

## 1. Objective

Prove that Tool Programs are reliable, non-TUI-capable, exact-model-testable, failure-contained, resource-bounded, context-efficient, evidence-preserving, operationally diagnosable, and ready for strict subsystem closure.

This milestone adds the reusable headless/ACP-oriented harness, local Eggpool validation profile, deterministic chaos framework, performance/context evaluation, security review, documentation, and final closure reconciliation. It must not conceal unresolved defects behind retry rates or token savings.

## 2. Readiness boundary

Hard dependency: M008 strict closure. Foreground/background execution, child jobs, projections, notification delivery, and recovery must be complete.

Soft dependency: M009. M010 may close the native subsystem before hosted OpenAI support only if the roadmap/registry explicitly leave M009 open and do not claim hosted capability. Full roadmap closure requires M009 closure or an explicit roadmap revision deferring it.

Operational dependency: maintainers supply a reachable local Eggpool endpoint and credential outside the repository. The test profile must pin exact model ID `mimo-v2.5`, reject model fallback, and verify that the selected model is the non-pro variant through the returned model identity/configuration. No private endpoint or API key is committed.

ACP dependency: use the production headless/native daemon protocol immediately. When the ACP adapter exists, run the same scenario corpus through an ACP transport adapter. Do not implement a second runtime or block native correctness on TUI automation.

## 3. Current implementation evidence

- `exec`/headless AgentLoop paths and scripted-provider harnesses already exercise turns, tools, permissions, retries, compaction, and follow-ups without TUI rendering.
- Session projections and daemon protocol provide frontend-neutral state that ACP can later consume.
- Provider connections and Eggpool are daemon-owned and support model selection/health/discovery.
- Existing tests include scheduler cancellation/recovery, Python adversarial sandboxing, command-routing ownership, and AgentLoop harness fixtures.
- Tool Programs after M008 include foreground/background execution, read-only calls, build/test jobs, artifacts, projections, and durable notification.
- No unified scenario runner currently injects provider/tool/worker/storage/restart faults at a configured rate while asserting terminal convergence and exactly-once call behavior.
- No closure corpus quantifies correctness and evidence retention against model turns, context bytes/tokens, nested calls, latency, cache hits, and artifact volume.

## 4. Invariants that must not regress

- Tests use the same daemon, scheduler, Tool Broker, providers, projections, and notification paths as production.
- The harness does not call internal executors directly except in focused unit tests.
- Exact-model tests fail if another model or fallback provider is used.
- Credentials/endpoints are read from ignored environment/config references and are redacted from logs/artifacts.
- Fault injection is deterministic from recorded seeds and does not weaken production timeouts or retry limits.
- Every logical program reaches terminal or explicitly recoverable state within a declared bound.
- Parent notification is delivered at most once logically and no completed nested call is repeated.
- Resource convergence is measured, not inferred from test completion.
- Correctness, source/evidence coverage, and failure reporting are acceptance gates; context/token reduction is secondary.
- Local execution is never described as CI.

## 5. Scope

### In scope

- A transport-neutral scenario/harness library and CLI/script.
- Scripted provider fixtures and deterministic exact-output scenarios.
- Production headless/native protocol execution; ACP adapter when available.
- Local Eggpool exact-model profile for `mimo-v2.5` without fallback.
- Fault injection across provider, broker tool, child job, process, storage, checkpoint, heartbeat, worker, notification, transport, and daemon restart boundaries.
- Repeated cancellation, timeout, stall, duplicate, contention, and recovery runs.
- Correctness/evidence/context/performance benchmark corpus.
- Security review, static guards, docs, examples, and reusable skill.
- Final roadmap/registry/closure reconciliation.

### Explicitly out of scope

- Benchmark marketing claims against unrelated products.
- Committing credentials, private endpoints, or captured proprietary responses.
- Treating one live model run as deterministic closure evidence.
- Expanding mutation-capable program tools.
- Building the full ACP adapter if it is not otherwise scheduled.
- Optimizing by bypassing artifact, ledger, schema, or scheduler boundaries.

## 6. Required production changes

### Scenario and harness architecture

Create a reusable scenario format with versioned fields:

- workspace fixture and repository revision;
- provider/model/backend policy;
- agent/profile/prompt input;
- direct or programmatic route expectation;
- allowed tool contracts and limits;
- foreground/background mode;
- fault plan and random seed;
- expected terminal class, result schema, evidence requirements, call count bounds, and deadline;
- resource/convergence assertions;
- redaction policy.

Provide one runner that can drive:

1. in-process scripted provider for deterministic unit/integration evidence;
2. daemon native protocol or headless exec path;
3. ACP transport adapter when available;
4. local Eggpool provider connection for operational model behavior.

All modes must query projections/RunStore/artifacts through public production interfaces rather than inspecting private runtime memory for closure assertions.

Suggested layout:

```text
tests/tool_program_scenarios.rs
tests/tool_program_chaos.rs
tests/tool_program_resource_convergence.rs
tests/tool_program_model_behavior.rs
scripts/e2e/tool_program_harness.py
scripts/e2e/tool_program_scenarios/
scripts/e2e/fixtures/
.opencode/skills/tool-program-harness/SKILL.md
```

The exact language of the external runner may be Python, but it must remain a client of CodeGG rather than an alternate executor.

### Eggpool exact-model profile

- Read endpoint and credential from documented environment/secret references, for example `CODEGG_EGGPOOL_URL` and `CODEGG_EGGPOOL_API_KEY`; never print their values.
- Require explicit provider connection selection.
- Require exact model string `mimo-v2.5` and disable/fail on model fallback, alias expansion to pro, or provider substitution.
- Probe model discovery/identity before the scenario and record only redacted endpoint class, provider connection ID, exact model ID, date, and capability summary.
- Keep live runs optional/skippable when operational input is absent; report skipped truthfully.
- Separate live behavioral evidence from deterministic correctness tests.

### Fault-injection framework

Add deterministic injection points with typed outcomes at minimum:

- provider rate limit, unavailable, disconnect, malformed tool call, idle stream, auth/model failure;
- Tool Broker transient failure, schema mismatch, permission denial, oversized result, cancellation delay;
- child job enqueue ambiguity, runner failure, process timeout/stall, cancellation race;
- source/IR/manifest/checkpoint/artifact read/write failure;
- call reservation/completion failure;
- heartbeat loss, worker panic/drop, daemon generation restart;
- terminal result persisted before job completion and inverse ordering;
- notification create/claim/ack failure and duplicate terminal event;
- projection disconnect/replay/backpressure.

Support a configurable failure probability with closure runs at or above 10 percent for eligible transient boundaries. Also include deterministic single-boundary tests; probabilistic runs do not replace them.

Record seed, injected point, attempt/call/job IDs, expected recovery policy, observed terminal state, elapsed time, and resource baseline/convergence.

### Reliability assertions

For every scenario assert:

- one logical program identity;
- bounded attempt/call/job counts;
- completed-call execution count exactly one;
- terminal/recoverable state before scenario deadline;
- no active process/task/permit/workspace lease/notification claim remaining;
- parent notification count zero or one according to mode/policy;
- artifact and call-ledger integrity;
- no secret in logs/events/artifacts/projections;
- unrelated session/program continuity during target failure.

Add a finalizer that diagnoses leaked ownership rather than merely timing out.

### Evaluation corpus

Include representative tasks:

- inspect several files and aggregate findings;
- search/read/filter with dependent paths;
- compare Git status/diff/log metadata;
- run a bounded crate/build/test matrix;
- background verification while parent performs another read-only task;
- partial/incomplete continuation after budget exhaustion;
- restart during sequential and parallel calls;
- malformed/unsafe source rejection.

Compare direct versus programmatic routes on:

- correctness and structured result equality;
- source/file/test evidence coverage;
- model turns and provider requests;
- input/output/cached tokens where available;
- parent transcript bytes;
- nested call count and cache hits;
- wall latency and scheduler wait;
- raw artifact bytes and retained failure evidence;
- recovery behavior under faults.

Use medians and tail percentiles over documented repetitions. Do not set a token-reduction target that can be met by discarding evidence.

### Performance and bounds

Measure and enforce:

- compile/runtime overhead for no-op and small programs;
- maximum concurrent programs and calls under configured limits;
- memory/task/process growth and convergence;
- checkpoint/call-ledger write amplification;
- projection/notification queue pressure;
- cache size/hit/eviction behavior;
- large output/artifact handling;
- scheduler fairness against normal tests/builds/agent turns.

Any optimization must preserve raw evidence and exactly-once/replay semantics. Batch ledger writes only where crash boundaries remain unambiguous.

### Security and static review

Perform focused review of:

- restricted-language escape resistance;
- caller/manifest/tool-contract enforcement;
- path/workspace authorization;
- cache/replay authorization;
- provider-hosted argument trust;
- source/artifact/notification/projection redaction;
- denial-of-service through source, AST, IR, loops, parallelism, child jobs, schemas, output, artifacts, and queues;
- credential handling in harness and diagnostics.

Add semantic guards for:

- no direct `Tool::execute` outside broker production boundary;
- no direct Python/build/test execution outside scheduler executors;
- no program-callable mutating/direct-only tools;
- no unbounded channels/tasks in program/notification paths;
- no committed Eggpool credentials/endpoints;
- no TUI-only closure scenarios.

### Prompt/model behavior validation

Through scripted and live models verify:

- agent chooses Tool Programs for predictable bounded aggregation/matrices;
- agent uses direct calls for semantic judgment, approvals, mutation, and final source validation;
- generated source declares a finite result and avoids unsupported syntax;
- invalid source diagnostics cause a bounded correction attempt, not an infinite regenerate loop;
- incomplete results lead to narrower continuation or explicit direct work;
- the agent does not manually poll background programs.

Bound model correction attempts and include a direct fallback after repeated invalid program generation.

## 7. Ordered work packages

### Work package A — Scenario schema and production-path runner

- Define scenario/result formats and deterministic scripted-provider driver.
- Add native/headless daemon client and public projection/artifact assertions.
- Add ACP adapter seam and shared corpus interface.

### Work package B — Fault injection and convergence probes

- Add named injection points and seed control.
- Build resource baseline/final probes for tasks, processes, permits, jobs, calls, leases, notifications, queues, and artifacts.
- Add targeted and 10-percent mixed-fault suites.

### Work package C — Eggpool model profile and behavior corpus

- Add ignored environment configuration and exact-model guard.
- Verify no fallback and record redacted operational evidence.
- Add bounded program-generation/correction/fallback tests for `mimo-v2.5`.

### Work package D — Correctness, context, and performance evaluation

- Implement direct/programmatic paired scenarios.
- Collect turns, tokens, transcript bytes, latency, calls, cache, artifacts, and evidence coverage.
- Add resource/fairness/load tests and identify safe defaults.

### Work package E — Security review, docs, skill, and static guards

- Complete threat-oriented review and adversarial corpus.
- Add `.opencode/skills/tool-program-harness/SKILL.md` with setup, deterministic mode, live mode, secret handling, scenarios, and evidence capture.
- Update all architecture/operator docs and guards.

### Work package F — Final reconciliation and closure

- Run focused, repeated, broad, and operational matrices.
- Create exact requirement-to-evidence matrix for M001–M010.
- Reconcile roadmap, registry, implementation statuses, docs, and accepted closure records.
- Do not close M009 or hosted capability without its own evidence.

## 8. Failure, cancellation, restart, and contention semantics

- Harness timeout is longer than but bounded relative to production program deadlines; it must diagnose the production terminal state rather than kill silently.
- Fault injection never disables cancellation, watchdog, schema, authorization, or scheduler limits.
- Live provider rate limits/unavailability are classified separately from CodeGG correctness.
- A failed live scenario may be operationally blocked while deterministic fixtures still pass; report both.
- Daemon restart scenarios use real process/store reopen where the environment permits, not only object reconstruction.
- Mixed-fault runs maintain at least one unrelated control program/session to detect global stalls.
- Repeated test runs must return resource probes to baseline within explicit tolerance; tolerance cannot hide monotonically growing leaks.
- Model invalid-program correction is bounded by a fixed small count, after which the agent receives a typed fallback instruction.

## 9. Compatibility and migration

- Harness artifacts and scenarios are versioned and remain useful for future ACP/provider adapters.
- Live Eggpool variables are optional and ignored by default test runs.
- Existing TUI behavior is not required for scenario execution.
- Native Tool Programs remain the baseline when hosted support is absent.
- Closure may document platform-specific unavailable evidence, but cannot claim cross-platform or ACP evidence that was not run.

## 10. Required tests

### Deterministic scenario tests

- all corpus tasks through scripted provider and production daemon path;
- direct/programmatic equivalence and evidence assertions;
- invalid source correction/fallback.

### Fault and recovery tests

- every named injection point individually;
- mixed transient injection at >=10 percent over documented seeds/repetitions;
- restart at each durable boundary;
- duplicate and ambiguous delivery/execution tests.

### Cancellation and contention tests

- cancellation in every program/call/child/notification phase;
- many concurrent programs plus normal builds/tests/agent turns;
- scheduler fairness and resource convergence;
- queue/backpressure and unrelated-session continuity.

### Security tests

- language escapes, manifest/caller bypass, path/cross-workspace access, cache/replay authorization, secret scans, result/artifact/schema bombs, provider malformed items, committed-secret guard.

### Operational model tests

- exact `mimo-v2.5` identity and no fallback;
- program generation for read aggregation and build/test matrix;
- unsupported syntax correction bound;
- background no-poll behavior;
- live failures recorded separately and redacted.

### Hosted equivalence tests

When M009 is closed, run shared scenarios through native and hosted backends using deterministic provider fixtures and compare normalized result/evidence/recovery behavior.

## 11. Required verification commands

```bash
cargo test -p codegg --test tool_program_scenarios
cargo test -p codegg --test tool_program_chaos -- --test-threads=1
cargo test -p codegg --test tool_program_resource_convergence -- --test-threads=1
cargo test -p codegg --test tool_program_model_behavior -- --test-threads=1
cargo test -p codegg --test agent_loop_harness
cargo test -p codegg --test projection_replay
cargo test -p codegg --test scheduler_cancellation
cargo test -p codegg --test scheduler_recovery
python3 scripts/e2e/tool_program_harness.py --mode scripted --scenario all
# Optional operational evidence; exact environment documented without values:
python3 scripts/e2e/tool_program_harness.py --mode eggpool --model mimo-v2.5 --no-model-fallback
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets -- --test-threads=1
```

If ACP is available:

```bash
python3 scripts/e2e/tool_program_harness.py --mode acp --scenario all
```

Closure must state exact commands, seeds, repetitions, skipped tests, environment, and whether evidence was local or CI.

## 12. Documentation updates

- `architecture/tool_programs.md` final architecture and operations
- restricted language and Tool Broker docs
- provider/OpenAI hosted docs if M009 is included
- session projection/notification docs
- testing/chaos/performance methodology
- operator troubleshooting and safe defaults
- `.opencode/skills/tool-program-harness/SKILL.md`
- repository overview/AGENTS references where architecture indexes require them

## 13. Acceptance criteria

1. The complete native Tool Program capability is usable through a non-TUI production path.
2. Every deterministic scenario converges to expected terminal/recoverable state with complete evidence.
3. At least 10 percent mixed transient fault injection over documented repeated runs produces no indefinite stall, duplicate completed call, duplicate logical parent notification, or unreconciled owned resource.
4. Cancellation/restart/contention tests prove task/process/permit/job/call/notification convergence.
5. Direct and programmatic routes meet correctness and evidence-equivalence thresholds; context/turn/token reductions are measured truthfully.
6. Exact Eggpool `mimo-v2.5` operational runs, when available, reject fallback and keep credentials/endpoints secret.
7. Agent behavior is bounded for invalid program generation and does not manually poll background work.
8. Security review and static guards find no programmatic path to direct-only/mutating tools or scheduler bypass.
9. All closure-bearing commands and unavailable evidence are recorded accurately.
10. No unresolved high or medium correctness, security, recovery, resource, or evidence finding remains.

## 14. Stop conditions

Stop and report rather than closing if:

- M008 is not strictly closed;
- a logical program can remain running beyond stall/deadline bounds;
- a completed call or parent notification can duplicate under restart/retry;
- resource probes show monotonic growth or unreconciled ownership;
- correctness/evidence regresses even when tokens improve;
- exact model selection cannot be verified or fallback cannot be disabled;
- credentials/private endpoints would need to be committed;
- ACP/live/CI evidence is unavailable but would otherwise be claimed;
- any high or medium finding remains.

## 15. Closure evidence required

Create `plans/closure/tool-programs/010-status.md` containing:

- implementation and reviewed-head commits for every included milestone;
- M001–M010 requirement-to-evidence matrix and status reconciliation;
- deterministic scenario inventory and outcomes;
- exact fault points, rates, seeds, repetitions, deadlines, and convergence counts;
- direct/programmatic correctness, evidence, turn, token/context, latency, cache, and artifact metrics;
- process/task/permit/job/call/notification/resource baseline and final measurements;
- security review and static guard output;
- Eggpool exact-model evidence or explicit operational blocker, with secrets redacted;
- ACP/native/headless/CI evidence distinguished truthfully;
- known limitations and severity-ranked findings;
- recommendation for subsystem closure or a named corrective plan.

## 16. Handoff notes

The repository intentionally constrains test concurrency because unconstrained Rust tests can create excessive threads, processes, and memory pressure. Preserve that policy in stress and closure runs. Prefer deterministic scripted evidence first, then live model behavior. Do not tune retries until faults disappear; closure requires bounded failure handling, not probabilistic success.
