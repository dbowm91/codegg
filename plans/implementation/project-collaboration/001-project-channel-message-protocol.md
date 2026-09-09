# Project Collaboration Milestone 001 — Project Channel, Message, and Synchronization Contract

Status: blocked

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/project-collaboration-roadmap.md#M001--project-channel-message-and-synchronization-contract`

Long-term requirements: `plans/000-long-term-specification.md#21-project-communication`, `#27-security-requirements`; roadmap Phase 12.

Applicable ADRs: none. Primary class: capability.

## 1. Objective

Implement canonical project-scoped channels/messages with replies/threads, mentions, typed CodeGG object references, edits/redactions, read markers, ephemeral composing state, retention and bounded idempotent incremental synchronization over the native protocol.

## 2. Why this milestone is ready

Blocked on identity/authorization/audit M005 and presence/observation M003. Typed ChannelId and mature project/session/run/job identities exist; no new ownership decision is required.

## 3. Current implementation evidence

No active channel/message store or protocol owns communication. TUI/project identity/projection/run/job foundations exist. The canonical terminology already distinguishes chat from audit and structured actions.

## 4. Invariants that must not regress

Project authorization guards channel enumeration/content/references; message IDs/idempotency prevent retry duplication; edits/redactions preserve append-only audit history; large/secret content uses redacted bounded text or handles; typing is ephemeral; free text has no execution semantics.

## 5. Scope

In: default project channel lifecycle, message/reply/thread/mention/reference DTOs, edit/redact semantics, read markers, composing leases/events, retention, store/migrations, sync cursors/pages/events, authorization/audit structural hooks. Out: TUI rendering (M002), task actions (M003), external bridges, DMs.

## 6. Required production changes

Core/domain: channel/message/read-marker/reference types with typed IDs and author principal/agent identity. Storage: ordered per-channel/project messages, idempotency key, revisions/redactions, read markers/retention. Protocol: namespaced versioned bounded list/history/send/edit/redact/read/composing operations/events with sync cursor. Runtime: daemon store/service and event publication. Security: resolve every reference under recipient authorization; secret/body bounds. Audit: structural message create/edit/redact metadata only under M005 policy.

## 7. Ordered work packages

A — domain schema, author/reference/visibility/retention and idempotency semantics.

B — durable channel/message/read-marker storage + restart/migration/order tests.

C — authorized service APIs for send/reply/edit/redact/read and ephemeral composing state.

D — bounded incremental sync protocol/events/cursors and reconnect/idempotency tests.

E — reference privacy/redaction/retention/audit integration and docs.

## 8. Failure, cancellation, restart, and contention semantics

Concurrent sends receive deterministic ordering; duplicate client idempotency returns original message. Edit/redact revision conflicts are typed. Restart preserves durable order/read markers but drops composing. Sync cursor expiry returns bounded resync. Failed send does not publish a committed message event.

## 9. Compatibility and migration

Additive protocol capability; older clients ignore chat. No legacy chat data. Future bridge IDs must map as external provenance without replacing CodeGG message IDs.

## 10. Required tests

Message ordering/idempotency/concurrent send; restart; replies/mentions/reference privacy; edit/redact revision; retention; read markers; composing expiry; unauthorized channel/project enumeration; sync resume/resync; secret/bounds; audit separation.

## 11. Required verification commands

```bash
cargo test -p codegg-core chat
cargo test --workspace collaboration --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

Project communication architecture/protocol/security/retention docs.

## 13. Acceptance criteria

Authorized members can exchange/reply/edit/redact and incrementally sync project messages across restart/reconnect; unauthorized callers cannot enumerate/read; retries do not duplicate; composing expires; chat structural events are auditable but chat storage remains separate.

## 14. Stop conditions

Prerequisites not closed; implementation requires external chat source of truth; references cannot be privacy-filtered within existing authorization model; text is proposed as executable command input.

## 15. Closure evidence required

Dependency closures, schema/migration, auth/privacy matrix, ordering/idempotency/restart/sync tests, retention/redaction/audit-separation evidence, exact verification results.

## 16. Handoff notes

Keep the first channel model small: one default project channel plus extensible ChannelId is sufficient if multi-channel administration would enlarge the milestone. Do not invent chat commands.
