# Identity, Authorization, and Audit Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#4-identity-terms`
- `plans/001-terminology-and-domain-model.md#12-audit-and-observability-terms`
- `plans/002-long-term-roadmap.md#phase-6--team-principal-model-and-daemon-authorization-seam`
- `plans/002-long-term-roadmap.md#phase-11--audit-foundation`

Related ADRs:

- None required. The canonical documents already select explicit principals, project-scoped semantic capabilities, daemon-side authorization, LocalOwner composition, and coordinator-owned append-only audit. Any proposal to replace those decisions requires an ADR.

## 1. Purpose and ownership boundary

This roadmap establishes the security/control-plane foundation required for team collaboration while preserving the zero-login personal-local workflow. It owns principal and project-membership records, authentication-to-principal binding, daemon authorization decisions, originating-principal attribution, and append-only structural audit.

It does not own provider credentials (already separate), presence/observation UI, project chat, node PKI, or enterprise identity-provider implementation.

## 2. Work classification

### Invariants

- Every network team operation has an authenticated principal.
- Personal-local operations resolve an explicit `LocalOwner` principal without an interactive login.
- Project existence, presence, sessions and operations are server-side authorized.
- Capabilities are semantic; roles are named bundles, not authorization logic embedded throughout handlers.
- Effective agent/tool authority can only narrow originating principal/project/session/agent/tool/workspace policy.
- Audit structural records are append-only and secret-redacted.

### Capabilities

Distinct team identities; project roles/capabilities; structured denials; attributable audit query/export.

### Infrastructure

Principal/membership/token/auth-session records; transport principal context; authorization service; audit store.

### Polish

Convergence of projection/client principal seams onto canonical `PrincipalId`.

## 3. Non-goals

OIDC/device flow implementation, SCIM, SAML, enterprise IdP, node certificate enrollment, ACLs on arbitrary filesystem paths, chat/presence UI, new auth framework, replacing provider-auth storage.

## 4. Current state

Typed `PrincipalId`, `AuditEventId`, and `ChannelId` already exist. Projection replay has a transport-derived `ProjectionAccessContext` with principal/client/capabilities and bounded project resolver, but uses projection-local identity wrappers/synthetic values. HTTP middleware still checks one deployment-wide bearer token and returns no individual principal. `ClientRegistry` tracks client metadata without principal identity. Tool execution context contains a string `principal_identity` used for authority receipts. Provider connection scope already models principal/project ownership. Session/agent/job/worktree systems are otherwise mature enough to consume attribution.

## 5. Target architecture

Authentication adapters resolve transport evidence to canonical `PrincipalId` plus authentication context. The daemon constructs an immutable request authority context; request payloads supply locators but never capabilities. A centralized authorization service evaluates principal + project/resource + semantic capability + current membership/policy and returns a structured decision. Local IPC resolves `LocalOwner` through trusted local transport/OS ownership where available.

Audit is a separate append-only structural log with typed identity/correlation and bounded redacted metadata. Authorization decisions are directly auditable but audit failure policy is bounded and explicit.

## 6. Dependency graph

```text
M001 principal/membership/capability domain
  |
  `--> M002 transport authentication + principal binding
          |
          `--> M003 daemon authorization + attribution
                  |
                  `--> M004 append-only audit foundation
                          |
                          `--> M005 audit instrumentation closure
```

Dependencies are hard. Existing closed identity, project catalog, session projection, durable agent-run/worktree and provider-connection work are foundations, not blockers.

## 7. Milestones

### M001 — Principal, membership, role, and capability domain
Class: infrastructure. Objective: durable canonical principal/project membership model plus LocalOwner and semantic capability expansion. Exit: storage/restart/migration tests; Viewer/Contributor/Maintainer/Owner expand deterministically; no transport yet claims team authentication.

### M002 — Transport authentication and principal binding
Class: infrastructure. Objective: resolve local/network connection evidence to a canonical principal and attach it to client/request context. Exit: personal-local remains login-free; network listeners fail closed; personal tokens support create/revoke/expire; global bearer remains only a compatibility/bootstrap seam with explicit disposition.

### M003 — Daemon authorization and originating-principal attribution
Class: invariant. Objective: enforce project/resource capabilities at daemon operation boundaries and propagate immutable origin attribution into sessions/turns/runs/jobs/provider selection/tool authority. Exit: privacy and mutation matrices pass; membership removal races fail safely; request payload cannot self-grant authority.

### M004 — Append-only audit foundation
Class: infrastructure. Objective: durable ordered audit store, bounded metadata/redaction, query/pagination/filter/export and retention separation. Exit: restart-safe ordering/idempotency, secret-negative tests, authorized reads, storage-failure policy.

### M005 — Audit instrumentation and attribution closure
Class: capability. Objective: instrument the security/execution/configuration surfaces required by Phase 11 and prove causal attribution from principal through session/turn/agent/job/tool/Git/worktree actions. Exit: representative end-to-end chains are queryable; high-volume writes remain bounded; no secret body leakage.

## 8. Cross-cutting requirements

Schema migrations must be restart-safe. New protocol fields/variants must be versioned/backward compatible. Revocation and membership updates require race-safe authorization, not cached indefinite grants. Secrets are references or digests, never audit metadata. LocalOwner is a normal principal composition, not a bypass. Existing projection semantic capability seam should converge rather than duplicate policy.

## 9. Verification strategy

Each milestone uses focused unit/integration/restart/negative tests, then existing repository verification. No new CI lane or scanner. Security matrices should be table-driven and executable.

## 10. Risks and decision points

Global bearer compatibility can accidentally remain the team identity model; M002 must explicitly disposition it. Principal identity is currently represented by several strings/wrappers; convergence must avoid a flag-day rewrite. Audit instrumentation can become unbounded scope; M005 uses a required event matrix and structured metadata rather than body capture.

## 11. Completion definition

Roadmap closes when M001-M005 have accepted closure records and team network requests are individually attributable and project-authorized, LocalOwner remains frictionless, and required structural activity is append-only auditable without secret leakage.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/identity-authorization-audit/001-principal-membership-capability-domain.md` | `plans/closure/identity-authorization-audit/001-status.md` | — |
| M002 | closed | `plans/implementation/identity-authorization-audit/002-transport-authentication-principal-binding.md` | `plans/closure/identity-authorization-audit/002-status.md` | — |
| M003 | closed | `plans/implementation/identity-authorization-audit/003-daemon-authorization-and-attribution.md` | `plans/closure/identity-authorization-audit/003-status.md` | — |
| M004 | closed | `plans/implementation/identity-authorization-audit/004-append-only-audit-foundation.md` | `plans/closure/identity-authorization-audit/004-status.md` | — |
| M005 | ready | `plans/implementation/identity-authorization-audit/005-audit-instrumentation-attribution-closure.md` | — | — |
