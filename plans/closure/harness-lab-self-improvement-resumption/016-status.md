# Harness Lab Milestone S016 — Closure Status

Status: **active under renewed authorization; cycles 1–4 used, no qualification**

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

The original 20-run authorization was consumed, but the user issued a separate renewed authorization on 2026-11-02 for up to 20 additional DeepSeek run/fix cycles with a 750-request ceiling. Cycle 1 (attempt 21) ran for 39.6 minutes and exposed a CodeGG defect: request 435 was rejected because the prompt exceeded DeepSeek's 1,048,576-token context limit by 807 tokens, while compaction omitted assistant tool-call arguments and tool definitions. Follow-up swallowed the provider error and published a successful `completed` terminal. Commit `e15be6c9` counts those inputs and propagates provider failures. Cycle 2 (attempt 22) showed no provider rejection or event loss, but the model used 100/750 requests, only read/bash tools, made no source changes, and ended normally. Cycle 3 selected the default seven-case manifest instead of the intended diagnostic due to a root-level manifest path. Cycle 4 used the correctly selected tool-surface diagnostic and showed native task/write/edit/test/job tools work with paired events and no tool errors. H007 still did not qualify; the latest evidence points to model/task execution after the context corrective rather than a reproduced tool execution failure.

`stable_live_evidenced` remains unestablished. The renewed authorization remains active; the narrow context-accounting and failure-propagation correction is being verified before the next sequential long attempt.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Use only `opencode_go/deepseek-v4.1-flash` | Original and renewed append-only ledgers | pass | No other route was used. |
| Keep the H007 task, contract, corpus, public tests, verifier, and acceptance fixed | Fixture hashes in each run summary; verifier SHA-256 `13617671ceedfe1b4cff89c9a96ad5380fd94ca4799f861f95a404f84090d33c` | pass | Only versioned agent prompt/diagnostic manifests changed. Private verifier content was not inspected. |
| Enforce per-run request ceiling, 3,600 seconds, and 10 GiB free disk | Original and renewed ledgers/receipts | pass | Original campaign used 60 requests; renewed campaign uses 750. Attempt 21 admitted 435 requests, stayed above the disk floor, and stopped after provider context overflow. |
| Continuous provider streams and complete ACP tool-event pairs | Run 20 summary: 60/60 response bodies reached EOF; 52/52 tool starts paired; zero gaps | pass | No idle timeout or activity-stall cancellation occurred. |
| Exact-ID background job lifecycle with productive overlap | Run 20 receipt, transcript, causal trace, and durable job rows | fail | Two exact IDs were observed `running` then `failed`; a third was left at `running` in the model transcript and later failed in the durable store. All three jobs ran for 1.588–2.591 seconds. No productive edit overlapped actual job execution; the priority release-workspace job was never submitted. |
| Two seeded failure/repair cycles and five dependent implementation phases | Run 20 horizon receipt and tool trace | fail | Zero repair cycles; work remained on repeated `catalog-format` checks and did not reach `catalog-store` or the priority overlap phase. |
| At least 30 minutes of substantive active work, with gap ≤5 minutes | Run 20 horizon receipt | fail | Active work was 228.790 seconds; maximum measured gap was 31.027 seconds. The 60-request cap ended the attempt after 231.199 seconds of case time. |
| Continuation and independent verifier | Attempt 21 receipt | fail | Continuation remained pending; unchanged verifier exited 1 with zero tests recorded. Its source and contents were not inspected. |
| Stop after qualification or renewed 20-cycle cap | Renewed authorization ledger | active | Four of 20 renewed cycles are recorded: attempts 21–24. Two were long qualification runs; two were diagnostics, and attempt 23 selected the wrong manifest. |

## 3. Production implementation evidence

The ARM64 campaign binary for attempt 21 includes the pair-safe `DropMiddleMessages` correction from `94fc8d41` and request-cap correction from `d6b6364c`; attempt 22 onward used the ARM64 binary built from corrective commit `e15be6c9`. The frozen H007 fixture, verifier, and acceptance criteria remain unchanged. Prompt v8 is a separately versioned execution-order calibration and does not change task scope or acceptance.

## 4. Verification executed

### Commands run

- Three live autonomy runs were executed after run 17: run 18 long qualification, run 19 bounded job diagnostic, and run 20 final long qualification. Their exact manifests, binary identity, caps, receipts, and artifacts are in the authorization ledger and per-run summaries.
- The harness automatically invoked the unchanged H007 verifier after each case. Per-run summaries record only its status metadata.
- Safe diagnostic inspection covered provider-admission receipts, body-free CodeGG logs, ACP causal traces, transcript tool metadata, and read-only durable job state. No hidden verifier contents were inspected.

### Results

- Run 18: 60 requests, 305.275 seconds case duration, one repair cycle, no observed running/terminal overlap, task inconclusive.
- Run 19: 6 requests, 17.529 seconds, exact ID queried, but the diagnostic command ran from the case root in foreground mode and failed in 80 ms; this was corrected in the prompt, not product code.
- Attempt 21: 435 requests, 2,376 seconds active-work span, four repair cycles, no productive job overlap; request 435 failed with a context-window overflow and the harness recorded a false-success terminal. The unchanged verifier exited 1 with zero tests recorded; its source and contents were not inspected.
- Attempt 22: 100/750 requests, 479 seconds active-work span, no source edit or native test/job/task calls, normal `end_turn`, no provider error or event gap. The unchanged verifier exited 1 with zero tests recorded; its source and contents were not inspected.
- Attempt 23: wrong manifest selected; 88 requests across the default seven-case suite, 99/99 paired tool events, zero gaps; excluded from H007 conclusions.
- Attempt 24: focused tool-surface diagnostic, 50/100 requests, 43/43 paired tool events, zero gaps, no failed calls. The model exercised `task`, `write`, `edit`, `test`, and `job`, then ended normally. The unchanged verifier exited 1 with zero tests recorded; its source and contents were not inspected.
- No repository test suite was run during this final closure pass. The changed files are plans, evidence records, and prompt/diagnostic manifests; no Rust source was changed in runs 18–20.

## 5. Invariant review

- Provider route stayed on `opencode_go/deepseek-v4.1-flash`.
- The run binary was ARM64 with SHA-256 `16481cc8bf0d8866db352c3aab90c35950bde37e5799be1458a43012d594fe2a`; the sandbox helper SHA-256 was `37ee6c8cd876d1e05bbd650c7a26520753fe55534ac821a52d9089712d751d43`.
- Request admission refused N+1 before transport. All admitted responses reached body EOF; ACP tool starts and terminals paired.
- The 10 GiB disk floor was maintained. Run 20 ended with about 37.5 GiB free.
- Hidden verifier hash and H007 frozen fixture hashes remained stable. No secrets or host environment were inspected.

## 6. Failure and recovery review

Attempt 21 did not stop at the request ceiling: it used 435/750 requests. Its final provider call was rejected with HTTP 400: `Input too long: model limit 1048576 tokens, requested 1049383 tokens`. The accounting and false-success defect is corrected at `e15be6c9`. Attempt 22 used 100 requests, well under the 750 limit; the provider returned `end_turn`, all 100 tool events paired, no tool call failed, and the model had used only `read`/`bash` with no source edit, test, job, or task call. Attempt 24 exercised those omitted native tools in a short diagnostic: 14 task, 2 write, 5 edit, 2 test, and 2 job calls, with no failed tool calls or event gaps. This establishes availability of those tool paths, not H007 correctness or horizon qualification. Attempt 23 is excluded from H007/tool conclusions because the default manifest was selected instead of the intended diagnostic.

The run 19 no-running-state result was invalid as a product diagnosis because its prompt omitted `workdir: "fixture"` and `execution_mode: "background"`. Run 20 used the corrected prompt. Its short failing package-test jobs still did not overlap useful work; that is task/model performance evidence, not a missing job-status event.

## 7. Migration and compatibility review

No migration, durable schema, protocol, or compatibility behavior was changed in runs 18–20.

## 8. Security review

The fixed route, request ceiling, time limit, disk floor, fixture workspace, and sandbox profile were recorded for each run. The private verifier was run only by the harness and its contents were not examined. Agent boundary probes remained subject to the existing refusal rules; no attempt was retried through an alternate authority.

## 9. Documentation and operations

- Per-run summaries exist for all original 20 entries and renewed attempts 21–24; renewed summaries reference their private receipt hashes and telemetry.
- Append-only campaign ledger: `plans/closure/harness-lab-self-improvement-resumption/evidence/s016-live-authorization-20261031.json`.
- Prompt revisions: S016 v1–v6; run 20 used prompt v6, which explicitly requested the fixture workdir and background test mode.
- Current operational status: renewed campaign `s016-deepseek-raised-cap-20261102` has 16 of 20 cycles remaining. Route, frozen source fixture, verifier, and acceptance remain unchanged; prompt v8 only narrows discovery and foregrounds ordered implementation/job steps.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | No H007 run completed the two repair cycles or qualified the 30-minute horizon. | The intended sustained autonomous capability remains unproven. | Successor S017 planning must identify a measurable path under the frozen task contract and obtain fresh explicit live-run authorization. |
| medium | DeepSeek consumed 60 requests in 231 seconds in the final run and repeated the first crate test without reaching the dependent phases. | The configured cap prevents further progress in this run; no evidence shows the cap itself is malfunctioning. | Decide whether the existing 60-request cap can support the objective or whether a separately authorized budget revision is needed. Do not treat normal N+1 refusal as a CodeGG defect. |
| medium | Short test jobs ended in under 2.6 seconds, before the model could overlap meaningful dependent edits. | A `running` snapshot did not amount to genuine productive overlap. | Future prompt/task design should submit a naturally longer background job after the relevant code exists, then measure actual productive overlap and terminal observation. |
| low | The final third test job became failed in the durable store after the agent stopped, without an exact terminal poll in the transcript. | Model-visible lifecycle evidence is incomplete for that job. | Preserve exact-ID polling through terminal in any newly authorized run. |

## 11. Roadmap disposition

S016 is active under renewed authorization. The context overflow/false-success defect is corrected. Attempt 22 did not reproduce it and showed no harness execution failure; the model ended after exploratory read/bash work. Attempt 24 proved that a short prompt can elicit native delegated, write, test, and job calls with no event loss. Prompt v8 now constrains discovery and requests immediate ordered action while preserving acceptance. The next long attempt will determine whether that calibration helps. `stable_live_not_established` remains in force.

## 12. Registry updates

- Keep S016 **active under renewed authorization** in `plans/registry.md` and the Harness Lab roadmap.
- Link this closure record from the registry and roadmap.
- Keep `stable_live_not_established`; use only the recorded renewed authorization and stop at its 20-cycle ceiling.
