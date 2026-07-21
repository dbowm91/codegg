# Session Projections Milestone 003 — Visibility, Redaction, and Artifact Handles

Status: implemented (closed; see `plans/closure/session-projections/003-status.md`)

Repository baseline: `f569386e4cb68d9752505c3b8d4205161a40c3c4` (`main`; planning-only commits after this baseline do not alter production behavior)

Activation criteria:

- `plans/implementation/session-projections/002-corrective-daemon-integration-and-closure.md` must be strictly closed;
- the daemon/client request context must expose a stable principal/capability filtering seam suitable for policy evaluation, even if final team roles are not yet implemented.

Source roadmap:

- `plans/subsystems/session-projections-roadmap.md#milestone-3--visibility-redaction-and-artifact-handles`

Primary class: invariant / security

## 1. Objective

Make projection persistence, replay, snapshots, and bounded artifact access safe for later multi-user observation by enforcing visibility and disclosure policy before shared durable storage and before transport delivery.

Milestones 001–002 define bounded projection DTOs, a canonical reducer, durable replay streams, cursors, subscriptions, retention, and an interim safe-publication gate. Milestone 003 must replace that interim classification with a complete, typed, fail-closed disclosure pipeline that:

- evaluates a request/client capability context;
- structurally redacts secret-bearing fields before serialization;
- prevents provider-private reasoning and internal diagnostics from entering shared streams;
- downgrades oversized or high-risk content to safe summaries or opaque handles;
- provides bounded, authorized reads for artifacts/log excerpts;
- produces stable redaction metadata without leaking the removed content;
- remains compatible with future roles/principals without defining the final team authorization model.

The milestone succeeds when adversarial payloads, credentials, environment values, sensitive tool arguments/outputs, internal reasoning, and unbounded artifacts cannot enter shared projection storage or be retrieved without an explicit capability decision.

## 2. Dependency assumptions

This plan assumes strict Session Projections Milestone 002 closure provides:

- one centralized daemon publication seam;
- canonical project/workspace/session binding resolution;
- real persisted stream descriptors and binding revisions;
- persist-before-deliver semantics;
- projection request dispatch and subscription-isolated live routing;
- bounded restart-safe replay, retention, checkpoints, and resync;
- per-client subscription ownership and cleanup.

It also assumes a transport/request context can provide a stable principal identifier or anonymous/local principal class plus explicit capabilities. This milestone does not require final role names or team membership storage; it requires a policy input seam that cannot be forged by request payload fields.

## 3. Current production evidence and gaps

At baseline `f569386` plus the landed M2 library implementation:

- `VisibilityClass::{Public, ClientLocal, Internal, Sensitive}` exists in projection DTOs;
- `ProjectionEvent::visibility()` classifies selected event families;
- the M2 `SafePublicationGate` classifies events and rejects/downgrades some classes;
- tool arguments/outputs have inline, summary, truncated, and handle variants;
- artifact projection DTOs exist as opaque references;
- replay rows persist serialized projection envelopes;
- raw run artifacts and logs already have daemon-owned stores/read operations in adjacent subsystems;
- final capability evaluation, field-level structural redaction, artifact-handle issuance, handle authorization, expiry/revocation, and read audit/metrics are not complete;
- several adapters construct public DTOs from raw strings before a full policy transform;
- heuristic secret scanning alone would be insufficient because sensitive values may be typed, encoded, nested, or fragmented;
- `ClientLocal` currently describes intended visibility but does not define which connected client owns the value;
- `Sensitive` currently signals danger but does not define stable downgrade behavior;
- no complete negative matrix proves that forbidden values are absent from replay rows, checkpoints, snapshots, diagnostics, metrics, or artifact responses.

## 4. Invariants

- Disclosure policy runs before shared projection persistence and before live delivery.
- Structural field classification is authoritative; heuristic scanning is defense in depth only.
- A caller cannot self-assign a principal, role, capability, ownership token, or visibility class through request payloads.
- `Public` means safe for any authorized subscriber to the stream scope, not safe for arbitrary unauthenticated internet exposure.
- `ClientLocal` content is delivered only to the originating/owning client context and is not persisted into a shared project stream unless transformed to a public-safe summary.
- `Internal` content is never serialized into shared snapshots, checkpoints, replay rows, or transport events.
- `Sensitive` content is denied or downgraded before serialization; raw sensitive values never reach replay storage.
- Provider-private hidden reasoning is never exposed. User-visible summaries may be projected only from explicit public output fields.
- Credentials, tokens, cookies, authorization headers, secret refs, decrypted secret material, environment secrets, private keys, and connection payloads never enter shared projection storage.
- Large content remains in its authoritative daemon store and is represented by bounded opaque handles.
- A handle conveys no authority by itself; read authorization re-evaluates caller, project/workspace/session scope, artifact type, bounds, and lifecycle.
- Handle IDs are opaque, unguessable or high-entropy, bounded, and never paths.
- Artifact reads are range/byte/time/count bounded and cannot traverse directories or select arbitrary files.
- Redaction failure fails closed. Serialization or classifier errors do not fall back to raw payload persistence.
- Redaction metadata is bounded and never includes the removed secret.
- Existing raw-core compatibility paths do not become a bypass for new observer/shared projection clients.
- Final role policy, audit retention, and legal/compliance export remain outside this milestone.

## 5. Scope

### In scope

- A typed `ProjectionAccessContext` / `ProjectionCapabilitySet` boundary derived by the daemon transport.
- Stable policy actions: allow, deny, redact, summarize, handle, client-local-only.
- Field-level structural classification and transformations for all projection DTO/event families.
- Secret-bearing typed-field exclusion.
- Bounded heuristic scanning for untyped text as a secondary defense.
- Client-local ownership metadata and delivery restrictions.
- Public/shared snapshot, checkpoint, replay, and live-event filtering.
- Artifact/log/diff/output handle issuance.
- Bounded artifact-read protocol operations and typed responses.
- Handle scope, expiry, revocation/invalidation, and lifecycle behavior.
- RunStore and other existing authoritative artifact-store adapters.
- Redaction/denial/handle metrics and bounded diagnostics.
- Adversarial, property, fixture, storage-negative, transport-negative, and authorization-negative tests.
- Projection/security/protocol/server/operations documentation.

### Explicitly out of scope

- Final team role names, group membership, invitations, or organization policy.
- Presence, chat, observer UX, or collaboration panel implementation.
- Full audit log or compliance export.
- Provider-private chain-of-thought exposure.
- Arbitrary file browsing.
- Replacing RunStore, session/message storage, or existing artifact authorities.
- Unlimited artifact streaming.
- Cross-daemon handle portability.
- Frontend migration to canonical projection replay; Milestone 004 owns it.

## 6. Policy input seam

Define a transport-derived context such as:

```text
ProjectionAccessContext
|-- principal_id: stable opaque identifier or local-single-user principal
|-- client_id: daemon-issued connection identity
|-- capabilities: ProjectionCapabilitySet
|-- allowed_projects: bounded policy resolver/reference
|-- allowed_sessions: optional bounded resolver/reference
|-- transport_class: local / authenticated_remote / internal_test
`-- request_correlation_id
```

Required capabilities should be semantic rather than role-named, for example:

- observe public project projection;
- observe session projection;
- receive client-local events owned by this client;
- read bounded run artifact;
- read bounded tool output handle;
- read bounded diff/log excerpt;
- view operational diagnostics.

The context is constructed by daemon/server/socket authority. Request DTOs may name desired project/session/handle but cannot name capabilities.

Provide an interface that future authorization code can implement without changing projection DTOs or replay storage.

## 7. Disclosure pipeline

Create one canonical pipeline:

```text
raw canonical event/context
    -> typed source classification
    -> access/scope policy
    -> structural field transform
    -> bounded heuristic text scan
    -> size policy: inline / summary / handle / deny
    -> normalized projection DTO
    -> final serialized-byte validation
    -> persist/checkpoint/live delivery
```

The final serialized bytes, not only the source object, must pass size and forbidden-pattern assertions before commit.

Required output:

```text
DisclosureDecision
|-- Allow(transformed_payload)
|-- ClientLocal(transformed_payload, owner_client_id)
|-- Summarize(public_summary, reason_code)
|-- Handle(public_metadata, handle_record)
|-- Deny(reason_code)
`-- ErrorFailClosed(reason_code)
```

Do not preserve raw denied values inside diagnostics, error chains, debug formatting, metrics labels, or tracing fields.

## 8. Structural classification matrix

Build and maintain an exhaustive matrix for:

- session/project/workspace summaries;
- turn/user/assistant messages;
- reasoning fields;
- tool names, arguments, outputs, errors, timings;
- permission/question prompts and answers;
- subagent descriptions/progress/results;
- file changes and diffs;
- run commands, logs, summaries, artifacts;
- job metadata and errors;
- provider/model/agent selection;
- token/cost metrics;
- diagnostics and unknown variants.

Each field must declare:

- source type;
- default visibility;
- allowed shared scopes;
- structural redaction rules;
- size limit;
- downgrade behavior;
- handle authority when applicable;
- test fixtures.

Unknown variants and unknown fields default to deny or bounded unknown-summary, never pass-through.

## 9. Secret and sensitive-data handling

Structural exclusions must cover at minimum:

- `Authorization`, proxy authorization, cookies, API keys, bearer/basic credentials;
- provider connection encrypted/decrypted credential payloads;
- secret references and resolved secret values;
- environment maps and known secret variable names;
- SSH/private keys, certificates with private material, signing tokens;
- URL userinfo and query secrets;
- command-line flags known to carry credentials;
- tool arguments marked sensitive by tool schema/policy;
- permission/question answers classified private;
- internal reasoning and hidden provider metadata.

Heuristic scanning should support bounded detectors for common token/key formats, PEM blocks, URL credentials, high-risk header patterns, and configured secret names. It must cap CPU and input bytes and produce false-positive-safe downgrade behavior.

Never claim heuristic scanning proves absence of all secrets; structural typing and source policy remain primary.

## 10. Artifact handle model

Define an opaque handle descriptor such as:

```text
ProjectionArtifactHandle
|-- handle_id
|-- kind
|-- project_id
|-- workspace_id: optional
|-- session_id: optional
|-- source_record_id
|-- content_type
|-- total_bytes: optional bounded metadata
|-- created_at
|-- expires_at: optional
|-- revision/generation
`-- public_summary
```

The public descriptor must not contain a filesystem path, raw command with secrets, storage key, credential, or signed URL.

Handle records may be transient or durable according to the authoritative artifact lifecycle, but must reference existing daemon stores rather than duplicate content into projection replay.

Invalidate or deny handles when:

- source artifact is deleted/expired;
- project/session binding changes incompatibly;
- caller loses capability;
- revision no longer matches;
- daemon instance cannot resolve the source;
- retention policy requires removal.

## 11. Bounded artifact-read APIs

Add additive operations such as:

- `ProjectionArtifactMetadataGet`;
- `ProjectionArtifactRead { handle_id, start, end, expected_revision }`;
- optional bounded line-window/log-tail operation with explicit maximums.

Requirements:

- capability and project/session scope re-evaluated per read;
- range normalized and capped;
- total response bytes capped below transport frame limits;
- no arbitrary path input;
- binary/text content type explicit;
- redaction applied to text excerpts where source may still contain secrets;
- stale revision returns typed mismatch;
- expired/missing/unauthorized handles return indistinguishable safe errors where enumeration risk exists;
- rate/concurrency limits per client;
- cancellation and timeout support;
- metrics contain IDs/counters only, not content.

## 12. Replay and checkpoint integration

- Apply disclosure before `projection_event` insertion.
- Checkpoints contain only already-transformed projection snapshots.
- Project streams receive only shared-safe content.
- Session streams receive content permitted for that subscriber class; if storage is shared across principals, persist only the least-disclosure canonical form and layer client-local events outside shared durable rows.
- Client-local queues are bounded and cleaned on disconnect.
- Resume cannot reveal events a newly connected principal is not permitted to observe.
- Capability changes force resubscribe/resync where cached snapshot visibility may differ.
- Retention/pruning treats handle records and replay rows consistently.

If principal-specific durable projections would multiply storage or violate one-canonical-stream semantics, stop for an ADR. Prefer one redacted canonical shared stream plus ephemeral client-local overlays.

## 13. Work packages

### A — Capability context and policy engine

- Add transport-derived access context.
- Define semantic capabilities and policy result types.
- Wire subscribe/resume/snapshot/artifact requests through policy.

### B — Structural classification and redaction

- Build exhaustive field matrix.
- Implement typed transforms and fail-closed unknown handling.
- Add bounded heuristic defense for untyped text.

### C — Replay/live/checkpoint enforcement

- Integrate disclosure before persistence and delivery.
- Separate client-local overlays from shared replay.
- Handle capability-change resync.

### D — Artifact handles and bounded reads

- Add handle registry/adapters over authoritative stores.
- Add read protocol, range/rate/concurrency bounds, expiry/revision checks.
- Apply text redaction to excerpts.

### E — Verification and documentation

- Add adversarial fixtures, storage-negative scans, property tests, and transport tests.
- Document policy matrix, handle lifecycle, metrics, operations, and limitations.
- Produce strict closure evidence.

## 14. Required tests

- typed credential fields never serialize into projection rows;
- authorization/cookie/token/private-key values in nested tool arguments are denied/redacted;
- URL userinfo/query secrets are removed;
- environment secret values do not enter replay/checkpoints/diagnostics;
- provider-private reasoning is absent from shared storage and transport;
- unknown event/field fails closed;
- redaction error cannot fall back to raw payload;
- client-local event reaches only owning client and is absent from shared project replay;
- reconnect as another principal cannot resume client-local content;
- capability removal forces resync/denial;
- project/session scope mismatch denies subscription and artifact reads;
- oversized output becomes summary/handle, never oversized inline payload;
- handle ID is opaque and path-free;
- guessed/stale/expired/revoked handle is denied safely;
- artifact range and response-size caps are enforced;
- concurrent/rate limits are enforced and cancellable;
- path traversal input is impossible or rejected before store access;
- text artifact excerpts receive redaction;
- source artifact deletion invalidates handle;
- replay/checkpoint database scan finds none of seeded forbidden secrets;
- metrics/tracing/errors contain no payload bodies or secret values;
- old clients and raw-core compatibility do not gain shared-observer privileges;
- restart preserves only safe handle metadata and redacted replay;
- full projection M1/M2, protocol, daemon, storage, and static-guard suites remain green.

## 15. Acceptance criteria

- A daemon-derived capability context controls projection subscribe/resume/snapshot/artifact operations.
- Every projection field family has an explicit visibility and downgrade rule.
- Structural redaction precedes persistence and delivery.
- Unknowns and failures fail closed.
- `Internal` and `Sensitive` raw values are absent from replay rows, checkpoints, snapshots, diagnostics, metrics, and shared transport.
- `ClientLocal` delivery is owner-isolated and not present in shared project replay.
- Large content is represented by bounded summaries or opaque handles.
- Handle reads are scope-authorized, range/byte/rate/concurrency bounded, revision-aware, cancellable, and path-free.
- No final role model or audit system is introduced.
- Existing replay determinism, retention, restart, and compatibility invariants remain intact.
- Security architecture, policy matrix, operations, and strict closure documentation are complete.

## 16. Verification commands

At minimum:

```bash
cargo fmt -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
cargo test -p codegg-protocol
cargo test -p codegg-core
cargo test --test projection_replay_storage
cargo test --test projection_replay_subscription
cargo test --test projection_replay_resume
cargo test --test projection_replay_retention
cargo test --test projection_replay_failpoint
cargo test --test projection_replay_safe_publication
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
bash scripts/check-core-boundary.sh
```

Add focused projection disclosure, artifact handle, adversarial secret, database-negative, and transport-isolation test targets.

## 17. Downstream unlock

When this plan is strictly closed:

- Session Projections Milestone 004 becomes dependency-ready;
- its frontend migration may consume the canonical shared projection and bounded artifact APIs without reimplementing redaction;
- final team authorization may later supply richer principals/roles through the capability interface without changing replay DTOs.
