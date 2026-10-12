# Desktop Frontend/IDE Post-Closure Corrective C004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation-post-closure-corrective/004-lsp-safe-navigation-target-identity.md`

Affected handoff:

- `plans/implementation/desktop-frontend-ide-foundation/009-lsp-editor-presentation.md` (M006-C)

Accepted decision:

- `plans/adrs/ADR-0018-lsp-workspace-relative-navigation-identity.md` — Option
  A, an optional validated workspace-relative target field on existing LSP
  location DTOs.

Repository baseline reviewed: `6b18122d35e6e05d1f2113c78e77467848769f24`

Implementation commits: `21ba6314` and `363f5bf9`.

Hosted verification:

- Root CI `38251214377` — success on `6b18122d`; all guards, formatting,
  Workspace Clippy, and all 12,778 workspace tests passed (5 skipped).
- Desktop E2E `38251230731` — success on `6b18122d`, including the built-app
  WebDriver trajectory.

## 1. Executive finding

C004 supplies the fail-closed location identity required by M006-C. Existing
display basenames remain unchanged. The daemon derives an optional
slash-separated workspace-relative path only after converting the LSP URI and
proving canonical containment under the exact authorized workspace root.
Outside, malformed, unresolved, sibling-prefix, and symlink-escaped targets
have no navigable identity. Older serialized DTOs remain readable and default
the optional field to `None`.

This corrective does not open files, change authorization, or implement LSP
presentation. M006-C is now dependency-ready and is being handed off with the
accepted identity contract.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Additive compatibility for older location responses | `older_location_responses_default_to_non_navigable` in `crates/codegg-protocol/src/lsp.rs` | pass | Display basename remains available; omitted locator is `None`. |
| Canonical in-workspace target yields normalized relative identity | `workspace_relative_target_requires_canonical_containment` | pass | Absolute workspace prefix is not exposed. |
| URI encoding is decoded only after containment | `uri_target_is_percent_decoded_only_after_containment` | pass | Spaces round-trip as a normalized path segment. |
| Symlink escape is non-navigable | `workspace_relative_target_rejects_symlink_escape` | pass | Unix regression test. |
| Display path is not used as authority; opening retains daemon gate | `DocumentOpen` contract and editor text-authority/authorization guards | pass | M006-C must submit only through existing `DocumentOpen`. |
| Full repository and user-facing app checks | `scripts/verify.sh quick`; hosted CI and Desktop E2E | pass | Exact implementation SHA is `6b18122d`. |

## 3. Implementation evidence

`LspRangeDto.workspace_relative_path` is optional with a serde default. The
daemon populates it for location, location-link, and symbol results from the
authorized workspace root. Both root and target are canonicalized before the
containment check; the resulting path is normalized to forward slashes. A
failed conversion or proof leaves the field absent. Existing result bounds,
warm-server-only reads, `file.read` authorization, and display basename
behavior are unchanged.

Updated `architecture/lsp.md`, the LSP and TUI skills, ADR-0018, M006-C handoff
notes, and the active registry. No database schema or migration changed.

## 4. Verification executed

Local exact-revision verification:

```text
cargo test -p codegg-protocol lsp                  passed
cargo test -p codegg --lib daemon_lsp              passed in hosted suite
python3 scripts/check_authorization_matrix.py       passed in quick
python3 scripts/check_tui_editor_text_authority.py  passed in quick
scripts/verify.sh quick                            passed
```

Hosted root CI `38251214377` passed all 12,778 tests (5 skipped). Desktop E2E
`38251230731` passed on the same reviewed implementation SHA. Both runs include
the final branch contents; CI also passed Workspace Clippy and all configured
guards.

## 5. Invariant and compatibility review

- The optional path is navigation metadata, never authorization authority.
- The daemon derives workspace authority from the authenticated session.
- An absent locator is display-only; the basename is never joined to a guessed
  root.
- Every future navigation must use the existing `DocumentOpen` request, which
  revalidates containment and `file.read` authority.
- Older payloads deserialize with no navigable target.
- No LSP operation, storage, migration, or process ownership boundary changed.

## 6. Unresolved findings and handoff

| Severity | Finding | Disposition |
|---|---|---|
| none | No C004 correctness or verification finding remains. | C004 closes. |

M006-C may now implement editor presentation using this identity while
preserving the existing basename for display. Its diagnostics remain
pull-authoritative (`Ready` versus `NotReady`), stale results must be rejected
by document generation, and target opening must go through `DocumentOpen`.
The undriven diagnostics publisher remains outside this contract; M006-C must
not claim live push updates.

## 7. Registry disposition

- Mark C004 closed and record this evidence in the corrective addendum and
  `plans/registry.md`.
- Return M006-C plan 009 to ready, then mark it active when implementation
  begins. No M006-C implementation is included in this closure.
