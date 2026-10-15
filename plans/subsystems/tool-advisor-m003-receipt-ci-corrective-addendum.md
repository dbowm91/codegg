# Tool-Advisor M003 Frozen Receipt — Post-Closure CI Corrective

Status: **active** — C001 **ready**, independently owned by tool-advisor; async C007 is a blocked consumer.
Opened: 2026-10-15, reviewed PR #119 head `247edec5f37851ca3f67db5943675c896d19e9d9` and `main` `90ab44775a36ebe30466a0d6e340f584324de114`; revalidate on execution.
Historical roadmap/closure: `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md`; `plans/closure/tool-selection-advisor-causal-frontier-experiment/003-status.md`. M003 **closed with negative disposition D** (structured effect-path frontend hides necessary gold tools; M002 remains selected). Historical receipt `assets/tool-advisor/causal-frontier-m003-result.json` is evidence, not a disposable CI cache.

## Observed defect, independent ownership and safety

Hosted `CI / verify` run `38650792114`, job `116148778906`, at PR #119 head `247edec5` failed exactly `tool_advisor::causal_frontier::tests::m003_checked_in_receipt_matches_recomputation`: stored effect catalog identity `94ef4d745c79450f8d4b27a0985551c82fb2f572d7f8eba6aa4718c5edfd7649`, fresh `a9c5afe90f2ad6ee7a165ed31d136b1087ff800d81964f0c27362722e228d9fb`. The job ran 4,267 passing tests and failed this one before 9,522 other tests ran. This is a **root-CI blocker regardless of async code ownership**. Existing C005 closure labels it pre-existing; that claim must be verified against `main` with an exact-head diagnostic rather than assumed.

The likely classes are legitimate current native-tool implementation/schema registry change against an immutable historical experiment, invalid catalog identity change, or fingerprint input non-determinism. Do not infer which without a sorted per-tool contract diff. Never adjust the frozen M003 D receipt in place solely to make a test green, mark a test ignored, or globally waive root CI.

| Milestone | Status | Plan | Qualification |
|---|---|---|---|
| C001 receipt identity diagnosis and hosted CI restoration | **ready** | `plans/implementation/tool-advisor-m003-receipt-ci-corrective/001-receipt-identity-reproduction-and-hosted-gate-restoration.md` | Reproduce the stored/fresh mismatch; identify exact changed tool-contract fields and historical baseline; fix correct source or versioned fixture semantics preserving D and M002; target regression + **full hosted CI / verify green on exact repair/integration SHA**, no skip/waiver. |

## Dependency, closure and release governance

This is a distinct tool-advisor corrective, not permission to re-open the M003 experiment or insert unrelated runtime behavior into PR #119. Preferred implementation on a narrow `main`-based tool-advisor branch, then independently verified integration into async PR #119. The C001 closure path is `plans/closure/tool-advisor-m003-receipt-ci-corrective/001-status.md`.

The separate async roadmap `plans/subsystems/async-background-jobs-integration-qualification-corrective-addendum.md` marks C007 blocked until this plan's substantive repair and a fully green exact-SHA root CI are demonstrated. Completion must update both registry dependency and roadmap statuses with specific commits/run IDs; no automatic merge. M003 historical negative result and M002 selected runtime frontier are immutable until a separate explicit experimentation decision.
