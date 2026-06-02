# Development Status Tracker

**Status**: UPDATED 2026-06-02
**Purpose**: Track truly completed items vs. active development items vs. future work.

Items marked **DONE** were implemented and verified.
Items marked **ACTIVE** are real development tasks for the current sprint/roadmap.
Items marked **FUTURE** are explicitly deferred to later phases.

---

## DONE - Verified Completed Items

All items from `prompting.md` (9/10 phases), `security.md` (all phases), `deepresearch.md` (all MVP), `improvements.md` (all MVP), and `tooluse.md` (Part 1) are verified complete. See individual plan files for details.

---

## ACTIVE - Items Requiring Development

All previously active items have been completed (verified 2026-06-02). See individual plan files for details.

---

## FUTURE - Explicitly Deferred to Later Phases

| Item | Plan | Notes |
|------|------|-------|
| eggsact crate integration | tooluse.md | External project, separate crate (being completed now) |
| MathEvalTool, TextInspectTool, ValidateJsonTool | tooluse.md | Depends on eggsact crate |
| Semantic embeddings search | tooluse.md | v3 upgrade path, requires model |
| Optional server API endpoints for research runs | deepresearch.md | Future follow-up |
| Hunk-level accept/reject | tui.md | Optional, low priority |
| CLI goal support | improvements.md | After TUI path |
| Subagent goal propagation | improvements.md | Phase 2 |
| Autonomous `/goal run` | improvements.md | Out of scope first pass |
| Ambient prompt hints | security.md | Only after Phases 1-2 stable |
| Security docs | security.md | `docs/security.md` not created |

---

## Summary

| Category | Count | Status |
|----------|-------|--------|
| Truly completed (verified in code) | ~105 | DONE |
| Remaining development tasks | 0 | COMPLETE |
| Explicitly future/deferred | 10 | FUTURE |

**Total**: ~115 discrete items across 10 plan files

All implementation items that could be completed without the eggsact crate dependency have been implemented. The only remaining work is explicitly deferred (eggsact integration, embeddings search, server API endpoints, etc.).
