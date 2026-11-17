# S061 closure — job-status event attribution

Status: **closed at event-attribution scope; attempt 82 completed unqualified**

S061 added the bounded, run-local alias and call-order projection required to attribute failed job-status calls without retaining raw job IDs, arguments, outputs, or transcript content. The focused diagnostic projection checks and exact-head ARM64 build passed before attempt 82. The frozen v97 task, verifier, manifest, route, limits, and acceptance gates were unchanged.

Attempt 82 used Muse Spark 1.3 Contributor through OpenCode Go at source head `2e2b5701eaf79f4f38a2c6d5312f9a14058aeb36`. Qualification duration was 3,299.121 seconds, including 3,282.652 seconds of active work. It admitted 308/1,200 provider requests. The runner timed out at the prompt-work boundary; no external wall or disk stop fired. Free disk was 75,553,091,584 bytes before the run, 75,542,933,504 bytes minimum, and 76,319,694,848 bytes at completion. USD is unknown if unreported.

All seven stages, five contract phases, twelve gates, review, productive release overlap, required final jobs, and exact-ID lifecycle observations were present. All 338/338 tool events paired with zero event gaps; all 87/87 applicable job request/result correlations matched and no job-tool call was unbound. `catalog_format` qualified. `catalog_store` did not: the required productive change between failed seed and rerun was absent, so the repair pair and continuation did not qualify. The independent verifier failed in `continuation_boundary` and `transcript_job_lifecycle`. H007 remains unqualified.

The safe trace showed persistent `tool_execution_failed` status-call classifications for two run-local aliases, J01 (seven calls) and J03 (five calls), plus one `job_not_found` call for J12. Their request/result IDs were correlated. This rules out event or ID loss for these observations. Source inspection identified the concrete attribution: `JobTool::execute_structured` derived the tool-call success bit from the returned job state for every action. A successful `status` lookup of a stored failed job therefore returned `success: false`; the broker surfaced a failed tool call even though the lookup correctly returned the job's failed state. The runner then classified that observation as `tool_execution_failed`, encouraging retries. Lookup failures remain genuine failures. This is a narrow model-facing tool result semantic defect, not evidence of a daemon, scheduler, ACP, or event transport failure.

Attempt 82 safe evidence:

- Preflight: `evidence/s019-attempt82-preflight-20261114.json`, SHA-256 `323544bdd795a8b384d89331a16a6d1973526ad87b57427ba2d3da74501997f6`.
- Diagnostic: `evidence/s019-attempt82-safe-diagnostic-20261114.json`, SHA-256 `7e4d617cd6f12215c505432eb044145b43d46ed58ca63824b383c200cb7e188d`.

The first monitor implementation read the campaign ledger during execution, before the runner had reconciled the current request count. The corrected safe monitor record explicitly marks that the ledger did not observe live provider start and records the provider-boundary safe aggregate poll as the live source; its 308 count matches the reconciled campaign count. This is an external monitor limitation, not a production CodeGG defect. The private receipt, raw output, transcript, verifier source/output, raw provider/tool payloads, and credential values were not inspected.

After attempt 82: 82/200 combined starts, 38/40 starts in grant `s019-supplemental-20261111-04`, and 53/120 task-authorized starts. Remaining: 118 combined, two starts in this grant, and 67 task-authorized. S062 owns the narrow status-query success semantic correction and one frozen-workload requalification attempt if offline checks and preflight pass.
