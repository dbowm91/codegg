# Memory-to-Skill Promotion Roadmap

Status: active planning — M002 closed, M003 ready

Repository baseline reviewed: `1bee32578566cc6cdf4025002af781309d8f29f4`

Long-term references:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-1--runtime-asset-registry-interoperability-and-refresh-correctness`
- `plans/003-planning-process.md`

Closed dependencies and current architecture:

- runtime-assets/harness-interoperability work is closed;
- `architecture/memory.md` describes persistent text-derived memory consolidation;
- `architecture/skills.md` describes source-aware portable `SKILL.md` discovery, validation, precedence, bounded resources, and immutable asset refresh;
- `src/agent/asset_refresh.rs` and the project asset snapshot path remain the publication/refresh authority after a file is deliberately written.

External design input:

- MiniMax, “MiniMax Agent Team: Built for Long-Running Tasks and Continuous Evolution,” `https://www.minimax.io/blog/minimax-agent-team-long-running-1779893953` — repeated team experience can be deposited into memory and valuable actions into Skills; the same article warns that memory-write constraints need runtime gates rather than model self-discipline.
- MiniMax, “MiniMax M2.7: Early Echoes of Self-Evolution,” `https://www.minimax.io/blog/minimax-m27` — an internal harness combined persistent memory, skills, feedback, and iterative evaluation.

These are design inputs only. CodeGG must not adopt unattended self-modifying-harness behavior. Generated reusable behavior is a proposal until the user explicitly approves publication.

## 1. Purpose and ownership boundary

CodeGG already learns durable textual preferences/conventions through `MemoryStore`, and it already consumes portable skills through `AssetRegistry`. The missing connection is a safe promotion pipeline from repeated, successful, bounded workflow evidence into a user-reviewable skill proposal.

This subsystem owns:

- host-derived workflow observations that are safe to retain as habit evidence;
- deterministic habit fingerprinting, repetition counting, candidate state, dismissal/supersession, and scope;
- a user-triggered model-assisted draft proposal that cannot publish itself;
- a bounded proposal store with source habit/memory provenance and content digest;
- explicit user approval followed by safe publication only into CodeGG-owned skill roots;
- validation through the existing portable skill parser/registry contract before write;
- transactional/atomic file publication and normal `ProjectAssetSnapshot` refresh behavior afterward;
- candidate/proposal/publish inspection and concise TUI/command surfaces.

It consumes, but does not redefine:

- `MemoryStore` and `PatternDetector` for durable textual preferences/conventions;
- runtime tool/run events for safe structural workflow observations;
- `AssetRegistry`, `EffectiveSkill`, portable `SKILL.md` parser, precedence, diagnostics, and bounded resource rules;
- `AssetRefreshCoordinator` and immutable turn asset snapshots;
- normal model/tool permission and provider-context rules when the current agent drafts a proposal;
- project/workspace identity and CodeGG config roots.

The governing rule is:

> Repetition may create a candidate; a model may draft a proposal; only an explicit user approval may create or replace a reusable skill file.

## 2. Why this is worth adding

MiniMax's public Agent Team material treats durable experience as part of the return on expensive agent work: pitfalls become memory and valuable actions become Skills. That idea maps well to CodeGG because both substrates already exist independently.

Today CodeGG's memory consolidation can remember “we use clippy” or an architectural preference, but it cannot represent that a developer repeatedly follows a reusable sequence such as:

```text
inspect changed Rust files
-> run focused tests
-> run clippy for the touched package
-> inspect diff
-> summarize remaining risk
```

Conversely, CodeGG can load a well-formed `SKILL.md`, but there is no safe bridge from observed repeated behavior to a proposed skill.

The useful feature is not autonomous self-modification. The useful feature is reducing the cost of noticing and packaging repetition while retaining user control and the existing asset security boundary.

## 3. Work classification

### Invariants

- Automatic observation never writes a skill.
- Habit detection is deterministic/host-owned in the first implementation and does not make background LLM calls.
- A single repetitive agent loop inside one session is insufficient to establish a durable habit. Candidate readiness requires independent observations across distinct sessions or explicitly distinct user-invoked operations.
- Automatic habit evidence never persists raw tool output, hidden reasoning, credentials, environment variables, arbitrary shell command strings, or full model prompts.
- Only a bounded allowlisted structural action vocabulary may participate in automatic workflow fingerprints.
- The existing memory detector continues to ignore tool-output/binary parts; habit tracking is a separate typed path rather than weakening that safety choice.
- Draft generation cannot grant tools or permissions. Initial generated skills contain one `SKILL.md` only: no scripts, executable resources, MCP definitions, plugins, or authority-bearing `allowed-tools` behavior.
- Publication writes only to CodeGG-owned project/global roots in the initial roadmap. `.agents`, `.opencode`, `.claude`, and other foreign harness directories remain read-only.
- Existing `AssetRegistry` parsing/validation and precedence rules remain authoritative. The publisher does not maintain a second permissive parser.
- Active turns remain pinned to their captured `ProjectAssetSnapshot`; successful publication affects only subsequent turns after normal refresh.
- A failed refresh after a durable publication does not silently delete the user-approved file. The prior runtime snapshot remains active and the publication reports “written but not active” with diagnostics.
- Existing skill files are never overwritten implicitly. Replacement requires an explicit later user action and digest/concurrency check; the first milestone set may simply reject collisions.

### Capabilities

- CodeGG can show repeated project workflows as habit candidates with confidence/evidence counts and the sessions in which they were observed.
- The user can dismiss a candidate without being repeatedly nagged for the identical fingerprint.
- The user can ask the current agent to turn one candidate into a portable `SKILL.md` proposal and inspect the proposed content before publication.
- The user can explicitly approve publication into `.codegg/skills/<name>/SKILL.md` or the CodeGG global skills root.
- Publication runs existing skill validation, path containment, collision detection, atomic file write, and asset refresh.
- The habit candidate records that it was promoted and links the published skill digest/path without becoming the source of runtime skill authority.

### Infrastructure

- `WorkflowObservation`/normalizer;
- file-backed `HabitStore` near the memory subsystem with bounded candidate records;
- `SkillProposalStore` or combined promotion store;
- proposal validation adapter that reuses the skill parser;
- CodeGG-owned `SkillPublicationService`;
- asset-refresh invocation and bounded result reporting.

### Polish

- `/habits` or equivalent inspect/dismiss commands;
- `/skill-promote <habit-id>` or equivalent user-triggered proposal flow;
- `/skill-proposals` and explicit approve/publish operation;
- concise candidate evidence and conflict diagnostics.

## 4. Explicit non-goals

This roadmap does not:

- autonomously install skills because a model thinks one is useful;
- allow a generated skill to add scripts, binaries, plugins, MCP servers, external dependencies, or permission grants;
- write foreign harness skill directories;
- modify agent definitions or system prompts outside normal skill activation;
- fine-tune or train models;
- create a vector database or new semantic-memory service;
- persist complete session transcripts as habit evidence;
- fingerprint raw shell command text or secret-bearing tool arguments;
- infer a reusable skill from a single session by default;
- automatically replace an existing skill with a generated version;
- make Tool Programs the initial promotion target;
- add background model calls at `AgentFinished` merely to decide whether a habit exists;
- add a new CI/release pipeline.

A later roadmap may consider explicit promotion of well-proven deterministic workflows into Tool Programs, but only after the human-reviewed `SKILL.md` path is mature.

## 5. Current-state evidence

At baseline `1bee3257`:

- `crates/codegg-core/src/memory/mod.rs` persists `Memory` records under the user config `codegg/memory` root, uses project namespaces based on domain-separated SHA-256, and saves through advisory locking and temp-file rename.
- `MemoryStore::consolidate_session()` uses a deterministic `PatternDetector`, scores candidates, keeps top bounded matches, and writes durable user/project conventions.
- `crates/codegg-core/src/memory/patterns.rs` reads only textual message parts. It recognizes preferences, coding conventions, deprecations, naming, architecture, and tool preferences. It does not consume tool-call structure or successful workflow sequences.
- memory auto-consolidation is already optional through `experimental.memory_auto_consolidate`.
- `architecture/skills.md` documents a source-aware `AssetRegistry` that discovers CodeGG, `.agents`, OpenCode, and Claude-compatible skill roots with deterministic precedence and diagnostics.
- portable skills require `name` and `description`; resources are lazy/bounded; symlink/path escape is rejected; `allowed-tools` metadata does not itself grant permission.
- runtime asset refresh creates immutable project snapshots; failed refresh retains the previous valid generation and in-flight turns remain pinned.
- there is no canonical service for writing generated skill packages, no habit lifecycle, and no promotion/proposal state.

This is a bridge problem between two existing mature subsystems, not a reason to replace either subsystem.

## 6. Safe workflow observation model

### 6.1 Observation sources

Create typed workflow observations from host-owned execution metadata at stable boundaries. Candidate sources may include:

- canonical tool name;
- tool effect/category class;
- structured execution status;
- high-level supervised test kind (`test`, `lint`, `build`, `format`) when the test subsystem exposes it as typed metadata;
- read-only/mutating Git operation class or safe subcommand enum already produced by typed Git classification;
- LSP operation kind;
- skill activation name/digest;
- delegated agent role/name;
- goal/todo phase transitions when already typed;
- project-relative file class only if represented without sensitive/raw path leakage and demonstrably useful.

Automatic fingerprinting must exclude by default:

- raw Bash/terminal command strings;
- arbitrary tool JSON arguments;
- raw output/error bodies;
- environment variables;
- URLs/tokens/secrets;
- hidden reasoning;
- model-generated free-form summaries used as if they were authoritative action identity.

A shell/terminal invocation may contribute only a coarse `shell_exec` action class until CodeGG has a safe structured command-intent representation suitable for durable habit identity. Do not persist the command text merely to make candidates more specific.

### 6.2 Successful occurrence

An observation sequence becomes an occurrence only after a logical operation/session segment reaches a stable successful boundary. Failed/cancelled/no-progress sequences may contribute negative evidence or be excluded; they must not increase “successful habit” confidence.

Use existing session/run/turn identity to distinguish independent occurrences. Multiple identical loops within one turn/session should collapse into one occurrence for readiness counting unless the user explicitly marks them as separate reusable procedures.

### 6.3 Fingerprint

Normalize the bounded action sequence deterministically and domain-separate the hash, for example:

```text
codegg-habit-v1\0
scope/project-id
[action-kind, safe-variant, ...]
```

The fingerprint is identity/deduplication metadata, not a secret and not a substitute for the human-readable candidate summary.

## 7. Habit candidate lifecycle

Introduce bounded candidate state such as:

```rust
enum HabitCandidateStatus {
    Observing,
    Ready,
    Dismissed,
    Promoted,
    Superseded,
}

struct HabitCandidate {
    id: HabitId,
    scope: HabitScope,
    workflow_fingerprint: String,
    action_skeleton: Vec<WorkflowAction>,
    successful_occurrences: u32,
    distinct_sessions: u32,
    first_seen: i64,
    last_seen: i64,
    status: HabitCandidateStatus,
    related_memory_ids: Vec<String>,
    promoted_skill: Option<PublishedSkillRef>,
}
```

Recommended initial readiness threshold:

```text
successful occurrences >= 3
AND distinct sessions >= 2
```

Keep a code-level minimum of at least two distinct sessions for automatically surfaced “Ready” candidates. Configuration may require more evidence; it must not reduce the hard minimum to one session.

Candidate action lists, session references, and memory references are bounded. Store only stable IDs/digests and safe summaries, not whole messages.

A dismissed fingerprint remains suppressed until its normalized workflow materially changes (new version/fingerprint). A promoted candidate remains linked for provenance but does not repeatedly re-propose itself.

## 8. Storage choice

Keep habit/proposal state near the existing user-editable/file-backed memory system unless implementation evidence demonstrates a need for SQLite transactional joins.

Preferred initial layout:

```text
<config>/codegg/memory/
  habits/
    project/<project-namespace>.json
    user/preferences.json            # optional later/global scope
  proposals/
    <proposal-id>.json
```

Exact filenames may follow existing namespace helpers. Requirements:

- advisory locking compatible with `MemoryStore`;
- temp write + `fsync` + atomic rename;
- strict file/count/record/string bounds;
- safe namespace/path handling;
- corrupt one candidate/proposal should produce diagnostics rather than silently wiping the complete store;
- no new daemon-global mutable singleton outside the normal project/user service construction.

If sharing `MemoryStore`'s lock/root is cleaner, refactor a small reusable atomic-file helper rather than duplicating subtly different persistence. Do not turn the Markdown `MEMORY.md` format into an overloaded workflow database.

## 9. Skill proposal contract

A proposal is not an effective skill. It is a bounded draft artifact with provenance:

```rust
struct SkillProposal {
    id: SkillProposalId,
    habit_id: HabitId,
    habit_fingerprint: String,
    name: String,
    description: String,
    skill_markdown: String,
    target_scope: SkillTargetScope,
    content_digest: String,
    status: SkillProposalStatus,
    created_at: i64,
    updated_at: i64,
}

enum SkillProposalStatus {
    Draft,
    Validated,
    Rejected,
    Published,
    Superseded,
}
```

Proposal bounds must be no larger than the existing skill parser's accepted `SKILL.md` bounds.

The draft's initial schema is deliberately restricted:

- required portable `name` and `description`;
- body instructions only;
- optional safe metadata/provenance;
- no bundled scripts/resources;
- no plugin/MCP declarations;
- no executable payloads;
- no generated `allowed-tools` authority. If portable frontmatter includes `allowed-tools`, initial generated proposals should reject or strip it with an explicit diagnostic rather than implying permission.

## 10. User-controlled drafting model

Do not make a model call automatically when a habit reaches `Ready`.

The preferred flow reuses the current interactive agent:

```text
user selects/promotes Habit H
   -> host exposes bounded H evidence to current turn
   -> current model drafts one portable SKILL.md
   -> model submits draft to a host `skill_proposal`/promotion tool or equivalent typed service
   -> host validates + stores proposal
   -> user previews proposal
```

This avoids a second direct-provider orchestration path and preserves the current session's model/provider context and permission semantics.

If implementation uses a dedicated model call instead, it must reuse the already-correct provider request context for the owning session and remain user-triggered. The proposal store/validator remains host authority either way.

## 11. Publication target and safety

Initial writable targets:

```text
project: <workspace/project>/.codegg/skills/<name>/SKILL.md
global:  <platform-config>/codegg/skills/<name>/SKILL.md
```

Foreign compatibility roots remain discovery-only.

Publication must:

1. authorize an explicit user action;
2. re-read proposal and verify expected content digest/revision;
3. normalize/validate the skill name and target root;
4. ensure canonical destination remains inside the exact CodeGG-owned root;
5. validate the complete in-memory `SKILL.md` through the same parser rules as discovery;
6. reject unsupported generated scripts/resources/authority metadata;
7. fail on an existing conflicting destination in the first implementation rather than overwrite it;
8. create the package directory safely;
9. write a temporary file, `fsync`, atomically rename, and synchronize the directory where supported by the repository's normal durability helper;
10. mark proposal/candidate publication state with the final content digest/path;
11. request normal project asset refresh;
12. report written path, digest, refresh generation/effectiveness, shadow/conflict diagnostics, and whether the current in-flight turn remains pinned to the old generation.

If refresh fails after the file was durably published, retain the file and the previous runtime snapshot. Report “published, activation refresh failed” with diagnostics; do not silently delete a user-approved artifact.

## 12. Dependency graph

```text
closed runtime-assets / skill registry
existing memory store + pattern detector
               |
               v
M001 safe workflow observations + habit candidate store
               |
               v
M002 user-triggered skill draft + proposal validation/preview
               |
               v
M003 explicit publication + CodeGG-owned writer + asset refresh
```

M001 is ready because its dependencies are current implemented code and closed runtime-assets work.

The agent-convergence roadmap is independent. A future integration may feed successful convergence workflow observations into habits, but neither roadmap blocks the other.

## 13. Milestones

### M001 — Habit observation and candidate store

Status: closed

Plan:

- `plans/implementation/memory-skill-promotion/001-habit-observation-and-candidate-store.md`

Objective:

Add a deterministic safe structural workflow-observation pipeline and durable candidate lifecycle without model drafting or skill writes.

Exit conditions:

- only allowlisted typed action metadata enters automatic fingerprints;
- raw shell/tool outputs/arguments/hidden reasoning are excluded;
- candidate readiness requires at least three successes across at least two sessions by default and never fewer than two sessions;
- candidate store is bounded, durable, restart-safe, and supports dismiss/promoted/superseded state;
- existing `MemoryStore` behavior remains compatible;
- focused tests and quick verification pass.

### M002 — User-triggered skill draft and preview

Status: closed

Plan:

- `plans/implementation/memory-skill-promotion/002-user-triggered-skill-draft-and-preview.md`

Closure:

- `plans/closure/memory-skill-promotion/002-status.md`

Objective:

Let a user select a ready habit, expose bounded evidence to the current model, submit one restricted portable `SKILL.md` proposal, validate/store it, and preview/reject it without writing to any runtime skill root.

Exit conditions:

- no proposal is generated without explicit user initiation;
- draft validation reuses existing skill parser rules;
- generated proposal supports one `SKILL.md` only and cannot grant permissions/scripts/resources;
- proposal retains habit/content provenance and revision/digest;
- preview/reject/dismiss flows are discoverable and bounded;
- no runtime asset refresh or effective skill change occurs in M002.

### M003 — Explicit approved publication and asset refresh

Status: ready

Plan:

- `plans/implementation/memory-skill-promotion/003-approved-skill-publication-and-refresh.md`

Objective:

Add an explicit user-approved publisher for validated proposals into CodeGG-owned project/global skill roots and integrate publication with existing asset refresh/snapshot semantics.

Exit conditions:

- only explicit user approval writes a file;
- foreign harness roots are never written;
- path/collision/symlink/TOCTOU checks fail closed;
- initial publication does not overwrite an existing different skill;
- write is atomic/durable and proposal/candidate provenance is updated;
- existing asset refresh determines effectiveness and retains old snapshot on failure;
- active turns stay pinned; subsequent turns use the new generation when refresh succeeds;
- focused tests and quick verification pass.

## 14. Storage, protocol, migration, and compatibility

No SQLite migration is expected for M001-M003 if the file-backed store remains sufficient. If implementation chooses SQLite, stop and justify why the existing memory/asset lifecycle cannot provide the needed durability; register a narrower storage decision if it changes ownership materially.

TUI/native protocol additions should be bounded optional candidate/proposal summaries. Old clients can ignore unknown fields. Detailed proposal bodies should be fetched on demand rather than placed in normal session snapshots.

Existing memories and skills require no migration. Existing `.codegg`, `.agents`, `.opencode`, and `.claude` skill precedence remains unchanged. Published generated skills simply appear as ordinary CodeGG-owned skill candidates on the next successful registry refresh.

## 15. Security, privacy, and reliability

- Workflow observations must be safe-by-construction structural data. Redaction after storing raw commands is not sufficient.
- User/project scope must prevent one repository's workflow from silently becoming a global habit.
- Global publication requires an explicit target selection distinct from project publication.
- Habit/proposal records must not contain credentials or arbitrary tool output.
- The model is never trusted to choose the destination path directly; it proposes a normalized skill name/body and the host derives the destination.
- Skill validation must be done before write and destination containment rechecked at write time to prevent symlink/path races.
- Publication state uses proposal revision/content digest so a stale approval cannot publish a proposal that changed after preview.
- A crash during write leaves either the old state/no file or a complete new `SKILL.md`, not a truncated package.
- A crash after file publication but before proposal-state update is reconciled by comparing destination digest and proposal expected digest; do not duplicate/rewrite blindly.
- Refresh failure retains prior runtime snapshot and surfaces a diagnostic.

## 16. Verification posture

Each milestone defines narrow tests. Roadmap minimum:

```bash
cargo fmt --all -- --check
git diff --check
cargo test -p codegg-core memory --locked
cargo test --test skills_registry --locked
scripts/verify.sh quick
```

Add a focused integration target such as `tests/habit_skill_promotion.rs` once cross-subsystem behavior exists.

Do not add a live-model test, CI workflow, background benchmark, or continuous scanner as a closure prerequisite.

## 17. Architecture/docs and static guards

Update as milestones land:

- `architecture/memory.md` — distinguish textual memory from structural habit evidence and candidate lifecycle;
- `architecture/skills.md` — proposal/publication boundary and CodeGG-owned write targets;
- `architecture/agent.md` or tool docs for user-triggered proposal submission if model-facing tooling is added;
- `architecture/config.md` for thresholds/enablement only if configuration is exposed;
- `architecture/overview.md` if a dedicated promotion architecture page is introduced.

Static/review guards should prevent:

- production writes to `.agents/skills`, `.opencode/skills`, or `.claude/skills` from the promotion service;
- generated proposal packages containing scripts/executable resource files in the initial feature;
- proposal publication without an explicit approval token/revision from the user-facing control path;
- raw shell/tool-output capture in automatic workflow observations.

Prefer tests at the owning module boundary over a new repository-wide verification framework.

## 18. Risks and deferred work

Risks:

- over-generalizing a workflow from noisy repetition;
- privacy leakage if raw command/tool details are retained;
- generating verbose or low-quality skills that clutter the catalog;
- candidate spam from common generic actions;
- stale approval publishing a changed proposal;
- users confusing generated instructions with new tool permissions.

Mitigations are multi-session thresholds, structural allowlists, explicit review/approval, collision-safe publication, parser reuse, and no generated authority.

Deferred:

- autonomous skill installation;
- generated scripts/resources;
- replacement/update flows for existing skills beyond explicit conflict handling;
- writing foreign harness roots;
- automatic convergence-to-skill promotion;
- model-driven candidate detection;
- Tool Program generation from habits;
- cloud/team sharing of proposed skills;
- automatic quality scoring from model telemetry.

## 19. User-visible roadmap exit

This roadmap may close when CodeGG can notice that a user repeatedly follows a safe structural workflow, show that pattern as an inspectable candidate, help the user draft a portable skill on request, and publish it only after explicit approval through the existing secure skill registry/refresh path. At no point may repeated behavior silently mutate the user's reusable harness.
