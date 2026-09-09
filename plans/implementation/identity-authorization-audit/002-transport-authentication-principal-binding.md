# Identity, Authorization, and Audit Milestone 002 — Transport Authentication and Principal Binding

Status: blocked

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/identity-authorization-audit-roadmap.md#M002--transport-authentication-and-principal-binding`

Long-term requirements: `plans/000-long-term-specification.md#81-personal-local`, `#82-personal-remote`, `#83-team-single-host`, `#85-human-authentication`, `#27-security-requirements`; roadmap Phase 6.

Applicable ADRs: none. Primary class: infrastructure.

## 1. Objective

Resolve every accepted client connection to a canonical principal using trusted transport/authentication evidence and carry that immutable principal through client/request context, while keeping personal-local startup login-free.

## 2. Why this milestone is ready

Blocked on M001 durable principal/membership contract. Existing local socket, HTTP/WebSocket, stdio/inproc transports and `ClientRegistry` provide stable integration seams.

## 3. Current implementation evidence

HTTP middleware validates one global bearer token and returns only success/failure. `ClientRegistry` stores client id/name/time/sessions/capabilities but no principal. Projection transport contexts synthesize local/authenticated-remote opaque principal strings. Local socket handshake already creates daemon-issued client identities.

## 4. Invariants that must not regress

Network team listeners fail closed; client/request payload cannot select its principal; LocalOwner is resolved only by trusted local transport/OS ownership policy; auth secret values never enter logs/events; principal binding is immutable for a connection/auth session; provider credentials remain separate.

## 5. Scope

In: authentication result/context type, local-owner resolver, personal token lifecycle/lookup, HTTP/WS and local socket binding, ClientRegistry principal, projection-context convergence/adaptation, compatibility disposition of global bearer. Out: OIDC/device login, project authorization decisions (M003), node PKI, audit store.

## 6. Required production changes

Core: canonical `AuthenticatedPrincipal`/request authority context. Storage: token records store digest/reference, owner, expiry/revocation, not plaintext. Protocol: authenticated principal metadata/capability negotiation only where safe; no caller-supplied principal. Runtime: transport creates context once and passes it to daemon. Security: constant-time token verification, expiration/revocation, fail-closed listener defaults, local peer validation where platform supports it. Docs: deployment profiles.

## 7. Ordered work packages

A — define authentication result/session and token storage semantics on M001 types.

B — implement personal-token create/revoke/expire/verify without logging secrets.

C — bind HTTP/WS clients and `ClientRegistry` to resolved principals; replace synthetic projection identity with adapter to canonical principal.

D — bind local IPC/inproc/stdio according to trusted local policy; preserve zero-login LocalOwner.

E — disposition old global bearer as migration/bootstrap compatibility only or remove if safe; add fail-closed tests.

## 8. Failure, cancellation, restart, and contention semantics

Revoked/expired credentials fail new authentication immediately. Existing connection behavior after revocation must be explicit and bounded; prefer disconnect/re-auth or per-request validation contract rather than indefinite grants. Token creation/revocation is transactional and restart-safe.

## 9. Compatibility and migration

Existing server token may remain temporarily as an owner/bootstrap credential but MUST NOT masquerade as distinct team identities. Document removal condition. No provider-auth migration.

## 10. Required tests

LocalOwner resolution; token create/revoke/expire/restart; wrong token timing-safe negative; network no-token fail closed; distinct clients/principals; principal cannot be spoofed in payload; projection context uses bound principal; compatibility-token disposition.

## 11. Required verification commands

```bash
cargo test --workspace auth --no-fail-fast
cargo test --workspace transport --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

Security/server/core/protocol architecture and operator auth setup; explicitly state personal-local has no login ceremony.

## 13. Acceptance criteria

Two remote team clients can authenticate as different canonical principals; unauthenticated network access is rejected; local trusted client resolves LocalOwner without login; client registry/request/projection context carries transport-derived identity; secrets are absent from events/logs.

## 14. Stop conditions

M001 not closed; platform local-peer evidence cannot meet the declared trust model without a canonical decision; implementation would require OIDC or node PKI.

## 15. Closure evidence required

M001 closure, transport-by-transport identity table, token lifecycle/restart tests, fail-closed negatives, secret-redaction evidence, global-bearer disposition, exact commands/results.

## 16. Handoff notes

Do not conflate authentication (who) with M003 authorization (may do what). Request DTOs remain locators, never authority.
