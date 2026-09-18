# ADR-0006: Project and Channel Chat Access Policy

Status: accepted

Date: 2026-09-18

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#15-read-only-session-observation`
- `plans/000-long-term-specification.md#21-project-communication`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md#11-presence-and-collaboration-terms`

Affected subsystem roadmaps:

- `plans/subsystems/project-collaboration-roadmap.md`
- `plans/subsystems/team-collaboration-corrective-addendum.md`

## Context

The closed project-collaboration implementation treats `project.chat` as a fixed role capability. Viewer lacks it and Contributor+ receives it. That makes a genuinely read-only observer unable to participate in discussion, while granting chat requires granting a broad Contributor bundle that also includes agent invocation, file mutation, command execution, job submission, Git writes, and worktree creation.

That coupling is too coarse for team operation. A project may need read-only reviewers who can discuss observed work, contributors whose chat access is temporarily removed, and restricted project channels that are visible only to a subset of a large project team. Chat remains communication, not execution authority.

## Decision drivers

- Active project membership must remain the prerequisite for all project communication.
- A chat grant must never imply agent, filesystem, command, job, Git, worktree, permission-response, or structured-action authority.
- Contributor and higher roles should retain chat by default for compatibility.
- Viewer should remain read-only by default but may receive explicit chat access.
- Project owners need project- and channel-scoped allow/deny controls without creating new project roles.
- Restricted channel existence and membership must not leak to unauthorized project members.
- The policy must remain deterministic under revocation, reconnect, retry, and concurrent administration.

## Considered options

### A. Add `project.chat` to Viewer

This fixes observer discussion but makes the grant universal and removes the ability to restrict chat for individual contributors or channels. Rejected.

### B. Add additional fixed roles such as Commenter

This improves one common case but creates role proliferation and still cannot express channel-specific team spaces. Rejected.

### C. Keep roles as defaults and add a narrow chat access overlay

This preserves the existing role model while allowing project/channel exceptions. Accepted.

## Decision

Project roles remain the coarse authorization bundles. Chat access is additionally evaluated by a daemon-owned `ChatAccessPolicy` after active project membership has been established.

Effective access for one principal and channel is resolved in this order:

1. The principal MUST have an active membership in the owning project. Suspended, revoked, absent, or inactive principals are denied before policy lookup.
2. Role baseline: Contributor, Maintainer, and Owner allow chat; Viewer denies chat.
3. A project-scoped principal override (`allow` or `deny`) replaces the role baseline when present.
4. A channel may be `inherit_project` or `restricted`. `restricted` changes the channel default to deny.
5. A channel-scoped principal override (`allow` or `deny`) is final and may grant a Viewer access to one channel or deny a Contributor access to one channel.

One access decision governs ordinary channel visibility and message participation in this corrective campaign. A future read-without-write split requires a new additive policy decision; it must not be inferred from this ADR.

`project.chat` remains the compatibility/default capability describing the Contributor+ role bundle, but it is no longer sufficient as the only authorization gate for chat protocol operations. Chat handlers MUST first require project membership/read scope and then evaluate the canonical chat policy for the resolved project/channel. No frontend may synthesize or cache authority.

Channel enumeration MUST filter rows through the same policy. Direct access to a denied or unknown channel must use the existing privacy-safe not-found behavior. Project/channel policy administration requires `member.manage`; changing chat access is a membership/collaboration policy operation, not ordinary message sending.

Structured chat actions remain separately authorized. A principal granted chat access but lacking `agent.delegate`, `job.submit`, `session.read`, or another action-specific capability cannot create the corresponding action.

## Persistence and concurrency

Policy is durable coordinator state. The implementation should use revisioned project/channel policy records and principal overrides with optimistic concurrency. Retrying an identical mutation is idempotent; stale revisions fail rather than overwriting a newer grant/revocation. Channel deletion or future archival must not leave an override capable of granting access to a different channel identity.

## Consequences

Positive: observer-with-chat becomes possible without broad write authority; Contributor defaults remain compatible; project owners can create restricted team spaces; structured actions remain independently gated.

Negative: chat authorization becomes resource-sensitive and cannot be represented solely by the static role capability matrix. Every channel lookup/list/send/edit/read/composing/action path must use the same policy resolver or a regression could leak a restricted channel.

Neutral: no general ABAC/RBAC engine is introduced. This is a narrow collaboration policy overlay and does not change project roles for non-chat operations.

## Compatibility and migration

Existing memberships and channels require no manual migration. With no policy rows, behavior is exactly the current role default: Viewer denied, Contributor+ allowed, and all channels inherit project policy. Existing `project.chat` capability serialization remains readable.

## Security and reliability implications

Membership revocation always wins over any chat allow. Policy denials must be indistinguishable from absence at cross-project/channel boundaries. Overrides never contain message content or secrets. Authorization is evaluated at request time so revocation and policy changes take effect without reconnect.

## Verification

Required evidence includes role-default compatibility, Viewer project grant, Viewer one-channel grant, Contributor project/channel deny, restricted-channel filtering, revocation races, stale administrative revisions, reconnect after policy change, structured-action non-escalation, and cross-project channel probing.

## Supersession

This ADR may be superseded by a broader project policy system only if that system preserves the active-membership prerequisite, resource-scoped privacy, explicit deny/grant semantics, and separation between communication and execution authority.
