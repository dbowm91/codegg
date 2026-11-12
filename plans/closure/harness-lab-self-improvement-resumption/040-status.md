# Harness Lab Self-Improvement Resumption S040 — Closure Status

Status: **closed; start accounting reconciled and regression-tested**

Source plan: `plans/implementation/harness-lab-self-improvement-resumption/040-idempotent-live-start-accounting.md`

## Corrective result

The attempt-50 wrapper selected attempt 49 by mistake. This replaced the previous row's request count and did not append attempt 50. Attempt 49 was restored from its mode-0600 safe evidence at 286 requests. Attempt 50 now has a distinct row at 183 requests. No credential value or raw run output was copied into the ledger.

`dev/harness_lab/campaign_authorization.py` now reconciles a provider-started run by unique attempt identity. It preserves the historical offset, checks that the start number is next, enforces grant/model/route/request ceilings before mutation, rejects duplicate or conflicting attempts, keeps counters monotonic, and copies only allowlisted fields. Repeated reconciliation leaves peer attempts untouched.

## Verification

- Six focused campaign-ledger tests passed, including duplicate, overrun, exhausted-grant, wrong-start, decreasing-count, idempotency, attempt-isolation, and field-allowlist cases.
- Assertion-quality and duplication guards passed.
- H007 85, autonomy runner 71, verifier diagnostics 32, Python compilation, and `git diff --check` passed.

Reconciled accounting: 50 counted rows plus one preserved historical offset = 51/200 combined starts; 149 combined starts remain. Grant `s019-supplemental-20261111-04` is 7/40 consumed, with 33 remaining. Attempt 49 remains 286 requests; attempt 50 is 183. Evidence digests are recorded in the ledger and the safe attempt evidence.

H007 remains unqualified. S041 owns the bounded transcript-lifecycle diagnostic correction and fresh v97 attempt.
