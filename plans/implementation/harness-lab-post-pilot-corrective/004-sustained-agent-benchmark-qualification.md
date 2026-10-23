# Harness Lab Post-Pilot C004 — Sustained Agent-Facing Benchmark and Frozen Task Pair Qualification

Status: blocked — hard dependency: C003 strict closure for final autonomy claim; C001 strict closure for any additional paid/live provider call
Repository baseline: `main@2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59` (2026-10-21; inspect latest at handoff).
Source roadmap: `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md#5-milestones`
Corrects M005 `plans/closure/harness-lab/005-status.md` finding F006 and remaining frozen Task 2 pair; preserves M001–M003 closures and original `plans/closure/harness-lab/evidence/live-pilot-20261021.json`.
Long-term requirements: `plans/000-long-term-specification.md` §§23–24, 26, 28–29; `plans/001-terminology-and-domain-model.md`.
Primary class: capability.

## 1. Objective

Qualify CodeGG's full native ACP tool experience during **sustained independent coding tasks** instead of only a short scripted tool fixture. Complete the previously frozen second task comparison *only after budget admission is proven*, and collect defensible measurements of task success, recovery, tool friction and reliability over repeatable 30–60 minute runs. Preserve the separation between CodeGG subject/model capability, harness defects and OpenCode proposer behavior.

## 2. Evidence and readiness

The deterministic 12-case ACP suite/ledger (M003) and private yolo subject lanes (M001/M002) exist; M005's first live comparison used two OpenCode Go routes on the identical `worklog` fixture. LongCat 2.5 passed all ten `python3 -m unittest` cases in 14 tool calls, with provider terminal usage. Muse Spark 1.3 made seven tool calls, then a native failure without terminal usage, no task change, and all ten verifier tests remained failing. The full pilot stopped *before* Task 2 (`ranges`) to comply with an unenforceable usage ceiling. These results are evidence of a partial pilot, **not** a comparative model benchmark with sufficient sample size or a complete autonomous loop.

C001 must resolve/explicitly constrain failure-path telemetry and trustworthy hard spending admission before any new live provider call. C002 must define verifier-backed outcome, repeat-count and metric policy. C003 must prove live autonomous improvement before C004 can **close** the overall self-improvement qualification. Task fixture and deterministic harness cases can be prepared in parallel with those closures, but model-backed claims stay blocked.

## 3. Invariants and non-goals

- Trusted developer workstation; **no VM/container setup**. Use private daemon/data/ACP instances per candidate and task, always record explicit `yolo+full-host` or `yolo+workspace-write`, never claim workspace-write covers unverified OS backends. Explicit Deny must remain effective.
- Independent source fixtures/hidden verifiers are immutable to the agent. The agent cannot replace tests, bypass builds or preload the expected answer. Negative controls remain negative controls, not “tasks failed.”
- Report denominator by **tasks, trials, model route, fixture/baseline SHA, policy, compiler cache state and run duration**. Never aggregate incomparable model revisions or task variants. Missing usage/cost stays `null`; provider eligibility and financial limits are enforced *before* call.
- LongCat/Muse Task 1 receipts retained byte-identically; do not silently rerun or overwrite them, and do not imply those original observations were paired Task 2 outcomes.
- Evaluation completion requires independent task verification, not an agent final message. A provider error/timeout, tool error, permission denial and actual inability to solve task are separately classified.
- No automatic merge/push, model training, always-on CI provider usage, SBC performance tests, general product acceptance testing, broad testing of dozens of changing model names, production daemon policy mutation, or unsupported claims of comparative superiority.

## 4. Work packages

**A — Benchmark taxonomy and frozen cases.** Introduce a versioned opt-in `dev/harness_lab/fixtures/agent_autonomy/` manifest with at least the following separately scoped tests (choose limited, verifiable tasks rather than a huge corpus):

1. Long read/search/patch cycle over an unfamiliar Rust/Python fixture; require whole-file/context recovery without redundant Bash workarounds and verify independent tests.
2. Compiling/debugging a Rust fixture with multiple failing tests and a genuinely broken implementation; verify source change and test results.
3. Shell subprocess lifecycle and background job wait/completion, including a long-running-but-valid call not incorrectly labeled a stall.
4. Repeated but distinct `read`/shell tool calls to exercise `shortcycle`/`equivalentresult` recovery without premature termination; paired with actual identical-failure stall stop control.
5. Worktree/agent delegation task with explicit nonconflicting subagent workstreams and build admission **only where their CodeGG feature milestones are actually closed and live-capable**. Otherwise record not-supported and defer, not false pass.
6. MCP/eggsearch and LSP integration fixtures only when their configuration can be isolated and their returned data verified; do not spawn multiple unmanaged servers to manufacture coverage.

Every case must have an independently executable verifier, resource/usage budget, abort/timeout criteria, clear expected allowed tool surface, representative agent prompt, frozen group/suite digest, protection against changing its own verifier and a test-only/no-external-network default. A task that requires internet access must explicitly disclose endpoints/allowed network.

**B — Pilot Task 2 completion and forensic Muse classification.** After C001 closure and validated spending authority, run the original frozen `ranges` Task 2 for each approved model route from **identical committed baseline copies**, same agent, policy, total stop budget and trial timeout, recording the chosen provider route. Keep Task 1 receipts unchanged; store new evidence as a new linked artifact. If Muse Spark continues to fail, record exact classified terminal and usage reason, not an assumed model weakness. If hard cap cannot be met, **skip live calls** and close only offline fixture preparation as conditional.

**C — Sustained live trials and resource-aware orchestration.** Run at least one 30–60 minute unattended scenario, measured from a frozen task start, under the full tool surface where policy allows. Include cancellation, provider disconnect, restart/abrupt ACP drop, long tool output, context_read, build failures and recovery. Set independent limits on per-trial wallclock, max tool calls, provider requests and spend; require a controlled test that an over-budget/hung attempt stops and cleans daemon/tool/subprocess descendants while the other parallel lane continues. Use separate candidate Cargo target paths or bounded build admission. For real-model sessions record 2+ repeated trials per qualifying case if budget permits; otherwise explicitly report inadequate sample size.
 
**D — Baseline-versus-candidate benchmark comparison.** With C002 scoring contract and C003 actual candidate, run a paired baseline/candidate evaluation over stable test groups, preserving outcomes and nonregression control cases. Report completion rate, exact 95%-uncertainty/CI only when statistically justified, median/P95 when enough samples, time-to-result, tool error/denial, repeated identical reads, number of read slices/context recovery, recovery escalations, resumption success, attempted vs unattempted trials, reported token and cost availability. Account for model stochasticity and cache warm/cold; do not claim a significant speedup from one small sample.
 
**E — Closure and operator handoff.** Document a minimal local command to run `smoke`, low-cost provider-free suite, opt-in live task pair, sustained suite, compare/report and bounded `optimize`. Provide explicit version/route preflight, accepted/blocked criteria, stop condition and expected ledger storage. Avoid new production dependencies.

## 5. Failure, restart, contention

If a provider emits failed turn without authoritative usage or a configured enforceable ceiling disappears, immediately **stop future live calls** and record partial evidence; never use a guessed cost to allow additional runs. Retry ambiguous attempts only as new scored attempt with preserved prior evidence. Cancellation of one lane must not cancel another. After restart, checkpoint and immutable ledger must exactly reconcile trials/attempts and original cumulative deadline, as qualified in C002/C003. Avoid accidental model switch/fallback; failing to reach the requested route should invalidate rather than silently substitute the run.

## 6. Required tests and verification

- Offline: content-addressed fixture loading, hidden verifier isolation, wrong source SHA, duplicate model/trial pair, fake success without source change, no-test mutation, false positive stall, genuine stall, tool truncation context recovery, background job continuation and wrong model route/cost sentinel. Use real daemon provider fixture where possible; do not mock native tool layer into a false positive.
- Real developer workstation: yolo/full-host and workspace-write where supported, distinct daemon/root/endpoint and effective policy, two lanes overlapping, a sustained scenario long enough to exercise repeated tool calls and long waits. Distinguish scripted fixture validation from paid live model trials.
- Live opt-in: Task 2 LongCat/Muse pair if admitted; one 30–60 minute run with model(s) under enforceable limits; report actual observed sample size and every failure.
- Commands: `python3 -m unittest discover -s dev/harness_lab/tests -v`, `cargo test -p codegg --test acp_stdio`, `cargo test -p codegg --lib acp`, `scripts/verify.sh quick`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, documented Python `evaluate`/`compare`/`report` and manual provider-backed pilot commands. Any commands not executed remain plainly unrun.

## 7. Acceptance / stop / closure

**Strict closure** requires: an approved real-provider budget; immutable paired Task 2 receipts or an explicit recorded hard blocker (which means C004 stays conditional); an actual sustained native ACP scenario with verifiable task success/failure, accurate stopped/interrupted outcomes and independently tested resource cleanup; protected negative controls; repeatable baseline/candidate reporting with appropriate caveats; and independent evidence that the self-improvement controller has already closed C003. A single strong-model success and smaller-model failure is not evidence of harness improvement or model ranking.

**Stop** if a route is unavailable/unmetered under mandatory hard cost rules, an unfinished tool spawns untracked children, a task/verifier is candidate-editable, or an unknown provider outcome is counted as success. Do not claim `full-host` capability from a `workspace-write` result.

**Closure** `plans/closure/harness-lab-post-pilot-corrective/004-status.md`, mapping each case/model/run to revision/hash, policy, trial duration, tool errors, terminal evidence, independent verifier, exact spend/usage vs null and sample counts, CI/local tests, original Task 1 artifact link, new Task 2 artifact or blocker and recommendation on changing M004/M005 conditionally closed statuses.

## 8. Compatibility and migration

No mandatory changes to the installed CodeGG binary, ordinary ACP v1 compatibility, production SQLite migration or default user daemon. The sustained suite and comparator formats are independently versioned; existing M003 12-case and original M005 Task 1 receipts remain readable but cannot silently be rescored as new evidence. If a native tool capability is not ready in the source branch, report it as `unqualified` and link its separate subsystem owner; do not add a fake harness implementation.

## 9. Documentation and static guards

Update `dev/harness_lab/README.md` with concise local commands for provider-free smoke and paid opt-in sustained tasks, resource/route admission, expected partial failure handling, usable per-lane worktree/daemon output paths, and how to compare baseline/candidate. Update `architecture/acp.md` only if its actual terminal/trace contract changes. If a safety invariant has repeatedly regressed, add a focused guard in the usual `scripts/verify.sh quick` path; no broad CI matrix of live providers.

## 10. Handoff and closure evidence

The implementer must inspect current CodeGG and pilot evidence before modifying fixtures, run a low-cost deterministic trial first, and preserve unrelated changes. Record the actual list of supported tool families and any deferred worktree/LSP/MCP cases, test command output and exact SHA, foreground/background cancellation behavior, live provider admission proof, total attempted/unattempted trial counts, frozen Task 1 hashes and either Task 2 receipt or a typed no-call blocker. Independent review must distinguish harness regression from model/provider error and declare M005 **conditional** whenever hard budget or complete comparative coverage is not actually demonstrated.
