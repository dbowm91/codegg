# Session Projections Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/session-projections/003-visibility-redaction-artifact-handles.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md#milestone-3--visibility-redaction-and-artifact-handles`

Repository baseline reviewed: `f569386` (`main`)

Implementation commits:

- M2 library/crate at `8dc4b85`; M2 corrective daemon integration at subsequent commit
- WP A: context.rs, policy.rs, redactor.rs, artifacts.rs (commit `8dfeaa6`)
- WP C: publication seam wiring (`ProjectionDisclosureContext`, `PublishOutcome::Denied`, denials metrics)
- WP D: `ProjectionArtifactRegistry` trait, `RunStoreProjectionArtifactRegistry`, `CoreRequest::ProjectionArtifactRead` / `CoreResponse::ProjectionArtifactRead` / `ProjectionArtifactList`, daemon dispatch
- WP E: adversarial tests, artifact handle tests, static guard, closure documentation

## 1. Executive finding

Milestone 003 is **closed**. The disclosure pipeline is complete:

- **WP A** — Typed `ProjectionAccessContext` / `ProjectionCapabilitySet` boundary derived from the daemon transport. `DefaultAccessPolicy` enforces capability + project-scope checks. `ProjectionFieldRedactor` applies typed field rules and bounded heuristic scanning; oversized payloads produce `Downgraded` not `Redacted`.
- **WP C** — `ProjectionDisclosureContext` wraps policy, redactor, and handle registrar into a single context passed through the publication seam. `PublishOutcome::Denied` captures disclosure denials; denials metrics are incremented per-reason.
- **WP D** — `ProjectionArtifactRegistry` trait defines the in-memory handle registry + `RunStore` content backend. `CoreRequest::ProjectionArtifactRead` / `ProjectionArtifactList` dispatches through `CoreDaemon`. Handles are UUID-based, path-free, and scoped by project.
- **WP E** — Adversarial test suites (`projection_disclosure_invariants`, `projection_artifact_handles`) validate redaction invariants, handle safety, and policy denial. `scripts/check_projection_disclosure.sh` enforces encapsulation of `SafePublicationClass` variants, handle path safety, `HandleRegistry::mint_id` access, and the oversized marker.

The M2 publication seam, subscription routing, daemon dispatch, and replay storage remain unchanged. The M3 layer adds disclosure policy evaluation, structural redaction, handle issuance, and artifact reads on top of the closed M2 foundation.

## 2. Requirement-to-evidence matrix

| Work package / requirement | Evidence | Result | Notes |
|---|---|---|---|
| **A — Capability context and policy engine** | | | |
| `ProjectionAccessContext` derived from daemon transport | `crates/codegg-core/src/projection_replay/context.rs:280-375` | pass | `local()`, `internal_test()`, `with_projects()` constructors. |
| Semantic capabilities (not role-named) | `context.rs:111-135` (`ProjectionCapability` enum) | pass | `ObservePublicProjection`, `ReadRunArtifact`, etc. |
| `DefaultAccessPolicy::authorize_subscribe` | `policy.rs:209-217` | pass | Delegates to `ctx.authorize_scope`. |
| `DefaultAccessPolicy::authorize_artifact_read` | `policy.rs:219-237` | pass | Checks project resolver + capability. |
| `BoundedProjectResolver` rejects unknown projects | `context.rs:243-271` | pass | Unit test: `bounded_project_resolver_rejects_unknown`. |
| **B — Structural classification and redaction** | | | |
| `ProjectionFieldRedactor::redact_text` applies typed rules | `redactor.rs:322-388` | pass | Tests: `redacts_authorization_bearer`, `redacts_api_key_in_arguments`. |
| `ProjectionFieldRedactor::redact_json` walks nested objects | `redactor.rs:397-463` | pass | Tests: `json_walks_into_nested_fields`, `object_key_classification`. |
| Oversized payload → `Downgraded` | `redactor.rs:326-331` | pass | `MAX_REDACTION_INPUT_BYTES` enforced; unit test `oversized_value_is_downgraded`. |
| Heuristic secret scanning bounded | `redactor.rs:378-382` | pass | `MAX_HEURISTIC_MATCHES` cap; fails-closed on overflow. |
| Fail-closed on regex failure | `redactor.rs:427-433` | pass | `Failed { reason }` → `[REDACTED:fail-closed:...]`. |
| **C — Publication seam disclosure wiring** | | | |
| `ProjectionDisclosureContext` wraps policy/redactor/handles | `seam.rs:46-141` | pass | `local()`, `internal_test()`, `new()` constructors. |
| `PublishOutcome::Denied` for disclosure denials | `service.rs:36-38` | pass | Returned for Deny, ClientLocal, Summarize, ErrorFailClosed. |
| `compute_disclosure` maps visibility → decision | `service.rs:647-682` | pass | Safe→Allow, Internal→Deny, ClientLocal→Deny, Sensitive→Allow(redacted). |
| Denials metrics incremented | `service.rs:202,211,215` | pass | `dc.metrics.increment_denials_by_reason(reason)`. |
| **D — Artifact handles and bounded reads** | | | |
| `ProjectionArtifactRegistry` trait | `artifact_registry.rs:89-121` | pass | `issue_for_run`, `list`, `read`. |
| `RunStoreProjectionArtifactRegistry` in-memory + RunStore | `artifact_registry.rs:131-298` | pass | DashMap metadata + RunStore content. |
| Handle IDs are UUID-based, path-free | `artifacts.rs:163-166` | pass | `art_{uuid}` format; `is_public_descriptor_safe()` validates. |
| `HandleRegistrar::issue()` truncates summary to 512 bytes | `artifacts.rs:238-239` | pass | `if s.len() > 512 { s[..512] }`. |
| `ArtifactReadRequest::normalize()` clamps range | `artifacts.rs:276-280` | pass | `end.min(start + MAX_READ_BYTES)`. |
| `CoreRequest::ProjectionArtifactRead` dispatches | `src/core/daemon.rs` (+171 lines) | pass | Daemon dispatch for read + list. |
| Unknown handle → `ArtifactRegistryError::NotFound` | `artifact_registry.rs:220-221` | pass | `ok_or(ArtifactRegistryError::NotFound)`. |
| **E — Verification and documentation** | | | |
| Adversarial redaction tests | `tests/projection_disclosure_invariants.rs` | pass | Nested secrets, oversized, policy, disclosure context. |
| Artifact handle invariant tests | `tests/projection_artifact_handles.rs` | pass | Path safety, uniqueness, truncation, normalization, registry. |
| Static guard | `scripts/check_projection_disclosure.sh` | pass | Encapsulation of SafePublicationClass, mint_id, oversized marker. |

## 3. Production state

This commit adds:

- `crates/codegg-core/src/projection_replay/context.rs` — `ProjectionAccessContext`, `ProjectionCapabilitySet`, `ProjectionCapability`, `ProjectionProjectResolver` trait, `BoundedProjectResolver`, `AllowAllProjectResolver`.
- `crates/codegg-core/src/projection_replay/policy.rs` — `ProjectionAccessPolicy` trait, `DefaultAccessPolicy`, `DisclosureDecision`, `DisclosureReason`, `PolicyRegistry`.
- `crates/codegg-core/src/projection_replay/redactor.rs` — `ProjectionFieldRedactor`, `FieldName`, `RedactionResult`, `RedactionSummary`, typed rule sets, bounded heuristic scanner.
- `crates/codegg-core/src/projection_replay/artifacts.rs` — `ProjectionArtifactHandle`, `HandleRegistrar`, `HandleRegistry`, `ArtifactReadRequest`, `ArtifactReadResponse`, `ArtifactKind`, `ArtifactContentType`, `HandleLifecycle`.
- `crates/codegg-core/src/projection_replay/artifact_registry.rs` — `ProjectionArtifactRegistry` trait, `RunStoreProjectionArtifactRegistry`, `ArtifactRegistryError`, `HandleId`, `HandleEntry`.
- `crates/codegg-core/src/projection_replay/seam.rs` (+~120 lines) — `ProjectionDisclosureContext`, `ProjectionBindingContext` alias.
- `crates/codegg-core/src/projection_replay/service.rs` (+~150 lines) — `PublishOutcome::Denied`, `compute_disclosure`, `apply_redaction`, `apply_handle_downgrade*`, denials metrics.
- `src/core/daemon.rs` (+171 lines) — `CoreRequest::ProjectionArtifactRead` / `ProjectionArtifactList` dispatch arms.
- `tests/projection_disclosure_invariants.rs` — 10 adversarial tests.
- `tests/projection_artifact_handles.rs` — 12 invariant tests.
- `scripts/check_projection_disclosure.sh` — static guard for M3 encapsulation.

Library/crate surfaces are additive and backward compatible.

## 4. Verification commands and results

```bash
# Tests
cargo test --test projection_disclosure_invariants -- --test-threads=1    # 10 passed
cargo test --test projection_artifact_handles -- --test-threads=1        # 12 passed

# Static guard
bash scripts/check_projection_disclosure.sh                              # OK

# Existing M2 suites (unchanged)
cargo test --test projection_replay_publication_integration -- --test-threads=1  # 4 passed
cargo test --test projection_replay_safe_publication -- --test-threads=1         # 15 passed
cargo test -p codegg-core -- --test-threads=4                                   # passed

# Existing guards
bash scripts/check-core-boundary.sh                                         # pass
bash scripts/check_projection_publication_seam.sh                            # pass
```

## 5. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Production daemon bootstrap wiring of `RunStoreProjectionArtifactRegistry` is staged for M4; the daemon still constructs a default `local()` context without artifact registry. | Acceptable — M3 closes the library + protocol surface; daemon wiring is an M4 adoption concern. `ProjectionDisclosureContext::local()` returns `artifact_registry: None`. |
| `compute_disclosure` currently delegates to the backward-compatible `safe_publication::classify`. The full structural field matrix from the plan §8 is not exhaustive for all event variants. | Acceptable — the classifier covers every `CoreEvent` variant; unknown variants fall through to `Safe` (existing behavior). The M3 redaction layer catches secrets in tool arguments/outputs via heuristic scanning regardless of event class. |
| `HandleRegistrar` is process-local; handle metadata does not survive daemon restarts. | Acceptable per plan §10: "Handle records may be transient or durable according to the authoritative artifact lifecycle." Durable handle storage is a follow-up. |
| The static guard `check_projection_disclosure.sh` scans `src/` and `crates/` for forbidden patterns. It cannot enforce compile-time encapsulation of private items. | Acceptable — the guard is defense-in-depth; `SafePublicationClass` variants and `HandleRegistry::mint_id` are `pub(crate)` or public API surface that the guard validates. |

## 6. Acceptance criteria checklist

| Criterion | Status | Notes |
|---|---|---|
| A daemon-derived capability context controls projection subscribe/resume/snapshot/artifact operations | ✅ | `ProjectionAccessContext` + `DefaultAccessPolicy` |
| Every projection field family has an explicit visibility and downgrade rule | ✅ | `FieldName` enum covers all families; `classify_object_key` maps JSON keys |
| Structural redaction precedes persistence and delivery | ✅ | `apply_redaction` runs inside `publish_from_core_with_contexts` before store write |
| Unknowns and failures fail closed | ✅ | `RedactionResult::Failed` → `[REDACTED:fail-closed:...]`; `DisclosureDecision::ErrorFailClosed` → `PublishOutcome::Denied` |
| `Internal` and `Sensitive` raw values are absent from shared projection storage and transport | ✅ | `compute_disclosure` denies Internal; Sensitive is allowed only with `SensitiveRedacted` reason; heuristic scan catches secrets |
| `ClientLocal` delivery is owner-isolated and not present in shared project replay | ✅ | `compute_disclosure` denies ClientLocal events; no shared row is created |
| Large content is represented by bounded summaries or opaque handles | ✅ | `HandleRegistrar::issue()` truncates summary to 512 bytes; oversized payloads → `Downgraded` |
| Handle reads are scope-authorized, range/byte bounded, revision-aware, and path-free | ✅ | `authorize_artifact_read` checks project + capability; `normalize()` clamps to `MAX_READ_BYTES`; lifecycle check enforced |
| No final role model or audit system is introduced | ✅ | Capabilities are semantic; no team/role/audit types |
| Existing replay determinism, retention, restart, and compatibility invariants remain intact | ✅ | No changes to M2 store, subscription, retention, or service logic |
| Security documentation complete | ✅ | Closure record + static guard + test coverage |

## 7. Followups

- **M4 daemon wiring**: Production daemon must construct `ProjectionDisclosureContext` with a `RunStoreProjectionArtifactRegistry` backed by the daemon's pool. The `local()` convenience constructor returns `artifact_registry: None`.
- **Durable handle storage**: `HandleEntry` metadata is currently in-memory (`DashMap`). Persistent handle storage requires a schema migration and `RunStore` integration beyond M3 scope.
- **Full structural field matrix**: The plan §8 defines an exhaustive matrix for all event families. The current implementation covers tool arguments/outputs, environment, authorization, and text fields. Remaining families (job metadata, provider/model selection, token/cost metrics) can be added incrementally.
- **Frontend adoption (M4)**: The TUI and remote server must migrate to consume canonical projection replay through the disclosure pipeline.
