# Harness Lab Continuation D003 — Live Event-Gap Sample

Status: qualified for the preregistered host, route, and task mix; pooled bound is conditional on trial independence
Run date: 2026-10-27
Source revision: `5a22f59bed969b088378fd8f12b819d3d7747d64`
Binary SHA-256: `dabd498172db3b96247864991a3e5524b9323f548bab0218ca57daa1586fec91`
Provider route: `opencode_go/muse-spark-1.3-contributor`

## Preregistered design and admission

The unit was one instrumented completed trial. The frozen case mix alternated
`worklog` and `ranges`; the target was a one-sided 95% Clopper–Pearson upper
bound at or below 5%, requiring at least 59 independent zero-gap trials. The
admitted hard caps were 63 provider requests, 3,600 cumulative seconds, 300
seconds per trial, and a 1 GiB minimum free-disk floor. The runner admitted
the request and wall-clock caps before starting. No fault injection was used.

## Result

All 63 planned attempts completed and were instrumented. There were 32
`worklog` trials and 31 `ranges` trials. All 63 agent terminals completed;
all receipts report `observed_complete`, zero gap notifications, zero dropped
events, paired tool starts and terminals, zero measurement failures, and
successful lane cleanup. No attempts were unattempted.

| Measure | Result |
|---|---:|
| Instrumented completed trials | 63 / 63 |
| Trials with an observed event gap | 0 / 63 |
| Observed gap rate | 0% |
| One-sided 95% Clopper–Pearson upper bound | 4.643845% |
| Cumulative wall clock | 3,567.128 seconds of 3,600 admitted |
| Provider requests | 63 of 63 admitted |
| Token usage | 1,756,050 input; 22,711 output; 1,778,761 total |
| USD cost | Unreported by provider; recorded as null |

The numeric gate passes for this sampled profile: the observed upper bound is
below 5%. The calculation is `1 - 0.05^(1/63)`. It does not establish a
zero event-gap rate.

## Review and limits

The trial stream was independently checked against its header and summary:
all rows use the same source revision, binary digest, model route, and one of
the two preregistered task fixtures; all 63 rows meet the completed and
observer-health eligibility conditions; the request and wall-clock totals are
within the admitted caps; and all 63 cleanup receipts are clean. The raw
newline-delimited receipts are preserved in
[`003-live-event-gap-sample-20261027.jsonl`](003-live-event-gap-sample-20261027.jsonl)
(SHA-256 `81974fa559fcaf63e4a755bfccf50157c9fb484ff50a36725239e55c899bd53f`).

All trials ran sequentially on one host and one OpenCode Go route. Fresh
private lanes and daemons were used and cleaned between attempts, but shared
host and route conditions can correlate outcomes. Therefore the pooled
Clopper–Pearson bound is conditional on the preregistered trial-independence
assumption and applies only to this binary, route, host, and case mix. It is
not a cross-host or cross-provider reliability claim. No provider-reported
USD cost was available.

## Disposition

The D003/C006 live-rate criterion is satisfied for the preregistered sampled
profile, with the independence qualification above. The earlier 0/14 result
remains historical and is not pooled into this sample. No conclusion is made
about whether the earlier C003 infrastructure error was caused by event loss.
The implementation and measurement work is closed; future routes, hosts, or
case mixes require their own qualification before a broader rate claim.
