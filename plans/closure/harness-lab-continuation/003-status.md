# Harness Lab Continuation D003 — Closure Status

Status: closed
Implementation head: `56279dfc7dc8ee82f0239d6412b4fa50a42a74d6`
Exact-head CI: [40383901325](https://github.com/dbowm91/codegg/actions/runs/40383901325) — success

## Result

The sustained runner uses the same loss-aware ACP event observer and tool-pairing contract as autonomy. Gap notifications are counted, preserved with attempt provenance, and force an `unmeasured` result independently of the agent's terminal or verifier outcome. Observer health is explicit, accounting avoids double-counting, and retries remain bounded by cumulative budget and deadline controls. Deterministic injected-gap fixtures cover the negative path and preserve prior immutable rows.

## Live qualification

The separately authorized, preregistered live sample completed on 2026-10-27.
Its 63/63 completed instrumented trials observed zero event gaps, giving a
one-sided 95% Clopper–Pearson upper bound of 4.643845%, below the predeclared
5% target. Trial receipts, source and binary identity, budget admission,
cleanup, usage and the independence caveat are recorded in
[`003-live-event-gap-sample-20261027.md`](003-live-event-gap-sample-20261027.md)
and its adjacent JSONL receipt stream. The bound is conditional on independent
trial outcomes and is scoped to the sampled host, binary, Muse Spark route and
two-case mix; no cross-host or cross-provider claim is made. The historical
0/14 result is not pooled into this sample.

D003's implementation, deterministic verification, and preregistered live-rate
criterion are closed. The earlier exact-head CI remains evidence for the
implementation source; the live run records the later plan-only source
revision and exact binary digest it actually used. No production source change
was made for the live measurement.
