# Repository Initialization Milestone 002 — /init Command, Preview, Guarded Publish and Refresh

Status: blocked (hard dependency: Repository Initialization M001 closure)
Repository baseline: `578b62bd9580382e00fb19e096fc1794e1f5618b` (2026-10-09 research; re-audit at execution)
Source roadmap: `plans/subsystems/repository-initialization-roadmap.md#7-milestones`
Long-term requirements: `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`; `#27-security-requirements`; `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`.
Applicable ADRs: existing typed command/project mutation/asset-refresh architecture. **Require an accepted new ADR first** if the existing daemon/client writer cannot safely publish AGENTS.md without a new write protocol or authority.
Primary class: capability

## 1. Objective

Ship a **real TUI /init** for the selected project that creates or improves its AGENTS.md like common coding-agent bootstrappers: gather bounded evidence using M001, show preview/diff, require explicit human confirmation, publish safely under daemon authority, and refresh runtime instructions for subsequent turns. No implicit Git repository initialization.

## 2. Why not yet ready

M001 must close with a tested inert proposal and exact original-target digest. Existing TUI typed command registry, project context, daemon authorization and refresh are stable; cross-agent Skills M002 is a soft interface dependency only for optional skill inventory prose, not a hard gate.

## 3. Current implementation evidence

`src/tui/command.rs` registers built-ins as typed `BuiltinSlashAction`, and `architecture/command.md` requires exhaustive actual dispatcher routing, not inert command registration. No /init built-in exists at baseline. `src/agent/instructions.rs` is the canonical AGENTS.md resolver, `src/agent/asset_refresh.rs` coordinates generations. `src/tui/commands/editor.rs`, `src/core/daemon_documents.rs`, `crates/codegg-client`, `crates/codegg-document`, `CoreRequest::DocumentSave` and `LspPreviewApply` are **candidate** existing file-writing surfaces, not proven authority for this target; inspect exact support before choosing an implementation.

## 4. Invariants

- Selected project/workspace identity and target path remain bound across async preview and approval; never use ambient cwd or a newly selected tab by accident.
- No write before **explicit positive human confirmation**, including under automatic/yolo agent approval modes.
- All mutations are daemon-owned and subject to existing workspace/path/sandbox authorization. No implicit new file-write power for models or skills.
- Existing instructions are preserved by diff/merge; stale digest, competing editor, target swap and symlink race must fail closed.
- Private global instructions, credentials, .env and secret files cannot leak into a commit-ready AGENTS.md.
- In-flight agent turns retain their pinned ProjectAssetSnapshot; refresh only affects future turns.

## 5. Scope

In scope: TUI command, preview/approve/reject UX, guarded single-file publish, conflict/staleness reporting, scoped refresh, tests/docs. Out of scope: `git init`, cloning, installing dependencies, running tests, executing hooks, modifying CLAUDE.md or vendor rules, global copying, headless CLI init, new plugin runtime, arbitrary file-editor or code-generation flows.

## 6. Required production changes

**Command/TUI.** Register canonical `/init` with `BuiltinSlashAction::Init` and appropriate Project domain, palette help and tests; add corresponding exhaustive dispatcher path in the actual TUI app command handler. Initiating must resolve authoritative selected workspace root and show relative target path plus mode create/update/no-op. Render proposed AGENTS.md and a readable diff against current contents, with provenance of observed/inferred facts. Offer approve, cancel and regenerate; no auto-write. If an existing AGENTS file contains unknown/custom content, preserve it conservatively and flag ambiguous merges.

**Draft source.** Call M001 read-only evidence/draft, not a separate scanner. Provider connection must **not** be required. Optional model-assisted enrichment may be implemented only if the input is an isolated allowlisted evidence envelope, response is inert for preview, and private global session/model context cannot bleed in. If the isolation is uncertain, ship deterministic-only functionality and defer enrichment. Hostile project instructions are untrusted data.

**Publish authority.** Determine the actual existing project-scoped daemon document/transaction writer, checking whether it authorizes AGENTS.md at selected root and supports atomic/CAS semantics. Route through `codegg-client` / daemon; do **not** write directly in TUI code. Before commit, verify project identity, root, target canonical containment, file identity/type, original digest or absent marker, proposal nonce and approval status. Apply the same permissions at commit time and report denied writes without side effects. Prefer the existing controlled document transaction or safe atomic rename. If the available API lacks these properties and a new CoreRequest or privilege is needed, **stop, create/accept a scoped ADR and revise this plan** before coding a new write protocol.

**Refresh.** After confirmed successful publication, call existing scoped asset refresh, report the new generation and tell user active turns remain pinned. On no-op, cancel or conflict do not refresh as though a write happened.

**Protocol/storage.** Keep draft ephemeral by default; any new DTO must be bounded, version-tolerant, and approved under ADR if introducing a frontend↔daemon write operation. No schema migration as baseline.

**Security/observability.** No scripts/manifest commands executed; no overbroad filesystem reads, symlink escape, forced overwrite, secret logs or absolute global home details. File writes must stay within selected workspace and target must be AGENTS.md, not an attacker-supplied relative traversal.

## 7. Ordered work packages

A. **Authority audit and contract choice:** verify existing document write and authorization path covers creation/update of root AGENTS.md with CAS and atomic failure behavior; document findings. If not, stop for ADR rather than self-authorized frontend write.
B. **Command registration and preview:** typed builtin registry, dispatcher and palette/help, stable selected-project context, inert draft rendering, cancellation and no-op.
C. **Authorized commit:** explicit approval, target digest/absent CAS, root/workspace binding, symlink and file-type revalidation, conflict recovery without blind overwrite or duplicate append.
D. **Refresh:** successful publication triggers project-scoped asset rebuild, with future-turn selection and in-flight pin tests; UI reports actual success/failure.
E. **Qualification:** realistic TUI→client→daemon trajectories, read-only and permission-denied failures, stale project switch, restart and concurrent publication tests; docs and closure.

## 8. Failure, cancellation, restart and contention

- Dialog cancel, app close or lost connection before approval yields zero writes.
- External modification, creation or deletion of AGENTS between preview and apply causes a clear stale/conflict result; regenerate/repreview, never force silently.
- Two simultaneous approved drafts from same original file cannot both write: exactly one commit on original digest, second conflict/no-op after re-check.
- Switch tabs while drafting: response remains bound to original project or is rejected as stale; cannot land in another tab's checkout.
- Read-only sandbox, permission denial, broken symlink, suspicious file type or oversize existing content fails visibly with original file intact.
- Crash after successful commit but before UI acknowledgment: reconstructed asset refresh on reopen sees the on-disk file, and idempotent retry does not append duplicate instructions.
- Missing/unusable model does not block deterministic M001 draft.

## 9. Compatibility and migration

Existing AGENTS and ancestor/project instruction conventions remain intact; never rewrite CLAUDE.md or foreign skill roots. No mandatory config or storage migrations. If new protocol is approved, advertise versioned capability and fail clearly for older clients, not with false success.

## 10. Required tests

**Focused unit:** /init registry and exhaustive dispatcher; selected root/target; candidate create/update/no-op and diff; idempotent re-run; approval never implied by agent mode.
**Integration:** initiate→preview→cancel (no write); initiate→preview→approve creates AGENTS; update hand-edited AGENTS without destroying sections; new turn reads refreshed instructions while active turn keeps pinned generation.
**Contention/recovery:** concurrent same-digest writes, external edit/delete/recreate before approval, symlink swap, two projects/tabs, stale client/draft, daemon restart/connection loss and duplicate callback.
**Security/negative:** denied authorization, malicious README, .env/global-decoy secret contents, binary/overlarge AGENTS, attempt to redirect target outside root, read-only sandbox; no command execution.
**Compatibility:** no provider connected, Git and non-Git workspaces, existing /reload, TUI command alias collisions, protocol client compatibility if changed.

## 11. Required verification commands

~~~bash
cargo test --lib tui::command
cargo test --lib tui::commands
cargo test --lib agent::instructions
cargo fmt --all -- --check
scripts/verify.sh quick
cargo clippy --workspace --all-targets --all-features -- -D warnings
~~~

Add actual /init TUI/core integration targets at implementation; if core DTO/dispatched matches are modified compile relevant server feature (`cargo check --features server`) and test transport compatibility. Record executed commands/results in closure, not assumed green.

## 12. Documentation updates

`architecture/command.md`, existing project instructions architecture document, `docs/tui.md`, `docs/agents-skills.md` and `docs/repository-init.md`; update README command list where appropriate. Do not claim an implemented command until positive TUI→daemon write trajectory exists.

## 13. Acceptance criteria

- A user can run /init, see factual instructions and diff, cancel safely, or approve create/update of the selected root AGENTS.md.
- Existing custom guidance is preserved; stale/conflicting/symlinked targets never receive blind overwrite; no writes without explicit approval.
- Actual core/daemon authority is used; tab switching cannot redirect writes.
- New instructions are loaded for the next turn after successful refresh, while active turns do not change.
- No provider, hidden global content, Git init or execution side effects required.

## 14. Stop conditions

Stop and propose/accept an ADR if no existing authorized daemon write can satisfy CAS, atomic publication, path containment and explicit approval, or a new protocol/privilege is necessary. Stop rather than fabricate technical commands or read private instructions.

## 15. Closure evidence required

Write `plans/closure/repository-initialization/002-status.md` documenting exact commits, real command dispatch, positive and negative end-to-end trajectories, authorization, conflict/symlink/privilege/restart and pinned-generation checks, test and CI exit codes, changed docs and unresolved risks. Update roadmap/registry; compiling the command registration alone is not closure.

## 16. Handoff notes

Prioritize a safe useful deterministic /init over prematurely adding a model dependency. M002 is the actual user-visible feature; M001 alone should never be advertised as a working /init.
