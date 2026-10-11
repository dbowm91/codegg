# CodeGG Desktop Frontend and IDE Foundation M006-C — LSP Editor Presentation

Status: ready

Repository baseline: `41eb955e`

Source roadmap: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m006-decomposition`

Audit of record: `plans/subsystems/desktop-frontend-ide-foundation-m006-tui-presentation-audit.md`

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#87-project-authorization`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md` (accepted)
- `plans/adrs/ADR-0008-lsp-preview-apply-authorization-and-runtime-ownership.md` (accepted)

Primary class: capability

## 1. Objective

Present the daemon-owned LSP state in the existing TUI editor: diagnostics,
semantic/syntax highlighting, and read-only symbol navigation. Use the M006-B
protocol and authorization contracts; do not introduce another LSP owner or
client-supplied workspace authority.

## 2. Current implementation evidence

- M006-A provides the editor route, controller-backed buffer, and diagnostic-free
  gutter in `src/tui/components/editor.rs`.
- M006-B provides `codegg_client::lsp::LspReadClient`, bounded LSP read DTOs,
  `DiagnosticsReconciler`, a project-scoped diagnostics subscription, and an
  authoritative diagnostics pull.
- ADR-0012 makes the pull authoritative and diagnostics stream project-scoped.
- The M006-B diagnostics publication task currently has no production caller;
  wire the existing publisher into daemon runtime lifecycle before depending on
  stream updates. The pull remains the recovery path.
- `syntect` is already a dependency and code-block rendering demonstrates the
  existing syntax theme integration. The editor currently emits one plain span
  per line.
- M006-C is intentionally limited to the existing TUI and read protocol. There
  is no need for an LSP protocol or permission expansion.

## 3. Invariants that cannot regress

- LSP state is requested only through `codegg-client` and remains daemon-owned.
- Session/workspace identity comes from the active tab and daemon records;
  renderers never infer authority from process cwd or arbitrary file paths.
- Diagnostics from another project, stale sequence, or mismatched document
  version must never appear in the active editor.
- `NotReady`, unsupported, and failed reads remain distinguishable from an
  empty diagnostic set.
- All event handling and requests remain bounded; rendering performs no I/O.
- Navigation is read-only and opens targets through the existing project-scoped
  file-tree/editor path. No preview apply or dirty-buffer behavior changes.
- Existing preview-apply authorization and `src/lsp/mutation.rs` stay untouched.

## 4. Scope and non-goals

In scope:

- Start and stop the daemon diagnostics publisher through its supported runtime
  lifecycle, and consume the authorized project diagnostics stream in socket TUI
  sessions with pull-on-open/resync behavior.
- Store a bounded diagnostics snapshot per active project/editor file and render
  severity markers in the gutter plus a concise current-line diagnostic status.
- Apply syntax styling in the editor using `syntect`; overlay LSP semantic token
  styles only where token ranges map safely to the current saved document.
- Add read-only definition/reference navigation actions using bounded
  `LspReadGet` results and the existing project-scoped editor open path.
- Surface unsupported/not-ready/error states without treating them as clean.

Out of scope:

- New protocol DTOs, authorization capabilities, LSP operations, or event kinds.
- LSP writes, code actions, rename, preview apply, or applying edits to dirty
  buffers.
- Hover popups, completion, workspace symbol search UI, configurable keymaps,
  or general editor redesign.
- Adding a second diagnostics poller or unbounded event channel.

## 5. Expected changes

- `src/core/daemon_lsp.rs` and daemon runtime construction/lifecycle: activate the
  existing bounded diagnostics publisher.
- TUI project event/session handling: subscribe and reconcile diagnostics, pull
  on editor activation and after sequence gaps/reconnects.
- `src/tui/app/editor.rs`, `src/tui/components/editor.rs`, and editor commands:
  diagnostics state, gutter/status rendering, syntax/semantic styling, and
  definition/reference navigation.
- Focused unit/integration tests for reconciliation, line/range mapping,
  navigation, not-ready state, project isolation, and stale completions.
- Update `architecture/lsp.md`, `architecture/tui.md`, `.opencode/skills/lsp-ide/SKILL.md`,
  and `.opencode/skills/tui/SKILL.md` to match shipped behavior.

## 6. Ordered work packages

1. Trace the diagnostics publisher and projection stream from daemon startup to
   the socket TUI; activate existing lifecycle hooks without changing event
   schema or authorization.
2. Add a bounded per-project diagnostics view model. Reconcile stream sequences,
   pull authoritative state on editor open/reconnect/gap, and reject results for
   a different project or document snapshot.
3. Render severity markers and current-line details in the editor. Preserve
   current-line and selection colors where overlays intersect.
4. Apply syntax highlighting and map semantic-token ranges only when the result
   is ready and belongs to the displayed saved revision; otherwise retain syntax
   colors.
5. Add explicit read-only definition/reference actions. Bound target counts,
   keep navigation within the authorized workspace, and activate targets via
   the existing file-tree/editor path.
6. Add regression tests, update module guides/architecture, then run focused
   suites and `scripts/verify.sh quick`.

## 7. Verification

- Focused tests for changed daemon, protocol/client, editor component, editor
  commands, and project event reducers.
- `cargo fmt --check`.
- `scripts/check_tui_project_authority.py` and
  `scripts/check_tui_editor_text_authority.py`.
- `python3 scripts/check_execution_ownership.py` and
  `python3 scripts/check_scheduler_bypass.py` if daemon/task ownership changes.
- `scripts/verify.sh quick`.
- Do not run `--all-features`; use only the repository's documented features.

## 8. Acceptance and stop conditions

Accept when the editor can show current project diagnostics, syntax/semantic
styles, and bounded definition/reference navigation; cold or unavailable LSP is
visibly distinct from a clean file; stale/cross-project events cannot replace
active state; the protocol and authorization matrices are unchanged; and all
verification passes.

Stop and record a blocker if runtime publisher activation requires changing the
accepted ADR-0012 delivery or authorization contract, or if semantic token data
cannot be tied to a document revision without a new protocol field. Do not
weaken dirty-buffer or preview-apply invariants to make the UI appear current.

## 9. Required closure evidence

Record implementation commits, feature-to-test evidence, daemon/project isolation
and stale-event results, all focused/static/broad verification outcomes,
documentation updates, compatibility/security review, known limitations, and a
clear closure recommendation.
