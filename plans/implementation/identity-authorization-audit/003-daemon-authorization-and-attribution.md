# Identity, Authorization, and Audit Milestone 003 — Daemon Authorization and Originating-Principal Attribution

Status: ready for handoff (unblocked by M002 closure at `plans/closure/identity-authorization-audit/002-status.md`)

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/identity-authorization-audit-roadmap.md#M003--daemon-authorization-and-originating-principal-attribution`

Long-term requirements: `plans/000-long-term-specification.md#87-project-authorization`, `#27-security-requirements`, `#29-system-invariants`; roadmap Phase 6.

Applicable ADRs: none. Primary class: invariant.

## 1. Objective

Enforce project/resource semantic capabilities at the daemon operation boundary using transport-bound principals and propagate immutable originating-principal attribution through sessions, turns, agent runs, jobs, provider selection and tool authority.

## 2. Why this milestone is ready

Blocked on M002. Closed project/session/projection/agent-run/job/worktree/provider systems already expose the resources to protect and attribution fields/seams to extend.

## 3. Current implementation evidence

Projection replay has semantic capabilities and bounded project resolver but not canonical team membership. Tool execution receipts carry a string principal identity; provider connections have principal/project scopes. `handle_request_for_client` is the transport-derived request seam. Team-wide daemon authorization is not yet canonical.

## 4. Invariants that must not regress

Authorization occurs server-side; project enumeration itself is protected; request payload cannot grant capability; LocalOwner goes through the same authorization API with broad local policy; agent effective authority is an intersection and never exceeds origin/parent; membership revocation is race safe; structured denial contains no secret/project leakage.

## 5. Scope

In: authorization service/decision, request capability map, project/resource resolution, privacy filtering, propagation into durable attribution, provider scope checks, projection policy adapter, mutation/read negative matrices. Out: audit persistence (M004), presence/chat, OIDC, node authorization.

## 6. Required production changes

Core: `AuthorizationRequest/Decision` or equivalent with principal/project/resource/capability/policy revision. Storage: consume M001 membership revisions; add attribution fields/migrations only where durable records lack canonical principal. Protocol: typed permission/authorization errors compatible with older clients. Runtime: authorize before side effects and capture decision context with turn/run/job. Security: fail closed on missing/ambiguous project scope; avoid TOCTOU by binding decision revision/operation as appropriate. Docs/static guard: operation-to-capability matrix.

## 7. Ordered work packages

A — inventory every native `CoreRequest` and map it to scope + semantic capability, including enumeration/admin operations.

B — implement centralized authorization service over M001 state with LocalOwner policy and structured denials.

C — integrate at daemon boundary and projection/project listing; prove request DTOs cannot bypass context.

D — propagate canonical origin principal into sessions/turns/runs/jobs/provider/tool receipts and child-agent authority narrowing.

E — add membership-removal/concurrent-request and privacy matrices; reconcile docs.

## 8. Failure, cancellation, restart, and contention semantics

Denied requests have zero side effect. Revocation races must not allow a stale long-lived cache to authorize a new mutation. In-flight work policy must be explicit: origin attribution remains immutable while continuation/cancellation rules use captured authority/policy as specified. Restart rebuilds no broader grants.

## 9. Compatibility and migration

Existing records with absent principal require an explicit legacy/local provenance mapping, never silent fabrication for team data. Projection-local principal wrappers may remain adapters but canonical ID owns semantics.

## 10. Required tests

Complete request/capability table coverage; Viewer/Contributor/Maintainer/Owner allow/deny matrix; project enumeration privacy; session owner vs observer/control; provider scope; membership removal race; child authority escalation negative; spoofed payload principal; restart/migration attribution.

## 11. Required verification commands

```bash
cargo test --workspace authorization --no-fail-fast
cargo test --workspace projection --no-fail-fast
cargo test --workspace agent --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

Authorization architecture, core request capability matrix, projection/security/provider/agent attribution docs.

## 13. Acceptance criteria

All native project-scoped operations have a canonical server-side capability decision; unauthorized users cannot infer project existence/presence; durable work is attributable to origin principal; child/tool/provider authority cannot widen; LocalOwner still uses the same policy API.

## 14. Stop conditions

M002 not closed; a request cannot be scoped without changing canonical domain identity; operation needs a new long-term capability/ownership decision rather than an implementation mapping.

## 15. Closure evidence required

Exhaustive request-capability matrix, role/privacy tests, revocation/contention results, attribution migrations, child-authority negatives, compatibility behavior, exact verification output.

## 16. Handoff notes

Do not sprinkle role checks through handlers. Handler code asks for semantic capabilities; role expansion stays in the authorization owner.
