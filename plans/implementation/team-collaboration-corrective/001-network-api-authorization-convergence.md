# Team Collaboration Corrective M001 — Network API Authorization Convergence

Status: ready for handoff

Repository baseline: `4e12ecc192ba7e2192d3a62acce6e15fc1bb1153`

Source roadmap: `plans/subsystems/team-collaboration-corrective-addendum.md#M001`

Long-term requirements: `plans/000-long-term-specification.md#8`, `#15`, `#24`, `#27`, `#29`.

Applicable ADRs: none. Primary class: invariant.

## 1. Objective

Remove the authorization split between `/core` and authenticated HTTP compatibility routes. Every network route that reads or mutates project/session/workspace state must either reuse canonical Core/AuthorizationService scope and capability decisions or be explicitly restricted to LocalOwner compatibility.

## 2. Why this milestone is ready

Identity/authorization M001-M005 are closed. `auth_middleware` already resolves network credentials to canonical principals and `CoreDaemon` already has exhaustive operation descriptors, canonical scope resolution, enumeration filtering, structured privacy-safe denials, and audit attribution. The defect is adoption at the HTTP compatibility edge, not missing authorization infrastructure.

## 3. Current implementation evidence

`src/server/http.rs` mounts session, project, workspace, file, permission/question, provider/tool/config/MCP, event, and WebSocket/Core surfaces behind authentication. Personal tokens become `AuthenticatedPrincipal` request extensions. Most `src/server/routes/*` handlers never consume that extension and instead call `SessionStore`, `ProjectCatalog`, filesystem helpers, registries, or `GlobalEventBus` directly.

`/api/projects` enumerates the full catalog rather than the membership-filtered Core projection. Session mutation routes operate directly on `SessionStore`. Permission/question GET routes expose pending IDs by session and POST routes answer registry entries without project capability evaluation. `/api/event` streams `GlobalEventBus` without project/session filtering.

## 4. Invariants that must not regress

- Authentication is not authorization.
- LocalOwner broad policy still passes through the canonical authorization service; no handler gains a bespoke superuser bypass.
- Team denials remain privacy safe (`project_not_found` or equivalent) and do not reveal hidden project/session/channel existence.
- Task-trigger fire remains its separately authenticated `cggtr_...` capability and must not be converted into a principal credential.
- `/core` behavior and protocol remain authoritative; REST is an adapter/compatibility surface.
- No HTTP handler may infer project identity from cwd.

## 5. Scope

In: every route mounted inside the authenticated API router; route classification; principal extraction; canonical project/session/workspace resolution; project-list filtering; session CRUD/mutations; permission/question listing and response; file routes; workspace/project routes; SSE events; config/provider/tool/MCP disposition; static guards and adversarial tests.

Out: chat ACL semantics (M002), membership UX (M003), controller lease (M004), task-trigger fire semantics, OAuth/OIDC, unrelated server refactors.

## 6. Required production changes

Create one server-side authorization adapter that accepts `AuthenticatedPrincipal`, canonical operation/scope information, and delegates to the existing `AuthorizationService`/Core authority. Prefer translating REST operations into existing `CoreRequest`s and calling a daemon/Core client. Where no Core equivalent exists, classify the compatibility route explicitly and call the same authorization service with canonical scope resolution; do not duplicate role expansion in handlers.

Build an exhaustive route disposition table. At minimum:

- project/session/workspace reads and mutations use the same capabilities as their Core equivalents;
- project enumeration filters by visible membership before building response rows;
- path/file operations require explicit canonical project/workspace context and the corresponding file capability; ambiguous/path-only team requests fail closed;
- permission/question list endpoints require authorized access to the owning session; response endpoints require mutation authority now and are designed so M004 can add controller narrowing without another bypass;
- config/provider/tool/MCP compatibility endpoints with no safe project scope are LocalOwner-only unless a canonical filtered Core operation already exists;
- global `/api/event` is not available as an unfiltered team-principal stream. Either adapt it to the existing authorized subscription/projection machinery with explicit scope or keep it LocalOwner-only compatibility.

Ensure `AuthenticatedPrincipal` is actually consumed at handler boundaries. Avoid trusting body/query `principal`, `role`, or capability fields; none should be introduced.

## 7. Ordered work packages

A. Inventory every authenticated HTTP route and record: canonical owner, scope kind, required capability, privacy behavior, and disposition (Core adapter / shared authorization adapter / LocalOwner-only / separate trigger capability). Add a test/static guard so new routes cannot be mounted without disposition.

B. Add the reusable server authorization adapter and principal extractor. Reuse canonical `ProjectId`/session/workspace resolvers and structured errors.

C. Convert project/session/workspace/file routes. Preserve response compatibility where safe; remove direct mutation before authorization.

D. Repair permission/question routes. Resolve pending item to canonical session/turn before exposing or mutating it. M004 will later add controller ownership; leave one explicit policy hook rather than another registry-only route.

E. Replace or restrict global SSE. Team principals must receive only authorized filtered events through canonical projection/subscription semantics.

F. Classify config/provider/tool/MCP and legacy `/ws`/`/tui` interactions. Do not break `/core`; document any LocalOwner-only compatibility route.

G. Add adversarial multi-principal tests and server architecture documentation.

## 8. Failure, cancellation, restart, contention semantics

Authorization failure occurs before side effect. Revocation between requests takes effect on the next request. Enumeration races may omit newly granted rows but must never include a row after the authorization snapshot says deny. SSE/subscription reconnect reauthorizes rather than trusting prior membership. Route adapters must not hold stale role/capability decisions across requests.

## 9. Compatibility and migration

No storage migration should be required. HTTP clients that used a global bearer/LocalOwner retain broad compatibility. Personal-token team clients may newly receive 404/403-style privacy-safe denials for routes they were never entitled to use; that is the intended security correction.

## 10. Required tests

Add a dedicated HTTP authorization integration suite with at least LocalOwner, Owner, Contributor, Viewer, outsider, revoked member, and two distinct projects. Prove project enumeration filtering, cross-project session GET/mutation denial, file scope denial, permission/question ID non-leakage, response denial, SSE non-leakage, revocation without reconnect, and zero side effects on denied mutations. Pin task-trigger bearer separation.

## 11. Required verification commands

- `cargo test --test identity_m003_daemon_authorization`
- focused server/auth HTTP integration tests introduced by this milestone
- `python3 scripts/check_authorization_matrix.py`
- `bash scripts/check-core-boundary.sh`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `scripts/verify.sh quick`
- `git diff --check`

## 12. Documentation updates

Update `architecture/server.md`, `architecture/authorization.md`, and any route documentation so authentication, authorization, LocalOwner-only compatibility, SSE filtering, and task-trigger exception are explicit.

## 13. Acceptance criteria

A personal token for Project A cannot enumerate, read, mutate, receive events from, or answer pending control items for Project B. Every authenticated route has an executable disposition. LocalOwner compatibility remains functional. No denied request produces a project/session/file mutation or event subscription.

## 14. Stop conditions

Stop rather than invent a new authorization framework. If a route cannot be scoped safely with existing canonical identities, make it LocalOwner-only and record the compatibility limitation. Do not weaken `/core` privacy semantics to preserve a legacy HTTP response.

## 15. Closure evidence required

Closure must include the route-disposition matrix, exact tests proving cross-project denial and SSE isolation, any routes intentionally LocalOwner-only, and confirmation that no authenticated project mutation path bypasses the canonical authorization service.

## 16. Handoff notes

This is the security gate for M002-M006. Keep changes concentrated at server adapters and canonical authorization seams; do not begin chat-policy or TUI work in this milestone.
