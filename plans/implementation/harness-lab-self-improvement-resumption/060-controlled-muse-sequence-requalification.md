# Harness Lab Self-Improvement Resumption S060 — Controlled Muse Sequence Requalification

Status: **active**

Repository baseline: `1761f8689bdaa36f29ba3b9dd2545929a1e0c9b8`

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#55-s060--controlled-muse-sequence-requalification`

Long-term requirements:

- `plans/000-long-term-specification.md` §§23–24, 26–29
- `plans/003-planning-process.md` §§6–7

Applicable ADRs:

- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`
- `plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md`

Primary class: capability

## 1. Objective

Run one controlled H007 requalification after S059 using the unchanged v97 task and independent verifier, only `opencode_go/muse-spark-1.3-contributor`, and the existing S019 limits. Determine whether the attempt-80 seed-before-repair ordering failure was a one-run model execution miss or a repeatable agent/harness interaction. Do not change production code, task text, verifier, route, or acceptance gates unless the new safe evidence identifies a concrete reproducible harness defect.

## 2. Why this milestone is ready

S059's broker error typing and bounded verifier setup attribution are implemented and passed focused offline checks. Attempt 80 then ran 3,282.832 seconds of active work, completed all seven stages, five contract phases, twelve gates, required review, release overlap, and both final success jobs. It remained unqualified because `catalog_store` repair was submitted before the seed's terminal failure was observed; `catalog_format` was the only qualified pair. All 428 tool events paired, all 123 applicable job request/result IDs matched, and the unchanged verifier reported only the public continuation and transcript/job-lifecycle failure domains. The runtime showed no disk, inactivity, external-wall, or event-delivery failure. The existing grant has four starts remaining.

## 3. Current implementation evidence

- `dev/harness_lab/autonomy_run.py` already emits explicit pair-specific feedback for a missing terminal seed status, out-of-order repair submit, or missing productive edit. The frozen v97 task independently states the required order.
- `dev/harness_lab/h007_results.py` derives `repair_followed_failure` from observed tool-call order and requires each named pair to be failure → productive edit → distinct completed rerun. An incomplete pair makes the public continuation invalid.
- The `pointer_block_is_terminal` telemetry field is informational for this interleaved layout; tests explicitly expect a nonterminal pointer block when job/review evidence follows. It is not an acceptance predicate.
- Attempt 80 reports complete event measurement and exact job request/result ID matching; no transport or projection loss is indicated.

## 4. Invariants that must not regress

- Use only Muse Spark 1.3 Contributor through CodeGG's `opencode_go` route.
- Keep the v97 frozen task and independent verifier byte-for-byte unchanged.
- Keep the 1,200 provider-request, 1,600 native-call, 3,300-second prompt-work, 3,600-second runner, 3,590-second external wall, 300-second inactivity, 10-GiB runtime disk, and 30-GiB preflight reserve limits.
- Require at least 30 minutes of active dependent Rust work, real exact-ID background overlap, two qualified failure/repair pairs, complete continuation, and a passing independent verifier. Duration by itself never qualifies.
- Retain only bounded safe diagnostics and aggregate counters; never inspect or commit private receipts, logs, transcripts, raw tool/provider payloads, credentials, or hidden verifier material.

## 5. Scope

### In scope

- Build and verify a paired ARM64 CodeGG/sandbox-helper from the exact current source head after this plan's documentation commit.
- Create a fresh side-effect-free preflight for attempt 81 with reconciled start counters, exact manifest/binary hashes, route check, and disk reserve.
- Run attempt 81 once under the existing grant and watchdog.
- Reconcile the sanitized diagnostic, monitor summary, and authorization ledger, then determine whether any additional corrective is supported.

### Explicitly out of scope

- Editing H007 task text, fixture inputs, verifier, prompt wording, acceptance gates, provider route/model, or caps.
- Adding a task-specific source-edit gate, forced job wait, synthetic repair, or continuation rewriting.
- Changing daemon, scheduler, ACP event projection, job identity, or job result semantics without new reproducible evidence.
- Inferring hidden assertion text or inspecting private run material.

## 6. Required production changes

None are justified by attempt 80. This milestone is a controlled end-to-end qualification experiment on the already-corrected S059 source behavior. If attempt 81 exposes a new stable CodeGG failure, record its safe cause and create a separate narrowly scoped corrective plan before changing runtime ownership.

## 7. Ordered work packages

### Work package A — Freeze and preflight attempt 81

Intent: Prove the exact build, task, route, budgets, disk, and authorization state before provider admission.

Required changes: No production changes. Rebuild/reuse only after verifying the exact current source head; create a fresh private lane and budget file; run the repository campaign preview without ledger mutation.

Acceptance evidence: ARM64 binary pair and hashes match the preflight; v97 manifest digest remains `e87faea3f5da93ae300f2ae21ec46e6ff1f899cff0fdf6c1b9e860c97ffe57ee`; route is available; 30 GiB free reserve is met; attempt 81 is the next authorized start.

### Work package B — Live H007 attempt 81

Intent: Measure the unchanged model/task behavior after S059.

Required changes: Run one campaign-accounted case with the exact model, route, binary pair, budget, private lane, and external watchdog.

Acceptance evidence: Safe preflight, safe monitor summary, safe campaign diagnostic, reconciled request count, and authorization ledger row.

### Work package C — Decide from safe evidence

Intent: Separate product execution, model sequencing, and verifier outcome.

Required changes: Compare the observed seed terminal status, repair submission order, productive edit, exact job lifecycle, continuation state, final test states, verifier state, event completeness, and disk/time bounds. Do not infer from private material.

Acceptance evidence: H007 is marked qualified only if every original gate and the unchanged verifier pass; otherwise record the precise public failure and whether a CodeGG execution defect was reproduced.

## 8. Failure, cancellation, restart, and contention semantics

No production execution semantics change. The wrapper cancels at 300 seconds of native inactivity, below 10 GiB free disk, or at 3,590 seconds external wall time, sends SIGTERM, and allows at most 15 seconds before SIGKILL. The runner itself remains capped at 3,600 seconds. Preserve private runner logs and receipt unread.

## 9. Migration and compatibility

No migration, persisted schema, or protocol change is in scope.

## 10. Required tests

No new deterministic tests are justified unless attempt 81 exposes a specific production defect. Reuse the S059 focused verification results for the unchanged binary source; run a fresh exact-head release build and architecture/hash checks. The unchanged independent H007 verifier is the end-to-end qualification gate.

## 11. Required verification commands

```bash
# exact-head ARM64 release build with the repository's ARM64 Rust and pkg-config paths
cargo build --release --locked -p codegg --bin codegg --bin codegg-sandbox-helper

# pre-provider validation uses the campaign reconciler and frozen manifest digest
python3 -m dev.harness_lab autonomy --help

# live command must specify the exact model, manifest, binary, lane, ledger, preflight, evidence, budget, and caps
```

Run preflight checks before any live invocation. Do not claim the `--help` command is live evidence.

## 12. Documentation updates

- Record attempt 80 in S019 closure/roadmap and close S059 at its technical scope.
- Register S060 and record attempt 81 evidence/counters when available.
- Keep the overall S019/H007 status unqualified unless all gates pass.

## 13. Acceptance criteria

- Attempt 81 uses the unchanged v97 task/verifier, Muse/OpenCode Go, and all existing caps.
- Exact-head ARM64 binary hashes, 30-GiB reserve, route, and campaign counters pass before provider admission.
- H007 qualifies only if active duration, dependent stages, overlap, both seeded repair pairs, continuation, final test jobs, event/job telemetry, and independent verifier all pass.
- If it fails, safe evidence identifies a reproducible runtime defect before another implementation corrective is invented.

## 14. Stop conditions

Stop before provider admission on a changed frozen manifest, mismatched source/pair identity, missing route, less than 30 GiB free disk, unresolved campaign accounting, exhausted grant, or changed task/verifier/caps. During execution stop at existing cancellation/disk/wall bounds. Do not claim success from duration or public completion alone.

## 15. Closure evidence required

Record attempt-81 preflight, safe diagnostic and monitor hashes; exact source/pair identity; request count and authorization counters; active/horizon duration; task stages, repair pair states, continuation validity, final job states, event/job-ID correlation, independent verifier outcome; minimum/ending disk; stop reason; and unreported-USD handling. Keep all raw evidence private and uninspected.

## 16. Handoff notes

Attempt 80 is historical failed evidence and remains immutable. Its provider count was 385 and it consumed one start. After that start, S019 had 80/200 combined starts, 36/40 starts in grant `s019-supplemental-20261111-04`, and 51/120 task starts; four grant starts and 69 task starts remain. Attempt 81 is authorized within that grant. If it fails, distinguish a repeated model sequencing failure from a reproducible CodeGG runtime issue before selecting S061. USD cost remains unknown if unreported.
