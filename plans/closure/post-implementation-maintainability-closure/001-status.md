# Post-Implementation Maintainability Closure M001 — Authorization/Collaboration Physical Decomposition

Status: closed

Source implementation plan: `plans/implementation/post-implementation-maintainability-closure/001-authorization-collaboration-physical-decomposition.md`

Source roadmap: `plans/subsystems/post-implementation-maintainability-closure-roadmap.md`

Repository production baseline: `f04866b1d817eee06fb3e51e73cc8f7b6679ad86`

## Outcome

M001 is complete. The canonical `codegg-core` authorization and collaboration owners were physically decomposed into narrowly named private child modules without creating a second policy engine, authority owner, collaboration service, repository abstraction, scheduler path, or coordination layer. The facade modules continue to own the public subsystem surface and re-export the moved stable helpers.

Implementation commit: `69503497cd21c32d0d3dee566693bd6fc3935e95`

The closure-record commit contains this evidence and the final planning-state reconciliation.

## Responsibility map

### Authorization

Before, `crates/codegg-core/src/authorization.rs` combined the canonical authorization facade and domain types with the exhaustive operation/capability matrix, request descriptors and fixtures, policy/decision behavior, child/provider authority and projection helpers, privacy/denial mapping, durable origin attribution types/store, and their tests.

After:

| Owner | Responsibility |
|---|---|
| `authorization.rs` | Canonical facade, principal/request/decision/error/service domain, compatibility re-exports, and authorization tests. |
| `authorization/policy.rs` | Existing exhaustive `ScopeKind`, operation descriptors, capability matrix, and representative request fixtures. |
| `authorization/authority.rs` | Existing authority narrowing, child delegation, provider-use authorization, capability projection, bounded resolution, audit provenance, visibility, and denial-as-not-found bridges. |
| `authorization/attribution.rs` | Existing immutable origin attribution value and concrete SQLite-backed attribution store, including validation/redaction helpers. |

The facade remains the only canonical authorization entry point. The child modules are private and expose only the `pub`/`pub(super)` items required by the existing facade and in-crate callers.

### Collaboration

Before, `crates/codegg-core/src/collaboration.rs` combined channel/message domain and DTO conversion, message and action validation/redaction, audit-safe metadata shaping, concrete schema/row/query helpers, composing/read state, service operations, structured action persistence/idempotency/status, and tests.

After:

| Owner | Responsibility |
|---|---|
| `collaboration.rs` | Canonical facade, domain types/DTO conversion, service and action operations, composing/read state, transaction sequencing, and collaboration tests. |
| `collaboration/validation.rs` | Existing message/channel/mention/reference/action bounds, secret/control-character rejection, redaction, and audit metadata shaping. |
| `collaboration/store.rs` | Existing collaboration table definitions, schema initialization, project/channel locator, row aliases/decoders, and reference/mention encoding. |

No separate `actions.rs` was introduced: action validation naturally shares the inert-message validation/redaction boundary, while action persistence remains coupled to the collaboration service's existing transaction and scheduler-submission sequencing. This preserves locality without manufacturing a fourth owner.

Descriptive final source sizes are 1,030 lines for `authorization.rs` plus 1,632/262/200 lines for its three children, and 2,593 lines for `collaboration.rs` plus 540/297 lines for its two children. These are evidence of responsibility movement, not a new size gate.

## Compatibility and invariant review

- No files under `crates/codegg-protocol` changed. No DTO, serde tag/default, serialized enum/string, capability version, stable error code, or public wire contract changed.
- No storage schema, migration, `STORAGE_LAYOUT_VERSION`, table/column, unique constraint, ordering, retry, retention, or conflict semantic changed. Collaboration SQL and row decoding moved verbatim into the concrete `store` child; no repository trait or persistence facade was added.
- No scheduler, job-submission, daemon composition, background task, cancellation, lock-order, or async lifetime behavior changed. Structured chat actions still submit through the existing scheduler-owned boundary, and free text remains inert.
- Authorization authority remains transport-derived through `AuthenticatedPrincipal`; DTOs and chat content cannot mint authority. Default-deny, scope mapping, revocation, project privacy, and denial-as-not-found behavior remain in the same canonical implementation.
- Secret rejection/redaction and structural audit metadata remain in the same validation paths. No event names, causation/correlation, attribution linkage, projection classification, or replay behavior changed.
- Existing facade imports remain available through re-exports. The only tightened moved helper visibility is `ChatReference::from_dto` to `pub(super)`, supported by repository-wide caller evidence.
- Architecture documentation now names the child-module source of truth, and `scripts/check_authorization_matrix.py` follows the moved policy/authority owners without adding a new guard or behavioral rule.

## Verification evidence

All required focused checks passed:

- `cargo test -p codegg-core --lib authorization` — 21 passed.
- `cargo test -p codegg-core --lib collaboration` — 17 passed.
- Native arm64 runs of `identity_m003_daemon_authorization`, `identity_m004_audit_foundation`, `identity_m005_audit_instrumentation`, `collaboration_m001_chat`, `collaboration_m002_chat_tui`, `collaboration_m003_chat_actions`, `presence_m002_collaborators`, and `presence_m003_observation` — 91 tests passed, 0 failed.
- `python3 scripts/check_authorization_matrix.py` — passed.
- `python3 scripts/check_execution_ownership.py` — passed.
- `python3 scripts/check_scheduler_bypass.py` — passed.
- `bash scripts/check-core-boundary.sh` — passed.

Broad checks passed:

- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed with no issues.
- `scripts/verify.sh quick` — passed, including generated-agent, sandbox, execution-ownership, core-boundary, and locked workspace all-target checks.
- `git diff --check` — passed.

The first default-toolchain attempts to link root integration tests used the host's x86_64 Homebrew Rust against arm64 MacPorts `liblzma`/`libiconv` libraries and failed before test execution. Re-running with the installed native arm64 Rust toolchain and an isolated target directory completed all required integration suites successfully; this is an environment/toolchain note, not an implementation finding.

## Planning and dependency closure

`plans/registry.md` now records the maintainability subsystem and M001 as closed, removes the stale dependency-ready paragraph, and points to this closure record. The subsystem roadmap and implementation plan are both closed.

A repository planning audit found no future implementation plan that names this M001 as a hard or interface dependency. No additional plan became unblocked as a result, so no unrelated future-plan status was changed. The two existing blocked entries in the registry remain unrelated operational-evidence blockers and were intentionally not altered.

## Findings and recommendation

No critical, high, medium, or low findings were discovered. The remaining concentration in the two facade files is intentional: authorization service/domain/tests and collaboration service/action transaction logic remain together where that preserves canonical ownership and sequencing. Further decomposition should require a separate responsibility-based plan rather than a line-count response.

Recommendation: close M001 and proceed with the now-consistent planning registry.
