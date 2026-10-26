# Harness Lab Continuation D003 — Sustained-Trial Gap Parity and Measured Reliability Bound

Status: closed (implementation); C006 live rate remains not established
Baseline: `main@957fcbf00e17bf108429f0d75dcbf760736ae511`; refresh latest before implementation.
Roadmap: `plans/subsystems/harness-lab-continuation-corrective-addendum.md`.
Prior conditional evidence: `plans/closure/harness-lab-post-pilot-corrective/006-status.md`, `004-status.md`, live C006 14-trial receipt; no change to historical milestone status until separately demonstrated.
Normative references: `plans/000-long-term-specification.md` §§24,26–29 and `architecture/acp.md`; primary class: infrastructure/capability.

## 1. Objective

Bring **`dev/harness_lab/sustained.py`** onto the same loss-aware measurement contract already implemented for `autonomy_run.py`, then produce enough **predeclared** evidence to bound the observable event-gap rate without calling 0/14 a zero rate. Explicitly preserve the already working C006 daemon gap notification, measurement-failure retries, per-candidate cumulative deadlines and provider budget guards.

## 2. Current evidence

C006 made bounded-broadcast gaps visible through `CoreFrame::Error(code=event_stream_gap)`, `ClientEvent::Gap`, ACP `codegg_event_gap` and `codeggEventGap`. Existing `autonomy` identifies gapped trial as unmeasured and can retry subject to a bounded per-pair cap. C006 live suite observed zero gaps in 14 trials (one-sided 95% upper limit ~19%), explicitly **not a qualified low rate**. The three-trial sustained scenario uses its own cumulative cap but does **not read gap notifications**, so it cannot establish either rate or absence in a prolonged session. C006 also documents raw client `subscribe()` remains loss-unaware unless callers choose the safe subscription.

## 3. Invariants and non-goals

- No silent success/failure inference from missing tool events; gap => an **unmeasured trial** with preserved prior agent terminal, gap count, and route/budget/provenance. Absence of observation is not proof of absence of loss unless observer health is asserted.
- Identical gap and tool-pairing semantics across autonomy and sustained runners, preferably one shared typed observation decoder. Do not create a second event log/daemon replay service or change ACP v1 standard fields.
- Exact source/build/model/route, sandbox/approval, fixture/score digest and run attempt IDs; no hidden provider messages or raw arguments in public ledger.
- Spending/wallclock caps remain preflight-enforceable and cumulative across actual supported resumes; completed prior attempts and unknown attempts retained, no invisible extra provider requests.
- No new cost/CPU claims; neither 0 events in N trials nor one sampled latency qualifies performance superiority. Provider-backed sampling manual, separately authorized, credential-free ordinary CI remains fast.

## 4. Dependencies

No new hard dependency. Soft coordination with D002 ACP event shape changes; existing C006 implementation is a closed interface even though statistical qualification remains conditional. CI exact-HEAD green is an operational requirement before claiming product qualification, not a prerequisite for implementing Python code.

## 5. Work packages

A. **Audit paths and contract:** inspect `sustained.py`, `autonomy_run.py`, `supervisor.AcpClient`, `ledger.py`, `src/acp.rs` and typed gap metadata. Identify where events are drained and where same attempt might be double-counted; record observed gap start/last sequence, dropped count, and observer-live status when available.

B. **Shared loss-aware observer:** route sustained trials through the same bounded ACP decoder as autonomy. Preserve terminal results distinctly, count every gap notification and tool started/terminal pair, and label `unmeasured` for gaps irrespective of final verifier pass/fail. Surface `event_gap_count`, `event_observation_state`, `attempt_id`, `measurement_failures`, and resolved/unresolved state in both receipts. Maintain size/redaction controls.

C. **Deterministic forced-gap evidence:** use fixture tool event bursts to overflow the tiny test broadcast/reader capacity and confirm observer receives typed gap (test must fail against old silent-loss implementation). Test correctly ignoring false positives when delivered events have no gaps; ordering across gap; partial tool terminal; transport drop; provider failure; cancellation; and high gap frequency exhausting retry caps under budget. Ensure two parallel daemon lanes remain isolated after killing one.

D. **Statistical plan, not speculative provider usage:** predeclare event-loss unit (per instrumented completed trial versus per ACP frame), independence caveat, provider route, case mix, preflight limits, and a practical upper-bound target, e.g. one-sided 95% Clopper–Pearson bound <=5% for a representative set (approximately 59 independent zero-event trials; if losses observed, more needed). Report the exact denominator and confidence interval or `insufficient_evidence`. The original 0/14 remains in historic record; do not append unrelated runs merely to increase denominator.

E. **Opt-in prolonged pilot:** only after operator approves a new limited provider budget, run repeated sustained 30–60 minute scenarios with fully active gap observation; report tool/terminal error attribution, lost event count, completed/cancelled/unattempted, time and usage from provider or null. If budget not authorized, complete offline implementation with C006's live-rate condition still open.

## 6. Failure/recovery/concurrency

A stream gap may conceal a mutating action; do not replay that tool because its completion is unknown. A measurement retry creates a fresh private lane/run attempt with previously committed evidence immutable and the remaining provider budget reduced. Daemon cleanup targets only owned processes; after restart preserve consumed wallclock from checkpoint+ledger per C006, never reset by supplying a new `--max-wallclock` value under the same experiment identity.

## 7. Compatibility/migration

Extend dev-only receipt schema additively. Previously stored sustained runs lacking gap observer metadata become `observation_state=not_instrumented`, **not** gap-free. Do not mutate 2026-10-22 receipts or promote C004/C006 from conditional based on solely offline fixture coverage.

## 8. Verification

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
cargo test -p codegg --lib core::transport::daemon_socket::tests
cargo test -p codegg --test acp_stdio
scripts/check_projection_disclosure.sh
scripts/verify.sh quick
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```
Report filter count; rerun targeted Rust tests if command path changes. Live trial calls are **not** a normal CI requirement. Verify caught-loss => unmeasured, bounded retry, prior immutable rows, resource budget exhaustion and observer-health falsification.

## 9. Acceptance / stop / closure

Strict D003 implementation close requires sustained and autonomy observer parity, failure-first gap injection, no double counting, exact attempt/denominator, and a complete offline negative matrix. The *C006 live flake-rate condition* closes only after an independently reviewed sample/cap, otherwise remains `not established` even when D003 code closes.

Stop and report if provider spend cap is not admitted, apparent gap telemetry lacks proven producer, confidence interval assumptions are unjustifiable or mutation replay would be necessary. Write `plans/closure/harness-lab-continuation/003-status.md`, plus any optional separate live evidence; include actual exact-head CI, sample count and lower/upper limits or null, and whether C004/C006 conditional gates changed.
