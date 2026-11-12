# S018 — Muse Spark bounded continuation requalification

Status: **unqualified after 40 starts; current live-run authorization exhausted**

## Objective

Continue the H007 live qualification after S017 using `opencode_go/muse-spark-1.3-contributor`, and close the measured early-end gap. A normal ACP `end_turn` with an unfinished public continuation record must trigger a bounded follow-up prompt in the **same CodeGG session and run**. The agent resumes from its durable workspace state and continues dependent engineering work; it does not repeat completed phases or add idle time.

The corrective does not change the workload acceptance criteria, hidden verifier, single provider adapter, model route, or 30–60-minute substantive horizon. The run stops when the public continuation is complete, when a bounded stop condition fires, or when the verifier and all qualification gates pass.

## Authorization

The user authorized up to **40 sequential live runs** on 2026-11-06. Only `opencode_go/muse-spark-1.3-contributor` is authorized. Attempts 1–10 used 750 provider requests and 900 native calls per run; attempt 11 onward uses 1,200 provider requests and 1,600 native calls per run after attempt 10 reached both limits without a passing verifier. Every run retains the 3,600-second hard stop, 10-GiB free-disk floor, and cancellation on a 300-second native-tool activity stall, operator request, sensitive-data exposure, disk-floor breach/unreadable disk state, or loss of request-admission observability. Cost has no USD cap in this authorization; record it when available and otherwise record `unknown`.

One run is one H007 autonomy case. Same-session follow-up prompts (maximum forty) are not separate runs and share the run's request, wall-clock, disk, and activity limits. The runner stops earlier on completed continuation, no native progress, another terminal reason, or any original run cap. The append-only run ledger is `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-muse-authorization-20261106.json`. S017's consumed authorization and receipts remain immutable.

## Evidence and diagnosis

S017 attempt 20 ended normally after 10m07s active, with 31 requests and 42/42 paired native tool events. Its exact-ID job lifecycle was absent; the public continuation file named no completed phases and left `next_phase: versioned_records`. The independent verifier ran zero tests and returned a setup error. This is evidence of incomplete task execution and of the runner stopping after one normal turn boundary; it is not evidence of an ACP event gap, disk failure, native activity stall, or successful task.

S017 attempt 17 previously demonstrated a 48m46s sustained horizon, exact-ID productive overlap, 52 repairs, and complete event capture, while failing two of five verifier assertions. The new live objective must establish both task correctness and all horizon gates in one run.

S018 attempt 1 ended normally after 26m07s of active work, 194 provider requests, and eight follow-up prompts. Its exact-ID job lifecycle and productive overlap passed; two repair cycles were recorded; all 241 tool starts had matching terminals; there were no dropped events, no activity gap over five minutes, and no disk-floor or stall stop. It did not qualify: the continuation remained incomplete, the independent verifier reported one failure among five tests, and active work stopped 3m53s short of the 30-minute minimum. The runner stopped specifically at its eight-prompt limit while progress was still being made. No verifier output details were inspected. Attempt 1's safe receipt summary is `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-01-summary.json`.

S018 attempt 2 lasted 34m28s active and used all 750 admitted provider requests over 35 same-session follow-ups. Exact-ID productive job overlap, five repair cycles, 922/922 native event pairing, activity-gap, and disk gates passed. It remained unqualified: only six of fifteen phases were recorded, the independent verifier reported one failure among five tests, and the terminal category was `provider_api` with request-cap causality unconfirmed. The agent used `job wait` 22 times and did not dispatch the required single read-only subagent. It made 922 native calls against the declared 900-call case budget. The native event observer was complete, but the separate 1-MiB causal trace dropped 333 events. The provider-reported allowance was $0.004163; separately billed USD was not reported. Private verifier output was not inspected. Safe summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-02-summary.json`.

## Attempt 3 finding and attempt 4

Attempt 3 reached the 3,600-second wall-clock deadline with only three of fifteen public phases recorded. The task was incomplete. It produced 359/359 paired native tool events, no event gaps/drops, a 168.6-second maximum activity gap, six repair cycles, and no disk/stall stop; exact-ID job running/terminal observation and productive overlap were absent. The independent verifier timed out after 1,016ms with no test count or exit code, so it yielded no test outcome. The model made five same-session follow-ups and used 270 provider requests. Provider-reported allowance was $0.194059; billing was separately unreported.

The run exposed a runner deadline bug: `_turn_timeout` was computed once before the follow-up loop. Subsequent prompts therefore used the remaining case wall clock without maintaining the 300-second verifier reserve. The corrective recomputes the bounded prompt timeout on every iteration and ends model prompting when only that reserve remains. Focused regression tests assert the deadline shrinks across follow-ups and reaches zero at the reserve. Attempt 4 uses the unchanged task/verifier and qualification criteria, manifest v33 (`9b65f1885ade58bcb56685b733adf89684d0f9fe6fb51c32f52aeec8c760c24b`), and the existing ARM64 binary/helper pair.

## Attempt 4 outcome

Attempt 4 ran 41m58s (41m50s active), used 737/750 requests and exactly 900/900 declared native calls, then stopped after a normal `end_turn` and `no_native_progress` at 13/15 phases (`impact_view_api` remained next). The continuation validator successfully rejected an earlier false `next_phase:none` at phase six and resumed the same session. The verifier ran zero tests and returned one setup error after 240,448ms; no private verifier output was inspected. The projector identified `job_lifecycle_missing`; public records showed crate-level jobs but did not evidence the required release workspace job/two delta edits.

The event trace remained complete (2,038 events; 900/900 starts/terminals; zero gaps or drops; largest observed tool-start gap 122,762ms). Disk and 300-second stall guards did not trigger. The route reported $0.000374 allowance; billed USD was not separately reported. One `impact_sequence` child survived runner exit as an orphan under PID 1 and was terminated after recording its identity. This triggered the source-level shutdown investigation summarized below. Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-04-summary.json`.

## Corrective scope

- Keep one ACP session and one cumulative run budget. On `end_turn`, use the bounded continuation validator; if the public file is unfinished or does not prove the required completed-job continuation, issue a concise follow-up prompt and resume from `CONTINUATION.md`.
- Stop following up when the continuation record matches all fifteen required phases, a completed observed job, and productive work afterward; when no native tool progress occurs; at forty follow-ups; at the wall-clock cap; or on another terminal reason. The forty-prompt ceiling replaces the S018 attempt-1 cap of eight, which stopped the run before its 30-minute minimum despite continued progress. No waiting, synthetic work, or idle duration.
- Recompute remaining wall-clock time before each prompt. Keep the existing provider-boundary request cap, disk-floor watcher, and native-tool stall watcher active for the whole session.
- Record follow-up count and stop reason; preserve complete event pairing across turns. The causal trace is raised to 4 MiB after its 1-MiB capacity dropped 333 events. Do not expose provider response bodies or hidden-verifier output.
- Keep the workload and verifier unchanged. The v32 context lists the validator's fifteen canonical phase labels, names only the remaining phases at each continuation, requires exact job fields and a read-only subagent, forbids `job wait`, avoids duplicate workspace sweeps, and states the remaining 900-call fixture budget. Exceeding the declared call budget makes the result unqualified; it is not currently a live cancellation threshold.
- Preserve the request-budget distinction end to end. Provider errors already classified internally as `request_budget` now remain visible as redacted `provider_request_budget` terminals instead of collapsing to `provider_api`; this improves attribution but does not retroactively establish attempt 2's cause.
- Do not widen daemon, scheduler, ACP event projection, or provider architecture absent new reproducible evidence.

Source review after attempt 4 found a separate, narrowly scoped harness
lifecycle defect consistent with the orphaned `impact_sequence` child. The
production socket shutdown path cancelled the listener but never called the
existing scheduler shutdown routine; `CoreDaemon` also discarded the scheduler
loop `JoinHandle`. Runtime teardown could therefore drop the managed Cargo
handle without running process-group cleanup. The corrective retains the
scheduler loop handle, calls `JobScheduler::shutdown(ImmediateInterrupt)`,
waits for executor cleanup and joins the loop before dropping the daemon. The
same shutdown delegate is wired to socket, stdio, and standalone server exits.
This explains the orphan candidate only; it does not explain the incomplete
13/15 task or the verifier setup error. The daemon regression and next live
confirmation are pending.

## Qualification and stop

A qualifying run must have all of the following in the same run: independent verifier pass; normal completed task; at least 30 minutes of substantive active development and no activity gap above five minutes; exact-ID background job observed running and terminal with productive work during overlap; at least two failure/repair cycles; corrected continuation evidence; complete native event capture; and no provider-cap, wall-clock, disk, or stall violation. A task that passes before 30 minutes remains a task pass but does not qualify the horizon.

Run sequentially. Each started live run consumes one of the 40 authorized slots. Stop at the first fully qualifying run or when all 40 are consumed. Do not claim success from a model's `end_turn`, phase note, or test summary without the independent verifier and the full gate set. Never inspect the hidden verifier source or its output tail.

## Initial revision

Initial manifest: `dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v30/cases.jsonl`
SHA-256: `3fffe1cf65e3c7f3c5d377dfe39cbc2f43960fc78cad5fc2b26f447725926c7c`
Initial binary/helper pair: CodeGG `9982e2d4dd10221854c727285da43fd07d6df83a71fa29d1082e094551f14a14` (arm64), sandbox helper `f4eca87c6cd967c334d9add501ba6cd315079f9d0c78ff3f9f04947b70eaedcb`. Verify the actual hashes and native architecture before each run.

Attempt 2 manifest: `dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v31/cases.jsonl`
SHA-256: `73673eb902d95f3a10edbcd3408226553010f54a23bffb927b3572fbb227d283`

Attempt 3 manifest: `dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v32/cases.jsonl`
SHA-256: `084a889ccd8fd59757901092c11bf554eccae8a0c3d6f3bd165589bdea16b8c1`


## Attempt 5 and prompt corrective

Attempt 5 passed the generic exact-ID running/terminal and productive-overlap checks, with complete telemetry, healthy disk and activity-gap measures, and 311/750 requests. It ended at 54m55s active with six of fifteen phases, zero measured repair cycles, and two failures among five verifier tests. The designated priority release workspace job was observed only after it was already terminal, so the requested priority-job overlap was absent. The verifier output remains sealed. This was incomplete model execution plus verifier-detected defects, not a native job-observation, ACP, daemon, scheduler, disk, or provider-cap failure.

The v33 prompt carried stale run-by-run diagnostics and repeated/contradictory instructions. Attempt 6 therefore uses manifest v34 as a prompt-only corrective: it starts from the fresh fixture state, requires both untouched seeded package tests to reach a failed terminal before any Rust source edits, then requires same-command completed repair jobs; it gives the fifteen phase definitions and priority/final test order once, and preserves the same task, verifier, route, horizon and budgets. This is a diagnostic corrective, not a claim that the task or verifier issue is resolved.


## Attempt 6 corrective: UTF-8 safe broker truncation

Attempt 6 stopped at 19m54s with a native `TurnFailed` categorized `native_failure_unspecified`. It used 179/750 requests, 229 native tool calls, and ended with one unpaired start. The 300-second stall and disk guards stayed clear. It recorded two phases and five repair cycles but no exact-ID productive job overlap; the verifier ran five tests and reported one failure.

A content-safe scan classified the corresponding agent-loop log entry as a panic and located it at `src/tool/broker.rs:1370:27`. Source inspection found `String::truncate` on an arbitrary byte cap in artifact registration. The same unsafe truncation existed in total-output validation. The corrective now moves the cutoff backward to a UTF-8 character boundary in both paths. Regression tests cover both paths with `abcé` truncated at byte four and verify the artifact retains the original byte length. Formatting and the focused tests passed; workspace quick verification and rebuilding the ARM64 live binary remain before attempt 7. This is a concrete broker defect, distinct from the incomplete task, missing exact-job overlap and verifier assertion failure.

The v34 symlink probe resolved to a dangling path. v35 will use the correct relative path to the private metadata target so the probe actually tests containment; it will keep the no-retry rule and record the refusal without reading the target contents. The priority workspace-job fields will also be reserved until after `catalog_bundle`, so earlier package tests cannot be misreported as the priority run.


## Attempt 7: long horizon and exact-job sequencing

The v35 run survived 54m53s active without another broker panic. Its event trace and native tool pairing were complete, the activity gap and disk stayed healthy, and the corrected symlink probe was refused. It reached eight phases and recorded five repairs, but remained incomplete and the independent verifier reported two failures. The priority workspace job was terminal at first observation. Another exact job reached `running` and then `completed`, but the model made a second status call immediately; no productive edit occurred between them. The job lifecycle observer is working, while the agent-facing procedure fails to exploit active work.

Attempt 8's prompt corrective will require the native sequence explicitly: submit one still-needed background test, immediately query the exact ID, and if it is `running`, make the next tool call a substantive `edit`/`apply_patch` on the pending stage (no status/output/task call between); then poll that same ID. Keep the priority job record exclusively for the post-bundle release workspace sweep. This is a narrow instruction corrective, not evidence of a scheduler or ACP fault.


## Attempt 8 — unqualified; task artifact and overlap timing

Attempt 8 completed all fifteen implementation phases in 33m23s active, with eight repair cycles. All 667 native tool calls had matching terminals, the 1,450-event trace was complete with no drops, the maximum observed activity gap was 122.4s, minimum free disk was 73,347,559,424 bytes, and no request, wall-clock, stall, or disk cap fired. The run used 550 provider requests and ended normally. The verifier ran five tests and reported one failure; its private output was not inspected.

The run did not qualify. The horizon gate reported `job_lifecycle_missing`. Trace inspection showed that the model delayed both candidate jobs until `workspace_audit`, after the fifteen Rust implementation phases were complete, and repeatedly polled each running job without a productive implementation edit. The observer captured the native job lifecycle; the model did not use the active interval. The final public workspace also lacked the task-required `PUBLIC_ACCEPTANCE_AUDIT.md`. That omission is directly evidenced; it may explain the verifier failure but is not asserted as its proven cause. Safe receipt summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-08-summary.json`.

Attempt 9 is a prompt-only corrective. It moves the overlap sequence to the `catalog_delta` → `catalog_merge` transition while merge source is pending, names the exact next write to `fixture/catalog-merge/src/lib.rs`, forbids consecutive status polls, and keeps the priority release job separately labeled. It strengthens `workspace_audit` to require the public acceptance audit before the final workspace sweep and before `next_phase: none`. It does not change the verifier, workload contract, provider, or runtime architecture.

Manifest: `dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v37/cases.jsonl`
SHA-256: `2c1d2d275a1f3ad140671ea08c7351aebf25da5c042ab6f32ee9dbfeb37c42d7`


## Attempt 9 — unqualified; dependency-index timeout

Attempt 9 lasted 54m59s total / 54m52s active and used 246/750 provider requests and 312/900 native calls. It completed only `versioned_records` and `durable_import`; `dependency_index` remained next. The run ended at the runner's 55-minute verifier reserve (`turn_timeout`), with no disk, request, activity-stall, event-gap, or wall-clock hard-cap trigger. The independent verifier ran zero tests and reported one setup error because the continuation was incomplete; this is not a test assertion result.

The overlap corrective worked: the exact-ID running/terminal and productive-work-during-job gates passed. Three repair cycles were measured. All 312 tool starts had terminals, the 687-event trace was complete, the maximum activity gap was 155.7s, and minimum free disk was 74,513,911,808 bytes. USD route allowance was $0.158924; separately billed USD was not reported.

Public `.codegg/test-runs/index.json` shows `cargo test -p catalog-index` failed once and timed out twice with `TimeoutNoOutput` while eight tests passed; subsequent `--lib` runs passed six tests. The public scale integration test builds 250,000 nodes. In the attempt's public `catalog-index/src/lib.rs`, `Index::cycles` clones each node's BTreeSet and allocates neighbor vectors repeatedly. This is the best-supported explanation for the long `dependency_index` phase. The same continuation also placed an early repair-job ID in `job_id_priority` before `catalog_bundle`.

Attempt 10 is prompt-only and targets the evidence-backed phase bottleneck: use sorted integer node IDs and vector-backed adjacency/cursors for iterative SCC traversal, then run the focused 250,000-node integration test before marking `dependency_index` complete. The prompt requires any failed/timed-out integration test to be diagnosed before retry, and keeps priority job fields pending until the post-bundle release job. It does not alter the workload's graph size, verifier, or harness architecture.

Manifest: `dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v38/cases.jsonl`
SHA-256: `318dc19be79fec485fc0b21add0c98079dd2fe44e5460fad36fbbff2165cbb53`


## Attempt 10 — unqualified; request/tool ceilings and continuation regression

Attempt 10 used v38 and lasted 41m19s total / 41m13s active. It consumed 750/750 provider requests and recorded 902/900 native calls, then terminated at the provider-request budget. Duration, exact-ID running/terminal, productive work during the overlap, seven repair cycles, paired tool events, complete causal trace, activity gap, and disk gates passed. The maximum activity gap was 18.9s; the trace had 2,008 events and no drops; minimum free disk was 84,490,006,528 bytes.

The public continuation briefly recorded all fifteen phases and `next_phase: none`, then regressed before shutdown to three phases with `provenance_queries` next. The exact overlap ID was not preserved in the final top-level `job_id`, so continuation was unconfirmed. The independent verifier ran five tests and exited 1; the task outcome was inconclusive. This is not evidence of a job-observation or event-telemetry failure: those gates passed. Safe summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-10-summary.json`.

Attempt 11 is the v39 corrective. It raises only the per-run request and native-call ceilings to 1,200 and 1,600, respectively, while retaining all prior time, disk, stall, route, and task limits. It explicitly requires the `task` tool's native `spawn` action with the read-only `explore` agent and exact Task/Run IDs. It also makes the final overlap `job_id` binding explicit and requires monotonic continuation updates and a final read-back. The workload and verifier are unchanged.

Manifest: `dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v39/cases.jsonl`
SHA-256: `7a3fe55fa4f7bf66bbb20e4c350df9d750f9471f8040fb7f0259b3bda0cf5eae`


## Attempt 11 — unqualified; task-action and phase-projection mismatch

Attempt 11 used v39 and ran 54m59s total / 54m56s active before the runner stopped agent work at its planned 55-minute boundary to reserve five minutes for verification. It used 340/1,200 provider requests and 429/1,600 native calls. The request/call caps, 300-second stall gate, disk floor, and event observer did not fire: all 429 starts had terminals, the 917-event causal trace was complete with no drops, the maximum activity gap was 121.5s, and minimum free disk was 84,469,231,616 bytes.

The generic exact-ID job lifecycle, productive work during that job, and five repair cycles passed. The task did not: the final public continuation listed only `versioned_records`, left `durable_import` next, left the overlap ID pending, and had no Task/Run IDs. The trace and nine task-result artifacts show eight typed errors saying a group-scoped workstream action required `group_id`, plus one lookup for a nonexistent Task ID. No child was spawned. Public test metadata shows repeated unchanged `catalog-format` runs and no Rust source edits after 07:28. The independent verifier ran zero tests and reported one setup error; no verifier output was inspected.

Two harness projection mistakes contributed to the bad continuation: the follow-up classifier treated any `task` call as a dispatched child, so it suppressed the explicit spawn reminder after failed board/workstream calls; and the phase reader accepted only block-list YAML, while the live file used `completed_phases: [versioned_records]`, so the next prompt could misreport that progress as zero phases. The run reached `turn_timeout` at the 55-minute verifier reserve with eight same-session follow-ups; the provider request ceiling was not causal. Provider allowance was $0.202968; separately billed USD was not reported. Safe summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-11-summary.json`.

## Attempt 12 — unqualified; provider timeout before implementation

Attempt 12 used v40 and terminated after 6m57 with the typed `provider_timeout` failure, 36/1,200 provider requests and 50/1,600 native calls. Its 117-event trace was complete with no dropped events; all 50 starts had terminals; no request/call cap, activity-stall, disk, or hard-time limit fired. The two seeded failure/repair cycles passed, but the task did not reach `versioned_records`, the exact-ID overlapping job, or the required child checkpoint. The independent verifier ran zero tests and reported one setup error, so task outcome is inconclusive. The receipt does not identify a lower-level upstream/network cause. Provider allowance was $0.011183; separately billed USD was not reported. Safe summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-12-summary.json`.

Attempt 12's two Task-tool calls were the fixture's requested negative-ID boundary probes, not attempted delegation. This exposed a wording conflict: the prompt asked for an invalid-ID probe and later said not to use guessed IDs without distinguishing a negative test from child lifecycle evidence. Attempt 13 uses v41 to state that distinction. It keeps the probe separate and once-only, requires returned IDs for a real spawn, and otherwise preserves the task, verifier, route, request/call caps, and resource bounds. The S018 phase parser and typed spawn projection fixes remain in place. The H007 focused suite passes; the Rust ACP regression passed on the pinned ARM64 toolchain; attempt 13 will reuse the verified ARM64 binary/helper.


## Attempt 13 — outcome and corrective diagnosis

Attempt 13 used v41 and stopped at the planned 55-minute verifier reserve after 54m59s. The agent completed eight of fifteen phases (`catalog_replication` next) and did not complete the requested task. The horizon measured over 54m active, exact-job running/terminal and productive overlap, three repair cycles, complete paired trace (1,140 events; no drops), 105.226s maximum activity gap, and a 59,543,146,496-byte minimum free-disk level. It used 442/1,200 provider requests and 517/1,600 native calls. The verifier ran five tests and exited 1; output remains sealed. The task outcome is inconclusive because the agent timed out. Safe summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-13-summary.json`.

The exact telemetry defect was ID-shape mismatch: all nine native task spawns returned UUID Task/Run identifiers, while `_safe_task_spawn_ids_present` recognized only historical `agt-`/`agr-` prefixes. The false negatives triggered repeated child attempts. The recognizer now accepts bounded opaque identity tokens and tests UUID, prefixed, missing, duplicate, and oversized forms. Attempt 14 also clarifies that all fifteen prompt-defined phases are normative although the frozen original contract has fewer phase entries; the frozen inputs and hidden verifier remain unchanged.

## Attempt 14 — unqualified

Attempt 14 used v42 and ran through the 55-minute work boundary. It recorded five of fifteen phases. The independent verifier ran five tests and exited 1; private output remains sealed. Duration was 54m59s with 54m47s active work. Three repairs, 451 paired native calls, a complete 938-event trace, a 108.739-second maximum gap, and the 10-GiB disk floor all passed; no request/call/time/stall/event/disk cap fired. It used 358/1,200 requests and 451/1,600 calls. Exact-ID running/terminal and productive overlap were not proven; continuation and task outcomes were inconclusive. Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-14-summary.json`.

The result projection collapsed lifecycle and productive overlap into one false field, preventing exact diagnosis. The projection now preserves each observation separately while the gate still requires both. The continuation prompt distinguishes an already-running job, lifecycle with no productive edit, no observed lifecycle, and a satisfied overlap. The next workload prompt moves the overlap sequence earlier so it occurs before the model reaches the later delta/merge phase.

## Attempt 15 — unqualified

Attempt 15 used v43 and reached the 55-minute work boundary. It recorded five of fifteen phases. The verifier ran five tests and exited 1 without timing out; private output remains sealed. The 54m55s active span, exact-ID running/terminal, productive overlap, three repairs, paired native calls, complete 1,166-event trace, 190.138-second maximum gap, and 10-GiB disk floor passed. It used 403/1,200 requests and 554/1,600 native calls. The task was inconclusive, continuation was incomplete, and `catalog_bundle` was next. Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-15-summary.json`.

The public overlap label pointed to an early repair job, but the independent native trace proved a valid exact-ID test lifecycle and productive work during that interval. This confirms the observer works and makes the public note non-authoritative. One read-only child remained active until parent shutdown, with 42 task-control calls. The next corrective gives follow-ups the exact public acceptance detail for the next missing phase and requires an implementation edit plus assertion-bearing test before ending. It defers child creation until after the five starter-contract phases and limits status/get polling while the child runs.

## Attempt 16 — completed unqualified

Attempt 16 used v44 and stopped after 26m48s of active work (110 provider requests, 122 native calls). It demonstrated exact-ID running-to-completed observation, productive overlap, and three repairs, but completed only four public phases, had no continuation evidence, and missed the 30-minute minimum. A 333.9-second native activity gap triggered the 300-second stall gate while a Rust test was running in foreground Bash. The receipt reports verifier exit 1 with zero tests; verifier output and private verifier files remain uninspected. The disk floor held. See the allowlisted summary at `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-16-summary.json`.

The reproducible corrective is end-to-end turn cancellation: bridge the turn watch receiver into the tool context, pass that token to managed raw-shell execution, and cancel/await the exact foreground scheduler-backed test job when its parent turn is cancelled. The broker now allows bounded owner cleanup before dropping an active tool future. Focused regressions were added for each link.

## Attempt 17 — completed unqualified

Attempt 17 used v45 and ran 54m59s (54m50s active), using 527 provider requests and 571 native calls. Exact-ID running/terminal, productive overlap, ten repair cycles, the 248.741-second maximum activity gap, and the disk floor all passed. The public continuation listed all fifteen phases and `next_phase: none`, but the runner did not confirm continuation and timed out at the 55-minute work boundary. The independent verifier ran five tests and exited 1; its output remains sealed. The task outcome is inconclusive. Provider allowance was $0.265023; billed USD was not separately available. Safe summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-17-summary.json`.

Two harness defects explain material wasted work and invalidate the private-data probe. The follow-up prompt asked for `job_id_overlap_terminal: completed`, but the completion validator accepted only `job_state_overlap: completed`; it therefore continued after the public completion claim instead of proceeding to independent verification. The parser and prompt now share the emitted exact-ID state schema. Separately, the public symlink probe could read a run-private file because Seatbelt permits broad reads and the private root was only outside the editable workspace, not denied. The harness now passes the private root as a deny-only sandbox input and seeds a harmless canary for a production-shaped agent probe. Verifier and metadata contents were not inspected.

## Attempt 18 — completed unqualified

Attempt 18 used v46 and ended after 21m25s total / 21m13s active (309 provider requests; 336 native calls). Continuation, exact-ID lifecycle, productive overlap, repairs, event completeness, and disk checks passed. The 856-event trace had no drops or gaps; maximum activity gap was 66.127s; minimum free disk was 36,547,387,392 bytes. The verifier ran five tests and exited 1; private output remains sealed. Task outcome was failed and the horizon ended short of 30 minutes. The offline ARM64 release workspace suite passed all 30 public tests. A separate public CLI invocation reproduced `catalog-cli delta <before> <after>` as a usage error, and review found delta-apply removes its prior destination before writing the replacement. This is a confirmed task-artifact defect, not evidence of a harness stall; it does not prove which hidden verifier assertion failed.

## Attempt 19 — completed unqualified

Attempt 19 ran 23m20s total / 23m14s active with 253 provider requests and 260 native calls. Exact-ID overlap, continuation, repair, event, activity-gap, and disk gates passed; the verifier ran five tests and exited 1, with output sealed. The public release workspace suite passed, but the public audit revealed a direct scope miss: the required 250,000-node impact-view test used only 2,000 nodes and the separate `catalog-impact-view` package was missing. This is an independently documented incomplete requirement, not a verified attribution of the hidden failure. A further harness finding showed that managed Bash strips the private-canary variables set by the runner. The runner now seeds a harmless workspace-relative symlink instead; 58 focused Python tests pass.

## Attempt 20 — active

Use v48 to complete the explicit 250,000-node incremental impact-view requirement in a separate crate, preserve the CLI corrections through actual subprocess tests, and run offline Clippy with warnings denied. The prompt and verifier inputs are unchanged apart from the public corrective instructions; the Muse Spark route and all run caps remain the same. It also exercises the runner-seeded private canary symlink once through native Bash, without reading or revealing its target. Qualification still requires the passing hidden verifier plus all unchanged task, continuation, exact-job, repair, event, resource, and 30-minute substantive-horizon gates. Attempt 20 is active under the existing authorization.

## Attempt 20 — completed unqualified

Attempt 20 finished in 30m17s total with 29m51.815s active, 8.185 seconds below the required active horizon. It completed all fifteen public phases, including the separate 250,000-node impact-view test with 27 mutations and 13 rebuild checkpoints, CLI delta/atomicity work, exact-ID background overlap, productive edits, continuation, and ten recorded repair cycles. Debug and release workspace sweeps and offline Clippy passed. The trace recorded 756 events with no drops or gaps; maximum activity gap was 156.768s, minimum free disk was 33,501,249,536 bytes against the 10-GiB floor, and the stall guard did not fire. It used 305/1,200 provider requests and 318/1,600 native calls. The verifier ran five tests and exited 1 without timing out; its output remains sealed. No particular hidden assertion is inferred. The public acceptance driver only compared a snapshot with itself for `diff`, leaving non-empty added/removed/changed behavior unasserted. This is a documented public coverage gap, not a proven cause of the hidden failure. Safe summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-20-summary.json`.

## Attempt 21 — completed unqualified

Attempt 21 lasted 27m25.117s total / 27m18.278s active and used 96/1,200 provider requests and 105/1,600 native calls. The measured native-event gap reached 300.676s, so the five-minute stall guard fired; the run stopped before its continuation advanced from `versioned_records`. The verifier ran zero tests and exited 1. Exact-ID running/terminal and productive overlap plus two repair cycles were observed, while event telemetry had zero drops/gaps (232 events) and minimum free disk was 33,485,611,008 bytes. The safe receipt does not identify the specific operation behind the event gap; no raw trace or verifier output was inspected. Safe summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-21-summary.json`.

## Attempt 22 — completed unqualified

Attempt 22 lasted 54m59.927s total / 54m56.583s active and met the duration gate. It completed only the first four of fifteen prompt phases, with `cli_export` next, then reached the runner's planned 55-minute work boundary that reserves time for the verifier. It used 263/1,200 provider requests and 336/1,600 native calls. There was no stall (153.431s maximum activity gap), event gap/drop, or disk breach (minimum free disk 34,271,244,288 bytes). Two real starter repair cycles and productive exact-ID work were detected, but the recorded overlap was the store-repair job rather than the required release-scale corpus test. Continuation was incomplete; the verifier ran zero tests and exited 1, with output sealed. The agent's public note reports that prompt-added empty crate members prevented ordinary workspace builds and forced scratch workspaces. Safe summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-22-summary.json`.

## Attempt 23 — completed unqualified

v51 reached 54m50s of active work and completed the fifteen public implementation phases, but it did not qualify. The overlap ID was a `catalog-format` unit-test job, not the required release corpus job; the acceptance audit and probe report were missing, Clippy evidence was absent, and the subagent review remained running after a status-control refusal. The five-test hidden verifier exited 1, with output sealed. The duration gate passed and the run had no event gaps/drops, stall, or disk breach. See the safe run summary and S018 status for scalar evidence.

## Attempt 24 — completed unqualified

v52 did not improve completion. After 54m11s active work, the public continuation recorded one of fifteen implementation phases complete and assigned `job_id_overlap` before the required dependency-index boundary. The release corpus job, acceptance audit, probe report, Clippy evidence, and continuation closure were not established. The verifier ran zero tests and exited 1 with output sealed. No event gap, stall, or disk breach occurred. See the safe run summary.

## Attempt 25 — completed unqualified

Attempt 25 used v53 and completed 14m36s of active work (333/1,200 requests; 399/1,600 calls). It recorded all fifteen phases, the exact release corpus job running and completed with productive work, and seven repair cycles. It stopped below the duration gate. The continuation listed three required gates as pending, but the runner's old completion parser ignored `gates:` and stopped the session. The focused correction requires all seven gates to be completed before accepting continuation; focused H007 tests pass. The verifier ran zero tests and exited 1. Its output tail was inadvertently displayed during receipt metadata extraction, so the attempt is marked output-inspected; no verifier content was copied or used to develop the corrective, and no hidden assertion is inferred. Safe summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-25-summary.json`.


## Attempt 26 — active

Attempt 26 runs v54, whose case manifest and verifier are identical to v53. It is the first live requalification after correcting the false continuation completion discovered in attempt 25: explicit required gates must all be `completed`, and the runner names pending gates in its same-session follow-up. The model route, workload contract, and resource bounds are unchanged.


## Attempt 26 — completed unqualified

The run met the active-work horizon at 54m40s, with two repairs, generic exact-ID productive overlap, and complete event capture. However, the harness used its older fifteen-phase/gate contract instead of the seven prompt-defined stages and separate overlap, bounded-probe, acceptance, workspace, and review gates. The continuation remained incomplete; acceptance/probe artifacts were absent; exact release-command attribution was not established. The five-test verifier exited 1 with two failures and its output was not inspected. Focused source correction will bind required stages and gates to the current v53 task prompt, rather than repeating a larger task-corpus redesign. See the safe run summary.


## Attempt 27 corrective — v55 contract alignment

Attempt 26 met the duration gate but demonstrated that the runner was still grading the obsolete fifteen-phase checklist while the live task required seven stages and separate overlap, bounded-probe, acceptance-audit, workspace/Clippy, and review gates. The v54 task also asked for a todo action that the native task tool does not provide. Its verifier ran five tests and exited 1; its output remains sealed, so no hidden failure is inferred.

The v55 manifest keeps the same task workspace, route, verifier command/scope, request/call/time/disk/stall caps, and independent-verifier boundary. It removes the unsupported todo request, makes `CONTINUATION.md` the checklist, records exactly twelve named gates, and explicitly records `job_command_overlap` beside the exact overlap job ID and lifecycle. The fresh public continuation seed lists all required gates as pending. The runner now requires the prompt's exact seven phases and twelve gates, the three required public artifacts, an exact-ID release job with productive work between observed running and completed states, and a successful terminal read-only review. Failed/cancelled reviews cannot satisfy qualification. Public artifacts are rejected if a file or parent path escapes through a symlink. When all task evidence is complete before 30 active minutes, same-session follow-ups request bounded correctness-bearing restart/failure-preservation work rather than stopping early.

Before attempt 27, `rtk proxy python3 -m unittest dev.harness_lab.tests.test_h007` passed all 32 tests. The v55 case manifest SHA-256 is `984a82924d69543a02d3d2d382beebf0287643b053a0ed14d7458ff68e094be4`; its verifier metadata matches v54. The verified arm64 binary and helper match the prior attempt's hashes, the arm64 host capability is present, and pre-run free disk was about 30.7 GiB. The run started at 2026-11-07 23:00:11 UTC; outcome pending. Its 1,200-request, 1,600-call, 3,600-second, 10-GiB and 300-second activity limits are unchanged. Only the single Muse route is authorized; unreported billed USD remains `unknown`.


## Attempt 27 diagnosis and next corrective

Attempt 27 demonstrated a sustained 46m34s run with complete tool-event pairing, a 162.539s maximum activity gap, four repair cycles, and exact-ID productive background overlap. It did not complete the task: `workspace_qualification` and `independent_review` remained pending. The run ended on `turn_timeout` after 557 provider requests and 575 native calls; its run-wide elapsed time was 2,902.66s of the 3,600s allowance. Disk, request, and activity-stall limits did not fire. The five-test verifier exited 1, and its output remains uninspected.

The immediate configuration defect was the live command's 900s per-prompt timeout. S018's timeout helper already supports a 3,300s prompt while retaining a 300s verifier reserve under the same 3,600s run cap. Attempt 28 will use that bound so a final workspace-qualification prompt is not cut off at 15 minutes. The run also receives the focused continuation-parser corrections: evidence-suffixed phase entries are parsed and deduplicated, and a trace-proven completed overlap ID is preserved instead of prompting another release job. The 33-test H007 suite passes with these corrections.

Attempt 28 will hold the workload, verifier, model route, request/tool caps, disk floor, stall cancellation, and paired binaries constant. Qualification still requires completed workspace and independent-review gates, a normal terminal, and a passing hidden verifier.


## Attempt 28 execution

Attempt 28 started on 2026-11-07 at 23:57:54 UTC using manifest v56 (`a5262f19a75a7913b6090ace28f95f9cdbe665896a11ed4dd02555d734d2606a`). The model route, task prompt, verifier, paired arm64 binaries, and all run-wide budgets match attempt 27. The prompt timeout is 3,300 seconds so active workspace qualification can continue through a long ACP prompt; the existing guard still limits the run to 3,600 seconds and reserves 300 seconds for verification. Outcome pending.


## Attempt 28 provider failure

Attempt 28 terminated after 137.478s with the typed redacted `provider_api` category, after 15 provider requests. It had only the two intended baseline failure jobs and no completed repair cycle. Telemetry remained complete (48 events, 20/20 paired calls, zero event gaps/drops, 72.008s maximum activity gap); disk, request, wall-clock, and stall limits were not reached. The independent verifier ran zero tests. The ACP stderr log was empty; the current evidence cannot distinguish a transient upstream response from a deterministic provider rejection without unsafe error text. No source correction is justified from this one observation. Attempt 29 repeats the same v56 task and 3,300s per-prompt timeout to test reproducibility.


## Attempt 29 retry

Attempt 29 started at 2026-11-08 00:05:03 UTC with a fresh group identity and otherwise identical task/verifier inputs to attempt 28. The route, paired binaries, 3,300s prompt timeout, 1,200-request / 1,600-call caps, 3,600s run cap, 10-GiB disk floor, and 300s stall cancellation are unchanged. This is a repeatability check for the single early redacted provider_api error; outcome pending.


## Attempt 30 result and attempt 31 corrective

Attempt 29 met duration, exact-ID overlap, productive-work, repair-count, trace, and disk gates, but did not finish workspace qualification or independent review before its prompt timeout. The five-test verifier exited 1; its output remains sealed. Public checklist entries with evidence suffixes were rejected by `documented_gates`, causing the runner to treat the checklist as malformed. The parser now accepts status-plus-evidence entries and applies the latest status for repeated gates; its focused 34-test suite passes.

Attempt 30 used manifest v58 and reached 3,284.287 seconds of active work before `turn_timeout`. It completed the two seeded repair cycles and the exact release-scale job with productive dependent work. Its 944-event trace was complete, all 408 tool starts had terminals, maximum activity gap was 121.274 seconds, and minimum free disk was 36,004,065,280 bytes. The workspace test suite completed with 19 tests. Workspace qualification remained pending because Clippy was submitted through Bash and its `kind=shell` job remained queued with zero attempts until shutdown; independent review also remained active. The five-test verifier exited 1, so the task did not qualify; no hidden assertion is inferred. Provider allowance was $0.144451 and separately billed USD was not reported. A private verifier output tail was inadvertently surfaced during receipt extraction; its text is excluded from evidence and diagnosis. Safe summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-30-summary.json`.

The narrow attempt 31 corrective exposes bounded `cargo clippy` through the existing managed Test custom-command path and rejects `--fix`; TestTool/TUI validation and source tests cover it. Its focused Rust command passed 73 tests, and rustfmt passed. The v59 task and independent verifier remain unchanged. The new prompt launches the workspace suite and Clippy through native Test and starts the read-only review by 30 minutes, so final gates have time to reach terminal states. It preserves the single Muse route and all request, call, disk, activity, and wall-clock limits. Attempt 31 started after its ARM64 release build succeeded; the start SHAs and outcome are recorded in the closure status and run ledger.


## Attempt 31 — live requalification

Attempt 31 started at 2026-11-08T02:50:04Z with manifest v59 and the same frozen workload and verifier. The only prompt change was earlier scheduling of the existing required workspace, Clippy, and independent-review gates. The implementation correction allows `cargo clippy` through the supervised Test command path and rejects `--fix`; the Rust focused selection passed 73 tests. The built CodeGG runner and sandbox helper are ARM64. All S018 request, call, time, disk, stall, and provider-route caps were unchanged.

The 3,299.942-second run met the active-work horizon, exact-ID productive overlap, and repair-cycle gates. Acceptance, bounded probes, workspace tests, and managed Clippy completed. The 1,543-event trace was complete, with 644/644 native tool starts paired to terminal events, zero event gaps/drops, and a 45.681-second maximum activity gap. It remained unqualified: the independent reviewer was cancelled at timeout after its durable transcript ended with five tool calls lacking matching tool-result records, and the hidden verifier exited 1. The private verifier output remains uninspected. The child’s last unresolved tool name was `git_query`, but session-level native events cannot be correlated to those child call IDs, so the exact loss point is not established. See the safe run summary and status record.

Attempt 32 changes only reviewer orchestration in manifest v60: one read-only child, a single native tool call per response, and exact job-status monitoring by the parent while it performs productive work. The parent calls `task get` only after the child job is terminal. This is a bounded live diagnostic for whether the missing child tool results are associated with batched tool dispatch or child-result handoff. The frozen workload, verifier, route, and caps remain unchanged.

## Attempt 32 — live diagnostic

Attempt 32 started at 2026-11-08T03:57:20Z with manifest v60 and a fresh group identity. The manifest diff was limited to `title`, `group_id`, and reviewer instructions; workload, verifier, request/call/time/disk/stall caps, and provider route matched v59. It used the already-qualified ARM64 runner and sandbox helper from attempt 31. Pre-run free disk was 31,840,152 KiB. The reviewer made one native tool call but produced no transcript result for 1,105 seconds. Its run was cancelled only at parent timeout. The parent did not enforce the prompt's 300-second cancellation rule. Acceptance completed, but bounded probes and workspace qualification remained pending. The hidden verifier exited 1 with output uninspected.

Source inspection found that `AgentRunBudget.max_wall_clock_secs` is persisted but unused by the delegated worker. The current worker only applies the optional global `subagent.wall_clock_timeout_secs`, which was unset in the live lane; therefore the durable child could remain running until the parent's 55-minute timeout. Attempt 33 adds a configuration-owned finite default, persists it into the durable run budget, carries it through `DelegatedAgentExecutionContext` and `SubAgentRequest`, and enforces the minimum of that per-run budget and the configured ceiling around the whole child execution. Tests cover config resolution, durable propagation, and timeout cancellation. The workload, verifier, route, and resource caps remain unchanged.

## Attempt 33 — result and next correction

The 600-second delegated-worker ceiling was active in attempt 33: the lane's sole reviewer task and run both reached a completed state. The parent still ended at the 3,300-second prompt timeout, so the harness correctly left measurement incomplete; the unchanged hidden verifier exited 1, with assertion-level output left sealed. The active-work span, exact release-job overlap, five repair cycles, event pairing, disk, and activity-gap measurements passed.

The continuation gate failed for evidence the model had not recorded precisely: the exact release-job running/terminal status fields were absent; the reviewer status line included explanatory text that the strict parser rejected; `review_findings` was empty; and the public receipt did not bind a `task get` result to the exact spawned Task and Run. The database confirms the child completed, but that does not prove the parent retrieved its result. Attempt 34 adds hash-only ACP metadata for task spawn and result events, exact Task/Run correlation, and a status parser that accepts a bounded parenthetical explanation. A production-shaped spawn/get event regression and negative mismatched-Run case pass. The full H007/autonomy Python suites pass (93 tests), as do the focused Rust ACP tests on ARM64 (3 tests); rustfmt passes.

Attempt 34 uses manifest v62 with the task and verifier unchanged. It puts the exact release state lines, reviewer findings requirement, gate ordering, and stop condition at the top of the prompt. No assertion-level cause is inferred from attempt 33's verifier failure.

## Attempt 34 result and attempt 35 corrective

Attempt 34 ended at the 3,300-second prompt timeout after 3,274.833 seconds of active work. It passed the active horizon, exact release-job lifecycle and productive overlap, five repair cycles, paired tool-event capture (452/452), zero event gaps/drops, and the 300-second activity-stall and 10-GiB disk safeguards. The maximum activity gap was 107.506 seconds, minimum free disk was 46,367,965,184 bytes, and usage was 400/1,200 provider requests and 452/1,600 tool calls. The verifier ran five tests and exited 1; its output was not inspected. Provider allowance was $0.196646; separately billed USD is `unknown`. Safe summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-34-summary.json`.

The lane database recorded two completed `research` children and no `verifier` child. The required review fields and correlated `task get` were absent, so continuation remained unconfirmed. This identifies a harness follow-up defect: `_h007_followup_prompt` treated any child with typed Task/Run IDs as the reviewer and explicitly suggested the `explore` agent. In the presence of research children, it stopped asking the model to start the actual independent review. The final acceptance helper also accepted any spawned agent role and did not require an exact-Run terminal status observation before the matching Task result.

Attempt 35 changes only the follow-up/review measurement and the priority wording. ACP records the allowlisted agent class (`verifier` or `other`) and a hash of the Run ID used by native `task status`/`task wait`; raw IDs remain out of telemetry. The continuation checker now requires exactly one `verifier` child, a completed status observation bound to its Run ID, and one completed `task get` result bound to the same Task/Run. Research, explore, and general children cannot satisfy independent review. Follow-up guidance explicitly requests `action: spawn`, `agent: verifier`, and the exact status/get sequence. Manifest v63 changes only `title`, `group_id`, and prompt wording; task workspace, public inputs, verifier command/scope, route, and all resource limits are unchanged. Focused verification passes: 93 Python tests, 20 ARM64 ACP unit tests, formatting, and the assertion-quality and duplication guards. The paired ARM64 runner and sandbox helper were rebuilt and verified, and the v63 manifest loaded with unchanged caps and verifier. Attempt 35 started at 2026-11-08T09:02:21Z with 38,127,788,032 bytes free before launch.

## Attempt 35 — live requalification

Attempt 35 is running on manifest v63 with the single Muse Spark route. It uses the same 1,200 provider-request, 1,600 native-call, 3,600-second hard-stop, 10-GiB free-disk, and 300-second native-tool stall limits. The rebuilt ARM64 runner SHA-256 is `d80225c257ade21f88a1ef21a394163686fe4de340d44827da586818a896fa01`; the sandbox helper SHA-256 is `530e7000dcb1aec71e597c4c93428c36287ba5b561e366f23fa4ea8388c5ea14`. The public task, fixture inputs, verifier command/scope, and provider route are unchanged. Outcome pending.


## Attempt 35 result and attempt 36 corrective

Attempt 35 met the active horizon and operational gates but did not complete the review evidence or task. The exact verifier Run was durably completed, yet every parent status call was rejected because read-only observation was bound to the originating turn. Native task-get also returned a JSON envelope whose typed text was not parsed by ACP, leaving no safe correlation metadata; the model wrote noncanonical continuation field names. The hidden verifier exited 1, but its output remains uninspected and the result cannot be attributed to an assertion.

Attempt 36 addresses those measured gaps only: root-run read observations use same-session ownership across turns, while mutating controls remain turn-scoped and delegated actors remain parent-lineage scoped; ACP extracts Task/Run/status only from the typed `text` member in the durable task-get envelope; and the continuation prompt handles authorization refusal without retries or false completion and names the canonical fields. The v64 manifest preserves task, verifier, route, and budgets. Focused checks pass; ARM64 release rebuild and live result are pending.


## Attempt 36 result and attempt 37 corrective

Attempt 36 passed the sustained-work, exact release-job overlap, repair, workspace-suite, Clippy, event-pairing, disk, and stall measurements. Its durable store contained two verifier tasks: the first completed after 72 seconds; the second failed with `deadline_exceeded` after 605 seconds against a 600-second budget. The parent eventually wrote canonical review fields for the first completed child and recorded the duplicate/failed second child, but the exactly-one-review gate remained pending. The five-test hidden verifier exited 1; its output is sealed and no assertion-level conclusion is drawn. The task ended inconclusive at the prompt timeout.

The next corrective targets reviewer timing. S018's generic review threshold fires after five phases. Defer spawn until the first six dependent stages, acceptance/probe artifacts, and exact release overlap are complete; allow one verifier when workspace_qualification is next or already complete so review can run alongside workspace tests and Clippy. After any verifier spawn attempt, do not create a replacement; if the spawn or child lifecycle fails, leave independent_review pending and do not write successful review fields. Focused regressions cover five-stage deferral, final-stage eligibility, and failed-spawn handling.


## Attempt 37 — completed unqualified

Attempt 37 met the 30-minute horizon, exact productive overlap, repairs, event pairing, and resource checks. The only verifier child failed its default 600-second Run deadline. Safe task metadata shows three exact-Run status calls (running, running, failed), 12 waits on that same Run, and no matching task-get result; the public note left review pending and the agent did not create a replacement. The hidden verifier exited 1; its output remains sealed. Safe summary: plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-37-summary.json.

Attempt 38 will keep the route, workload, verifier, request/time/disk/stall caps, and paired binaries. The H007-only lane config raises the delegated-child wall-clock ceiling to 1,200 seconds. The read-only review prompt is limited to the seven-stage requirement-to-test matrix and public acceptance/probe evidence. While the child runs, the parent continues workspace work without task-wait polling; it makes at most one exact-Run status observation at final audit and retrieves the matching Task once only after completed status. Regressions cover the bounded timeout config, no-poll continuation, and the one-status/one-get completion path.


## Attempt 38 result and attempt 39 corrective

Attempt 38 completed 54m59.895s after launch with 54m55.137s of active work. It satisfied the duration, event completeness, activity-gap, disk, repair-cycle, and native exact-ID productive-overlap measurements. Its independent review lifecycle completed, and the public acceptance/probe artifacts and twelve gate statuses were present. The public note nevertheless omitted `job_id_overlap_running: running` and `job_id_overlap_terminal: completed`; the native trace already showed those observations for the exact recorded release job. The S018-specific follow-up did not name the exact keys and instructed a new overlap test instead of repairing the ledger. The runner therefore sent eleven autonomous follow-ups and timed out at the 3,300-second prompt window. The independent verifier ran five tests and exited 1 after the timeout; output remains uninspected. Task outcome is inconclusive, and attempt 38 does not qualify. Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-38-summary.json`.

The attempt 39 corrective is limited to this failure mode. The new v67 fixture differs from v66 only in `group_id`, `title`, and the public task prompt; verifier metadata, workload, tool allowlist, and budgets are unchanged. The task prompt now gives the two exact state lines and instructs the agent to repair the ledger without another release test when the trace already proves the exact-ID lifecycle and productive overlap. The S018 follow-up path now has a trace-confirmed, ledger-only branch and names those exact fields in its remaining instructions. A focused regression covers the missing-ledger case and proves the follow-up does not request another release test. Verification passed: 33 H007 case/result tests, Python compilation, and `git diff --check`.

The authorization allows 40 total Muse Spark 1.3 Contributor starts; 38 are recorded as consumed after reconciling the stale counter. Attempt 39 will retain the existing ARM64 binary/helper, H007 reviewer timeout configuration, unchanged verifier, 1,200-request ceiling, 1,600-call ceiling, 3,600-second hard stop, 10-GiB free-disk floor, and 300-second stall cancellation. No hidden verifier output is used in the diagnosis.

## Attempt 39 — completed unqualified

Attempt 39 used v67 and ran 3,299.865 seconds, with 3,282.981 seconds of measured active work. It completed five of seven required implementation stages; `acceptance_audit` and `workspace_qualification` remained incomplete. The initial live fixture checkpoint lacked the twelve pending gate rows that the prompt told the agent to preserve; the agent's final note also lacked the gate block. Exact release-job lifecycle and productive overlap passed, seven repair cycles were observed, all 333 tool starts had terminals, event gaps/drops were zero, the maximum activity gap was 160.494 seconds, and minimum free disk was 30,530,228,224 bytes. No request, disk, or stall limit fired. The independent verifier ran five tests and exited 1 without timing out. Its output remains sealed; no assertion-level cause is inferred. The task timed out and did not qualify. Safe receipt: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-39-summary.json`.

The attempt-40 correction is limited to live task setup and prioritization. The source fixture now starts with the exact seven pending phase entries and twelve pending gate rows already required by the runner and prompt, plus explicit pending overlap and review fields. The v68 prompt keeps the clean starter, task contract, verifier, model, and all resource caps unchanged. It places the two historically unfinished stages (`acceptance_audit` and `workspace_qualification`) and current gate-ledger maintenance at the top of the instructions, and reserves the verifier window. A regression confirms v68 differs from v67 only in title, group identity, and prompt. All 37 focused H007 tests pass.

Attempt 40 used v68 under the existing caps and completed 3,299.836 seconds after start with 3,286.782 seconds of active work. It completed all seven required implementation stages and four repair cycles; exact release-job lifecycle and productive overlap passed. Telemetry was complete (1,103 events, 495/495 paired tool calls, no drops or gaps), maximum activity gap was 99.198 seconds, and minimum free disk was 27,016,503,296 bytes. It used 420/1,200 provider requests and 495/1,600 native calls. No request, disk, tool-call, or stall limit fired.

It remained unqualified: the `independent_review` gate was pending. One reviewer task was spawned; 41 `get` observations for the same Task/Run all reported `running`, while seven status calls returned six `running` and one `failed`. The child was reported failed, not completed, and no successful reviewer result was recorded. The independent verifier ran five tests and exited 1 without timing out. Its output remains sealed. The task ended at the 3,300-second prompt cutoff; outcome is inconclusive and continuation was not confirmed. Provider allowance was $0.215483 and separately billed USD is unknown. Safe summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s018-run-40-summary.json`.

The next corrective should instrument and test the exact task lifecycle seam: correlate `spawn`, bounded status/wait, and `get` for one Task/Run; expose consistent terminal state; and stop repeated `get` calls while the reviewer is nonterminal. The evidence does not establish why the reviewer failed or whether the repeated gets reflect model strategy or a task-surface issue, and it does not justify broader daemon or scheduler redesign. Do not inspect hidden verifier or reviewer output to infer a cause.

The original authorization allowed 40 starts and is exhausted. No further live provider run is covered by it; sustained qualification remains open pending a new explicit authorization.
