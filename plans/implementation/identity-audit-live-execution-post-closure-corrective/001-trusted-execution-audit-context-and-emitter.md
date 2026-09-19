# Identity / Audit Live Execution Corrective M001 — Trusted Execution Audit Context and Emitter

Status: ready for handoff

Repository baseline: `a996e20060a0103a152a80fb62463241c1fd1162`

Source roadmap: `plans/subsystems/identity-audit-live-execution-post-closure-corrective-addendum.md`

Original finding source: `plans/closure/identity-authorization-audit/005-status.md#10-unresolved-findings`.

Primary class: invariant / infrastructure corrective.

## 1. Objective

Create one trusted, bounded audit-emission seam that can be used by canonical execution owners outside `CoreDaemon` without fabricating actor identity or giving ToolBroker/Git/scheduler direct ownership of the audit store.

This milestone establishes the context/emitter contract. It should not attempt to land all command/Git/job events in the same patch.

## 2. Current evidence

- `CoreDaemon::append_audit_event` owns current root-crate appends and wraps store access with timeout/counters/warnings.
- `AuditEventBuilder` construction requires `AuthenticatedPrincipal` + `AuditDecisionProvenance`.
- `BrokerInvocationContext` has a verified `ToolAuthorityGrant` and `principal_ref` but not a complete `AuthenticatedPrincipal` or `AuditDecisionProvenance`.
- `ToolExecutionContext` has `origin_principal`, `origin_auth_method`, `origin_decision_id` fields, but ToolBroker currently creates an execution context with those fields unset.
- Reconstructing a principal from `principal_ref` alone would violate the existing attribution contract.
- Git and scheduler owners similarly need an injected emitter/context rather than independent SQL writes.

## 3. Target contract

Introduce one internal cloneable emission service (name/location implementation-defined) constructed by the daemon/coordinator from the same pool/store policy as `append_audit_event`. It owns:

- bounded append timeout;
- existing emit counters/warn behavior;
- no separate queue/store/schema;
- no authorization decisions of its own.

Introduce one trusted execution-audit context containing only what builders need:

- cloned/immutable transport-bound `AuthenticatedPrincipal` or an equivalently lossless trusted actor object;
- `AuditDecisionProvenance` copied from the admitted daemon decision;
- `AuditChainContext` locators/correlation;
- no command/body/secret content.

Thread the context from the admission/turn/job boundary to canonical execution owners. It may travel through `BrokerInvocationContext`/`ToolExecutionContext` and scheduler/Git composition, but it must be constructed only from trusted daemon-owned state.

## 4. Required work

- Extract or wrap `CoreDaemon::append_audit_event` so family handlers and injected execution owners use the same emission policy.
- Add the trusted execution audit context type in the narrowest dependency-safe layer.
- Preserve codegg-core boundary rules; do not import root authorization/UI/server code into codegg-core.
- Thread trusted origin through ToolBroker without synthesizing it from `principal_ref`, tool input, model output, or a grant string.
- Provide additive optional context for legacy/local callers that genuinely lack team attribution; those paths must use existing explicit `legacy_local`/LocalOwner provenance rather than invented human identity.
- Add seam unit tests for trusted principal preservation, correlation, no-secret fields, timeout/failure counters, and absence of duplicate stores.
- Update `architecture/audit.md` and relevant core/tool/scheduler skills/docs.

## 5. Non-goals

- Emitting `command_execute`/`git_operation`/`job_complete` everywhere in this milestone.
- Changing wire protocol or storage schema.
- Adding public SDK APIs.
- Allowing execution layers to query TeamStore/authorization to reconstruct a principal.

## 6. Verification

```bash
cargo test -p codegg-core --lib audit_instrumentation -- --test-threads=1
cargo test --test identity_m005_audit_instrumentation -- --test-threads=1
cargo test -p codegg --lib tool::broker -- --test-threads=1
cargo test -p codegg --lib scheduler -- --test-threads=1
python3 scripts/check_audit_coverage.py --verbose
bash scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

## 7. Acceptance criteria

- All audit append paths use one bounded store/emission policy.
- Execution owners can receive trusted actor/provenance/chain context without reconstructing authority.
- ToolBroker preserves trusted origin when supplied.
- No request/tool/model payload can set the trusted context.
- No storage/protocol migration.
- M002 and M003 can be implemented without designing a second audit mechanism.

## 8. Stop conditions

Register an ADR instead of continuing if the only workable design:

- moves sequence/store authority away from the coordinator;
- introduces a second audit database or event bus;
- exposes trusted audit principal/provenance in public untrusted request DTOs; or
- changes the authorization decision model.

## 9. Closure evidence

`plans/closure/identity-audit-live-execution-post-closure-corrective/001-status.md` with context provenance tests, emitter failure-policy parity, boundary guard results, and M002/M003 unblock audit.
