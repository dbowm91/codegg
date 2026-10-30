# LSP Follow-up Scope — 2026-10-30

Baseline: `52a6dabf365cebdf5e6e7b1a6c33ce1ff785bbc6`

The registered daemon LSP/frontend convergence milestones M001–M006 remain
closed. This note separates later product follow-ups from defects or closure
gates in those milestones.

## Follow-up inventory

| Area | Current evidence | Next bounded work | Existing constraint |
|---|---|---|---|
| Helix syntax-node text objects | M005 delivers async `SelectionRange`; `DocumentSymbols` is also a typed warm read. `SelectionRange` is a generic parent-range chain, `DocumentSymbol` gives symbol kinds/hierarchy, and semantic tokens label token spans rather than complete syntax nodes. The standard LSP feature set has no matching-delimiter request. See the Microsoft LSP 3.18 specifications for [selection ranges](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/language/selectionRange.md), [document symbols](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/language/documentSymbol.md), and [semantic tokens](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/language/semanticTokens.md). | M014 is ready under `plans/implementation/tui-editor-helix-parity/014-syntax-node-text-objects.md`. Start with provider/version evidence and map each gesture to a specific typed response field. | ADR-0034 Option B keeps parsers out of the frontend and forbids pretending a generic range is a typed syntax node. Full M014 may need another owner decision if any gesture requires a parser or server-specific operation. |
| Preview-registry command parity | M004 intentionally leaves `/lsp-previews`, `/lsp-preview`, `/lsp-preview-clear`, `/lsp-preview-refresh`, and `/lsp-preview-apply` unserved in socket mode. | Plan a session/turn-bound daemon surface using the already accepted ephemeral-preview policy; confirm every operation loses access at the documented teardown boundary and uses the existing checked apply path. | No durable preview registry or alternate mutation path. Apply remains `LspPreviewApply` under existing authorization and digest checks. |
| Other `/lsp-*` commands | `/lsp-doctor`, `/lsp-repair-hunk`, `/lsp-review-diff`, and `/lsp-security-review` remain unavailable in socket mode. | First classify each form separately. `/lsp-doctor` can be evaluated as a bounded composition of existing read routes; the repair/review forms need source, output, and authority bounds before implementation. | Do not reuse ambiguous hunk selectors or invent an unbounded diff/security workflow. Keep unsupported reasons visible until a safe typed mapping exists. |
| Selective desktop LSP completion | TUI and desktop share the daemon/client contracts, but their editor gesture sets are intentionally not identical. | After branch integration, audit the current visible desktop gesture matrix and select the highest-value missing reads as a separate capability plan. | Keep renderer authority-free; use existing typed daemon routes and discard stale results. Do not reopen the closed LSP convergence roadmap. |
| Interactive latency | Corrected visible key-to-paint samples are p50 30 ms / p95 36 ms unloaded and p50 27 ms / p95 34 ms with four CPU workers, on the no-warm-server local path. The warm LSP request sample included intentional debounce. | Keep the measurements as evidence. Before changing debounce or claiming an improvement, define a user-visible latency budget and capture a truly warm-server path with separate input, coordination, daemon, and paint timings. | M002 is closed and claims no numerical SLO. Do not manufacture a warm visible sample by letting a presentation gesture start a server. |
| Resource budget and platform support | M006 bounds live child count only when opted in. It does not impose RSS/CPU/disk limits. Windows constrained LSP remains unavailable because no filesystem-enforcement backend is registered. | Treat OS-enforced per-process resource controls and a Windows containment backend as separate feasibility/qualification work. Preserve fail-closed behavior while no backend exists. | Do not turn process count into a claim that memory or CPU is bounded. Do not enable constrained Windows LSP without a qualified containment backend. |
| Process-budget operations | M006's limit and typed exhaustion outcome are configured and enforced, but dedicated process-count visibility and exhaustion diagnostics are not an operator-facing capability. | After integration, add a bounded status projection for live/limit counts and structured refusal diagnostics, without exposing argv, environment, or raw protocol data. | Preserve the process-count-only limit and existing authorization boundary; do not add resource claims or automatic eviction. |

## Suggested sequence

1. Reconcile and qualify the current branch on exact-head CI, Desktop E2E, and
   Linux gates. Treat integration readiness as separate from M001–M006 closure.
2. After integration, begin M014's provider-feasibility audit. Decide whether
   its exact acceptance can fit ADR-0034 Option B before writing production code.
3. Plan selective desktop LSP completion and process-budget operations as
   separate capability slices, chosen from current visible user needs.
4. Plan preview-registry command parity as a separate capability, using the
   accepted ephemeral session/turn boundary and the existing apply route.
5. Evaluate `/lsp-doctor` independently; leave repair-hunk and review recipes
   unavailable until their input/output and authority contracts are bounded.
6. Keep latency-target selection and OS-level resource controls separate from
   command capability work. Neither is a remaining M001–M006 closure gate.

## Current disposition

This is a sequencing record, not a closure addendum. It does not reopen M001–M006,
change ADR-0034 or ADR-0056, or claim that the deferred commands are implemented.
