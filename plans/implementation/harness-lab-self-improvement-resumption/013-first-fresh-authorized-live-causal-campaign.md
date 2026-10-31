# Harness Lab Self-Improvement S013 — First Freshly Authorized Causal Improvement Campaign

Status: **live diagnostic authorized; execution in progress**. S010/S011/S012 offline prerequisites are closed. A known defect is not a precondition for a live diagnostic: discovering an operational failure is part of the objective. A live observation becomes a corrective candidate only after independent trusted reproduction and S011 intake; task-quality or provider-only failures remain distinct. Primary class: **capability**. S003's historical `no_reproducible_defect` closure remains unchanged.

Authority: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`; accepted `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`; `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`. Existing S001–S009 closure records are historical evidence, not re-opened milestones.

Baseline reviewed: `main@2b3785bfd743325f08149e9b6eb41828e823e4df` (2026-10-30), containing PR #219 and S001–S009 closure artifacts. At implementation, re-read `origin/main`, its CI, existing user changes and current `dev/harness_lab` interfaces; use the actual resulting source SHA in every receipt. Do not assume this planning baseline remains HEAD.

Ownership: development-only Python `dev/harness_lab` controls evaluation, qualification, admission and receipts. Native CodeGG daemon/broker remains authority for tools, permissions, jobs and ACP; `opencode_go` is the sole CodeGG provider adapter in scope; OpenCode CLI is an external proposer and never the evaluator.

## 1. Objective and existing evidence

Run a bounded live diagnostic campaign on native CodeGG using the approved `opencode_go/muse-spark-1.3-contributor` route. The live agent must use native tools and delegation, complete a realistic dependent engineering task, create and test a small fixture-local helper tool, and make explicitly frozen, non-destructive probes of tool, path/symlink, job-ID, and error boundaries. A pre-known defect is not required. The campaign may run at most 20 separately receipted attempts, stopping earlier on a reproducible harness failure, integrity concern, unresolved admission/telemetry, or the operator's cancellation. Each attempt has an explicit 60-request ceiling, 3600-second hard stop, 10 GiB disk floor, and 300-second inactivity cancellation; USD cost may be unavailable and must then be recorded as unknown.

The live run discovers candidate operational failures; it does not by itself prove a CodeGG defect or authorize a patch. For each suspected harness issue, classify task/model behavior, provider behavior, and daemon/scheduler/ACP/tool behavior separately. Require an independent trusted reproduction and S011 intake before making a harness change. A genuine defect receives a narrow failing-first correction and follow-up live run under the remaining campaign authorization. If no defect is observed, record only the tested boundaries and limitations; do not infer absence of defects. Do not use the H007 task's task-quality failures as presumed harness defects.

## 2. Preconditions (stop before launch on any failure)

- S010/S011/S012 offline contracts and current native ARM64 binary/runfile/sandbox preflight pass. The live diagnostic may start without an eligible defect dossier. Any proposed corrective must first satisfy S011's two independent native red observations, exact source/binary/verifier and protected controls; refuse task-quality failures, historical C003, synthetic seeded faults or provider-only errors.
- Freeze before proposal: source SHA, lane binary digest, case and suite digests, independent expected verifier, reserved holdout, negative controls, allowed product source prefixes, exact number/repeats of baseline and candidate tasks, success and retry policy, candidate cleanup/retention, resource ceiling, model route(s), budget and cancellation. No adaptive change to pass counts or holdouts after results.
- Record the fresh operator authorization for the single CodeGG subject route, maximum 20 attempts, per-attempt pretransport provider-request ceiling, 3600-second hard stop, 10 GiB disk floor, `workspace-write` profile, 300-second inactivity cancellation, user cancellation, evidence retention, and `USD=unknown` treatment when the route does not report cost. Never reuse a consumed H007/S003 authorization. A subscription allowance is not billed USD. An OpenCode proposer request is outside this diagnostic authorization and must not be started.
- Verify actual installed OpenCode CLI version and live-capable gateway configuration against the frozen endpoint, including plugin/ambient proxy prohibition, credential isolation, redirected/refused request handling and current cap N/N+1 behavior. First use local loopback preflight without charged upstream; a subsequent **live** trial is disallowed until authorization. No implicit multi-provider adapter work.

## 3. Ordered work packages

A. **Freeze the diagnostic task and probes.** Content-address the task manifest, baseline source/binary, independent verifier, route, per-attempt and campaign limits, cancellation rules, probe paths and expected safe boundary behavior. The verifier remains private. A live agent may report an observation but cannot attest that it is a CodeGG defect.

B. **Operational admission proof.** Preflight native CodeGG binary+sandbox helper, disk and path limits, `workspace-write` policy, the CodeGG provider-boundary request cap, and cumulative attempt accounting. Use only the one authorized CodeGG `opencode_go` route. No external proposer gateway is part of this diagnostic.

C. **Execute and inspect.** Run one attempt at a time; preserve every complete or incomplete receipt and inspect it before deciding whether another attempt adds diagnostic value. Enforce the request cap before transport, 300-second inactivity cancel, 60-minute stop, disk floor, no unexpected permissions, private evaluator, and immutable task contract. Preserve task outcome separately from duration qualification and harness attribution.

D. **Independently reproduce and correct.** For a suspected CodeGG defect, freeze a failing-first reproduction outside the live lane and run it in two independent native lanes. Use S011 to classify eligibility. Only then make the narrowest correction supported by the evidence, run focused offline verification, and spend a remaining live attempt to check the corrected operational path. Do not label a model error, task failure, provider timeout, or expected policy refusal a harness defect.

E. **Handoff without promotion.** Generate a review bundle with probe-by-probe outcomes, defect identity if any, independent red/green reproduction, exact patch and binary hashes, all run receipts, provider admission counts, missing/billed cost classification, causal traces, resource/cleanup evidence, residual risks and proposed owning maintainer. Human review owns commit, push and merge. Cleanup must not destroy operator-owned artifacts.

## 4. Safety, failure, storage and migration

All new data remain versioned local sanitized receipts and existing ledger/checkpoint formats. No product protocol or DB migrations; any CodeGG product fix is scoped to the demonstrated defect and normal review process. SIGKILL/ACP loss, unreadable provider response, ambiguous admission ledger, orphaned daemon or unknown terminal must produce typed `inconclusive/blocked` status, retain cumulative budget, preserve owned artifacts for inspection and never replay an ambiguous provider request. Never mask uncontrolled authorization via `--auto` or bypass explicit Deny.

## 5. Verification

Before live execution, run S010–S012 offline regressions and gateway CLI/loopback N+1 preflight; after a real candidate, run appropriate frozen failing-first red/green test, fmt/build/focused tests, native ACP/holdout controls, Python suite, static guards, quick/full verification and exact-head hosted CI on any proposed source changes. Archive code/candidate and measurement SHAs separately; record missing tests as incomplete rather than pass.

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
scripts/verify.sh quick
# Run scripts/verify.sh full and exact-source-head hosted CI when production/integration code changes.
```

## 6. Acceptance, stop and closure

Close S013 as an **observed live diagnostic and corrective cycle** only with accurate terminal outcomes, complete admission/identity/verification receipts, evidence for each probe, and an operator packet. A positive defect is not mandatory to close the diagnostic, but no-defect observations must be bounded to the probes actually attempted. Do not claim `stable_live_evidenced` from one attempt or infer universal harness health from a green verifier. Stop on mismatched pinned inputs, private evaluator access, integrity loss, budget refusal, unauthorized path, sensitive output leakage, uncontained process, unknown provider admission, 20 attempts, or operator cancellation. Closure: `plans/closure/harness-lab-self-improvement-resumption/013-status.md`.
