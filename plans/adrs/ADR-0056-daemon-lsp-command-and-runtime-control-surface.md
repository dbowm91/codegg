# ADR-0056: Daemon-Owned LSP Operational and Command Surface

Status: accepted — Option B accepted 2026-10-28 by the project maintainer

Proposed: 2026-10-27
Accepted: 2026-10-28

Decision owner: project maintainer

Affected: `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md` M003/M004; `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` M007-F; `architecture/lsp.md`, `architecture/command.md`, `architecture/authorization.md`.

Long-term: `plans/000-long-term-specification.md` §§4, 13, 24–25; `plans/001-terminology-and-domain-model.md` `Command`/`Run` terms.

Related **accepted** ADR-0008, ADR-0010, ADR-0011, ADR-0012, ADR-0018, ADR-0021 and ADR-0052. This does not rewrite or supersede those records.

## Context

The daemon already owns `egglsp::LspService`, language-server child processes, managed documents and the checked preview-apply path. `LspReadGet`, `LspDiagnosticsGet`, `LspDiagnosticsSubscribe` and `LspCompletionGet` serve frontends. However, the daemon's slash-command catalogue calls 26 `/lsp-*` forms daemon effects and currently serves only `/lsp-status` and `/lsp-errors`. The socket-mode TUI has no local `LspTool`; non-socket mode has a second frontend-owned one. M007-D deliberately deferred the missing routes to M007-F and required an ADR.

## Decision drivers

Preserve one authority, explicit per-operation authorization, safe server process lifecycle, bounded results, reproducible cross-client behavior, stable audit semantics and user-friendly unavailable/readiness distinctions. Do not turn a GUI into a process launcher or expose untrusted server-supplied commands.

## Options

A. **Forward arbitrary LSP JSON-RPC through the native protocol.** Flexible, but exposes raw URIs/commands, makes filesystem/permission checks per-method, and allows process control through an unbounded proxy. Rejected.
B. **Typed domain operations and daemon-owned low-frequency command dispatch (accepted).** Extend the existing bounded LSP read/control DTO families only where the current typed operations do not express a required effect. Project/session/workspace binding is resolved by daemon from authenticated scope. Read-only summaries and command catalogue entries reuse authorized `CommandExecute` for human command forms; interactive editor gestures remain direct typed reads, avoiding one durable `Run` per keystroke. Accepted by the project maintainer as recorded below.
C. **Let each frontend instantiate `LspTool` for its missing forms.** Easy short-term parity but duplicates servers, bypasses daemon execution profile and prevents remote parity. Rejected.
D. **Omit the unserved 24 forms indefinitely.** Correctly advertises unavailable but fails intended IDE command parity. Rejected as end state; explicit refusal is required as a safe interim state.

## Decision

**Accepted: Option B — typed domain operations and daemon-owned low-frequency command dispatch.** M003 owns readiness, lifecycle and inspection; M004 owns composite repair/review recipes and checked preview handoff. Extend bounded LSP DTO families only for required effects not already expressed by typed operations. Resolve project, workspace, session, server and target identity in the daemon from authenticated scope. Use the authorized command catalog and `CommandExecute` for low-frequency human commands. Keep ordinary editor gestures on `LspReadGet`, `LspCompletionGet`, and the existing diagnostics operations; never turn keystrokes into durable command runs.

The project maintainer selected these defaults on 2026-10-27 and formally
accepted them as part of this ADR on 2026-10-28:

- Add the narrow `lsp.control` capability for daemon-shared restart, stop and
  cache-clear effects. Grant it to Maintainer and Owner roles; do not infer it
  from `file.read`, `command.execute`, or `project.configure`.
- Human preview artifacts are session-bound and ephemeral. They are not
  visible across sessions, and are inaccessible after their owning session or
  turn registry is torn down. Do not add durable or daemon-global preview
  storage.
- Preserve the standalone TUI `LspTool` behavior during migration. Socket TUI
  and desktop use the daemon command path.
- Keep `/lsp-status` and `/lsp-errors` names, but give them the accurate
  bounded health/status and per-server error semantics. The server argument
  must be resolved and validated daemon-side.

The policy decision authorizes implementation, not blanket serveability. A
form remains unavailable until its typed route, authorization, bounded result,
failure behavior and frontend consumer are implemented and verified. Stop and
cache-clear controls must resolve to the selected session's daemon-owned LSP
scope; `--all` never means every daemon client. Controls cannot enumerate or
operate on Linux per-turn `M004LspServiceScope` processes. Preview artifacts
remain inert and ephemeral; application uses only the existing checked
`CoreRequest::LspPreviewApply` path.

For each of the 26 catalogued command *forms*, first produce a current-head disposition table: (1) already typed and reachable, (2) low-frequency daemon command needing wiring, (3) bounded new typed read, (4) restart/stop/cache-control effect needing explicit authorization, (5) unsupported for a recorded reason. Do not blindly replicate a legacy local `LspTool` API. Reuse `CoreRequest::LspPreviewApply` for any accepted mutation; do not create an alternate apply. Read-only preview creation is distinct from application.

Operational control must have an explicit capability/authorization descriptor and immutable daemon-selected server scope; `file.read` alone MUST NOT authorize process restart/stop/start, cache reset that affects other clients, or server argv/env. The implementer must map controls to existing capability taxonomy or propose a separate minimal policy decision before implementation; NEVER mint an arbitrary command-execute bypass. Distinguish FullHost daemon servers from Linux `M004LspServiceScope`, which is private to a constrained turn. A user command must not operate on or enumerate another principal's constrained process.

Results are sized and typed, include server/root identity where authorized, generation/readiness/health, truncated flags and actionable `NotReady`/`Ambiguous`/`Unsupported`/failure outcomes. Target identity is workspace-relative with server-side path validation; no frontend-provided root or process identifier is trusted. Diagnostics subscriptions remain gated by `file.read` and are not redirected into generic `project.observe`.

The daemon's command catalogue continues to publish `daemon_served` truthfully. When implementing a form, wire the native request, authorize it, test it end-to-end and only then advertise it as served. M003 migration must preserve standalone TUI compatibility until separate explicit deprecation; desktop and daemon-mode TUI use the same implementation.

## Consequences

Benefits: process sharing and security continuity, honest cross-client command parity, no second LSP frontend process, reviewed mutations in one write authority. Costs: new DTO/authorization/test coverage and an explicit compatibility mode for standalone TUI. Some commands remain intentionally unserved until M004; daemon command routing adds request/run overhead appropriate only for low-frequency human actions.

## Compatibility, security and migration

All native-protocol extensions additive and negotiated/typed; older daemon/client combinations explicitly refuse unsupported variants. Neither diagnostics nor model-context semantics may silently change. No storage migration is expected; if a proposed operation needs persistent state, stop for a separate plan/ADR. Use the server's trusted configured argv and existing managed-process admission, never untrusted args from renderer/model. Restart invalidates generations, managed-document replay and diagnostics sync witness must re-establish before completion acceptance. Failure preserves canonical editor text.

## Required verification

The accepted decision authorizes the bounded contract, not an unimplemented
route. Verify classification generation, authorization matrix,
two-client parity, socket vs standalone behavior, permission denial without
root enumeration, malicious paths/server responses, competing restart, server
crash, replay freshness and preview-write fencing. Record exact current-head
command count and routes rather than treating the 2026-10-25 26/2/24 inventory
as immutable. Require closure records for M003 and M004. No other accepted ADR
is amended.

Supersession: none.
