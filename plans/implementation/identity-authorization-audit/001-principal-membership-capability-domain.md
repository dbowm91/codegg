# Identity, Authorization, and Audit Milestone 001 — Principal, Membership, Role, and Capability Domain

Status: closed; see `plans/closure/identity-authorization-audit/001-status.md`

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/identity-authorization-audit-roadmap.md#M001--principal-membership-role-and-capability-domain`

Long-term requirements: `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`, `#85-human-authentication`, `#87-project-authorization`; `plans/001-terminology-and-domain-model.md#4-identity-terms`; `plans/002-long-term-roadmap.md#phase-6--team-principal-model-and-daemon-authorization-seam`.

Applicable ADRs: none. Primary class: infrastructure.

## 1. Objective

Add the durable canonical principal/project-membership domain and deterministic role-to-capability expansion required by later authentication and daemon authorization, including an explicit LocalOwner principal model.

## 2. Why this milestone is ready

Typed `PrincipalId`/`ProjectId` and durable project identity already exist; project catalog/session/worktree/provider scopes are closed. This milestone does not require transport authentication yet.

## 3. Current implementation evidence

`codegg-core::identity` defines `PrincipalId`; provider connection records already reference principal/project scope. Projection policy uses its own opaque principal wrapper and semantic capabilities. No canonical durable human/service/local-owner membership store or Viewer/Contributor/Maintainer/Owner capability mapping currently owns team policy.

## 4. Invariants that must not regress

Paths never define identity; LocalOwner is explicit rather than an authorization bypass; roles expand to capabilities centrally; unknown capability/role input fails closed; project membership is project scoped; no credential secret belongs in membership records.

## 5. Scope

In: principal kinds/records, membership lifecycle, roles, semantic capability enum/set, deterministic expansion, durable store/migrations, service APIs, restart/idempotency tests. Out: token generation/authentication, request middleware, audit implementation, OIDC, presence/chat.

## 6. Required production changes

Core/domain: add canonical types in `codegg-core`, reusing typed IDs. Storage: durable principal/membership tables or repository-standard store with migration. Protocol: only administrative/domain DTOs needed for later consumers; do not expose secret material. Runtime: service supports concurrent read/update with clear revision semantics. Frontend: none required beyond diagnostics. Security: capability sets are data, never caller-supplied authority. Docs: identity/security architecture.

## 7. Ordered work packages

A — define principal kinds and semantic capability vocabulary aligned to canonical spec.

B — implement Viewer/Contributor/Maintainer/Owner role expansion and membership state/revision semantics.

C — add durable principal/membership store with LocalOwner bootstrap representation and restart-safe migration.

D — add service/query APIs and executable role/capability/membership matrices; document future transport contract.

## 8. Failure, cancellation, restart, and contention semantics

Writes are transactional/idempotent where retried. Concurrent membership update/remove uses revision/transaction semantics so a stale writer cannot silently restore revoked authority. Restart preserves durable records. LocalOwner bootstrap is deterministic.

## 9. Compatibility and migration

No existing global bearer/provider credential record is reinterpreted as a human principal. Existing string principal fields remain compatibility projections until M003. Migration must be forward/restart safe.

## 10. Required tests

Role/capability matrix; principal ID validation; membership create/update/remove; concurrent stale revision; restart persistence; LocalOwner bootstrap; project isolation; malformed/unknown capability negative cases.

## 11. Required verification commands

```bash
cargo test -p codegg-core principal
cargo test -p codegg-core membership
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

Add/update identity and authorization architecture docs; document role mapping and LocalOwner domain semantics without claiming transport enforcement.

## 13. Acceptance criteria

Canonical durable principals and memberships can be created/read/revoked across restart; four initial roles expand deterministically to semantic capabilities; project isolation and stale-update negatives pass; no transport/team capability is falsely claimed complete.

## 14. Stop conditions

Need to change canonical role/capability ownership, introduce external IdP semantics, or reuse provider secrets as user credentials.

## 15. Closure evidence required

Schema/migration evidence, domain/API map, complete role/capability matrix, restart/contention tests, LocalOwner behavior, exact commands/results, known compatibility projections.

## 16. Handoff notes

Keep capability names semantic and operation-oriented. Avoid embedding handler names or frontend roles into the durable vocabulary.
