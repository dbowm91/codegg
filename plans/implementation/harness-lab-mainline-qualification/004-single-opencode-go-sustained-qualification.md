# Harness Lab Mainline Qualification H004 — Single-OpenCode-Go Live Long-Horizon Run

Status: **blocked** (H001 mainline integration + H002 v2 controls + H003 offline workload qualifications are hard dependencies; fresh operator-approved budget and route admission are operational prerequisites).
Source roadmap: `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`.
Reviewed baseline: `plans/harness-lab-continuation-20261026@cd9fde42491c85941c31efbc2d5d200e8db4d638`.
Normative: `plans/000-long-term-specification.md` §§23,24,26,28,29; accepted ADR-0040; H001–H003 planned closure records; D003 `plans/closure/harness-lab-continuation/003-live-event-gap-sample-20261027.md`. Primary class: capability.

## 1. Objective

Measure CodeGG's ability to perform **one or more coherent 30–60 minute coding projects** through real native ACP/daemon/tools and CodeGG's real background-job machinery without requiring additional upstream provider integrations or artificial repetition of tiny tasks. The 63/63 prior D003 sample established a bounded **short-trial event-gap observation rate** for one host and route, not the success rate of sustained, multi-step coding. This test addresses that different question.

The benchmark must reveal problems, not manufacture a high pass rate. A model that stops early, abandons a job, changes its verifier, exceeds budget, loses tool events, produces unverified code, or requires repeated manual guidance is recorded accordingly.

## 2. Single-provider contract — non-negotiable

- **One CodeGG provider adapter: `opencode_go`.** CodeGG route syntax is `opencode_go/<model-id>`. The provider acts as a proxy to upstream model-serving organizations. Those upstream services are **out of scope** for provider integration, SDK compatibility, adapter parity or provider diversity.
- No second native provider (`openai`, `anthropic`, `google`, `minimax`, etc.) and **no second upstream model route** is needed to pass. Do not call a different route “another provider” in receipts or metrics.
- Prefer the demonstrated, currently routable `opencode_go/muse-spark-1.3-contributor` as a candidate **only if the operator expressly approves it at execution time** and provider preflight confirms its current availability. Otherwise stop or use another expressly approved `opencode_go/<model-id>`; record which model was used and do not blend outcomes from separate models into one scored pool.
- `opencode run` as an optional *patch proposer* has a separate CLI routing namespace; this scenario drives **CodeGG via native ACP**, not the OpenCode CLI's tool executor. No implicit CLI-assisted edits to the scenario.
- Do not infer a dollar cap from “free” model branding or subscription. Missing `cost_usd` is null. Enforce controller-side request and wall-clock caps and all other genuinely enforceable approved guardrails; fail closed at budget-check otherwise.

## 3. Dependencies

Hard: H001 integration is merged and exact resulting `main` head is green, H002 v2 benchmark controls have independent negative tests, H003 real Rust/job fixture has a passing and failing offline native CodeGG path. Operational: explicit maintainer/provider-account authorization for chosen `opencode_go` route; bounded provider requests, observed remaining quota when available, token/cost provenance, wallclock and host disk floor. H004 is **not** authorization to spend or run unattended with host-full permissions today.

No hard dependency on multiple models, on-prem SBCs, delegated cross-worktree writers, remote Eggwork nodes, isolated MCP/LSP or extra CI pipeline.

## 4. Invariants

- The running binary and source SHA are from the **exact merged main head** or an explicitly labeled candidate; no mismatch between baseline and verification artifacts, immutable fixture and hidden verifier, effective policy and requested policy, or benchmark version.
- Use `workspace-write` in owned private lanes unless explicitly approved otherwise. Explicit Deny and protected paths are never relaxed for performance. Real job subprocesses are tied to daemon/session/job identity and canceled/cleaned up at the end. No outbound network from Rust fixture unless separately approved and fully scoped.
- Separate model quality from harness reliability: verifier/correctness, agent continuation, job waiting, tool delivery and request/idle timeout are distinct failure classes. A true compile error corrected by the model is **not** an infrastructure failure; missing tool terminal is **unmeasured** and must not be scored as candidate regression. An agent that ignores output is not a transport flake.
- Native ledger must account for **all requests and every attempt**, including retries and failures. C006 cumulative deadline, D003 typed gap and E002 restart behavior remain in force.
- No self-scored model narratives. Hidden verifier outside the workspace decides correctness. Security, resource, orphan-process and cleanup failures are blocking.

## 5. Ordered qualification

A. **Preflight on latest actual main:** confirm H001–H003 closure references, exact program and verifier SHA-256, fixture version, CodeGG binary hash, provider adapter identifier `opencode_go`, serving model ID, provider catalog/profile resolution, effective approval/sandbox policy, private daemon roots, sufficient host disk/CPU/RAM, source/binary/test target sizes, no prior unmerged source changes. Record root source SHA and no out-of-root writable path.

B. **Budget admission:** maintainer selects route + hard ceiling for provider calls and wallclock, token/quota if enforceable and any genuinely enforced cost authority. Prepare immutable run manifest with `max_provider_requests`, `max_wallclock_seconds`, per-trial/step timeouts, abort safety stop, free disk minimum, and exact number of scenarios. Invoke existing `budget-check`; any refusal ends the experiment before contacting a model. 30–60 minutes is the **intended** test duration, not permission to exceed a smaller approved cap.

C. **Warm-up and negative controls:** independently run existing offline tool, Deny and event-gap fixtures against exact binary and v2 controls; no live provider calls for this. Calibrate build cost with pinned Rust offline artifacts and cold/warm cache labels. Verify background job status really comes from CodeGG's lifecycle rather than a Python timer or Bash busy loop.

D. **Live coherent project:** drive a single long Rust repair task requiring multi-module read/search/patch, compile diagnostics, multiple edits and hidden verifier, with a real asynchronous build/job that progresses while the agent does independent work. Exercise prolonged wait/reentry and possibly separate authorized interruption/recovery subcase. Every tool status tracked by call ID; every job by daemon/turn/job ID. Require passing verifier rather than answer text.

E. **Attribution and controls:** record typed terminal, actual goal/WorkPlan state and context epochs where exposed, tool-call count, tool successes/errors/denials, agent correction steps, provider request/tokens/provenance/null cost, background job schedule/start/wait/completion/cancel, source changes, verification results, gap metrics, recoveries and limits. When measuring a fault scenario, label injection, don't mix it with nominal success denominator; make benchmark independent of model's tendency to repeat a failing call.

F. **Interruption/cleanup:** optional second, separately budgeted real daemon interruption while job runs; verify expected idempotent restart semantics before attempting. Fail closed if a job could have executed twice or an unowned process would be killed. Always stop exact owned daemon generation and child processes, remove owned scratch or retain only explicitly approved evidence, and prove no socket/orphans.

G. **Evidence and scoring:** freeze JSONL receipts and one human summary with schema/digests. Report scenario attempts, solved/failed/unmeasured/stopped/not_attempted, exact error categories, time against cap, request/token count, model ID and provider `opencode_go`; confidence intervals only where repeated independent trials justify them. No “zero flake rate” or provider/model superiority claim from one long scenario. Compare a candidate against a pinned baseline **on the same route and task** only if there is an actual candidate correctness change; do not invent performance benefit or rerun old C003 accepted work.

H. **Review and status:** new `plans/closure/harness-lab-mainline-qualification/004-status.md` with observed progress/limits, independent verifier, negative controls, real source SHA/CI, actual operational spending admission and cleanup. Any uncovered product bug becomes a narrow plan under its genuine owner; record exact failure without masking it. Only update C004 legacy status after separate H002 disposition and accepted reconciliation.

## 6. Verification and run classes

Offline (without contacting OpenCode Go, required before live):
```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
cargo test -p codegg --test acp_stdio
cargo test -p codegg --test tool_programs
scripts/verify.sh quick
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 -m dev.harness_lab doctor --binary target/debug/codegg
python3 -m dev.harness_lab smoke --binary target/debug/codegg --subjects 2
```
Use exact current CLI help to construct the new long-scenario invocation; do not invent flags or silently bypass existing budget checks. Hosted `CI / verify` on resulting main is an operational gate. The optional live run uses a command receipt with exact argv and user-approved budget file location; do not put credentials/policy secrets in committed documents.

## 7. Acceptance / failure / stop

H004 can close **negatively** if the model genuinely fails a protected fixture but the harness truthfully reports it, remains stable and bounded, and all integrity/cleanup gates hold. It need not manufacture a “model success” or multi-model comparison. A true product defect means H004 may close as *measurement completed with corrective required*, not as CodeGG production reliability qualified.

If fresh route/budget authority unavailable, H004 stays **blocked**, even if the previous 63 trials succeeded. Stop on missing event observer health, indefinite provider stall outside cap, denied path bypass, lost daemon/job ownership, hidden verifier edit, unexpected network attempt, orphan child, unauthorized provider fallback, or unrecoverable ambiguous mutation state.

The purpose is defensible long-horizon evidence, not more architecture.
