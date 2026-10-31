# Harness Lab Milestone S016 — Closure Status

Status: **conditionally closed**

Source implementation plan:

- `plans/implementation/harness-lab-self-improvement-resumption/016-deepseek-long-run-corrective-loop.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#10-s016-deepseek-long-run-corrective-loop`

Repository baseline reviewed: `24af4ce7a` (run 20 controller commit; the authorization ledger includes the final run receipt)

Implementation commits or pull requests:

- `94fc8d41` — pair-safe compatibility context compaction correction, carried by the ARM64 binary used for this campaign.
- `771f58ff7` — run 18 evidence and release-job diagnostic prompt.
- `c082faec4` — run 19 evidence and explicit background-test prompt correction.
- `24af4ce7a` — run 20 authorization record.

## 1. Executive finding

The authorized diagnostic/corrective loop is complete because all 20 sequential run slots are consumed. The capability objective is **not** complete: none of the H007 attempts passed its unchanged hidden verifier and sustained-work criteria. The evidence does not reproduce a provider stream, ACP event-pairing, or durable job-projection defect in the final runs. The remaining result is attributable to the model not completing the required repair sequence within 60 requests, with the configured request cap ending each long attempt after only a few minutes.

`stable_live_evidenced` remains unestablished. A successor plan and fresh explicit authorization are required before any further provider run.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Use only `opencode_go/deepseek-v4.1-flash` | Append-only ledger, runs 1–20 | pass | Seventeen runs admitted provider requests; three attempts stopped before admission. No other route was used. |
| Keep the H007 task, contract, corpus, public tests, verifier, and acceptance fixed | Fixture hashes in each run summary; verifier SHA-256 `13617671ceedfe1b4cff89c9a96ad5380fd94ca4799f861f95a404f84090d33c` | pass | Only versioned agent prompt/diagnostic manifests changed. Private verifier content was not inspected. |
| Enforce 60 requests, 3,600 seconds, and 10 GiB free disk per long run | Ledger and provider/disk receipts | pass | Request 61 was refused before transport. Run 20 minimum free disk was 33,894,371,328 bytes; no disk-floor stop occurred. |
| Continuous provider streams and complete ACP tool-event pairs | Run 20 summary: 60/60 response bodies reached EOF; 52/52 tool starts paired; zero gaps | pass | No idle timeout or activity-stall cancellation occurred. |
| Exact-ID background job lifecycle with productive overlap | Run 20 receipt, transcript, causal trace, and durable job rows | fail | Two exact IDs were observed `running` then `failed`; a third was left at `running` in the model transcript and later failed in the durable store. All three jobs ran for 1.588–2.591 seconds. No productive edit overlapped actual job execution; the priority release-workspace job was never submitted. |
| Two seeded failure/repair cycles and five dependent implementation phases | Run 20 horizon receipt and tool trace | fail | Zero repair cycles; work remained on repeated `catalog-format` checks and did not reach `catalog-store` or the priority overlap phase. |
| At least 30 minutes of substantive active work, with gap ≤5 minutes | Run 20 horizon receipt | fail | Active work was 228.790 seconds; maximum measured gap was 31.027 seconds. The 60-request cap ended the attempt after 231.199 seconds of case time. |
| Continuation and independent verifier | Run 20 receipt | partial / fail | Continuation was confirmed. The unchanged verifier ran automatically, reporting exit code 1 and zero tests. Its source and contents were not inspected. |
| Close after qualification or the shared 20-run cap | Authorization ledger | pass | All 20 run indices are recorded; no authorization remains. |

## 3. Production implementation evidence

The campaign binary includes the pair-safe `DropMiddleMessages` compatibility compaction correction from `94fc8d41`. Later long runs did not reproduce the earlier invalid tool-history compaction error. Runs 18–20 made no additional production-code changes. The corrective iterations after run 18 were prompt-only: run 19 initially omitted the test tool’s fixture workdir and background mode; prompt v6 explicitly set both. The frozen H007 fixture and verifier remained unchanged.

## 4. Verification executed

### Commands run

- Three live autonomy runs were executed after run 17: run 18 long qualification, run 19 bounded job diagnostic, and run 20 final long qualification. Their exact manifests, binary identity, caps, receipts, and artifacts are in the authorization ledger and per-run summaries.
- The harness automatically invoked the unchanged H007 verifier after each case. Per-run summaries record only its status metadata.
- Safe diagnostic inspection covered provider-admission receipts, body-free CodeGG logs, ACP causal traces, transcript tool metadata, and read-only durable job state. No hidden verifier contents were inspected.

### Results

- Run 18: 60 requests, 305.275 seconds case duration, one repair cycle, no observed running/terminal overlap, task inconclusive.
- Run 19: 6 requests, 17.529 seconds, exact ID queried, but the diagnostic command ran from the case root in foreground mode and failed in 80 ms; this was corrected in the prompt, not product code.
- Run 20: 60 requests, 231.199 seconds case duration, zero repair cycles, no productive job overlap, task inconclusive.
- No repository test suite was run during this final closure pass. The changed files are plans, evidence records, and prompt/diagnostic manifests; no Rust source was changed in runs 18–20.

## 5. Invariant review

- Provider route stayed on `opencode_go/deepseek-v4.1-flash`.
- The run binary was ARM64 with SHA-256 `16481cc8bf0d8866db352c3aab90c35950bde37e5799be1458a43012d594fe2a`; the sandbox helper SHA-256 was `37ee6c8cd876d1e05bbd650c7a26520753fe55534ac821a52d9089712d751d43`.
- Request admission refused N+1 before transport. All admitted responses reached body EOF; ACP tool starts and terminals paired.
- The 10 GiB disk floor was maintained. Run 20 ended with about 37.5 GiB free.
- Hidden verifier hash and H007 frozen fixture hashes remained stable. No secrets or host environment were inspected.

## 6. Failure and recovery review

The provider request budget stopped each long attempt before its 3,600-second wall-clock limit. The cap behaved as configured, and its refusal was recorded as a provider API turn failure. No stream stall, event loss, disk exhaustion, or supervisor cancellation was observed in run 20. Test jobs entered the durable store and produced exact IDs. Two failed jobs were polled through terminal states; the third job became terminal in the store after the agent stopped, but the model did not observe that terminal status. No retry or unbounded recovery occurred.

The run 19 no-running-state result was invalid as a product diagnosis because its prompt omitted `workdir: "fixture"` and `execution_mode: "background"`. Run 20 used the corrected prompt. Its short failing package-test jobs still did not overlap useful work; that is task/model performance evidence, not a missing job-status event.

## 7. Migration and compatibility review

No migration, durable schema, protocol, or compatibility behavior was changed in runs 18–20.

## 8. Security review

The fixed route, request ceiling, time limit, disk floor, fixture workspace, and sandbox profile were recorded for each run. The private verifier was run only by the harness and its contents were not examined. Agent boundary probes remained subject to the existing refusal rules; no attempt was retried through an alternate authority.

## 9. Documentation and operations

- Per-run summaries exist for all 20 ledger entries, including the three pre-admission attempts; the run 18–20 summaries reference their private receipts and telemetry hashes.
- Append-only campaign ledger: `plans/closure/harness-lab-self-improvement-resumption/evidence/s016-live-authorization-20261031.json`.
- Prompt revisions: S016 v1–v6; run 20 used prompt v6, which explicitly requested the fixture workdir and background test mode.
- Current operational status: the 20-run authorization is consumed. Do not start another provider run without a new explicit authorization.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | No H007 run completed the two repair cycles or qualified the 30-minute horizon. | The intended sustained autonomous capability remains unproven. | Successor S017 planning must identify a measurable path under the frozen task contract and obtain fresh explicit live-run authorization. |
| medium | DeepSeek consumed 60 requests in 231 seconds in the final run and repeated the first crate test without reaching the dependent phases. | The configured cap prevents further progress in this run; no evidence shows the cap itself is malfunctioning. | Decide whether the existing 60-request cap can support the objective or whether a separately authorized budget revision is needed. Do not treat normal N+1 refusal as a CodeGG defect. |
| medium | Short test jobs ended in under 2.6 seconds, before the model could overlap meaningful dependent edits. | A `running` snapshot did not amount to genuine productive overlap. | Future prompt/task design should submit a naturally longer background job after the relevant code exists, then measure actual productive overlap and terminal observation. |
| low | The final third test job became failed in the durable store after the agent stopped, without an exact terminal poll in the transcript. | Model-visible lifecycle evidence is incomplete for that job. | Preserve exact-ID polling through terminal in any newly authorized run. |

## 11. Roadmap disposition

S016 is **conditionally closed** at the exhausted 20-run authorization limit. This closes the bounded diagnostic/corrective loop, not the sustained-agent capability. `stable_live_not_established` remains in force. S017 is the residual planning owner; it should not assume a CodeGG production defect without new evidence and must request fresh live authorization before provider execution.

## 12. Registry updates

- Mark S016 **conditionally closed** in `plans/registry.md` and the Harness Lab roadmap.
- Link this closure record from the registry and roadmap.
- Keep `stable_live_not_established`; no further S016 run is authorized.
