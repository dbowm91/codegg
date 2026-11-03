# Harness Lab Milestone S016 — Closure Status

Status: **renewed authorization exhausted (20/20 cycles); no H007 qualification**

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

The original 20-run authorization was consumed, but the user issued a separate renewed authorization on 2026-11-02 for up to 20 additional DeepSeek run/fix cycles with a 750-request ceiling. Attempt 21 exposed a CodeGG context-accounting/false-success defect, corrected at `e15be6c9`. Attempts 25–31 then repeatedly reached the provider's 1,048,576-token ceiling well below the 750-request cap. Attempt 31 showed why: the OpenCode Go DeepSeek route inherited a 32k “local model” profile, and the live runner omitted automatic compaction, causing non-durable compatibility compaction on nearly every turn. The forced emergency path could not reduce the accumulated history enough to retry. Commit `0121ad1` binds the H007 route to the observed provider limit and enables early durable hybrid compaction. Attempt 32 validated the config path (effective preflight limit 1,030,704 tokens, no per-turn compaction churn), completed four repair cycles and the final workspace test, then ended normally after 7.3 minutes and 95/750 requests. Qualification still failed: the priority background test terminated before productive overlap, and the unchanged hidden verifier exited 1. There were no event gaps, activity stalls, or disk-floor events in attempts 25–32.

`stable_live_evidenced` remains unestablished. Attempt 34 demonstrated complete job lifecycle and productive overlap, with complete tool-event observation and no request, context, stall, or disk failure. Attempt 35 then tested v11, which added more demanding differential/recovery work and two staged reviewers. The model ended normally after 10 minutes with `next_phase: durable_import`, no completed implementation phase, and the required priority job unreached. Its one observed child run remained `running` after the parent turn and lane daemon ended. The provider admitted only 88/750 requests; all 79 ACP calls paired, there were zero event gaps, no inactivity cancellation, and disk stayed above 210 GiB. This is incomplete model execution under an over-complex prompt, not provider/request/stream/event/disk failure. Prompt v12 returns to the manageable v10 scope and adds a concise completion gate for phases and exact child/job IDs. Five cycles remain.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Use only `opencode_go/deepseek-v4.1-flash` | Original and renewed append-only ledgers | pass | No other route was used. |
| Keep the H007 task, contract, corpus, public tests, verifier, and acceptance fixed | Fixture hashes in each run summary; verifier SHA-256 `13617671ceedfe1b4cff89c9a96ad5380fd94ca4799f861f95a404f84090d33c` | pass | Only versioned agent prompt/diagnostic manifests changed. Private verifier content was not inspected. |
| Enforce per-run request ceiling, 3,600 seconds, and 10 GiB free disk | Original and renewed ledgers/receipts | pass | Original campaign used 60 requests; renewed campaign uses 750. Attempt 21 admitted 435 requests, stayed above the disk floor, and stopped after provider context overflow. |
| Continuous provider streams and complete ACP tool-event pairs | Run 20 summary: 60/60 response bodies reached EOF; 52/52 tool starts paired; zero gaps | pass | No idle timeout or activity-stall cancellation occurred. |
| Exact-ID background job lifecycle with productive overlap | Attempts 32–33 receipts and exact-ID traces | fail | Attempt 33 observed the exact priority ID terminal after 3.62 seconds; productive edits came afterward. No ACP event loss or job-ID projection failure was reproduced. |
| Two seeded failure/repair cycles and five dependent implementation phases | Run 20 horizon receipt and tool trace | fail | Zero repair cycles; work remained on repeated `catalog-format` checks and did not reach `catalog-store` or the priority overlap phase. |
| At least 30 minutes of substantive active work, with gap ≤5 minutes | Attempt 33 horizon receipt | fail | Active work was 560.603 seconds; maximum measured gap was 58.739 seconds. The model completed all five phases and ended normally; the frozen prompt directs honest early completion and forbids padding. |
| Continuation and independent verifier | Attempt 21 receipt | fail | Continuation remained pending; unchanged verifier exited 1 with zero tests recorded. Its source and contents were not inspected. |
| Stop after qualification or renewed 20-cycle cap | Renewed authorization ledger | active | Four of 20 renewed cycles are recorded: attempts 21–24. Two were long qualification runs; two were diagnostics, and attempt 23 selected the wrong manifest. |

## 3. Production implementation evidence

The ARM64 attempt 32 binary SHA-256 is `25cdef11543d434d0e9f0fc20c154ee72f8ec16ad078af956da616cb4e329f10`; its sibling sandbox helper SHA-256 is `27723caa3dccd0e999055bc252468de5d4f72876524c7a873b761ab2157a7479`. Corrective `0121ad1` adds provider-context recovery and an H007 run configuration with the observed context limit and explicit durable Hybrid/Balanced compaction. The frozen H007 fixture, verifier, and acceptance criteria remain unchanged. Prompt v9 changes only the ordering of the already-required priority background job so it is not launched against placeholder query/CLI code.

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

- Per-run summaries exist for all original 20 entries and renewed attempts 21–32; renewed summaries reference their private receipt hashes and telemetry.
- Append-only campaign ledger: `plans/closure/harness-lab-self-improvement-resumption/evidence/s016-live-authorization-20261031.json`.
- Prompt revisions: S016 v1–v6; run 20 used prompt v6, which explicitly requested the fixture workdir and background test mode.
- Current operational status: renewed campaign `s016-deepseek-raised-cap-20261102` has 8 of 20 cycles remaining. Route, frozen source fixture, verifier, and acceptance remain unchanged. Prompt v9 changes only the placement of the priority job and its existing required overlap edits.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | No H007 run completed the two repair cycles or qualified the 30-minute horizon. | The intended sustained autonomous capability remains unproven. | Successor S017 planning must identify a measurable path under the frozen task contract and obtain fresh explicit live-run authorization. |
| medium | DeepSeek consumed 60 requests in 231 seconds in the final run and repeated the first crate test without reaching the dependent phases. | The configured cap prevents further progress in this run; no evidence shows the cap itself is malfunctioning. | Decide whether the existing 60-request cap can support the objective or whether a separately authorized budget revision is needed. Do not treat normal N+1 refusal as a CodeGG defect. |
| medium | Short test jobs ended in under 2.6 seconds, before the model could overlap meaningful dependent edits. | A `running` snapshot did not amount to genuine productive overlap. | Future prompt/task design should submit a naturally longer background job after the relevant code exists, then measure actual productive overlap and terminal observation. |
| low | The final third test job became failed in the durable store after the agent stopped, without an exact terminal poll in the transcript. | Model-visible lifecycle evidence is incomplete for that job. | Preserve exact-ID polling through terminal in any newly authorized run. |

## 11. Roadmap disposition

S016 remains active under renewed authorization. The context overflow/false-success defect is corrected at `e15be6c9`; the model-profile/automatic-compaction misconfiguration is corrected at `0121ad1` and exercised in attempt 32. Attempt 32 completed its five implementation phases and final workspace test quickly, but did not meet the sustained horizon or exact-ID productive-overlap requirement. This is evidence against request-limit exhaustion and against an ACP event-gap failure; the current open questions are whether the prompt's job ordering can produce the required overlap and whether the frozen task can naturally exercise a 30-minute horizon without padding. Prompt v9 makes that ordering change without changing acceptance. `stable_live_not_established` remains in force.

## 13. Attempts 25–32 and corrective disposition (2026-11-02)

Attempts 25–30 repeatedly stopped at provider context overflow between 284 and 447 requests, with all ACP tool starts paired and no activity-stall or disk-floor event. Attempt 28 was a preflight-only timeout with zero provider requests. Attempt 31 reproduced the provider's exact 1,048,576-token maximum: it rejected 1,051,297 tokens at request 284. The emergency Hybrid/Balanced path created a continuation candidate but kept 354 messages and increased the local estimate from 970,845 to 973,976; checkpoint validation refused installation because the result remained above the send budget. The request cap was not the cause.

Source inspection and attempt 31 telemetry isolated a separate configuration defect: the DeepSeek model-name heuristic assigned a 32k local profile to the cloud OpenCode Go route, while the autonomy launcher wrote only `{"model": ...}` and therefore left `compaction.auto` unset. The turn runtime consequently took `auto=false` compatibility compaction repeatedly, retained the context, and accumulated preserved control reminders. Commit `0121ad1` provides an H007-scoped model-profile override at the observed provider limit and explicitly enables Hybrid/Balanced auto compaction with 35% headroom before that limit. Attempt 32's preflight logs showed an effective 1,030,704-token context limit and `compaction_needed=false` at 70k estimated history, confirming the run config applied. The run ended before reaching the compaction threshold, so long-session compaction recovery itself remains unqualified.

Attempt 32 used 95/750 requests, lasted 439.038 seconds (404.634 seconds active span), had 88/88 paired tool events and zero gaps, and stayed above 32 GiB free disk. The receipt records four repair cycles and continuation. The model's final workspace test completed with 40 passing tests. The exact priority job `4020739d-1b29-483b-9b19-28f063b2f763` failed before productive edits could overlap; the model recorded the miss and did not resubmit it. The unchanged hidden verifier exited 1 (five tests run; three failures); its source was not inspected. The agent then ended normally (`end_turn`) rather than reaching the 30-minute requirement. Prompt v9 moves the existing priority job after the five phases and seeded repairs, while keeping the acceptance criteria, contract, verifier, and route unchanged.

## 12. Registry updates

- Keep S016 **active under renewed authorization** in `plans/registry.md` and the Harness Lab roadmap.
- Link this closure record from the registry and roadmap.
- Keep `stable_live_not_established`; use only the recorded renewed authorization and stop at its 20-cycle ceiling.


## 14. Attempt 33 and task/horizon diagnosis (2026-11-02)

Attempt 33 used v9 and the same ARM64 binary as attempt 32. It completed 130 provider requests, had zero event gaps and complete ACP start/terminal pairing, recorded seven repair cycles and continuation, and kept at least 36.2 GB free. The model ended normally after 9.6 minutes. Its exact-ID workspace job completed in 3.62 seconds, before the subsequent productive edits. The hidden verifier failed; its private output was not inspected. These data do not support request-cap exhaustion, stream/ACP loss, disk pressure, or an unfinished CodeGG turn as the cause of the short horizon. The prompt said to stop when substantive work ended, so the agent's early `end_turn` is consistent with its instructions. The correction is task calibration, not a forced continuation after a completed turn. Prompt v10 adds substantive scale/correctness tasks and bounded harness probes; the H007 verifier and acceptance remain frozen. The next attempt will establish whether those tasks expose a reproducible harness defect or show that this fixture still does not sustain 30 minutes.

Per-run metadata: `plans/closure/harness-lab-self-improvement-resumption/evidence/s016-run-33-summary.json`. Private receipt `/private/tmp/s016-attempt33.json`, SHA-256 `f324d41e5bd014e680f5d21baf4f07d15a0442aca43fdfe9ee51064a98103a0d`. Renewed authorization ledger now records 13 of 20 cycles; seven remain.


## 15. Attempt 34 and overlap diagnosis (2026-11-02)

Attempt 34 used prompt v10 and admitted 118/750 requests. It ran 569.843 seconds with 554.751 seconds of active work, recorded three repair cycles and continuation, and ended normally. ACP observation was complete (113 starts and terminals, zero gaps); the maximum activity gap was 115.795 seconds. Free disk bottomed at 38,180,364,288 bytes and ended at 228,619,943,936 bytes. The unchanged H007 verifier exited 1 after five tests; its output was not inspected. The horizon gate reported `activity_span_short`. Unlike attempt 33, attempt 34 observed an exact-ID job running during productive work and then terminal, so the job lifecycle and overlap path passed in this run. No CodeGG stream, ACP, context, request-cap, or disk defect was reproduced. The agent still ended after about 9.5 minutes; therefore the H007 task remains too small for a 30-minute horizon even after the v10 stress additions.

The six remaining cycles are still bounded by the existing authorization. Prompt v11 adds larger, bounded differential and recovery correctness work plus two staged read-only reviews; this tests whether actual engineering scope, rather than an artificial wait, can meet the horizon. The acceptance, verifier, fixture contract, and route remain unchanged. If attempt 35 again completes below 30 minutes, stop iterating on H007 prompt variants and create a successor plan for a genuinely deeper task. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s016-run-34-summary.json`; receipt hash `c226f00a82f4b20be390a9e16700435d05f21bb14055c9653885c1dae3ad8769`.


## 16. Attempt 35 and premature-end diagnosis (2026-11-02)

Attempt 35 used v11, admitted 88 provider requests, and ended with normal `end_turn` after 603.686 seconds (598.481 active). ACP observation was complete (79 starts and terminals, zero gaps); maximum activity gap was 95.619 seconds. Disk stayed above 210 GiB. The model did not finish the task: its agent-authored continuation record had no completed phases and named `durable_import` as the next phase; it had not reached the priority workspace job. The unchanged hidden verifier exited nonzero and its contents were not inspected. The exact task tool had most recently returned child status `running`; the private lane database still held that run as `running` after the parent turn and harness daemon teardown. No terminal child result was observed in the transcript.

This run did not stop for a CodeGG provider, stream, ACP-event, request-cap, or disk failure. The actionable finding is model follow-through: the v11 prompt stacked several large property/recovery matrices and two staged child reviews, and the model ended before phase 2 rather than completing or polling the child. Attempt 34 showed the same model/route can complete the base five phases and productive exact-ID job overlap. Prompt v12 removes v11's oversized additions and makes the completion gate explicit. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s016-run-35-summary.json`; receipt SHA-256 `cc9880aca692e1617dc731ab55083927b0fd20876d6162dbd1e981e5b7ed8c3d`.


## 17. Attempt 36 and next workload correction (2026-11-02)

Attempt 36 used v12 and ran for 845.327 seconds (819.503 active). It admitted 151 provider requests, recorded 146/146 tool start/terminal pairs, zero event gaps, and a 66.757-second maximum activity gap. Free disk bottomed at 230,305,292,288 bytes. The model completed the five original stages, the added v10 robustness checks, all four repair cycles, a 48-test final workspace suite, and the read-only child lifecycle. Its exact priority job was observed running during productive edits and then terminal. It ended normally below 30 minutes; the unchanged hidden verifier returned failure status, and its contents were not inspected. This run does not show a provider, context, event, job-lifecycle, disk, or stall defect. The bounded workload itself is still too short for the required horizon.

Prompt v13 adds one dependent feature stage: a local `catalog-bundle` crate and CLI support for deterministic snapshot bundle creation, verification, and atomic import. This is substantive scoped Rust work and does not change H007's original contract or acceptance. If it still completes below 30 minutes, use the remaining three authorized cycles only for useful workload calibration; do not reopen stable runtime architecture without a reproduction. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s016-run-36-summary.json`.


## 18. Attempt 37 and priority-job scheduling correction (2026-11-02)

Attempt 37 used prompt v13 and ended normally after 1,079.304 seconds (1,045.124 seconds active), with 196/750 provider requests, 191/191 paired tool events, zero event gaps, and a 96.781-second maximum activity gap. Disk stayed above 212 GiB. The agent completed all six stages and ten repair cycles, but its priority release workspace job completed in 5.64 seconds before the first state observation; no productive edits overlapped it. The horizon gate reports `job_lifecycle_missing`, and the hidden verifier failed; its output was not inspected. This was not a request, provider, event-delivery, context, stall, or disk failure. The prompt placed the job after all implementation and left only two small test edits as possible overlap; the job was over before useful work could start. The active span was still below 30 minutes.

Prompt v14 corrects the sequencing: it places a fresh offline release workspace check after the six existing stages but before a substantive seventh `catalog-delta` feature, begins that feature before the first job poll, and records the exact same job ID through terminal. It also adds deterministic delta creation/verification/atomic-application semantics as a real dependent feature. H007's original acceptance and verifier remain unchanged. Three cycles remain under the existing authorization. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s016-run-37-summary.json`; receipt SHA-256 `b3104731702f49cfedf5fab54aac8f04cc9edecd04acc4aab66012d1aee1ff61`.


## 19. Attempt 38 and tool-policy correction (2026-11-02)

Attempt 38 used v14 and completed all seven implementation phases, twelve repair cycles, the read-only task, and the final workspace tests. It ended normally after 1,104.120 seconds (1,086.836 active), using 179/750 provider requests. All 174 tool calls paired with zero event gaps; the maximum activity gap was 107.353 seconds; free disk stayed above 212 GiB. The priority job was not admitted: its command chained `cargo clean && cargo test`, and CodeGG correctly refused shell metacharacters in the native test tool. The model recorded the exact refusal, did not retry the denied action, and completed the substantive work. No provider, context, request-cap, ACP, stall, or disk failure occurred. H007 remains unqualified because there was no exact-ID priority-job lifecycle or productive overlap and the active span was under 30 minutes. The private verifier returned failure status; its contents were not inspected.

Prompt v15 now uses only the allowed single `cargo test --offline --locked --release --workspace` command and explicitly prohibits shell chaining. It starts the catalog-delta feature before the first poll of that exact job. It deepens the workload with a 100,000-package indexed graph check and an eighth, substantive three-way catalog-merge feature. Two cycles remain under the current authorization. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s016-run-38-summary.json`; receipt SHA-256 `65e71a0a297e662837d6c7e4c6f0e7e273fa742ad50f4e8496d75434fa2087ce`.


## 20. Attempt 39 and first-poll sequencing (2026-11-02)

Attempt 39 used v15 and completed nine feature/qualification phases, twelve repair cycles, the 100,000-package scale test, the read-only child, and the final 53-test workspace sweep. It ended normally after 828.233 seconds (816.723 active), using 150/750 requests. Event observation was complete (145/145 pairs, zero gaps), maximum activity gap 62.615 seconds, and disk stayed above 212 GiB. The priority workspace test was accepted and completed in 5.24 seconds. The model inserted a continuation write before its first status poll, which occurred about 10.3 seconds after job submission; it observed only terminal, with no productive overlap. The horizon remained below 30 minutes. No request, provider, context, ACP, inactivity, or disk failure occurred; the private verifier failed and was not inspected.

The final authorized attempt uses v16. Its first `job status` call must immediately follow the exact workspace-job submission, before any continuation write or other tool call. If the job is still running, the agent then begins a real replication implementation edit and records that same job through terminal. The new ninth feature is resumable, checksummed, paginated catalog replication with atomic complete-feed application. The scale check rises to 250,000 packages to keep the workspace verification substantive. One cycle remains under authorization. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s016-run-39-summary.json`; receipt SHA-256 `8edcffb0d91d01f96236ad6b59fd25587fe12a342c640dcecf3cf8dcccc32d88`.


## 21. Attempt 40 and campaign conclusion (2026-11-02)

Attempt 40 used the last cycle in the renewed authorization. It ended normally after 1,264.062 seconds (1,249.024 active), using 202/750 provider requests. The immediate exact-ID observation correction worked: the priority workspace job was observed `running` by the next tool call, productive catalog-delta edits occurred during the same job, and that exact ID was later observed `completed`. The agent completed all nine implementation stages, 25 repair cycles, the resumable replication feature, and an 80-test final workspace suite. Continuation was confirmed. ACP observation was complete (197/197 pairs, zero gaps); maximum activity gap was 47.439 seconds; disk stayed above 214 GiB.

H007 remains unqualified. The model ended normally at 20.8 minutes, below the required 30-minute substantive activity span. The hidden verifier returned failure status with three failures; its contents were not inspected. No provider, context, request-cap, ACP/event, inactivity, or disk failure was observed. Across attempts 37–40 the model completed increasingly substantial catalog tasks in about 13.6–20.8 minutes; attempt 40 proves the exact-ID job overlap works when polled immediately. The active span remains below 30 minutes. A later public-artifact audit also found a plausible task-contract cause for the three verifier failures: the prompt required adding local crates while the frozen-input list included the three manifests those additions changed; the exact verifier failures remain unknown. Raising the request cap further is not supported by these results (attempt 40 used 202/750). The renewed authorization is exhausted at 20/20; any new live effort requires fresh explicit authorization and a workload designed to exceed 30 minutes through substantive work. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s016-run-40-summary.json`; receipt SHA-256 `df4d05af7ceb7f12b914fbcd34fff1475a3b0953153a081d7362e64eda9d1f31`.

## Post-run task-contract corrective (2026-11-03)

The attempt-40 failure remains unqualified and its private verifier output remains sealed. A separate public-artifact audit found that the v16 prompt required new workspace crates while the case marked the root `Cargo.toml`, `Cargo.lock`, and `catalog-cli/Cargo.toml` frozen. Attempt 40 changed those three files, so this contradiction is a plausible explanation for the reported three failures, not a proven attribution. The existing frozen hashes for README and contract were also stale relative to the checked-in starter.

The offline correction synchronizes the starter/reference README and contract, removes only the three task-required workspace manifests from the frozen-file set, and adds a guard that checks every remaining frozen digest and confirms the three manifests are editable. Prompt v17 clarifies that only local workspace/path-dependency edits are allowed and adds a substantive crash-safe checkpoint/compaction stage. Its manifest SHA-256 is `402b3c1981a3ba5a53a4331a20825e5f6a7fd8479a5de6cae399aa0e618ba4bc`. The metadata regression, assertion-quality guard, duplication guard, and v17 loader preflight pass. No live run was started. H007 is still unqualified, S016's 20-cycle authorization remains consumed, and this corrective requires fresh explicit authorization and hosted review before any provider attempt.
