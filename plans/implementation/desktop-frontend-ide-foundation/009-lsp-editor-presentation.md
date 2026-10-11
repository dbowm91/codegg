# CodeGG Desktop Frontend and IDE Foundation M006-C — LSP Editor Presentation

Status: active

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
- M006-B provides bounded LSP read DTOs, `DiagnosticsReconciler`, a project-
  scoped diagnostics subscription, and an authoritative diagnostics pull.
- ADR-0012 makes the pull authoritative and diagnostics stream project-scoped.
- The M006-B publication function has no periodic daemon task. This plan drives
  one publisher pass from each authorized diagnostics pull, and pulls on editor
  open/save/reload; it does not add another poller or TUI stream consumer.
- `syntect` is already a dependency and code-block rendering demonstrates the
  existing syntax theme integration. The editor currently emits one plain span
  per line.
- M006-B point-location DTOs carried only file names in `path`, which is
  ambiguous in a workspace and insufficient for safe navigation. This plan
  normalizes in-root results to workspace-relative paths and omits external
  results; the DTO shape remains unchanged.
- M006-C is intentionally limited to the existing TUI and read protocol. There
  is no need for an LSP protocol or permission expansion.

## 3. Invariants that cannot regress

- LSP state is requested through the daemon-owned `CoreClient` transport and
  remains daemon-owned.
- Session/workspace identity comes from the active tab and daemon records;
  renderers never infer authority from process cwd or arbitrary file paths.
- Diagnostics from another project must never appear in the active editor;
  diagnostics are cleared when local edits make their document snapshot stale.
- `NotReady`, unsupported, and failed reads remain distinguishable from an
  empty diagnostic set.
- All event handling and requests remain bounded; rendering performs no I/O.
- Navigation is read-only and opens targets through the existing project-scoped
  file-tree/editor path. No preview apply or dirty-buffer behavior changes.
- Existing preview-apply authorization and `src/lsp/mutation.rs` stay untouched.

## 4. Scope and non-goals

In scope:

- Run the existing bounded publisher during an authorized diagnostics pull;
  pull on editor open/save/reload. The pull is the correctness source and the
  pass also updates project stream subscribers.
- Store a bounded diagnostics snapshot per active project/editor file and render
  severity markers in the gutter plus a concise current-line diagnostic status.
- Apply bounded visible-line syntax styling in the editor using `syntect`.
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

- `src/core/daemon_lsp.rs`: make pulls project/session coherent, reject cold
  clients, normalize in-root locations, and run the existing publisher pass.
- `src/tui/app/state/editor.rs`, `src/tui/components/editor.rs`, and editor
  commands: diagnostics state, gutter/status rendering, syntax styling, and
  definition/reference navigation.
- Focused unit/integration tests for project isolation, path mapping, navigation,
  not-ready state, and stale completions.
- Update `architecture/lsp.md`, `architecture/tui.md`, `.opencode/skills/lsp-ide/SKILL.md`,
  and `.opencode/skills/tui/SKILL.md` to match shipped behavior.

## 6. Ordered work packages

1. Make the authorized diagnostics pull verify session/project coherence and a
   warm daemon-owned client, invoke the bounded publisher, and return its
   complete tracked snapshot.
2. Add bounded editor diagnostics state. Pull on open/save/reload, and drop the
   snapshot after local edits or stale editor generations.
3. Render severity markers and current-line details in the editor. Preserve
   current-line and selection colors where overlays intersect.
4. Add explicit read-only definition/reference actions. Bound target counts,
   keep navigation within the authorized workspace, and activate targets via
   exact-path lookup in the active file tree and existing editor path.
5. Add regression tests, update module guides/architecture, then run focused
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

Accept when the editor can show current project diagnostics, syntax styles, and
bounded definition/reference navigation; cold, unavailable, or not-yet-published
diagnostics are visibly distinct from a clean file; local edits clear stale
diagnostics; stale/cross-project completions cannot replace active state; the
authorization matrix is unchanged; and all verification passes.

Semantic-token presentation remains out of scope because `LspReadResultDto`
does not carry the document revision needed to bind tokens to a changing buffer.
Do not infer revision identity from a path or weaken dirty-buffer or preview-
apply invariants to make semantic coloring appear current.

## 9. Required closure evidence

Record implementation commits, feature-to-test evidence, daemon/project isolation
and stale-event results, all focused/static/broad verification outcomes,
documentation updates, compatibility/security review, known limitations, and a
clear closure recommendation.
