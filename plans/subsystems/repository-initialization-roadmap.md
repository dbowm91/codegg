# Repository Initialization (/init) Roadmap

Status: active; M001 ready, M002 blocked until draft-contract closure

Research baseline: `578b62bd9580382e00fb19e096fc1794e1f5618b` (2026-10-09)

Long-term references:
- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md` (project/workspace identity)
- `plans/003-planning-process.md`

Related closed lines: Runtime Assets M002–M004 (explicit project instructions/snapshot/refresh); TUI Command Registry M010 (`plans/closure/tui-project-sessions/010-status.md`); existing typed CoreClient, authorization, file mutation, and scheduler domains. No second instruction loader.

External reference behaviors:
- OpenCode `/init` analyzes the repository and generates/updates `AGENTS.md`: https://docs.opencode.ai/docs/rules/
- Codex `/init` creates `AGENTS.md`; do not assume it always edits existing files: https://github.com/openai/openai-cookbook/blob/main/examples/codex/iterating-development-workflows-with-codex.md
- Claude Code `/init` generates `CLAUDE.md`: https://claude.com/resources/articles/using-claude-md-files
- Codex disclosure hazard for personal rules leaking into repository files: https://github.com/openai/codex/issues/2529

## 1. Purpose and ownership boundary

Provide a truthful built-in **TUI `/init`** that helps initialize or improve repository-specific coding instructions based on verified local evidence, producing a **reviewable proposed change to the selected project's root `AGENTS.md`** and applying it only after explicit authorization. The core/domain service owns bounded collection, factual evidence and draft provenance; the daemon/core owns authoritative project scope, mutations and refresh; TUI owns presentation and human accept/reject. It is not `git init`, a project scaffolding generator, or a blanket rewrite of every vendor rules file.

## 2. Work classification

**Invariants:** never derive active project from ambient CWD/hidden prior session; do not import global/private instructions into shared `AGENTS.md`; no writes before review and explicit approval; user-owned existing content is preserved unless accepted in diff; mutation flows through daemon authorization and canonical file transaction policy; in-flight asset pinning remains unchanged.

**Capabilities:** `/init` works in both empty and mature repositories, shows detected commands/conventions with evidence or uncertainty, previews a proposed creation/update, and refreshes future turns on success. **Infrastructure:** bounded repo evidence scan, proposed markdown, provenance with target file revision/CAS, typed request/reply and stale-route tokens. **Polish:** onboarding help, straightforward no-change response, short and useful AGENTS instructions.

## 3. Non-goals

No implicit `git init`, dependency installation, test execution, shell eval, code generation into non-instruction files, `CLAUDE.md`/Cursor rules overwrites, automatic skill/plugin installation, remote source fetching, or background monitoring. No direct TUI filesystem write or second project instructions authority. A future headless `codegg init` may reuse the same host service, but is not required in this line.

## 4. Baseline findings

`src/tui/command.rs` has a typed `BuiltinSlashAction` registry and command tests but no registered `/init`. `src/agent/instructions.rs` already reads root/ancestor `AGENTS.md` into an immutable `ProjectAssetSnapshot`; `src/agent/asset_refresh.rs` coordinates refresh. `src/tui/app/prompt_turn.rs` routes the selected tab/workspace explicitly and rejects stale context for human shell; `src/core` owns authenticated project operations. `codegg-document` and existing guarded edit/transaction machinery are preferable to ad hoc file writes. `docs/agents-skills.md` and `architecture/command.md` are the user and command surfaces to update.

## 5. Target architecture

1. A **read-only evidence pass** over bounded, explicitly selected repository files: README, language manifests (`Cargo.toml`, `pyproject.toml`, `package.json`, `go.mod`, etc.), known CI/test configuration, existing root `AGENTS.md`, project-local instructions and key workspace metadata. Honor ignores, reject unsafe symlinks, cap entries/bytes/time; never run discovered commands. Classify facts as observed, inferred, or unknown.
2. A **draft operation** producing concise `AGENTS.md` markdown plus evidence/provenance, no unconditional claims. Use an existing agent/model route only through owned turn/broker/scheduler and existing permissions if model-assisted drafting is selected; deterministic fallback for empty repos or unavailable providers. No global CLAUDE/Codex personal content.
3. A **preview/approval/commit operation** on the exact intended path using observed file digest and project/root binding, atomic write or current canonical document transaction, explicit rejection on external edit, stale tab/session, permission refusal, symlink/path replacement, or partial failure. For existing AGENTS prefer surgical diff/merge, no silent clobber. After success call the existing project asset refresh, tell user that in-flight turns remain pinned.

## 6. Dependency graph

~~~text
closed Runtime Assets / typed project context / typed TUI command registry
            |
            v
M001 bounded evidence and inert candidate draft (ready)
            |
            v
M002 /init UI + guarded publish + refresh (hard M001)
   interface with cross-agent skills M002 for latest source inventory;
   publication correctness does not require another registry.
~~~

## 7. Milestones

**M001 — Evidence-based draft (infrastructure):** implement bounded, passive repository discovery and compact, evidence-attributed `AGENTS.md` creation/update candidate API without writing any files. Exit: deterministic fixtures prove accuracy, absence of secrets/private globals, and explicit unknowns, including empty/monorepo repos.

**M002 — /init end-to-end command (capability):** add canonical slash command and help, typed daemon request lifecycle or existing project-scoped host facade, preview dialog and guarded publish, stale-preview and conflict handling, refresh, user-visible status, restart/cancellation/authorization tests. Exit: create and update trajectories work with approved writes and no overwrite on rejection; subsequent turn sees new instructions without altering active turns.

## 8. Cross-cutting requirements

Storage: prefer ephemeral draft, no schema migration; if persisted for restart, store bounded metadata in existing run/asset storage, never duplicate secrets. Protocol: version any new core request/response types, keep older clients gracefully unsupported. Auth: `AGENTS.md` write is a mutation requiring project capability and approval; user consent is not an agent-issued tool permission. Cancellation: discard unused draft, no partial target write; retry must detect CAS mismatch. UX: disabled/absent provider still shows deterministic supported evidence or a clear refusal without guessing. Performance: bounded read budget and no whole-repo expansion.

## 9. Verification strategy

Empty repository, Rust/Python/JS/Go repos, monorepo with child AGENTS, existing hand-edited AGENTS, secrets/global memory decoys, hostile README prompt injection, symlink escape, binary/oversized files, concurrent file edit during preview, missing provider, denied write, stale tab, daemon restart and asset refresh. Use focused test binaries and `scripts/verify.sh quick`. Document optional hosted qualification honestly.

## 10. Risks and decision points

The generator may hallucinate valid commands from filenames: every claim must be tied to observed evidence or marked unverified. Imported foreign guidance may be private or adversarial: use it as *untrusted local evidence* at most and never copy global files. If safe write cannot be achieved through existing daemon/document seams without adding a new operation contract, stop and make an ADR rather than mutate from TUI.

## 11. Completion definition

An actual user can invoke `/init` on the selected project, inspect accurate bounded guidance, explicitly approve creation or merge into `AGENTS.md`, recover safely from conflicts/failures, and see the new instructions on the next turn. Both milestones have closure evidence.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | ready | `plans/implementation/repository-initialization/001-bounded-evidence-and-draft.md` | future `plans/closure/repository-initialization/001-status.md` | closed runtime asset and scoped project services |
| M002 | blocked | `plans/implementation/repository-initialization/002-init-command-preview-publish-refresh.md` | future 002 | hard M001 |
