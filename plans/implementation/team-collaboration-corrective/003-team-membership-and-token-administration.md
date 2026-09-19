# Team Collaboration Corrective M003 — Team Membership and Device-Token Administration

Status: implemented (closure at
plans/closure/team-collaboration-corrective/003-status.md;
implementation `f7afefb1`)

Repository baseline: `4e12ecc192ba7e2192d3a62acce6e15fc1bb1153`

Source roadmap: `plans/subsystems/team-collaboration-corrective-addendum.md#M003`

Long-term requirements: `plans/000-long-term-specification.md#8.3-team-single-host`, `#8.5-human-authentication`, `#8.7-project-authorization`, `#25-tui-target-behavior`.

Applicable ADR: `plans/adrs/ADR-0006-project-channel-chat-access-policy.md`. Primary class: capability.

Hard dependencies: M001 and M002 strict closure.

## 1. Objective

Productize the existing team identity primitives: project Owners can inspect/manage project memberships and chat overrides, while LocalOwner can create/disable principals and issue/revoke per-device personal tokens through a secret-safe CodeGG surface.

## 2. Why this milestone is blocked

Membership administration must not be exposed before the network boundary is corrected, and the final `/team` surface should manage the M002 chat policy rather than shipping a second temporary UI.

## 3. Current implementation evidence

`TeamStore` already implements principal creation/status, project membership create/update/revoke, optimistic revisions, and effective capability queries. `PersonalTokenStore` already provides CSPRNG one-time `cggt_...` issuance, digest-only persistence, verification, expiry, and revocation. Production documentation currently describes these primitives operationally, but there is no normal Core/TUI administration family. `/team` is effectively a collaborator/presence alias.

## 4. Invariants that must not regress

- Project `member.manage` may change membership only inside that project.
- Project Owners cannot create/revoke deployment-wide device credentials for principals merely because they share one project.
- Global principal creation/status and personal-token lifecycle remain LocalOwner-only in this milestone.
- Token plaintext is returned once, rendered only in a secret-safe modal, never logged/chat/audited/persisted in TUI state.
- Membership and principal revision checks remain fail-closed.
- `/collaborators` remains ephemeral presence; `/team` becomes durable administration.

## 5. Scope

In: Core DTOs/requests for project membership list/create/update/revoke; LocalOwner-only principal list/create/status and personal-token list/create/revoke; chat-policy admin integration; `/team` TUI; one-time secret presentation; docs/tests/audit.

Out: password authentication, OIDC/device login, invitation emails, group/organization roles, self-service account recovery, cross-deployment identity federation.

## 6. Required production changes

Add canonical Core operations. Membership operations are `DirectProject + member.manage`. Principal/token operations are classified so ordinary team principals fail closed and LocalOwner broad policy is required (for example Opaque operations with no project-derived widening). Token list returns metadata only; create returns plaintext exactly once.

Add audit events for principal lifecycle, membership mutation, token create/revoke metadata, and chat-policy changes; never include token digest/plaintext.

Turn `/team` into a FocusManager-backed project administration surface: bounded member list, role/state, coarse chat policy status, add existing principal, change role, suspend/revoke/reactivate via revision, project/channel chat grant/deny/clear. If the caller is LocalOwner, expose a separate device/principal management path and one-time token dialog. Keep `/collaborators` as the live presence/observe chooser.

## 7. Ordered work packages

A. Add protocol/admin requests with authorization descriptors and bounded DTOs.
B. Add daemon handlers over existing `TeamStore`/`PersonalTokenStore`; no duplicate identity store.
C. Instrument audit and redaction/static guards.
D. Implement `/team` project member administration using spawn-and-complete tasks and stale route/revision guards.
E. Add LocalOwner-only principal/device flow with one-time secret modal and explicit copy/close semantics.
F. Integrate M002 project/channel chat policy editing.
G. Add multi-principal end-to-end and revocation/reconnect tests.

## 8. Failure, cancellation, restart, contention semantics

Stale revisions conflict visibly and do not overwrite. Closing a token modal destroys plaintext frontend state. Retrying token create with no explicit idempotency contract must not silently mint multiple credentials; use an operation/idempotency key or require explicit retry confirmation after ambiguous transport failure. Membership revocation takes effect at the next authorization boundary and invalidates visible admin/chat state on refresh/reconnect.

## 9. Compatibility and migration

No new identity migration should be necessary beyond any M002 policy tables. Existing principals/memberships/tokens appear in the new surfaces. `/team` behavior changes from collaborator alias to durable administration; `/collaborators` remains the compatibility path for presence.

## 10. Required tests

Owner can list/add/update/revoke membership in own project; Maintainer/Contributor/Viewer cannot use `member.manage`; Owner cannot administer another project; project Owner cannot issue global tokens; LocalOwner can create principal and one device token; plaintext appears once and is absent from debug/audit/chat; token revocation rejects reconnect; stale membership revision conflicts; M002 chat override changes are revision-safe; `/collaborators` presence behavior remains unchanged.

## 11. Required verification commands

- `cargo test -p codegg-core team`
- `cargo test -p codegg-core transport_auth`
- `cargo test --test identity_m003_daemon_authorization`
- new team-administration integration/TUI tests
- `python3 scripts/check_authorization_matrix.py`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `scripts/verify.sh quick`
- `git diff --check`

## 12. Documentation updates

Update `architecture/identity.md`, `architecture/server.md`, `architecture/tui.md`, user help for `/team` vs `/collaborators`, and team single-host setup documentation.

## 13. Acceptance criteria

A LocalOwner can create a human principal, issue a device token, add that principal to a project, and hand off the one-time credential without manual database/tool code. A project Owner can subsequently manage that membership and its chat access without gaining deployment-wide token authority.

## 14. Stop conditions

Do not add a new global admin role or remote secret-delivery protocol ad hoc. If LocalOwner-only token issuance is insufficient for a desired deployment, record a future authentication/admin ADR rather than weakening project scoping.

## 15. Closure evidence required

Show exact authorization classifications, one-time-secret proof, cross-project negatives, revocation behavior, TUI screenshots/snapshots where available, and `/team`/`/collaborators` semantic separation.

## 16. Handoff notes

This milestone is intentionally administrator-created-user/token scope matching the long-term staged authentication plan. OIDC/device login is later work.
