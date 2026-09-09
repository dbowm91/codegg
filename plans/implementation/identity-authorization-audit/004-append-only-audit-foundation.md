# Identity, Authorization, and Audit Milestone 004 — Append-Only Audit Foundation

Status: blocked

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/identity-authorization-audit-roadmap.md#M004--append-only-audit-foundation`

Long-term requirements: `plans/000-long-term-specification.md#22-audit-architecture`, `#27-security-requirements`; `plans/001-terminology-and-domain-model.md#12-audit-and-observability-terms`; roadmap Phase 11.

Applicable ADRs: none. Primary class: infrastructure.

## 1. Objective

Implement the coordinator-owned append-only structural audit record/store with typed attribution, bounded redacted metadata, deterministic ordering/idempotency, authorized query/pagination/filter/export, and content-retention separation.

## 2. Why this milestone is ready

Blocked on M003 so audit actor/decision semantics are canonical before persistence. Typed `AuditEventId` and all major correlation IDs already exist.

## 3. Current implementation evidence

Tracing/projection/run records provide operational evidence but are not the canonical append-only security audit log. No accepted audit store currently owns global sequence, actor/decision/causation/visibility plus retention/export semantics.

## 4. Invariants that must not regress

Audit structural records are append-only; caller cannot rewrite actor/decision; secrets and raw credential values never enter metadata; content bodies have separate configurable retention; duplicate event IDs are idempotent; audit query itself is authorized; execution must not block indefinitely on audit I/O.

## 5. Scope

In: audit event schema/store/migration, sequence assignment, idempotent append, bounded metadata/redactor/digest, query/filter/pagination/export, retention separation, failure/backpressure policy. Out: comprehensive instrumentation (M005), external SIEM bridge, immutable WORM storage, distributed node sequencing.

## 6. Required production changes

Core/domain: canonical audit event builder accepts trusted auth/causation context. Storage: append-only table/indexes and optional body/reference store with retention. Protocol: bounded query/page/export DTOs with capability negotiation. Runtime: single coordinator sequence authority and bounded writer/backpressure. Security: metadata allowlist/redaction and `audit.read` authorization. Docs: retention/failure guarantees.

## 7. Ordered work packages

A — define schema, visibility/action taxonomy, bounded metadata and correlation fields.

B — implement transactional append/idempotency/sequence and restart-safe migration.

C — implement redaction/content-digest/body-retention separation and secret-negative tests.

D — implement authorized bounded query/filter/pagination/export.

E — define write failure/backpressure observability without silently fabricating audit success.

## 8. Failure, cancellation, restart, and contention semantics

Concurrent append yields deterministic unique sequence; duplicate ID returns prior event. Restart continues sequence safely. Audit write failure follows explicit fail-closed/fail-visible policy per event class and never blocks forever. Query cancellation releases resources.

## 9. Compatibility and migration

New storage is additive. Existing logs/traces are not backfilled as authoritative audit unless a separate migration is justified. Unknown event actions/metadata fields degrade safely in readers.

## 10. Required tests

Concurrent ordering; duplicate append; restart sequence; migration; secret corpus negative; metadata/body bounds; retention expiry preserving structure; pagination/filter stability; unauthorized reads; writer failure/backpressure; export digest/integrity.

## 11. Required verification commands

```bash
cargo test -p codegg-core audit
cargo test --workspace audit --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

New audit architecture/operator retention documentation; security and protocol references.

## 13. Acceptance criteria

Authorized owner can query an ordered attributable structural log across restart; bodies can expire independently; duplicates do not duplicate events; secrets never appear in stored metadata/export; failure/backpressure is bounded and observable.

## 14. Stop conditions

M003 not closed; need for distributed global sequencing/node protocol; proposed design captures raw secrets/content by default; storage choice changes coordinator ownership.

## 15. Closure evidence required

Schema/migration, ordering/idempotency/restart tests, secret-negative corpus, retention/query/export evidence, authorization tests, failure/backpressure behavior, exact commands/results.

## 16. Handoff notes

Keep event metadata deliberately small and structural. M005, not this milestone, owns breadth of instrumentation.
