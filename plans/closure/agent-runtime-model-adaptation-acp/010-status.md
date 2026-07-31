# Agent Runtime, Model Adaptation, and ACP Milestone 010 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-runtime-model-adaptation-acp/010-acp-v1-daemon-projection-adapter.md`
Source subsystem roadmap: `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-010--acp-v1-daemon-projection-adapter`
Repository baseline reviewed: `e1133610` (pre-implementation worktree)

Implementation commits: `1f553c1a` — feat(acp): add v1 daemon projection adapter; closure/status commit follows

## 1. Executive finding

M010 is strictly closed. CodeGG now exposes `codegg acp` as an ACP v1
newline-delimited JSON-RPC agent. It negotiates only the implemented v1
surface, attaches lazily to the singleton daemon, creates/loads native
sessions, submits native turns, consumes canonical projection events, maps
visible updates, and propagates cancellation to native turns. No ACP-owned
runtime, provider, scheduler, or durable session store was introduced.

The official ACP Rust crate was reverified during implementation. Its current
release requires Rust 1.88, above CodeGG's Rust 1.81 MSRV, so a bounded local
wire wrapper was used against the official v1 protocol contract.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| ACP v1 negotiation and truthful capabilities | `src/acp.rs`; `tests/acp_stdio.rs` | pass |
| Discoverable stdio entry point | `src/main.rs` `Acp` command and `codegg acp --help` build path | pass |
| Singleton daemon ownership | `ensure_client` uses `connect_or_start_daemon`; no in-process runtime construction | pass |
| Native session identity and cwd binding | `session/new` uses absolute canonical cwd and `CoreRequest::SessionCreate` | pass |
| Prompt and turn ownership | `session/prompt` uses native `TurnSubmit`; turn ID comes from native events | pass |
| Canonical projection mapping | `ProjectionStreamEvent` mapping in `handle_event`; raw core deltas are not emitted | pass |
| Load/resume/close | native `SessionLoad`, bounded message replay, projection subscribe/unsubscribe | pass |
| Cancellation | `session/cancel` and `$/cancel_request` map to `TurnCancel`, including pre-turn correlation | pass |
| Private reasoning disclosure | projection mapping accepts public message visibility only; reasoning variants are ignored | pass |
| Bounded protocol surface | 1 MiB frame/prompt limit and native projection bounds | pass |
| Stdout purity | isolated writer, stderr tracing, process fixture | pass |
| Unsupported methods/versions | explicit JSON-RPC errors and v1-only negotiation | pass |

## 3. Production implementation evidence

- `src/acp.rs` contains the adapter, transient session/subscription map,
  daemon attachment, ACP request dispatch, projection mapping, replay, and
  serialized writer.
- `src/main.rs` routes `Acp` through stderr logging initialization and
  exposes the command without changing native TUI or daemon behavior.
- `architecture/acp.md` records the ownership boundary, MSRV decision,
  supported capabilities, bounds, and example client configuration.
- `tests/acp_stdio.rs` spawns the real binary and validates initialize and
  shutdown frames as newline-delimited JSON with no stdout contamination.

## 4. Verification executed

Local commands:

    cargo fmt --all
    rtk cargo test -p codegg --lib acp
    rtk cargo check -p codegg --bin codegg
    rtk proxy cargo test --test acp_stdio -- --nocapture

Results: formatting, three ACP unit tests, binary compilation, and the
process-level stdio fixture passed. The process fixture completed in 3.31s.
No live editor installation or external provider call was required; native
turn/projection behavior is covered by the existing daemon and projection
test suites and the adapter's bounded mapping path is unit-tested through the
repository's typed contracts.

## 5. Invariant review

- ACP has no independent AgentLoop, scheduler, provider, session database, or
  projection reducer.
- ACP session IDs are native session IDs; transient mapping is discarded on
  close and durable sessions are preserved.
- The projection event stream is the sole ACP content-update path. Raw core
  events only establish the native turn correlation and cannot duplicate
  chunks.
- Capabilities advertise text prompts and session loading only; image/audio,
  elicitation, MCP-over-ACP, and draft v2 features are not claimed.
- Cwd is validated as absolute and canonicalized at the ACP boundary; no
  process-global cwd mutation is performed.

## 6. Failure and recovery review

Daemon attachment is lazy after initialization. If it cannot be established,
session operations return an explicit ACP error and do not fall back to an
independent runtime. Client disconnect/exit drops the in-memory adapter and
leaves native durable sessions intact. Active session close and cancellation
send native `TurnCancel`; descendant propagation remains owned by M003/M006.

## 7. Migration and compatibility review

The native protocol and TUI are unchanged. ACP is additive and uses existing
projection/session protocol versions. The official Rust SDK was not added
because of its Rust 1.88 requirement; the local wrapper is intentionally
limited to stable ACP v1 wire fields and is documented for future replacement
when the repository MSRV permits.

## 8. Security review

Permission authority remains native. ACP cannot grant a denied operation or
answer a request belonging to another native session. Private reasoning is
not serialized. Tool/output payloads pass through bounded projection fields;
unsupported/private projection variants are omitted. Oversized frames and
text prompts are rejected before native submission.

## 9. Documentation and operations

`architecture/acp.md` documents the command, ownership, supported methods,
limitations, bounds, diagnostics channel, and a reference editor launch
configuration. `codegg acp` keeps stdout reserved for protocol frames and
sends tracing to stderr.

## 10. Unresolved findings

None at critical, high, or medium severity. ACP v1 optional plan/usage/file
surfaces remain intentionally unadvertised until native projection contracts
provide a truthful bounded mapping.

## 11. Roadmap disposition

M010 is closed. M011 is ready: its remaining predecessor set M004 through
M010 is now strictly closed, and its integration-evidence work can proceed.

## 12. Registry updates

- M010 moved from dependency-ready/active tracking to recently closed with
  this closure record.
- The blocked M011 row was removed and M011 was registered as ready in the
  same status-change commit.
- No other registered plan listed M010 as a satisfied blocker; no additional
  downstream plan was unblocked.
