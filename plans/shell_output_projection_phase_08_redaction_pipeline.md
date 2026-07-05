# Shell Output Projection Phase 8: Redaction Pipeline for Model-Visible Command Output

## Objective

Replace the placeholder redaction hook with a real deterministic redaction pipeline for model-visible command output. Redaction must apply consistently to raw, truncated, structured, native, expanded, and RTK-backed projections before any command output becomes model context.

This phase should not attempt perfect secret detection. It should implement a conservative, auditable, test-covered baseline that prevents common credential classes and sensitive values from being exposed through shell-output projection.

## Dependency

This phase assumes:

- Projection selection is centralized.
- `ProjectionTarget` distinguishes model context, model tool expansion, local TUI transcript, and local TUI detail views.
- Expansion handles are implemented or planned.
- Projection results carry redaction state and warnings.
- RTK/native projectors cannot bypass the selector-level redaction hook.

## Design Direction

Redaction should happen at the boundary where output becomes model-visible, not at raw storage by default. Local raw retention remains useful for user inspection and debugging, but model-visible text should be filtered.

Separate policies:

- local raw artifact retention policy
- TUI local display policy
- model-visible projection policy
- model-visible expansion policy

Default behavior should be:

- retain raw output locally within configured limits
- redact model-visible projections
- redact model-visible expansions
- mark redaction state and replacement counts
- never reveal removed sensitive values in warnings, logs, or replacement metadata

## Redaction State

If current `RedactionState` is too coarse, refine it. Suggested enum:

```rust
pub enum RedactionState {
    NotApplied,
    Applied { replacements: usize },
    AppliedNoMatches,
    SkippedByPolicy,
    Unavailable,
}
```

If backwards compatibility requires preserving existing variants, add enough metadata to distinguish placeholder/no-op behavior from real redaction.

## Rule Classes

Implement deterministic rules for common high-risk classes.

### Authorization headers and inline access tokens

Detect common authorization header forms, bearer-token style values, and named token assignments. The implementation should preserve the field name and replace the sensitive value with a stable marker.

### Environment-style secret assignments

Detect shell-style and dotenv-style assignments where the variable name indicates a key, token, secret, password, private credential, or connection credential. Match provider-specific names and generic uppercase names conservatively.

### Private-key or certificate-like sensitive blocks

Detect private-key block boundaries and replace the full block with a single marker. Avoid line-by-line partial redaction for these blocks because partial key material is still sensitive.

### Cloud and service-account credentials

Detect common cloud credential fields in command output, JSON snippets, and diagnostic dumps. Preserve non-sensitive structure but replace secret-bearing values.

### URLs with embedded credentials

Detect connection strings and URLs that contain user/password material. Preserve scheme, host, and path where useful, but replace the password or secret-bearing component.

### Cookies and session material

Detect cookie headers, session IDs, and CSRF-like tokens when they appear in HTTP logs or command output.

## Replacement Format

Use stable markers:

```text
[REDACTED:api-key]
[REDACTED:bearer-token]
[REDACTED:private-key-block]
[REDACTED:connection-password]
[REDACTED:session-token]
```

Do not include hashes of the removed value in model-visible text. If deduplication is needed later, use a keyed local-only mechanism that is never emitted to the model.

## Pipeline Placement

Apply redaction after a projector produces text but before the result is returned for model-visible targets. For structured projectors, also consider redacting structured fields before formatting if those fields can contain sensitive values.

Minimum required call sites:

- model-facing command projection
- model-facing command-output expansion
- RTK external-compressed projection result
- native Git/Rust projection result
- generic raw/truncated/error-retention projection result

Do not redact local raw artifact storage by default unless a separate config requests local redaction-at-rest.

## Config

Expose or validate:

```toml
[shell.output.redaction]
enabled = true
model_visible = true
tui_local = false
show_replacement_counts = true
```

If the config system already has `redact_model_visible_output`, wire the new redactor behind it.

## Test Strategy

Use synthetic non-realistic fixtures only. Test values should be obviously fake and must not resemble active credentials.

Add fixture-driven tests for:

1. Authorization header redaction.
2. Named token assignment redaction.
3. Provider-style key-name redaction using fake values.
4. Private-key block redaction using fake block contents.
5. Connection-string password redaction.
6. JSON service-account-like field redaction using fake values.
7. Multiple sensitive values in one output.
8. False-positive resistance for ordinary compiler diagnostics containing words such as token, key, or secret in prose.
9. Redaction applied to raw projector output.
10. Redaction applied to truncated output.
11. Redaction applied to error-retention output.
12. Redaction applied to native cargo/git output.
13. Redaction applied to RTK output.
14. Redaction applied to model-visible expansion.
15. TUI local detail remains unredacted when policy says local display is allowed.

## Success Criteria

- Placeholder redaction hook is replaced with real deterministic redaction.
- Model-visible projections and expansions pass through the redactor.
- Redaction state accurately reports applied/no-match/skipped behavior.
- Common sensitive-value classes are redacted with stable markers.
- Local raw retention remains available unless explicitly configured otherwise.
- Tests cover all projector classes and expansion path.

## Non-Goals

- Do not claim perfect secret detection.
- Do not implement ML-based redaction.
- Do not redact local raw storage by default.
- Do not remove raw-output handles solely because model-visible text was redacted.
- Do not expose original sensitive values in warnings, logs, or replacement metadata.

## Handoff Notes

This phase is a prerequisite for making RTK or command-output expansion comfortable in more workflows. The selector-level call site from Phase 2 is the right control point; keep all projector backends subject to the same redaction contract.
