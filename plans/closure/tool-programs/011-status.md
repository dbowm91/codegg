# Tool Programs Milestone 011 — Closure Record

Status: conditionally closed — historical implementation and evidence record; strict closure transferred to M012

## Scope and historical disposition

Milestone 011 implemented a substantial production correctness and ownership pass for the native restricted-Python Tool Program path. The implementation was reviewed at `0ae10673eb2d1264909977abf694d6d96fbcac9d9`:

`feat(tool-programs): close production ownership boundaries`

The original closure/governance reconciliation was committed at `705ae2cd`:

`docs(plans): close Tool Programs M011`

The implementation and original evidence remain useful and are retained without rewriting the audit trail. A later post-closure review of merged head `d71a5eee5b31876545981fdb0bd8e437aadee39c` found unresolved high and medium production correctness defects. M011 is therefore conditionally closed and is not the current strict-closure authority.

Current corrective authority:

- `plans/implementation/tool-programs/012-authority-recovery-and-delivery-corrective-closure.md`
- `plans/subsystems/tool-programs-correctness-closure-addendum.md`

Expected strict closure record:

- `plans/closure/tool-programs/012-status.md`

## Post-closure findings transferred to M012

| Finding | Severity | Post-closure disposition |
|---|---|---|
| M011 authority proof is synthesized from constants/digests rather than a scope-verifiable permission/path-policy decision | high | transferred to M012 authority-grant work |
| Broker error-valued results can be treated by the program adapter as successful completed calls | high | transferred to M012 Broker/interpreter failure semantics |
| notification claim/ack is decided in process memory and later upserted, permitting concurrent claims and success after storage failure | high | transferred to M012 transactional delivery work |
| scheduler timeout may drop the parent executor before descendant cancellation, and child jobs lack canonical scheduler-queryable parent lineage | high | transferred to M012 scheduler/child ownership work |
| checkpoints are persisted but not restored and replay identity is not bound to the full authority/context/contract/manifest/workspace/control-flow fingerprint | high | transferred to M012 recovery work |
| child jobs cannot be durably reattached by parent call identity and return no real artifact handles | high | transferred to M012 child/result work |
| typed result projection emits an unconditional empty artifact list and does not verify stored result digest on load | medium | transferred to M012 result-integrity work |
| hosted policies are selectable but normal production runtime construction cannot execute the hosted adapter | medium | transferred to M012 hosted truthfulness decision |
| M011-specific evidence is primarily component-level and does not prove required daemon restart, concurrent claim, child reattachment, or capacity-one mechanisms | medium | transferred to M012 process-level harness |

These findings invalidate the original statement that no unresolved high or medium finding remained. They do not invalidate every mechanism added by M011.

## M011 implementation retained

The following M011 changes remain valid foundations for M012:

- source identity and invocation identity are represented separately;
- an explicit invocation key and serialized execution context are carried in Tool Program jobs;
- direct AgentLoop tool calls and Tool Program calls route through `ToolBroker`;
- input/output schema checks, execution timeout wrapping, cancellation selection, and workspace artifact storage were strengthened;
- interpreter hooks reserve calls, persist successful completions, and emit checkpoints before advancement;
- scheduler-level outer timeout and durable attempt-heartbeat plumbing were added;
- child call sequence identity and narrowed deadlines were added;
- terminal background notifications are derived from typed terminal results rather than actionable submission-time records;
- typed terminal result records are used by foreground waiting instead of reconstructing counters from summary text;
- SQLite notification schema and recovery plumbing were introduced;
- hosted policy parsing and explicit fail-closed behavior were introduced, although production hosted execution remains unreachable.

M012 must refine these foundations rather than discard them without cause.

## Original M011 evidence record

The following commands and results were recorded by the original M011 implementation/closure pass:

- `cargo fmt --all -- --check` — passed;
- `cargo check -p codegg --all-targets` — passed;
- `cargo check --workspace --all-targets --all-features` — passed with no errors;
- Tool Program matrix covering Broker integration, build/test matrix, child recovery, context artifacts, fault injection, lifecycle, notifications, read palette, recovery, runtime, and storage migrations — 193 passed across 11 suites;
- background and M011 contract suites — 14 passed across 2 suites;
- `cargo test -p codegg --lib tool::broker` — 4 passed;
- `cargo test -p codegg-providers backend_policy -- --test-threads=14` — 2 passed;
- migration and repository-owned ownership/security guards recorded as passing.

This evidence establishes useful component coverage. It is not sufficient evidence for M012's strict process-level and concurrent-state closure criteria.

## Known repository-wide baseline gates recorded by M011

The original M011 record documented the following broader conditions:

- `cargo clippy -p codegg --all-targets -- -D warnings` reported six pre-existing errors in untouched `crates/codegg-core/src/projection_replay/` files;
- `python3 scripts/check-tokio-test-flavors.py` reported the repository's pre-existing bare `#[tokio::test]` annotations;
- the capped full workspace all-features test reproduced a stack overflow in the unchanged daemon socket integration test.

M012 does not own unrelated cleanup, but all M012-owned warnings, migrations, tests, process-level scenarios, and guards must be clean.

## Hosted and operational evidence

No live external hosted-provider, Eggpool, or ACP run was claimed by M011. M012 does not require an external service for native correctness. It must, however, ensure production configuration and model-facing schema expose only backends that are reachable through normal runtime construction.

The recommended M012 disposition is explicit native-only production status while retaining the M009 Responses adapter as experimental/library infrastructure, unless a narrow existing runtime injection seam can satisfy the complete hosted production acceptance gate.

## Final disposition

M011 is conditionally closed as a historical implementation and component-evidence milestone. It does not establish strict production correctness for authorization, nested failure semantics, transactional notification delivery, descendant cancellation, restart replay, child reattachment/artifacts, typed result integrity, hosted runtime selection, or process-level evidence.

Strict Tool Programs closure is transferred to Milestone 012. M011 may be cited as predecessor implementation evidence, but no document or registry entry may claim strict subsystem closure until `plans/closure/tool-programs/012-status.md` is independently accepted.