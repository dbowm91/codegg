# LSP Phase 17 Decision Note

Date: 2026-06-27
Status: **Deferred** — no implementation required

## Decision

Phase 17 (Manual Lifecycle Controls for Start and Replay) is closed as a no-op. No evidence exists of real-world lifecycle control failures that justify adding `/lsp-start` or `/lsp-replay-docs` commands.

## Evidence Assessment

The plan's evidence gate (Workstream 1) requires one or more of the following triggers before implementation:

| Trigger | Evidence Found |
|---------|----------------|
| Auto-start fails silently in common cases | No — `get_or_create_client()` handles on-demand start reliably |
| Users need to pre-warm LSP before long agent turns | No — auto-start latency is not reported as a pain point |
| Servers fail to replay open docs after restart | No — `replay_documents()` in the restart coordinator works correctly |
| Per-key stop-all fallback is too coarse | Partial — per-key stop still uses `shutdown_all()`, but no user complaints exist |
| Root/profile selection needs explicit user override | No — root detection via `/lsp-root` and `/lsp-doctor` provides diagnosis |
| Real server smoke tests show lifecycle race conditions | No — smoke tests pass without lifecycle-related failures |

## Rationale

1. **Service auto-starts on demand.** `LspService::get_or_create_client(file_path)` initiates a server when first needed. Adding `/lsp-start` would duplicate this behavior with no functional gain.

2. **Document replay is handled internally.** The restart coordinator calls `replay_documents()` after reinitializing a client. This is scoped, tested, and correct. Adding `/lsp-replay-docs` would expose internal implementation details without user-facing benefit.

3. **Per-key stop lacks a clean scoped API.** The service has `terminate_runtime()` as an internal helper, but it requires generation tracking and is tightly coupled to the restart coordinator. Exposing a per-key stop would require either:
   - A new public `stop_client(key)` method with proper generation validation, or
   - Refactoring `shutdown_all()` to support selective termination.
   Neither is justified without user demand.

4. **Adding manual controls risks a parallel lifecycle model.** The plan explicitly warns: "Manual start/replay can easily become a parallel lifecycle model if service APIs are not clean." Without evidence of need, this risk is not worth taking.

## What Remains

- `/lsp-restart <key>` and `/lsp-stop [key|--all]` continue to work as before
- Auto-start on first file access continues to work
- Document replay after restart continues to work internally
- Per-key stop uses `shutdown_all()` fallback (acceptable for current usage)

## Reconsideration Criteria

Phase 17 may be reopened if:
- User reports show auto-start fails in common scenarios
- Document replay after restart is observed to be unreliable
- Multi-root sessions require finer-grained stop control
- Pre-warming LSP before agent turns becomes a performance requirement

## References

- Plan: `plans/lsp_phase_17_manual_lifecycle_controls_plan.md`
- Roadmap: `plans/lsp_phase_13_17_roadmap.md`
- Architecture: `architecture/lsp.md` (Phase 9 lifecycle section)
