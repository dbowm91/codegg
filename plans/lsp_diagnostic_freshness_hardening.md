# LSP Diagnostic Freshness Hardening Plan

## Purpose

The previous pass successfully wired diagnostic freshness metadata into the LSP diagnostic cache, public diagnostics output, semantic context packets, and security context packets. This hardening pass should close the remaining correctness gaps before moving on to broader semantic-context DTO consolidation.

This plan is intentionally narrow. The goal is to make the current diagnostic freshness implementation precise, internally consistent, and well-tested.

## Current State

As of `0e08d317e61ee22ddb6946d98a774bdf23526b68`:

- `DiagnosticCacheEntry` stores diagnostics with `received_at`, `source`, and `content_version`.
- `LspClient::diagnostic_snapshot()` classifies diagnostics as `Fresh`, `PossiblyStale`, `Stale`, or `Unavailable`.
- The public `diagnostics` tool output exposes `freshness`, `source`, `generated_at_ms`, `usable_evidence`, and `diagnostics_may_still_be_warming`.
- `semanticContext` and `securityContext` include optional `diagnostic_evidence` metadata.
- Optional LSP enrichment in `semanticContext` and `securityContext` is gated by `LspCapabilitySnapshot`.
- A transitional adapter exists from local `SemanticContextPacket` to `egglsp::semantic_context::SemanticContextResponse`, but it is not yet the runtime source of truth.

Known issues to harden:

- `generated_at_ms` currently behaves like elapsed age since receipt, not a generated-at timestamp.
- `uri_to_path_str()` strips `file://` manually instead of using URL decoding and platform-correct file URI conversion.
- `save_file()` does not participate in freshness tracking.
- `DiagnosticsCollector::get_diagnostics_for_file()` still bypasses the snapshot path and reconstructs legacy output from raw diagnostics.
- Capability snapshots inside `semanticContext` and `securityContext` use placeholder server/language metadata.
- Tests should explicitly cover fresh, possibly-stale, stale, unavailable, compatibility wrapper, and public output behavior.

## Non-Goals

Do not rewrite the LSP tool.

Do not remove the existing public `semanticContext` or `securityContext` response shape.

Do not make the shared semantic-context DTO the source of truth in this pass.

Do not add new LSP operations.

Do not require live language servers in tests.

Do not change model, security-review, or agent-loop behavior beyond better diagnostic metadata semantics.

## Phase 1 — Fix Diagnostic Timestamp Semantics

Choose one clear representation and make code, field names, and documentation agree.

Preferred option:

- Rename serialized `generated_at_ms` to `received_age_ms` or `age_ms` because the implementation uses `Instant::elapsed()`.
- Keep `received_at: Instant` internally.
- Serialize age as a relative duration, not an absolute timestamp.

Alternative option:

- Add `received_at_system: SystemTime` to `DiagnosticCacheEntry`.
- Keep `generated_at_ms` as Unix epoch milliseconds derived from `SystemTime`.
- Continue using `received_at: Instant` only for freshness comparisons.

Recommendation: use the preferred option unless a downstream consumer already expects epoch milliseconds. Relative age is enough for model/tool consumers and avoids wall-clock correctness edge cases.

Implementation notes:

- Update `LspDiagnosticSnapshot` field name if choosing the preferred option.
- Update `DiagnosticEvidenceMeta` and public diagnostics output accordingly.
- Update docs in `architecture/lsp.md`, `.opencode/skills/lsp/SKILL.md`, and AGENTS verified facts if needed.
- Preserve backward compatibility only if necessary. If compatibility matters, temporarily emit both fields:

```json
{
  "received_age_ms": 42,
  "generated_at_ms": 42
}
```

and document `generated_at_ms` as deprecated/misnamed.

Acceptance criteria:

- The field name matches the actual semantics.
- Docs do not describe age as an absolute generated timestamp.
- Tests assert the value is non-negative and monotonic enough for snapshot usage without relying on exact timing.

## Phase 2 — Replace Manual File URI Stripping With Robust Conversion

Replace `uri_to_path_str()` with URL-aware conversion.

Current problem:

```rust
fn uri_to_path_str(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_string()
}
```

This loses URL decoding and mishandles spaces, percent-encoded characters, Windows paths, and non-file URI forms.

Suggested replacement:

```rust
fn uri_to_path_buf(uri: &str) -> PathBuf {
    url::Url::parse(uri)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .unwrap_or_else(|| PathBuf::from(uri))
}
```

Use this helper in `LspClient::diagnostic_snapshot()`.

Acceptance criteria:

- File URIs with spaces or percent encoding produce readable filesystem paths.
- Non-file URI strings degrade to the original string/path without panicking.
- Unit tests cover at least:
  - `file:///tmp/a%20b.rs`
  - a plain non-URI string
  - platform-safe normal file URI path

## Phase 3 — Make Save Semantics Explicit in Freshness Tracking

Decide whether `didSave` should alter freshness state and encode that decision.

Preferred behavior:

- If `save_file(uri, Some(text))` is called, update `last_content_change_at[uri] = Instant::now()` because the server may now recompute diagnostics for saved content.
- If `save_file(uri, None)` is called, either:
  - leave freshness unchanged because there is no new content payload; or
  - record a separate `last_save_at` only for diagnostics warming/diagnostic-request logic.

Recommendation: update `last_content_change_at` only when save includes text. Document this.

Acceptance criteria:

- Save-with-text can make a previously fresh snapshot `PossiblyStale` until new diagnostics arrive.
- Save-without-text behavior is documented and tested.
- `diagnostics_may_still_be_warming()` remains consistent with the chosen behavior.

## Phase 4 — Derive Legacy Diagnostics Output From Snapshots

Make `LspDiagnosticSnapshot` the single diagnostic read model.

Current issue:

- `DiagnosticsCollector::get_diagnostic_snapshot_for_file()` uses the new snapshot path.
- `DiagnosticsCollector::get_diagnostics_for_file()` still calls `get_diagnostics_for_key()` and manually rebuilds old output.

Change `get_diagnostics_for_file()` so it calls `get_diagnostic_snapshot_for_file()` and derives `DiagnosticsOutput` from the snapshot.

Rules:

- `diagnostics_may_still_be_warming` should be derived from snapshot freshness where possible.
- If freshness is `PossiblyStale` and there are no diagnostics yet, warming should be true.
- If freshness is `Unavailable` shortly after open/change, warming can remain true via service helper, but document this compatibility edge.
- Do not let a legacy empty diagnostics vector erase freshness semantics internally.

Acceptance criteria:

- There is one authoritative diagnostic read path.
- Compatibility callers retain the same output shape.
- Tests verify the compatibility wrapper uses snapshot semantics.

## Phase 5 — Improve Capability Snapshot Metadata in Context Operations

`semanticContext` and `securityContext` currently build capability snapshots with placeholder metadata such as `Some("lsp")` and `None` language.

Refactor into a helper:

```rust
async fn capability_snapshot_for_file(
    &self,
    file: &Path,
) -> Option<egglsp::LspCapabilitySnapshot>
```

The helper should:

- call `get_or_create_client(file)`;
- retrieve `ServerCapabilities`;
- derive language via `crate::lsp::language::detect_language(file.to_str().unwrap_or(""))`;
- derive server name from the client key or server id consistently with the public `capabilities` operation;
- call `LspCapabilitySnapshot::from_capabilities(&caps, server_name.as_deref(), lang)`.

Then use the helper in:

- public `capabilities` operation, if useful;
- `semanticContext` optional operation gating;
- `securityContext` optional operation gating.

Acceptance criteria:

- Capability metadata is consistent across public `capabilities`, `semanticContext`, and `securityContext`.
- Unsupported-operation notes can include useful server/language metadata later without refactoring again.
- No behavior regression in fail-open behavior when no snapshot is available.

## Phase 6 — Tighten Security Context Diagnostic Evidence Semantics

Security review should never treat stale or unavailable diagnostics as negative evidence.

Current behavior already adds notes for stale/unavailable diagnostics. Harden this by ensuring filtering logic cannot produce misleading clean results.

Implementation notes:

- If diagnostic evidence is `Unavailable`, keep `security_relevant_diagnostics` empty but add a note explaining that no diagnostic evidence was available.
- If diagnostic evidence is `Stale`, preserve stale diagnostics if present, but add a low-confidence note.
- Consider adding a `diagnostic_evidence.usable_evidence` check in any later synthesis adapter so stale/unavailable diagnostics are not used as proof of absence.
- Do not drop stale diagnostics silently.

Acceptance criteria:

- Tests verify stale diagnostics are preserved with low-confidence metadata.
- Tests verify unavailable diagnostics produce explanatory notes.
- The evidence extraction path for security review does not convert unavailable diagnostics into a clean bill of health.

## Phase 7 — Add Focused Tests Without Live LSP Servers

Add tests at the lowest practical layer.

Suggested tests in `crates/egglsp/src/client.rs` or a nearby test module:

- `dispatch_notification_records_cache_metadata`
- `diagnostic_snapshot_unavailable_when_no_entry`
- `diagnostic_snapshot_fresh_when_entry_after_last_change`
- `diagnostic_snapshot_possibly_stale_when_change_after_entry`
- `diagnostic_snapshot_stale_after_client_invalidation`
- `diagnostic_snapshot_uses_url_decoded_file_path`
- `save_with_text_marks_diagnostics_possibly_stale`

Suggested tests in `crates/egglsp/src/diagnostics.rs`:

- `compat_diagnostics_output_derives_from_snapshot`
- `snapshot_usable_evidence_matches_freshness_policy`

Suggested tests in `src/tool/lsp.rs`:

- `diagnostic_evidence_meta_serializes_expected_fields`
- `semantic_context_response_adapter_preserves_basic_fields`
- `capability_snapshot_for_file_uses_real_language_metadata` if practical with fake/service-level setup

Avoid live LSP servers. Use direct cache insertion, fake diagnostics, or small helper constructors.

Acceptance criteria:

- Fresh/possibly-stale/stale/unavailable transitions are covered.
- Path conversion is covered.
- Public output metadata shape is covered where practical.
- Tests are deterministic and not timing-flaky.

## Phase 8 — Documentation Cleanup

Update the docs only after code behavior is finalized.

Files:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `AGENTS.md` verified facts table, only for facts that remain true after hardening

Documentation should clearly state:

- Whether diagnostic timestamp metadata is an age or epoch timestamp.
- Which operations update content-change freshness.
- What `Fresh`, `PossiblyStale`, `Stale`, and `Unavailable` mean.
- That stale/unavailable diagnostics are not proof that code is clean.
- That capability-gated optional operations fail soft.

Acceptance criteria:

- No stale wording such as `last_opened_at` if the implementation uses `last_content_change_at`.
- No claim that `generated_at_ms` is absolute if it is relative age.
- Docs match public JSON fields exactly.

## Suggested Verification Commands

Run:

```bash
cargo fmt --all
cargo test -p egglsp
cargo test --lib lsp
```

Then, if feasible:

```bash
cargo test --all --workspace
```

If the full workspace test is skipped, record why in the handoff summary.

## Review Checklist

Before considering this hardening pass complete:

- Diagnostic timestamp field semantics are correct and documented.
- File URI conversion is URL-aware.
- Save behavior is explicit and tested.
- Legacy diagnostic output derives from snapshots.
- Capability metadata is consistent across operations.
- Security context does not treat missing/stale diagnostics as negative evidence.
- Tests cover all freshness states without live servers.
- Public JSON remains backward compatible or the breaking change is intentional and documented.

## Expected Follow-Up

After this hardening pass, the next LSP phase should be semantic-context consolidation: make `egglsp::semantic_context::SemanticContextResponse` the internal source of truth and treat `SemanticContextPacket` / `SecurityContextPacket` as presentation DTOs. That should be a separate pass after diagnostic freshness semantics are stable.
