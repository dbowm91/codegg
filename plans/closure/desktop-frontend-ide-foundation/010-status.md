# Desktop Frontend and IDE Foundation Milestone 010 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/009-lsp-editor-presentation.md` (M006-C)

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Controlling decisions and prerequisites:

- ADR-0012 — native LSP reads and authorization.
- ADR-0018 Option A — optional validated workspace-relative location target.
- Corrective closures C003 and C004: `plans/closure/desktop-frontend-ide-foundation-post-closure-corrective/003-status.md` and `004-status.md`.

Implementation revision: `91cbe784aa442ad268c95ed8aca709820d5413e2` (PR #111).

## 1. Executive finding

M006-C presents the existing daemon-owned LSP read surface in the TUI editor.
Current-file diagnostics use the pull authority and display explicit readiness
and stale states. Editor actions expose the existing hover, definition,
references, and symbol reads. Location results navigate only when the
workspace-relative target is present and accepted by the existing document
open and authorization path. Results remain tied to the active editor
generation; invalid or stale ranges fall back safely. No LSP server, protocol
operation, storage, or second text buffer was added.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Current-file diagnostics are complete only after a successful pull | Editor diagnostics pull and explicit `Ready`/`NotReady` presentation; refresh on open/save/manual action | pass |
| Late results cannot decorate a different document | Editor generation matching and switch/close regression tests | pass |
| Existing read operations are exposed without new protocol authority | Existing `LspReadGet` operations and `LspDiagnosticsGet`; daemon remains LSP owner | pass |
| Navigation requires a safe workspace-relative location | ADR-0018 optional locator, daemon containment conversion, and `DocumentOpen` revalidation | pass |
| Malformed ranges/tokens fail closed | Bounded span validation and plain-text fallback tests | pass |
| Text remains owned by `DocumentController` | `check_tui_editor_text_authority.py` passed; no alternate document cache | pass |
| Focused and quick verification | M006-C focused editor/document trajectory tests and `scripts/verify.sh quick` passed at implementation revision | pass |
| Hosted root CI | Run `38256472016`, exact SHA `91cbe784`; verify job success, 24/24 steps; workspace sweep 12,783 passed, 5 skipped | pass |
| Hosted built-app trajectory | Desktop E2E run `38256472313`, attempt 2, exact SHA `91cbe784`; success | pass |

## 3. Implementation and lifecycle contract

The editor consumes existing LSP DTOs and keeps its own presentation state
bounded to the active document generation. Diagnostics are refreshed through
`LspDiagnosticsGet`; the delivered diagnostics publisher remains undriven, so
this closure makes no claim of live push updates. `NotReady`, failed reads, and
stale snapshots remain distinct from an authoritative empty diagnostic set.

Location display basenames remain available for all results. Navigation uses
only the optional validated workspace-relative locator and opens through the
existing document request, which rechecks workspace containment and
`file.read` authorization. Missing, outside-workspace, or malformed targets
remain display-only. Existing LSP write/apply behavior is unchanged.

## 4. Verification executed

- Focused editor/presentation tests and `cargo test --test document_client_trajectory` passed.
- `cargo fmt --all -- --check` passed.
- `python3 scripts/check_tui_editor_text_authority.py` and
  `python3 scripts/check_authorization_matrix.py` passed.
- `scripts/verify.sh quick` passed.
- Hosted CI run `38256472016`: success, 24/24 steps; workspace suite 12,783 passed, 5 skipped.
- Hosted Desktop E2E run `38256472313` attempt 2: success on the exact implementation SHA.

## 5. Invariant and security review

- `DocumentController` remains the only editor-text owner.
- The TUI starts no language server and does not accept principal, project, or
  workspace authority from LSP result fields.
- Navigation targets are locators, not authority; the existing document-open
  handler performs final containment and authorization.
- LSP messages and symbols remain bounded text. Invalid ranges and tokens do
  not alter document text or widen navigation.
- Diagnostics remain pull-authoritative; no new push cadence is claimed.

## 6. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| none | No M006-C correctness or verification finding remains. | M006-C closes. |

## 7. Registry disposition

- Mark M006-C closed in its implementation plan, subsystem roadmap, and
  `plans/registry.md`.
- The M006-A through M006-E presentation sequence is complete. M002's
  independent Windows qualification remains unchanged.
