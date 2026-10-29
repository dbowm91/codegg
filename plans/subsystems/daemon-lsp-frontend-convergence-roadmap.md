# Daemon LSP and Frontend Editor Convergence Roadmap

Status: active — M001–M004 closed; M005/M006 are decision-ready for implementation

Planning baseline: `main` at `7de6f237c0c4ab2abff120f39b7ae98e7107bec3` (2026-10-28). Revalidate current source before implementing each milestone.

Primary class: infrastructure, with separate capability and latency-polish milestones.

Long-term authority: `plans/000-long-term-specification.md` §§4, 13, 24–25; `plans/001-terminology-and-domain-model.md`; `plans/002-long-term-roadmap.md`; `plans/003-planning-process.md`.

Controlling accepted ADRs: ADR-0008 (checked LSP preview application), ADR-0010 (shared client), ADR-0011 (canonical document and optimistic replicas), ADR-0012 (native LSP reads/diagnostics), ADR-0018 (authorized navigation identity), ADR-0021 (revision-bound completion and local application), ADR-0034 (daemon structural selection), ADR-0052 (command catalog/execute), and ADR-0056 (typed daemon-owned LSP operations and low-frequency command parity). ADR-0034 Option B is accepted; M005 is ready for bounded daemon structural reads, with implementation evidence gates remaining. Related existing roadmap: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` M007-F and `plans/subsystems/tui-editor-helix-parity-roadmap.md` M014. This roadmap does NOT reopen already closed milestones.

## 1. Purpose and ownership

Finish the daemon-centric LSP architecture while retaining instant local editor interactions and independently functional TUI/desktop clients. CodeGG daemon owns the authoritative `DocumentService`, `egglsp::LspService` process lifetime, LSP document sync, readiness, diagnostics snapshots, authorization, checked mutations and run/session/workspace binding. `codegg-client` owns transport-facing, frontend-neutral lifecycle/request coordination. TUI and desktop own focus, cursor, viewport, decoration layout, popup state, editor gestures, local optimistic document replicas and optional *pure* syntax-derived presentation structures.

The daemon is the sole owner of executable LSP processes **for daemon-connected clients**. Standalone/non-socket legacy mode exists today and MUST remain functional until an intentional compatibility decision; do not silently delete that path or claim it is daemon-backed.

No renderer/host/CLI may create a competing daemon-session LSP process, bypass `DocumentController`, treat unsaved frontend text as durable disk truth, or elevate an LSP result into an unchecked mutation.

## 2. Verified starting point and known gaps

- `crates/egglsp/src/service.rs`: client-per-root/server-id map, restart generations, managed document replay; `src/lsp/m004.rs`: separate Linux constrained-turn scope, not shareable with FullHost LSP.
- `src/core/daemon_documents.rs` + `src/document_service.rs`: canonical managed editor state; `crates/codegg-client/src/document.rs`: optimistic replica, writer lease and revision handling.
- `src/core/daemon_lsp.rs` and `src/core/daemon_lsp_completion.rs`: warm-only native reads, diagnostics pull/subscribe and revision-witnessed completion. `src/main.rs` starts the daemon-owned 750-ms diagnostics driver in both serving entry points.
- `src/tui/commands/editor.rs` and `src/tui/editor_lsp.rs`: native editor diagnostics/semantic/navigation; `src/tui/editor_completion.rs`: its own epoch/invalidation. `apps/desktop/src-tauri/src/document.rs`: native diagnostics/completion/navigation, no independent language-server owner.
- `crates/codegg-client/src/lsp.rs` owns the consumer-tested cross-frontend request lifecycle and diagnostics reconciliation contract. M002 owns migration of remaining gestures, parity coverage and renewed latency measurements against that contract.
- `src/main.rs` still attaches `app.lsp_tool` in **non-socket** mode. M003 adds session-rooted inspection plus `lsp.control` lifecycle/cache routes; M004 adds eight bounded daemon semantic workflows and desktop format/rename preview with explicit checked apply. M007-F remains split across M003/M004 under accepted ADR-0056. Keep the generated form inventory current and advertise a form as served only after its typed path, authorization and failure behavior exist. Preview-registry commands, doctor, hunk repair, diff review and security review remain explicitly unavailable.
- ADR-0034 Option B was accepted on 2026-10-28. M005 is ready to implement bounded daemon structural reads and the TUI gesture; no frontend parser is approved. Distinguish syntax-aware *local text interaction* from cross-file semantic LSP reads.

## 3. Non-negotiable invariants

1. One daemon process authority; sharing is per compatible canonical workspace/root, LSP server config and execution profile. Separate Git worktrees are different roots; restricted and FullHost profiles NEVER share a process. Do not assume shared toolchain caches make language-server contents interchangeable.
2. Daemon document revision, frontend optimistic revision, LSP `didChange` version and last-confirmed delivered revision are distinct. Completion acceptance is revision-checked locally and never applies snippet/command/additional edits by guessing.
3. `NotReady`, `Unsynced`, `Stale`, `Ambiguous`, `Unsupported`, `Unavailable` and `Ready(empty)` must stay meaningfully distinct. A lost diagnostic stream must reconcile by authoritative pull, never display `clean` from missing data.
4. `file.read` covers semantic reads/diagnostic disclosure; `file.modify` plus canonical checked preview apply covers writes. Restart/stop/launch need explicit operation authorization, not implicit read grants. No raw LSP JSON-RPC, server-provided commands, unvalidated server argv or arbitrary path in a frontend command.
5. High-frequency keypress/render paths do not block on process creation, LSP IPC or network RTT. Keep LSP requests asynchronous and deduplicated; no speculative background server spawns from warm-only reads.
6. Daemon/protocol changes must degrade safely with older clients/servers, deny on unclear scope, and preserve isolated constrained scopes. No new whole-document frontend buffers; no CRDT, silent local draft replay, or agent reads against unsaved text without explicit semantics.

## 4. Milestones and dependency graph

| ID | Primary class | Deliverable / exit | Dependencies | Status |
|---|---|---|---|---|
| M001 | infrastructure | Shared `codegg-client` LSP request lifecycle + diagnostics reconciliation, consumed by one real editor path in each frontend | Existing ADR-0011/0012/0021 interfaces (closed) | **closed — 2026-10-28** |
| M002 | capability/polish | Both editors use shared coordination for remaining LSP gestures, bounded debounce/coalescing and stale drop; measurable local vs IPC latency | M001 **hard — closed** | **closed — 2026-10-28** |
| M003 | capability | Typed daemon LSP server inspection, status, restart/stop/cache controls and low-frequency `/lsp-*` command parity (M007-F part 1) | ADR-0056 **accepted** | **closed — 2026-10-28** |
| M004 | capability | Daemon LSP semantic repair/review recipe parity and checked human preview flow (M007-F part 2) | M003 **closed**; ADR-0056 **accepted** | **closed — 2026-10-28** |
| M005 | capability | Bounded daemon LSP structural selection for the TUI/Helix M014, with stale discard and explicit unsupported state; no frontend parser | ADR-0034 **accepted Option B**; M001 interface **soft** | **ready** |
| M006 | infrastructure/polish | Opt-in bounded LSP process admission, canonical worktree/config/profile identity and qualified retirement/resource evidence | Maintainer-approved admission defaults recorded in M006 plan | **ready** |

Milestone plans: `plans/implementation/daemon-lsp-frontend-convergence/001-shared-lsp-client-coordination.md` through `006-worktree-aware-lsp-process-admission.md`. M003 and M004 are an implementation continuation of desktop M007-F, NOT a second command-catalog workstream. M005 coordinates with Helix M014 rather than duplicating its bindings.

Execution order: M001–M004 are closed; M005 and M006 are ready under their recorded decisions. They remain separate implementation milestones with their own qualification gates.

## 5. User-visible completion

A daemon-connected TUI and desktop attached to the same workspace reuse daemon LSP generations, show consistent diagnostics and independent responsive UI state, and never create another LSP child. Cross-frontend LSP command forms actually execute or advertise a precise unsupported status. Completion never inserts against stale text; diagnostics never appear falsely clean during warmup/disconnect. Structural selection stays asynchronous and explicitly unavailable when a warm daemon LSP cannot answer; no frontend parser fallback is introduced. Separate worktrees and sandboxed turns obey independent resource/authority budgets. Standalone TUI behavior remains explicitly documented.

## 6. Cross-cutting qualification and risk

Exercise daemon IPC and real/fake LSP stdio, two concurrent TUI/GUI clients, two projects with same leaf names, two worktrees of one Rust repository, dirty/saved file changes, stale completion, delayed responses, restart replay, server failure, slow editor input, disconnect/reconnect, viewer vs writer, restricted Linux LSP scope and platform-gated Windows/macOS behavior. Exercise typed authorization denials without leaking root existence. Measure 50th/95th percentile key-to-paint and request-to-result latency under idle and busy LSP (environment, samples and non-comparable baselines recorded). No strict numerical SLO is invented until measured; performance regressions must be disclosed. For GUI use the separate desktop gate; for daemon/TUI use root CI and feature-gated builds. Archive superseded plans only after truthful closure records are accepted.

## 7. Deferred and excluded

No general-purpose remote LSP proxy or Monaco WebSocket bridge; no arbitrary server-side LSP executeCommand; no LSP feature implementations in TypeScript; no multi-writer/CRDT; no shared LSP process across distinct worktrees or incompatible sandbox profiles; no frontend parser dependency under selected ADR-0034 Option B. Remote network path optimization and durable unsaved-document recovery are separate workstreams.
