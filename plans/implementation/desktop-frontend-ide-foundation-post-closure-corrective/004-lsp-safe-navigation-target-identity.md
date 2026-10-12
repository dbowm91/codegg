# Desktop Frontend/IDE Post-Closure Corrective C004 — LSP Safe Navigation Target Identity

Status: active

Repository baseline: `5fcfab317c446ec4d206a9b28c0cb22deb128368`

Source corrective addendum:

- `plans/subsystems/desktop-frontend-ide-foundation-post-closure-corrective-addendum.md`

Affected implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/009-lsp-editor-presentation.md` (M006-C; blocked at WP-A)

Applicable ADRs:

- Proposed `plans/adrs/ADR-0018-lsp-workspace-relative-navigation-identity.md`
- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md` (accepted)
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md` (accepted)

Primary class: invariant/capability

## 1. Objective

Provide an additive, fail-closed identity for LSP definition/reference/symbol
targets so M006-C can navigate only to daemon-validated workspace-relative
paths. ADR-0018 is accepted with Option A: add an optional validated
workspace-relative target field to existing LSP location DTOs.

## 2. Why this corrective is required

M006-C's WP-A audit found that `LspRangeDto.path` is a display basename for
location results. The daemon intentionally omits absolute URIs because results
may point outside the requesting workspace. A basename cannot be safely opened
and must not be joined or guessed by the TUI. This blocks the navigation
acceptance criteria in M006-C.

The existing `DocumentOpen` path already provides final containment and
`file.read` authorization. The missing contract is an optional response
locator derived by the daemon from the authorized workspace and returned LSP
URI.

## 3. Current implementation evidence

- `crates/codegg-protocol/src/lsp.rs` defines `LspRangeDto.path` as a string.
- `src/core/daemon_lsp.rs::location_to_dto` and `location_link_to_dto` retain
  only `uri_file_name` because the target may be outside the request workspace.
- `LspSymbolDto.range` uses the same bounded range type.
- M006-C plan 009 records its WP-A state/generation audit and is blocked pending
  a safe location identity contract or a bounded scope amendment.
- `DocumentOpen` remains the authority for opening a workspace-relative path.

## 4. Invariants that must not regress

1. A response path is never authorization authority; `DocumentOpen` revalidates
   every target.
2. The daemon derives the workspace root from the authorized session, not from
   DTO fields or LSP payload metadata.
3. Outside-workspace, absolute, malformed, and symlink-escaped targets have no
   navigable identity.
4. Older responses without the additive field deserialize to non-navigable.
5. Existing display paths, result bounds, warm-only LSP behavior, and `file.read`
   authorization remain unchanged.
6. No document text is copied into TUI presentation state.

## 5. Scope

### In scope if Option A is accepted

- Add an optional workspace-relative target field to the existing location DTO
  with a `None` compatibility default.
- Derive the field only after validating URI conversion and canonical
  containment beneath the exact authorized workspace root.
- Preserve basename display behavior for targets that cannot be proven inside
  the workspace.
- Add protocol and daemon tests for valid, outside, malformed, encoded, and
  symlink-escaped targets; add compatibility deserialization coverage.
- Update M006-C plan 009 and registry state when the interface is stable.

### Explicitly out of scope

- New LSP operations, authorization capabilities, storage, or migrations.
- Opening targets by guessed basename or URI from the frontend.
- Starting a cold LSP server or changing diagnostics delivery.
- Implementing the rest of M006-C's editor presentation.

## 6. Required production changes

Protocol: add the optional target identity only if Option A is accepted; do not
replace the existing display field.

Daemon: derive the target from the session's server-resolved workspace. Convert
the URI to a filesystem path, canonicalize both target and workspace root, and
emit a normalized slash-separated relative path only when the target remains
under that exact root. Any failed proof yields `None`.

Frontend: treat the field as untrusted, reject malformed relative paths, and
submit the result only through existing `DocumentOpen`. Never join the display
basename to a path or navigate to an absolute URI.

Documentation: update `architecture/lsp.md`, `architecture/tui.md`, the LSP
and TUI skills, and `docs/tui.md` to state that outside-workspace results are
display-only and opening always returns through daemon authority.

## 7. Ordered work packages

1. Reconfirm the authorized workspace root and URI normalization behavior.
2. Add the optional fail-closed protocol identity and compatibility test.
3. Populate the identity at the daemon boundary with canonical containment.
4. Prove outside-workspace and symlink-escape results remain non-navigable.
5. Update M006-C handoff and registry only after the interface is stable.

## 8. Failure and compatibility semantics

If the field is missing, malformed, or cannot be derived, the result remains
display-only. A failed `DocumentOpen` leaves the editor on the current
document and surfaces its existing bounded error. No response can bypass
reconnection or workspace selection.

## 9. Migration

No storage migration. Option A is additive and older daemons omit the field;
new clients treat omission as no navigation target.

## 10. Required tests

- Valid in-workspace file URI produces a normalized workspace-relative path.
- Percent-encoded path segments round-trip without changing identity.
- Outside workspace, sibling-prefix, malformed URI, and symlink escape produce
  no target path.
- Older serialized location responses deserialize with no target path.
- The TUI submits a target through `DocumentOpen` and does not directly read or
  open the path.
- Existing LSP authorization and editor text-authority guards pass.

## 11. Required verification

```bash
cargo fmt --all -- --check
cargo test -p codegg-protocol lsp
cargo test -p codegg --lib daemon_lsp
python3 scripts/check_authorization_matrix.py
python3 scripts/check_tui_editor_text_authority.py
scripts/verify.sh quick
```

Hosted root CI is required before closure. Desktop E2E is required if the final
change wires navigation into the user-facing editor route.

## 12. Acceptance criteria

- A valid target carries only a normalized workspace-relative identity.
- An outside or unprovable target cannot be opened from the editor.
- Old daemon responses remain displayable and cannot trigger navigation.
- Existing authorization and document ownership remain authoritative.

## 13. Stop conditions

- Maintainers choose ADR-0018 Option B or C; this plan then requires replacement
  or supersession before implementation.
- Workspace identity cannot be derived from the authorized session.
- Canonical path validation cannot reject outside-root or symlink-escaped
  targets without broadening authority.

## 14. Closure evidence

Record the chosen ADR outcome, URI/path state table, compatibility tests,
containment and symlink tests, authorization/text-authority guards, quick
verification, hosted CI, and any M006-C status change. Do not close M006-C from
this infrastructure contract alone.
