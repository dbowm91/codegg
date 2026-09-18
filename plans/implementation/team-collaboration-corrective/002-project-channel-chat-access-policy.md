# Team Collaboration Corrective M002 — Project/Channel Chat Access Policy

Status: implemented (closure at
plans/closure/team-collaboration-corrective/002-status.md;
implementation `2f4bec32`)

Repository baseline: `4e12ecc192ba7e2192d3a62acce6e15fc1bb1153`

Source roadmap: `plans/subsystems/team-collaboration-corrective-addendum.md#M002`

Long-term requirements: `plans/000-long-term-specification.md#15`, `#21`, `#27`.

Applicable ADR: `plans/adrs/ADR-0006-project-channel-chat-access-policy.md`. Primary class: capability.

Hard dependency: M001 strict closure.

## 1. Objective

Implement the accepted chat access overlay so a Viewer can be explicitly granted project/channel chat without receiving write authority, Contributor+ remains chat-enabled by default, and owners can grant/deny chat at project or channel scope.

## 2. Why this milestone is blocked

The current `chat.v1` daemon boundary is safe, but exposing new team administration before M001 closes would leave inconsistent network authority. Begin only after M001 closure confirms there is no authenticated compatibility bypass.

## 3. Current implementation evidence

`ProjectRole::capabilities()` grants `ProjectChat` only to Contributor+. Every chat operation in `operation_descriptor` is currently described as requiring `project.chat`. `CollaborationService` already resolves channels to projects, enforces bounded channel/message storage, and has privacy-safe not-found behavior. `ChatState` is project keyed and can consume denial/unavailable state.

## 4. Invariants that must not regress

- Active membership is mandatory; override rows cannot resurrect revoked/suspended membership.
- Chat free text remains inert.
- Chat grant never satisfies structured-action capabilities.
- Existing role-default behavior is unchanged when no policy rows exist.
- Channel list/history/direct lookup use the same effective policy and cannot leak restricted channel existence.
- Policy is daemon truth; frontend hints never grant access.

## 5. Scope

In: durable policy schema/store, effective resolver, protocol/admin operations, chat daemon gates, channel enumeration filtering, audit metadata for policy changes, TUI-compatible policy DTOs, tests/docs.

Out: general role customization, read-only-vs-write chat split, groups, OIDC, team UI (M003), Workspace presentation (M005).

## 6. Required production changes

Add revisioned durable chat policy state implementing ADR-0006. A compact design is acceptable, but it must support project principal override allow/deny, channel mode `inherit_project|restricted`, and channel principal override allow/deny. Foreign/stale channel IDs fail closed.

Refactor chat authorization so active project membership/read scope is established before policy evaluation. `project.chat` remains the role baseline/compatibility capability, not the only gate. Centralize `effective_chat_access(project, channel?, principal)`; every channel/message/read-marker/composing/action entry point uses it.

Add Core requests/responses for authorized policy inspection/mutation under `member.manage`. Use optimistic revision checks and privacy-safe errors. Policy events carry structural IDs/revisions only.

Structured actions must still pass both chat access and their action-specific capability checks.

## 7. Ordered work packages

A. Add schema/store/migration plus restart/idempotency/revision tests.
B. Implement pure effective-access resolver with exhaustive role/project/channel precedence tests.
C. Integrate resolver into channel list/ensure/history/sync/send/edit/redact/read/composing and message/reference access.
D. Add `member.manage` policy Get/List/Set/Clear operations and authorization matrix coverage.
E. Add revocation/reconnect/event filtering and structured-action non-escalation tests.
F. Update collaboration/authorization architecture docs.

## 8. Failure, cancellation, restart, contention semantics

Stale administrative revisions return typed conflict with zero overwrite. Duplicate identical set/clear requests converge. Membership revocation wins immediately. Reconnect re-evaluates policy. If policy storage is unavailable for a team principal, fail closed rather than falling back to Contributor role defaults.

## 9. Compatibility and migration

Migration is additive. Absence of policy rows exactly preserves Viewer denied / Contributor+ allowed and existing channels inherit project policy. Older clients can continue ordinary chat when authorized; they simply cannot administer new policy.

## 10. Required tests

Viewer default deny; Viewer project allow; Viewer one-channel allow; Contributor default allow; Contributor project deny; channel deny over project allow; restricted channel explicit allowlist; revoked member with stale allow; channel list filtering; direct denied-channel lookup; concurrent override CAS; restart; structured action denied despite chat allow; cross-project locator probe; old-client default behavior.

## 11. Required verification commands

- `cargo test -p codegg-core collaboration`
- `cargo test --test collaboration_m001_chat`
- `cargo test --test collaboration_m003_chat_actions`
- new chat-policy integration tests
- `python3 scripts/check_authorization_matrix.py`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `scripts/verify.sh quick`
- `git diff --check`

## 12. Documentation updates

Update `architecture/collaboration.md`, `architecture/authorization.md`, `architecture/identity.md`, protocol docs, and help/admin command descriptions when protocol operations land.

## 13. Acceptance criteria

A Viewer can observe a session and participate in explicitly granted project/channel chat while all agent/file/command/job/Git mutation capabilities remain denied. A Contributor can be denied one channel without losing unrelated capabilities. Restricted channels are not enumerable or distinguishable to denied members.

## 14. Stop conditions

Do not generalize this into an arbitrary policy engine or add new project roles. Stop if implementing the override would require trusting frontend authorization state or exposing denied channel identity.

## 15. Closure evidence required

Record migration version, resolver truth table, authorization matrix changes, privacy tests, role-default compatibility, and structured-action non-escalation.

## 16. Handoff notes

M003 and M005 consume this API. Keep policy DTOs suitable for both TUI administration and selected-project chat rendering without embedding display-only state in the core model.
