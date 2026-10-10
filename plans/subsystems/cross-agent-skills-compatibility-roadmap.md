# Cross-Agent Skill Compatibility Roadmap

Status: closed; M001–M005 closed

Repository research baseline: `578b62bd9580382e00fb19e096fc1794e1f5618b` (2026-10-09)

Long-term references:
- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/001-terminology-and-domain-model.md` — project/workspace identity, provenance, immutable assets
- `plans/002-long-term-roadmap.md#phase-1--runtime-asset-registry-interoperability-and-refresh-correctness`
- `plans/003-planning-process.md`

Predecessor (closed): `plans/subsystems/runtime-assets-roadmap.md`, M001–M004; `plans/closure/runtime-assets/004-status.md`. Plugin contributions and portable packaging remain owned by `plans/subsystems/plugin-ecosystem-interoperability-roadmap.md` and its closures. This is new compatibility work, **not** a retroactive reopening of those closed milestones.

Research references (verify current conventions at implementation time):
- https://docs.opencode.ai/docs/skills/
- https://geminicli.com/docs/cli/skills/
- https://agentskills.io/specification
- https://developers.openai.com/codex/skills
- https://code.claude.com/docs/en/skills
- `architecture/skills.md`, `docs/agents-skills.md`

## 1. Purpose and ownership boundary

Extend the existing `AssetRegistry` / `ProjectAssetSnapshotBuilder` / `AssetRefreshCoordinator` so Codegg recognizes useful **existing on-disk skill packages** written by other coding agents, without importing an executable agent framework or maintaining copies. Source discovery, normalization, diagnostics, precedence and lazy bounded resource access are owned by `src/skills/`; project/workspace identity and generation pinning remain in `src/agent/`. Plugin-contributed skills continue to use the existing package bridge.

## 2. Work classification

**Invariants:** never infer daemon workspace from process CWD; foreign trees are read-only; loading instructions cannot grant permissions or execute scripts; project/user scopes and source provenance are explicit; duplicates resolve deterministically; package reads stay within approved trust boundaries; failed refresh retains the previous published generation; running turns keep pinned skill digests.

**Capabilities:** discover Codex/Claude/OpenCode skills in their actual project/global locations and additional common agent directories; expose compatible resource references and transparent reasons when a skill was skipped; inspect effective and shadowed skills.

**Infrastructure:** bounded discovery-root descriptors, canonical deduplication, optional vendor metadata compatibility, scoped ancestor traversal, nested resource inventory with safe reads.

**Polish:** precise activation prompt, documentation drift corrections, repo-maintenance skill coverage, focused validation rather than a broad new CI framework.

## 3. Non-goals

- Installing, downloading, executing, or modifying third-party skill packages during discovery.
- Emulating Claude command substitution, permission bypass, agent forks, Gemini user-consent semantics, or vendor-specific hooks; unknown features are inert metadata with diagnostics.
- Recursively indexing every descendant repo, walking past trusted git/worktree boundaries, or following untrusted symlinks outside the authorized roots.
- Reading every home directory or embedding global/private instructions in project-generated documents.
- A second registry, parser, asset-refresh path, scheduler, tool authority, or plugin runtime.
- A mandatory bundled general-purpose skills distribution.

## 4. Baseline findings

`src/skills/source.rs` defines four project roots (`.codegg`, `.agents`, `.opencode`, `.claude`) and global roots as children of `dirs::config_dir()`, leaving home-relative `~/.agents/skills` and `~/.claude/skills` unaccounted for; macOS OpenCode's `~/.config/opencode/skills` also differs from `dirs::config_dir()`. `src/skills/registry.rs` independently scans symlink aliases, lacks project-ancestor discovery, and injects the obsolete “Use /skill:<name>” string. `src/tool/skill.rs` already provides the actual model-facing `skill` tool. `src/skills/parser.rs` enforces portable name/description, does not retain all foreign extension fields, and enumerates only top-level resource files. `ResourceHandle` supports relative paths with containment; its safety must not be weakened for recursive resources. Source-path changes must also update `src/agent/asset_context.rs`, skill-proposal collision checks, compat facades, TUI consumers, and daemon refresh.

Other useful conventions to qualify: Cursor (`.cursor/skills`), Gemini CLI (`.gemini/skills`), Copilot (`.github/skills`), Cline (`.cline/skills` / `.clinerules/skills`), Roo (`.roo/skills` with mode-specific exclusions), Pi (`.pi/skills`), and optional Factory/Windsurf/Devin. Paths are inputs to a maintained fixture table, **not** promises until supported by source evidence and tests.

## 5. Target architecture

One `AssetRegistry`, with ordered `SkillSourceDescriptor`-style inputs (source kind, physical root, logical scope, precedence, vendor, trust policy). Detect supported source roots without changing the established effective-asset API gratuitously. Canonicalize and deduplicate physical packages; keep observable aliases/provenance without inflating effective candidates. Enforce project/workspace traversal boundaries with a bounded ancestor chain and zero recursive descendant scan. Keep the existing priority for Codegg-owned project skills, preserve existing native .md support, and document any new tier ordering.

A portable core parses standard frontmatter; vendor adapters may provide narrowly scoped fallbacks (e.g. a missing Claude name inferred from its containing directory) and **never** turn foreign metadata into authority. Nested `references/`, `assets/`, and `scripts/` are inventory entries only. Runtime activation remains the `skill` model tool via the pinned snapshot, with no implicit script execution.

## 6. Dependency graph

~~~text
closed Runtime Assets M001–M004 + Plugin Ecosystem passive contributions
        |
        v
M001 truthful activation + parser/contract baseline (closed)
        |
        v
M002 cross-agent paths + canonical deduplication (closed after M001)
        |
        v
M003 metadata, missing-name and nested resource compatibility (closed after M002)
        |
        v
M004 scoped discovery + /skills diagnostics (closed after M003)
        |
        v
M005 maintenance skills + lightweight conformance guard (closed after M004)
~~~

Dependencies are **hard** within this sequence for handoff/closure discipline; repository initialization M001 can research/draft independently, while its M002 publication qualification consumes the stabilized effective-asset refresh interface.

## 7. Milestones

**M001 — Accurate activation and portable validation (invariant):** correct the system prompt, compatibility facades, misleading skill docs; establish parser fixtures separating strict portable conformance from backward-compatible native behavior. Exit: prompting names the real model tool and tests prove tool invocation and no permission increase.

**M002 — Cross-agent source discovery (capability):** add concrete, tested project/global locations with home/XDG/macOS/Windows handling, source deduplication, bounded trust roots, and explicit conflicts. Exit: Codex, Claude, OpenCode, Cursor, Gemini, Copilot fixtures show consistent effective skills; symlink aliases are not parsed twice; no unrelated project is scanned.

**M003 — Portable package and resource compatibility (infrastructure):** vendor-specific safe name fallback, preserved unrecognized metadata, bounded recursive reference inventory, strict containment and no implicit scripts. Exit: standard package layouts activate with discoverable resources, and hostile fixtures fail closed.

**M004 — Workspace scoping and operator visibility (capability):** canonical ancestor walk and nested-workspace behavior, `/skills` view for effective/shadowed/invalid provenance, refresh and stale-epoch semantics. Exit: nested workspace sees only eligible ancestor assets; active turn pins remain unchanged; diagnostics are actionable.

**M005 — Quality and maintenance skill guidance (polish):** add/refresh codegg's own targeted `testing-ci`, `tool-execution`, `security-hardening` guides and the smallest appropriate skill fixture/check; consider `lsp-ide` only where maintenance coverage materially improves. Exit: accurate guidance referencing actual code; no fictional capability or permanent broad scanner.

## 8. Cross-cutting requirements

Storage: prefer no schema migrations; any new persisted source kind or versioned DTO needs explicit additive compatibility. Protocol: reuse runtime-asset summary and refresh; if more diagnostic fields are necessary, make them bounded and version tolerant. Security: careful about TOCTOU on symlinked packages, metadata/tool escalation, hostile filenames, secrets in diagnostics, and absent/permission-denied roots. Concurrency: single-flight refresh, nonblocking/async TUI commands, pinned generations. Observability: report effective source, shadow reason and error, not resource bodies. Performance: bounded root/ancestor enumeration and read budgets.

## 9. Verification strategy

Temporary-home and temporary-worktree fixtures across Linux/macOS/Windows path semantics; duplicate physical symlink fixture; 2-project concurrent isolation; malformed YAML, long names, traversal, size limits, symlink escape, nested resource size/count/depth, unknown vendor extensions, stale turn/reload, and unchanged permission map. Use focused `cargo test -p codegg --test skills_registry`, `cargo test -p codegg --test skills`, relevant module tests and `scripts/verify.sh quick`; no remote network or live-provider CI.

## 10. Risks and decision points

Canonical path equality must not accidentally suppress a deliberate explicit override: separate path alias dedup from name collision semantics. Preserve existing enum/serialized discriminants if consumed by snapshots/protocol. For external symlink packages, choose a single explicit trust policy before implementation and refuse a relaxation that cannot be safely enforced across supported OSes. Foreign agent metadata is not an instruction to execute vendor hooks. If a public auth/trust boundary changes, stop for an ADR rather than improvising.

## 11. Completion definition

All five milestone closure records accepted; source roots and behavior documented and qualified; no new execution privileges; both skill activation and discovery work for genuine project and home installations; operator can explain missing/shadowed skills; prior runtime-asset invariants remain intact.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/cross-agent-skills-compatibility/001-activation-parser-contract.md` | `plans/closure/cross-agent-skills-compatibility/001-status.md` | Closed Runtime Assets only; M002 unblocked after accepted closure. |
| M002 | closed | `plans/implementation/cross-agent-skills-compatibility/002-source-roots-and-deduplication.md` | `plans/closure/cross-agent-skills-compatibility/002-status.md` | Hard M001 satisfied; M003 unblocked after accepted closure. |
| M003 | closed | `plans/implementation/cross-agent-skills-compatibility/003-package-metadata-and-resources.md` | `plans/closure/cross-agent-skills-compatibility/003-status.md` | Hard M002 satisfied; M004 unblocked after accepted closure. |
| M004 | closed | `plans/implementation/cross-agent-skills-compatibility/004-scoped-discovery-and-inspection.md` | `plans/closure/cross-agent-skills-compatibility/004-status.md` | Hard M003 satisfied; M005 unblocked after accepted closure. |
| M005 | closed | `plans/implementation/cross-agent-skills-compatibility/005-maintenance-skills-and-validation.md` | `plans/closure/cross-agent-skills-compatibility/005-status.md` | Hard M004 satisfied; terminal milestone. |
