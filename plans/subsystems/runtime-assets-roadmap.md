# Runtime Assets and Harness Interoperability Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/001-terminology-and-domain-model.md` — project assets, snapshots, generations, provenance
- `plans/002-long-term-roadmap.md#phase-1--runtime-asset-registry-interoperability-and-refresh-correctness`

Related ADRs:

- None required initially. The canonical specification already fixes the portable skill locations, precedence expectations, refresh baseline, and immutable in-flight snapshot rule.

## 1. Purpose and ownership boundary

This subsystem owns discovery, parsing, validation, precedence, provenance, refresh, publication, and runtime snapshotting for repository-scoped agents, skills, project instructions, and related prompt assets. It ensures a long-running daemon remains correct when repository assets change and interoperates safely with other coding harnesses using the same checkout.

It consumes stable project/workspace identity, configuration, session lifecycle, agent resolution, skill activation, and turn runtime construction. It must not own provider credentials, project catalog scanning, worktree creation, team authorization, or distributed blob transport beyond defining the later manifest seam.

## 2. Work classification

### Invariants

- Runtime assets are scoped to an explicit project/workspace, never process `PWD`.
- Session-open refresh is the correctness baseline; file watching is optional acceleration.
- Refresh is transactional: invalid candidates do not replace the last valid snapshot.
- Active turns and agent runs retain the immutable asset snapshot captured at start.
- Foreign harness directories are read-only unless explicitly selected as a write target.
- Discovery or activation never executes bundled scripts.
- Precedence and shadowing are deterministic and inspectable.

### Capabilities

- CodeGG uses compatible skills from `.agents/skills`, `.opencode/skills`, and `.claude/skills` without copying them.
- Users can manually refresh all or selected project assets.
- Sessions automatically see newly added, changed, removed, or shadowed assets on open and before subsequent turns.
- Operators can inspect source, digest, generation, diagnostics, and shadowing.

### Infrastructure

- Source-aware asset registry.
- `ProjectAssetSnapshot` and generation management.
- Portable `SKILL.md` parser and bounded resource index.
- Explicit-context agent registry.
- Refresh coordinator, protocol DTOs/events, and runtime snapshot pinning.

### Polish

- Clear refresh reports and TUI diagnostics.
- Optional watcher/coalescing support.
- Documentation for cross-harness repositories.

## 3. Non-goals

- Executing foreign skill scripts automatically.
- Treating foreign agent-definition formats as trusted CodeGG agents without an explicit adapter.
- Writing synchronization files into other harness directories.
- Real-time distributed asset replication in this phase.
- Replacing the plugin or MCP systems with skills.
- Making asset refresh mutate an in-flight turn.

## 4. Current state

`src/skills/mod.rs` stores a mutable vector and loads only the global CodeGG skills directory plus `.codegg/skills`. Loading clears and rebuilds the vector when explicitly invoked. It accepts direct Markdown files and `SKILL.md` inside immediate child directories, but lacks source kinds, deterministic duplicate handling, digests, generation, bounded resources, and transactional publication.

`src/agent/registry.rs` already provides useful provenance and overlay diagnostics for built-ins, global CodeGG agent files, project agent files, config agents, and modes. However, project agent discovery reads `PWD`, making it incompatible with a multi-project singleton daemon. Registry construction is not yet a project service with an immutable published generation.

Session and turn runtime construction already has explicit workspace execution context, which is the correct anchor for asset loading. Existing async TUI command patterns and protocol request/response structures provide seams for refresh commands and reports.

## 5. Target architecture

Create a project/workspace-scoped asset service that builds immutable candidate snapshots from ordered `AssetSource` instances. Each resolved asset retains:

- logical name and kind;
- source kind and path;
- content digest and modification fingerprint;
- parsed metadata;
- validation diagnostics;
- precedence rank;
- shadowed alternatives;
- bounded resource descriptors.

`ProjectAssetSnapshot` contains effective agents, skills, project instructions, diagnostics, source inventory, and a monotonic generation. A refresh coordinator coalesces concurrent refresh requests, builds outside the publication lock, validates the candidate, and atomically swaps the latest valid snapshot.

Turn runtime creation captures an `Arc<ProjectAssetSnapshot>`. Active turns never observe later swaps. New sessions and newly opened sessions trigger refresh before their next turn runtime. Manual commands return a structured diff report.

## 6. Dependency graph

```text
Milestone 1: source-aware asset model and portable discovery
        |
        +--> Milestone 2: explicit-context agent integration
        |           |
        |           v
        +--> Milestone 3: refresh coordinator and protocol surface
                    |
                    v
Milestone 4: runtime pinning, resource security, and closure
```

- All milestones have a hard dependency on Domain Identity Milestone 3 or a stable interface providing explicit project/workspace context.
- Milestone 2 has a hard dependency on Milestone 1.
- Milestone 3 has a hard dependency on Milestone 1 and an interface dependency on session lifecycle hooks.
- Milestone 4 has hard dependencies on Milestones 2 and 3.

## 7. Milestones

### Milestone 1 — Source-aware registry and portable skill discovery

Class: infrastructure

Objective: replace the mutable skill vector with deterministic, validated, source-aware discovery across CodeGG and compatible harness locations.

Dependencies: hard on stable project/workspace identity interface.

Deliverable boundary: asset types, source enumeration, portable parser, precedence, diagnostics, content digests, bounded resource inventory, and compatibility loading for native direct Markdown files.

User or operator value: CodeGG recognizes portable repository skills and explains conflicts.

Exit conditions:

- all required project/global locations are covered;
- duplicate names resolve deterministically with provenance;
- invalid higher-precedence assets preserve valid lower-precedence fallback with diagnostics;
- symlink/path traversal and size limits are enforced;
- foreign directories are never modified.

Deferred work: session triggers and runtime pinning.

### Milestone 2 — Explicit-context agent and instruction resolution

Class: invariant

Objective: remove `PWD` from project agent resolution and merge agents, skills, and project instructions into one project-scoped snapshot.

Dependencies: hard on Milestone 1.

Deliverable boundary: `AgentRegistry` construction from explicit context, preserved built-in/config overlay semantics, source digests, diagnostics, and project-instruction loading.

Exit conditions:

- agent resolution uses project/workspace context only;
- existing CodeGG overlay semantics remain compatible;
- snapshots expose effective agent-definition digests;
- two concurrently active projects cannot leak assets into one another.

Deferred work: refresh commands and watcher acceleration.

### Milestone 3 — Refresh lifecycle and operator surface

Class: capability

Objective: add transactional refresh on project/session lifecycle and a discoverable manual command.

Dependencies: hard on Milestone 1; interface dependency on Milestone 2 snapshot builder.

Deliverable boundary: refresh coordinator, project activation/session create/open/attach/rebind triggers, protocol requests/responses/events, `/reload` and focused aliases, structured reports, coalescing, and generation display seams.

Exit conditions:

- opening or attaching a session refreshes before the next turn runtime;
- manual refresh reports added/removed/changed/shadowed/invalid/retained entries;
- failed refresh preserves the prior valid generation;
- concurrent refresh requests converge without duplicate publication.

Deferred work: file watchers and distributed node manifests.

### Milestone 4 — Immutable runtime pinning and closure

Class: invariant

Objective: guarantee reproducible in-flight turns and secure bounded skill resources.

Dependencies: hard on Milestones 2 and 3.

Deliverable boundary: snapshot capture at turn/agent-run start, activated-skill digest recording, generation transitions for subsequent turns, resource access controls, closure tests, and architecture documentation.

Exit conditions:

- active turns do not change behavior after refresh;
- subsequent turns use the latest successful generation;
- skill resources are bounded and cannot escape the skill directory;
- discovery/activation executes no scripts;
- remote workspace manifest types are defined without implementing transport.

## 8. Cross-cutting requirements

### Storage and migration

Snapshots may be reconstructed rather than fully persisted, but generation/digest metadata needed for sessions and audit seams must be durable. Existing direct Markdown support remains a documented compatibility path.

### Protocol and compatibility

Add bounded asset summaries, refresh requests, refresh reports, diagnostics, and capability flags. Large skill bodies/resources stay behind handles or are loaded locally on activation.

### Security and authorization

Validate paths, symlinks, file sizes, UTF-8/frontmatter limits, and resource enumeration. Unknown foreign metadata cannot grant tool authority. Later team authorization must gate asset inspection and refresh.

### Concurrency, cancellation, and recovery

Use single-flight/coalesced refresh per project/workspace. Cancellation before publication leaves the previous snapshot. Daemon restart reconstructs the current snapshot deterministically.

### Observability and audit

Log source counts, duration, generation, validation failures, and shadowing without logging secret content. Preserve digests and activated-skill references for later audit.

### Performance and resource use

Metadata-first discovery and bounded reads are required. Do not eagerly read all bundled resources. Avoid repeated full scans within one refresh generation.

### Documentation and operations

Update skills, agents, config, session, protocol, and daemon architecture docs. Document precedence and interoperability behavior.

## 9. Verification strategy

Use table-driven discovery/precedence tests, temporary Git worktrees, malformed and adversarial skill fixtures, concurrent refresh tests, two-project isolation tests, session lifecycle integration tests, in-flight snapshot pinning tests, and restart reconstruction tests.

## 10. Risks and decision points

- Different harnesses may extend frontmatter incompatibly. Preserve unknown metadata and diagnose unsupported semantics.
- File watcher behavior differs across platforms. Watchers remain optional and never the correctness mechanism.
- Full agent-format interoperability is not standardized. Keep foreign agent adapters explicit and separately reviewed.
- Snapshot persistence could overcomplicate Phase 1. Persist only the metadata needed for reproducibility and reconstruct bodies from the workspace where safe.

## 11. Completion definition

This roadmap closes when project assets are explicit-context, interoperable, source-aware, refreshable without daemon restart, transactionally published, immutable for in-flight execution, and secure against passive discovery causing code execution or path escape.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/runtime-assets/001-project-asset-registry.md` | `plans/closure/runtime-assets/001-status.md` | — |
| 2 | closed | `plans/implementation/runtime-assets/002-explicit-context-agent-instruction-resolution.md` | `plans/closure/runtime-assets/002-status.md` | — |
| 3 | closed | `plans/implementation/runtime-assets/003-refresh-lifecycle-operator-surface.md` | `plans/closure/runtime-assets/003-status.md` | — |
| 4 | ready | `plans/implementation/runtime-assets/004-immutable-runtime-pinning-and-closure.md` | — | Milestones 2–3 closed; handoff ready |
