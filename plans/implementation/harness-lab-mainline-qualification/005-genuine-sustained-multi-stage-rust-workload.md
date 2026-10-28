# Harness Lab Mainline Qualification H005 — Genuine Sustained Multi-Stage Rust Workload

Status: **blocked — one authorized live attempt was consumed and did not qualify; no further live attempt is authorized**. H004's authorization is consumed and is not reused.

Source roadmap: `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`.
Reviewed main baseline: `fed10ce13be7fd99a6d5aa5f8b6639ba98d007bc` (2026-10-27).
Predecessors: H001–H004 and the H003 offline native Rust/job fixture.
Long-term references: `plans/000-long-term-specification.md` §§16, 24, 26, 28–29; accepted ADR-0003 and ADR-0040.
Primary class: capability — qualify sustained, autonomous native Rust development through one CodeGG provider integration.

## 1. Objective

Measure whether one CodeGG native session can carry a substantial Rust change through dependent design, implementation, asynchronous validation, failure diagnosis, correction and final independent verification. This is a new workload with meaningful engineering stages; it is not the H004 fixture with an added wait, repeated prompt, artificial sleep or inflated timer.

The optional live run uses exactly `opencode_go/muse-spark-1.3-contributor`. It makes no model-ranking, upstream-provider-diversity or multi-adapter claim. The operator approved the one-run budget in §8 on 2026-10-27; execution remains gated on the offline implementation and verification below.

## 2. Workload: a dependency-free event-ledger workspace

Add a separately versioned autonomy case to `dev/harness_lab/fixtures/agent_autonomy_h005/cases.jsonl`, with its case body under `dev/autonomy_fixtures/sustained-event-ledger/{fixture,verifier}/` and H005 digests/resource metadata under `dev/harness_lab/fixtures/sustained_event_ledger.case.json`. Use the existing `autonomy_run.run_case` native ACP path for the live session. Leave the historical autonomy manifest, H003's `long_horizon_rust_job` fixture and all historical H004 evidence byte-for-byte unchanged. The candidate starts from an incomplete, multi-crate Rust workspace with a human-readable contract, representative sample input, public tests and a hidden verifier stored outside the writable lane. No task prompt contains the solution or hidden assertions.

The deliverable is a local event-ledger program. Its stages form a required dependency chain:

1. **Domain and ingestion (`ledger-core`).** Define typed records, stable IDs, validation errors with source-line attribution, and a streaming parser for the documented line format. Reject malformed required fields without losing the remaining valid records.
2. **Canonicalization and index (`ledger-index`).** Depend on `ledger-core`; normalize the documented aliases, deduplicate identical IDs, reject conflicting duplicate IDs with a useful diagnostic, and merge out-of-order shards into deterministic time-window summaries.
3. **Journal and recovery (`ledger-store`).** Depend on the preceding crates; append versioned records and checkpoints, replay them idempotently, and recover a torn final journal entry without accepting corruption in an earlier entry. Public and hidden tests cover restart and repeated replay.
4. **CLI and reports (`ledger-cli`).** Depend on the domain, index and store crates; implement ingest, summarize, inspect and export commands with stable ordering and documented error codes. A user-visible end-to-end path must read multiple shards, update the journal and emit a deterministic report.
5. **Integration and scale validation.** Process a frozen, deterministic corpus of at least 50,000 records across multiple out-of-order shards, including duplicates, malformed rows and a torn final journal record. Validate correctness against an independently computed hidden oracle and record runtime and artifact hashes. The corpus exists to exercise real parsing, merge, replay and reporting work; no busy loop or sleep may be added to stretch runtime.

Use only checked-in/local dependencies and locked offline builds. If the actual runner cannot build offline, the plan must either pin the required cached inputs before authorization or stop and revise the fixture. The hidden verifier, expected-output oracle and fixture digests remain outside the agent's writable roots.

## 3. Sustained autonomous sequence

One initial user objective asks the agent to deliver all five stages. There are no human approvals, hand-written follow-up prompts or proposer handoffs during the measured session. Record the same daemon/session identity across the staged work; if current instrumentation cannot establish continuity, report it as unavailable and do not claim continuation.

The planned progression is:

1. Inspect the workspace, contract, existing tests and fixture data; identify the crate dependency order before editing.
2. Implement the domain and canonicalizer, run focused checks, and repair at least one observed compile or contract failure before proceeding.
3. Implement index merge semantics and journal recovery. Submit a real CodeGG-owned `test`/build job using the private lane's `Cargo.toml`, lockfile and target directory.
4. While that job remains `running`, implement the CLI and integration path using ordinary native file/read/test tools. Prove overlap by observing the same job ID as running after at least two substantive source-edit or validation actions; do not poll in a tight loop or hold the agent idle.
5. Wait on the exact returned job ID, inspect its terminal output, diagnose any failure, make a bounded correction and submit a new job only after the first job is terminal. Never infer completion from elapsed time or start duplicate work to hide an ambiguous outcome.
6. Complete the integration and scale stage, recover from a separate behavioral failure (for example, merge-order or torn-tail handling), rerun the relevant check, then run the whole-workspace check and hidden verifier.
7. Continue autonomously through at least three distinct implementation/validation phases and two observed failure→diagnosis→correction→passing-check sequences. A naturally clean run that does not exercise both recovery sequences may solve the task, but is insufficient evidence for the H005 recovery objective.

Only product-supported job wait/output and same-session continuation paths may be used. Do not claim daemon-restart, ACP compaction or cross-process session recovery unless the live transcript and state actually prove that path. An asynchronous job that finishes before the agent has done other work does not satisfy the overlap requirement; revise the workload offline and seek new authorization rather than inserting a sleep.

## 4. Fixture quality and negative controls

- The initial source, required dependency chain, sample inputs and public checks are frozen before the live run. Hidden verification and expected-output generation are outside the writable lane and digest-checked.
- The verifier must reject: skipped crates/stages, altered/deleted tests, fabricated reports, dropped malformed-row accounting, order-dependent shard merging, conflicting-ID acceptance, duplicate replay, corrupt interior journal acceptance, incomplete CLI integration and output that differs from the independent oracle.
- At least one deterministic invalid input produces a named, expected failure and a later valid input succeeds. Failure truth, tool terminal truth and job terminal truth are reported separately.
- The agent must not be able to write the hidden verifier, benchmark controller, evidence destination, budget receipt or source fixture snapshot. No network access or unapproved provider fallback is part of the case.
- Run a credential-free scripted native ACP trajectory first, including the two recovery sequences, exact job-ID overlap/wait, hidden-verifier rejection controls and cleanup. It must not contact OpenCode Go.

## 5. Measurement contract

Version the new scenario and receipt separately (schema v3); preserve H003/H004 manifests, ledgers and receipts byte-for-byte. Extend the live ACP projector to retain only per-call elapsed timestamps and narrowly parsed job ID/action/status fields from `test` and `job` results. Do not persist `rawOutput`, tool arguments or transcript bodies. The model-facing `job` result must name its own job ID so both the agent and the observer can prove exact-ID continuation. Add a disk-floor watcher to the live autonomy runner; an unreadable disk state or a breached 10 GiB floor cancels the session and makes the run unqualified. Record only bounded, secret-safe evidence:

- case ID, schema, fixture/contract/verifier digests, source revision and candidate binary digest;
- session and turn identities where exposed; completion of each dependent stage and its check result;
- exact provider request admissions, approved route, wall-clock interval, provider usage and reported cost (or explicit unavailable reason);
- tool-call hashes/statuses and failure classes; each background job ID, submitted/running/terminal observations, waits, output reads, retries and whether other work occurred while it ran;
- event-observer health and gaps, independent-verifier result, corpus size, cache/target placement, build/test duration and available resource measurements;
- cleanup proof for daemon, job descendants, sockets, worktree, Cargo target and private lane.

Do not persist raw credentials, full provider prompts/responses, or unrelated transcript data. Missing instrumentation stays `unavailable`/`unknown`, never zero. A gap or ambiguous job terminal makes that part of the run unmeasured.

## 6. Sustained qualification threshold

This task is intended to occupy the agent with engineering work for **30–60 minutes**, with 45 minutes as the target. Sustained evidence requires at least 30 minutes from the first admitted provider request to the agent's terminal response, at least three completed dependent stages, both recovery sequences, and the asynchronous overlap/wait requirements. Job runtime may contribute to wall-clock duration but does not count as agent work while the agent is idle; record the overlap interval separately.

Do not pad a fast completion. If a correct solution ends before 30 minutes, report successful task completion but leave sustained qualification unmet. If the task reaches its time/request cap unfinished, stop and report the exact state. Neither outcome authorizes a second attempt.

## 7. Implementation and verification packages

1. Inventory current native tools, job API, same-session turn lifecycle, request-admission receipt, observer health and cleanup behavior. Confirm exact supported CLI invocation from `--help`; do not invent flags.
2. Author the new multi-crate fixture, frozen public contract, hidden oracle/verifier, independent test corpus and schema-v3 receipt contract.
3. Add scripted native ACP correct and negative trajectories. Prove each named rejection and both failure-recovery sequences; prove real job overlap by exact job ID.
4. Run focused Harness Lab tests, fixture-immutability guards, formatting, strict Clippy, `scripts/verify.sh quick`, doctor/smoke and applicable workspace tests. Keep the 50k-record workload and any long compile outside routine CI if their runtime is unsuitable; CI must still exercise the deterministic fixture and verifier quickly.
5. Only after the operator approves §8, build the exact candidate binary, recheck free disk, pin one approved `opencode_go` route, freeze the budget receipt and run one live session. Capture exact-head hosted CI for the resulting PR/merge state before claiming integration.
6. Write an H005 closure record with pass/fail/unmeasured states, requirement-to-evidence links, exact caps consumed, resource cleanup and non-claims. Update the registry/roadmap only after the offline fixture and any authorized live gates are actually satisfied.

If product-supported same-session continuation, job identity, observer health or cleanup cannot be proven, stop and hand off that specific gap to its owning subsystem. Do not create a fake adapter, synthetic long wait or weakened verifier to finish H005.

## 8. Fresh live-run authorization

H004 authorization is consumed. H005 has its own one-run approval, separate from H004:

| Cap | Proposed value |
|---|---:|
| CodeGG provider route | `opencode_go/muse-spark-1.3-contributor`; verify current route availability immediately before use |
| Outbound provider requests | 300 maximum across credential preflight and H005, enforced at the provider boundary and recorded per request; 2 credential-probe requests plus 1 conservatively reserved catalog request used; the case budget declared 297, but the live lane did not receive the provider-boundary cap |
| Wall-clock deadline | 60 minutes maximum, from run start; no automatic extension or retry |
| Minimum free disk | stop and cancel if free space is unreadable or below 10 GiB |
| USD cost | provider may report no cost; no USD ceiling is claimed |

**Authorization received 2026-10-27:** the operator approved one H005 live attempt on `opencode_go/muse-spark-1.3-contributor`, capped at 300 provider requests across qualification and the attempt, 60 minutes, and stop below 10 GiB free disk; provider USD cost may be unreported. This authorization is consumed. Credential probes used two requests and one more was reserved for the populated catalog whose original admission record is unavailable. The case manifest declared 297, but the live lane did not receive `CODEGG_HARNESS_MAX_PROVIDER_REQUESTS`; the attempt logged zero boundary admissions despite 65,608 tokens, so its actual outbound request count is unknown. Do not start another run under this authorization.

**Credential preflight result (2026-10-28):** the operator authorized testing both stored account records. Each was exercised separately through CodeGG's standalone `opencode_go` adapter using the approved Muse Spark model; each returned a successful completion with one provider request admitted. The secret values and response bodies were not printed or saved. The non-secret receipt is `plans/closure/harness-lab-mainline-qualification/evidence/005-credential-preflight.json`. These two probes consume 2/300 approved provider requests. Historical per-account use remains unavailable. The operator selected the newer account (`newer stored account`). A read-only local-daemon connection listing found an active `opencode_go` connection with that ID, a populated 26-model catalog, and a four-character masked-credential match to the selected stored account. The check did not create or replace a connection. Reserve one catalog request conservatively because the connection creation request was not captured; the live case cap is therefore 297/300. Evidence: `plans/closure/harness-lab-mainline-qualification/evidence/005-selected-route-preflight.json`. The sole H005 live attempt ran for 614,218 ms (10m14.218s) and ended normally without solving the case. The hidden verifier found one of two required recovery cycles; seven background test jobs were submitted but their projected status values were null, with no exact-ID running/terminal wait proven. The provider receipt reports zero boundary admissions despite 65,608 tokens, because the lane did not receive `CODEGG_HARNESS_MAX_PROVIDER_REQUESTS`; actual outbound request count and enforcement are unknown. The 30-minute threshold was not reached, the authorization is consumed, and no second live attempt is authorized. See `plans/closure/harness-lab-mainline-qualification/evidence/005-live-attempt.json`.

**Implementation and qualification update (2026-10-28):** H005 implementation is pushed on `plans/harness-lab-h005-sustained-workload-20261027`. PR #211 exact head `68020e74fc6cddfc7821224f1f495f5e6c8ec177` passed hosted CI run [40601163463](https://github.com/dbowm91/codegg/actions/runs/40601163463), including strict Clippy and the hosted workspace sweep. Local Harness Lab tests, the Python native-ACP safety/cleanup suite (18 tests), default workspace nextest (16,650 tests), and strict Clippy passed. The local feature-gated library suite had 7,828 passes, four ignored and one failure in an existing interactive-terminal screen-clear test; that test passed immediately when rerun alone. This is not recorded as a clean local feature-suite pass. The full wrapper was deliberately stopped at the 10 GiB disk guard during an earlier build; no claim is made that one local `verify.sh full` invocation completed cleanly. Both stored credentials subsequently passed one-request CodeGG completion probes; two requests now count against the approved 300-request total.

The H005 fixture, hidden verifier, disk watcher, privacy-bounded job projection and manifest isolation are implemented. The live session ended after 614,218 ms without solving the task; the verifier observed one recovery cycle and the job projection had seven submissions with null status. The lane did not receive `CODEGG_HARNESS_MAX_PROVIDER_REQUESTS`, so the 65,608-token session has no reliable request count. `SubjectSpec` now accepts the trial policy cap explicitly and `autonomy_run` supplies it, with focused tests passing. The sanitized exact PR head `26f2d8030d6d84bfb26a25fa006bc486202d08d8` passed hosted CI run [40632587553](https://github.com/dbowm91/codegg/actions/runs/40632587553). The sole live authorization is consumed. See `plans/closure/harness-lab-mainline-qualification/005-status.md` and `evidence/005-live-attempt.json` for the result.

## 9. Closure and handoff

H005 closes as **sustained-qualified** only when the offline verifier/negative controls, native job overlap, both recovery sequences, minimum-duration threshold, request/time/disk enforcement, exact evidence integrity and cleanup all pass. A correct short run, cap stop, event gap, unknown continuation, incomplete cleanup or unavailable cost is recorded accurately and cannot be recast as a sustained reliability pass. A discovered production defect gets a narrowly owned corrective plan; H005 does not absorb unrelated architecture work.

Closure path: `plans/closure/harness-lab-mainline-qualification/005-status.md`.
