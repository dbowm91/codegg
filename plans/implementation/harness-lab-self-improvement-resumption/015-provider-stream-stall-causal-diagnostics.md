# Harness Lab Self-Improvement S015 — Provider Stream Stall Causal Diagnostics

Status: **active**

Repository baseline: `ac610d1cdc8b6b4bcde0262cff12eaa7da899c41` (S010–S014 branch; PR #223)

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#9-s015-provider-stream-stall-diagnostics-and-corrective`

Long-term requirements:

- `plans/000-long-term-specification.md` §§23–29
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- None. Keep the existing provider adapter, agent stream ownership, and retry policy unless live evidence proves a narrow current defect.

Primary class: **capability** (live causal diagnosis), with supporting infrastructure diagnostics.

## 1. Objective

Identify which boundary causes the repeated Muse Spark/opencode_go stream stalls, make the smallest evidence-supported correction if CodeGG is responsible, and independently confirm the result in bounded live runs. Preserve the historical S013 attempts and authorization as consumed; this plan uses only the new operator authorization issued 2026-10-31 for up to 20 additional runs.

## 2. Why this milestone is ready

- S012 provides a redacted causal tool trace and S013 provides complete receipts for two repeated `provider stream stalled waiting for next event` outcomes.
- Attempts 17 and 20 had complete ACP tool-event pairing, but that does not observe provider response bytes or decoder output.
- The user explicitly authorized up to 20 additional diagnostic/corrective runs, still using only `opencode_go` with Muse Spark 1.3 Contributor.
- No architecture change or additional provider integration is a prerequisite. If evidence points outside CodeGG or requires changing shared provider policy, stop and record the boundary instead of guessing.

## 3. Current implementation evidence

`src/agent/provider_turn.rs` applies a 90-second timeout between decoded `ChatEvent`s. `crates/codegg-providers/src/opencode_go.rs` awaits response headers and passes `response.bytes_stream()` into `crates/codegg-providers/src/wire.rs::shared_stream`. Existing request-admission receipts are recorded before transport and contain no response-header, body-chunk, decoder, finish, or EOF lifecycle. Existing timeout receipts therefore establish decoded-event silence, not its cause.

S013 attempt 17's last request admission preceded the timeout by about 91 seconds; attempt 20 shows the same interval. Both receipts preserve the failure and remain unqualified. Do not rewrite their status or infer that the provider, transport, decoder, daemon, or ACP caused the stall.

## 4. Invariants that must not regress

- Only the CodeGG `opencode_go` provider and Muse Spark 1.3 Contributor are used for live attempts.
- Do not log prompt/message bodies, tool arguments, credentials, authorization headers, raw SSE bytes, or secret-bearing URLs.
- The provider request ceiling is enforced before transport; N+1 must remain refused without a network request.
- Cancellation, existing retry/no-replay policy, stream idle timeout, and provider surface selection remain unchanged until evidence requires a narrow correction.
- Preserve S013 receipts, authorization ledger, and closure text as historical evidence.
- A provider timeout, model/task failure, verifier result, and harness defect are separate outcomes.
- One live run at a time; stop if free disk falls below 10 GiB.

## 5. Scope

### In scope

- Correlate one provider attempt from the agent stream loop through the OpenCode Go request, response headers, body-byte chunks, SSE-decoded events, terminal stream state, and final timeout/error.
- Record bounded, body-free diagnostics sufficient to distinguish: no response headers; headers with no body bytes; bytes without decoded events; decoded events not consumed by the turn; Finish/EOF; transport/decode error; and cancellation/timeout.
- Add local production-shaped tests proving event ordering and that diagnostics never contain payload/secret material.
- Run up to 20 sequential, budgeted live diagnostics under the new authorization; stop early after a CodeGG correction is independently confirmed or the evidence identifies a non-CodeGG boundary.
- Implement and test only the narrow corrective supported by those observations.

### Explicitly out of scope

- Changing model, provider route, credential, multi-provider behavior, fixture/verifier semantics, or the 30-minute scoring threshold.
- Extending per-run duration or request caps, benchmarking model quality, or claiming sustained qualification from a diagnostic attempt.
- Reopening daemon, scheduler, ACP event protocol, job lifecycle, or retry architecture without a reproduction at that boundary.
- Logging raw provider traffic or adding a general-purpose observability subsystem.

## 6. Required production changes

### Core/domain

None expected.

### Storage and migrations

None. Diagnostics remain bounded structured logs and per-run sanitized receipts; no durable schema change.

### Protocol and DTOs

None. This plan does not introduce provider lifecycle events into the public protocol.

### Runtime and concurrency

Add a stable attempt correlation span at `provider_turn::stream_once`, carry it while polling the returned stream, and emit body-free lifecycle observations at the OpenCode Go request and decoded-stream boundary. Capture response status, elapsed durations, chunk/event counts, byte lengths, and terminal class only. The bounded live-lane environment must explicitly enable only the provider-turn and OpenCode Go diagnostic targets; the default `error` filter would otherwise suppress the new observations. All observations share attempt identity and never include bodies.

At the 90-second idle terminal, report which stages advanced and their last-progress elapsed times. Distinguish setup timeout, no bytes, bytes/no decoded event, decoded event consumption, transport error, and cancellation without changing timeout behavior in the telemetry-only phase.

### Frontend or operator surface

No new user-facing UI. The live receipt should summarize the diagnostic stage and cite its private log/trace path without copying sensitive payloads.

### Security and authorization

Use only the explicit 2026-10-31 authorization: up to 20 runs, `opencode_go/muse-spark-1.3-contributor`, one run at a time, maximum 60 provider requests and 3,600 seconds per run, 10 GiB free-disk stop floor, immediate cancellation on user request or sensitive-data exposure, no fallback route, and record billed USD as unknown if unavailable. The request ceiling is enforced by the CodeGG subject boundary before transport. Maintain private run directories and sanitized committed evidence.

## 7. Ordered work packages

### Work package A — Instrument the stream stages

Intent: make a timeout causally attributable to the last observable boundary.

Required changes: correlate attempt ID across setup, response headers, byte stream, decoded `ChatEvent`s, timeout/error, and terminal `Finish`/EOF; expose bounded counters and elapsed times; avoid bodies, prompts, headers, tool arguments, and credential data.

Acceptance evidence: production-shaped local test asserts ordering/correlation and checks captured diagnostic output for forbidden payload and secret markers.

### Work package B — Run the live diagnostic cohort

Intent: reproduce under the authorized route after instrumentation is in the exact binary.

Required changes: sequentially run the existing pinned Rust task using the native CodeGG path. Preserve per-run source/binary hashes, request admission and receipt, provider lifecycle summary, ACP trace, verifier/task outcomes, cancellation, and free-disk minima. Do not require a run to pass the long-horizon objective to count as a diagnostic observation.

Acceptance evidence: each started run has a complete sanitized terminal receipt; no more than 20 runs or 1,200 total requests; USD is explicit or `unknown`; stop at the first reproducible CodeGG defect or at an identified external boundary.

### Work package C — Correct and requalify narrowly

Intent: fix only the reproduced CodeGG failure mechanism.

Required changes: add a failing-first regression at the owning boundary, implement the minimum correction, run focused verification, then use up to two fresh runs within the original 20-run authorization to confirm the stage signature changes and no regression occurs. If no CodeGG defect is reproduced, make no behavioral change and close with a bounded inconclusive/external-cause finding.

Acceptance evidence: independent reproduction and post-fix confirmation receipts are linked, or a precise non-CodeGG boundary is identified with an honest incomplete result.

## 8. Failure, cancellation, restart, and contention semantics

Runs are sequential, not resumed or retried under a new identity. Request cap, wall-clock cap, and free-disk stop are fail-closed. Any incomplete provider lifecycle is explicitly `unknown` at the unobserved stage. On cancellation, terminate the live run and contained children and preserve a terminal receipt. An interrupted run consumes its started-run slot and all admitted requests. Keep private per-run state; no daemon or provider state is shared between runs beyond normal configured credentials.

## 9. Compatibility and migration

No persisted or wire schema changes are expected. If the sanitized receipt shape must grow, add optional versioned diagnostic fields with old receipts parsing as unavailable. Never rewrite old S013 JSON receipts.

## 10. Required tests

### Focused unit tests

- Attempt ID remains attached across provider setup, response, byte chunks, decoded events, and terminal outcomes.
- The diagnostic distinguishes no headers, no bytes, bytes without decoded events, decoded events, Finish, EOF, transport/decode error, and cancellation.
- Body-free summaries expose only count/size/timing/status/type fields and exclude supplied sentinel prompt, tool-argument, auth-header, URL, and SSE-body values.
- The 90-second idle error remains typed and unchanged until evidence justifies a behavioral correction.

### Integration tests

- Local HTTP fixture emits: normal SSE; headers then no body; partial SSE bytes without event delimiter; decoded event then stall; and clean Finish/EOF.
- Assert provider request N/N+1 cap behavior remains pre-transport.

### Restart and recovery tests

Not applicable; no durable state is added.

### Contention and cancellation tests

- Cancellation while awaiting response headers and while awaiting body chunks exits promptly and records the correct terminal stage.

### Security and negative tests

- No prompts, credentials, authorization headers, raw body, or sensitive URL can enter the structured diagnostic record.

### Migration and compatibility tests

- Existing S013 evidence loads unchanged; absent new diagnostic fields remain explicitly unavailable.

## 11. Required verification commands

```bash
cargo test -p codegg-providers opencode_go --locked
cargo test -p codegg-providers wire --locked
cargo test -p codegg --lib provider_turn --locked
python3 -m unittest discover -s dev/harness_lab/tests -v
scripts/verify.sh quick
```

Run the focused commands actually covering changed targets; run the broader commands appropriate to the final diff. Record exact commands and results. Before each live run, verify at least 10 GiB free; check again after builds and run cleanup.

## 12. Documentation updates

- Record the new live campaign authorization and each started attempt under `plans/closure/harness-lab-self-improvement-resumption/evidence/`.
- Update this line's roadmap and registry only with observed results.
- Do not alter S013 historical closure/evidence or claim S014's independent-improvement criteria are met.

## 13. Acceptance criteria

- A timeout receipt identifies the furthest observed stage and last body/event progress under the same attempt ID.
- A real live attempt reproduces the failure with enough safe telemetry to identify the responsible boundary, or records precisely why the failure did not recur.
- Any CodeGG fix has an independent failing-first regression and at least one post-fix live confirmation within the authorization.
- The single-provider route, per-run caps, 10 GiB disk floor, payload privacy, and consumed S013 ledger remain intact.
- Closure distinguishes task success, verifier result, stream lifecycle, and long-horizon qualification.

## 14. Stop conditions

Stop and report if only changing provider/model/credential or widening transport/retry policy could proceed; the stall is attributable to upstream/network behavior with no CodeGG defect; telemetry would require raw body/secret capture; source/binary identity is uncertain; any request or time cap is unobservable; free disk drops below 10 GiB; a sensitive artifact is exposed; or the 20-run ceiling is reached. Do not spend the remaining authorization to force a positive result.

## 15. Closure evidence required

- Implementation commit and exact binary SHA.
- Focused and broad verification, including negative privacy tests.
- New authorization ledger with run index, start/terminal state, admitted requests, elapsed time, model/provider, free-disk minimum, and reported USD or `unknown`.
- Sanitized stage-level diagnostics and referenced private run artifacts.
- Failing-first reproduction, narrow diff, and post-fix confirmation if a CodeGG defect is corrected; otherwise an explicit non-CodeGG/inconclusive disposition.
- Remaining live authorization, S013 historical status, and S014 status stated accurately.

## 16. Handoff notes

Build artifacts have consumed substantial disk during prior campaigns. Keep targets private and avoid parallel builds; remove only this campaign's private artifacts after preserving evidence. Current free disk at plan start: 58 GiB. Prior S013 authorization is exhausted and is not reused. The new user message explicitly authorizes up to 20 additional bounded Muse Spark 1.3 Contributor runs via the existing OpenCode Go adapter; no USD ceiling was stated and USD may be unavailable, so report it as unknown rather than inventing a charge estimate.
