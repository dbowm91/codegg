# Memory-to-Skill Promotion M003 — Approved Publication and Asset Refresh

Status: blocked on M002

Repository baseline: `1bee32578566cc6cdf4025002af781309d8f29f4`

Source subsystem roadmap:

- `plans/subsystems/memory-skill-promotion-roadmap.md`

Hard dependency:

- M002 `plans/implementation/memory-skill-promotion/002-user-triggered-skill-draft-and-preview.md` must be strictly closed.

Long-term requirements:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Current authority:

- M001/M002 candidate/proposal lifecycle and digest/revision become authoritative after closure;
- `AssetRegistry` remains the parser/discovery/precedence authority;
- `AssetRefreshCoordinator` remains the immutable project-asset publication authority for runtime turns;
- foreign harness skill roots remain discovery-only;
- no new ADR is required if the publisher writes only CodeGG-owned roots and preserves existing refresh/permission semantics. Stop if implementation would change foreign compatibility ownership or authorization semantics.

Primary class: capability / filesystem safety / reliability

## 1. Objective

Add the final explicit publication boundary: after the user has previewed a validated `SkillProposal`, one explicit approval may publish exactly that proposal revision into a CodeGG-owned skill root, record provenance, and invoke the existing runtime asset refresh path.

Target flow:

```text
Validated SkillProposal P@revision R,digest D
       |
explicit user approval of P/R/D and target scope
       |
       v
SkillPublicationService
  - revalidate proposal + destination
  - reject collision/unsupported package content
  - atomic write SKILL.md
       |
       v
record proposal Published + habit Promoted
       |
       v
AssetRefreshCoordinator.refresh(project/workspace)
       |
       +-- success -> new generation available to subsequent turns
       |
       `-- failure -> file remains published; previous valid snapshot remains active; diagnostic returned
```

No model can cross this boundary on its own.

## 2. Explicit non-goals

M003 must not:

- auto-approve or auto-publish after proposal validation;
- let an agent call the approval operation as if it were the user;
- write `.agents/skills`, `.opencode/skills`, `.claude/skills`, or arbitrary configured foreign roots;
- publish generated scripts, executables, resource bundles, plugins, MCP definitions, or dependency installers;
- grant tool permissions through generated `allowed-tools`;
- silently overwrite an existing different skill file;
- silently change an in-flight turn's runtime asset snapshot;
- delete a successfully written user-approved skill merely because refresh later fails;
- make published-skill provenance the runtime permission authority;
- add automatic cloud/team synchronization;
- add a new file watcher requirement;
- add a new CI/release pipeline.

## 3. Re-inspection required before implementation

After M002 closes, re-read:

- M002 closure and exact proposal store/revision/digest APIs;
- `src/skills/source.rs` for authoritative CodeGG project/global root resolution and enabled source semantics;
- `src/skills/parser.rs` and proposal validation adapter;
- `src/skills/registry.rs` precedence/conflict diagnostics;
- `src/skills/resource.rs` containment/symlink checks to reuse equivalent path policy;
- `src/agent/asset_refresh.rs`, asset snapshot builder/coordinator, and turn runtime pinning behavior;
- current `/reload`/skill refresh TUI/service operation;
- project/workspace execution context to derive project target root without `current_dir()`;
- filesystem atomic-write helpers already used by memory/config/snapshot code.

Do not derive the project skill root from process-global working directory. Use the explicit current project/workspace context and canonical CodeGG source-root policy.

## 4. Explicit approval contract

Provide one user-only operation equivalent to:

```text
/skill-proposal publish <proposal-id> [project|global]
```

The user-facing UI must show the proposal name, target scope, digest/revision, and conflict status immediately before or as part of approval.

The host approval request carries:

```text
proposal_id
expected_revision
expected_content_digest
target_scope
expected_existing_state = absent   # initial policy
```

Do not accept a filesystem path from the model/user as the target. `target_scope` is an enum from which the host derives the CodeGG-owned destination.

If the proposal content/revision changed after preview, fail with `StaleApproval` and require a new preview/approval. Do not publish “latest” implicitly.

If the proposal is already `Published` with the same destination/digest, an idempotent retry returns the existing publication result. If it is published differently or target state conflicts, fail closed.

## 5. Writable target policy

Initial writable roots are only:

```text
Project -> <resolved project/workspace root>/.codegg/skills/
Global  -> <platform config>/codegg/skills/
```

The exact project root must follow current asset-discovery containment rules for nested workspaces/worktrees. Do not walk above the declared project/worktree boundary.

The publisher must reject any attempt to target:

```text
.agents/skills
.opencode/skills
.claude/skills
arbitrary configured SourceRoot
absolute user path
path containing .. or backslash escape
symlinked package root escaping CodeGG root
```

Foreign roots remain readable compatibility inputs, not CodeGG-owned publication outputs.

## 6. Destination derivation and path safety

Derive destination as:

```text
root / normalized_skill_name / SKILL.md
```

Use the same skill-name validation as `AssetRegistry` and a dedicated safe path constructor.

Required pre-write checks:

1. resolve authoritative CodeGG root from explicit scope/project context;
2. create/check root without following an escaping symlink;
3. validate normalized skill name as one safe path component;
4. compute package/destination path;
5. canonicalize existing ancestors and ensure they remain under the root;
6. inspect destination/package for symlinks, unexpected file types, or existing content;
7. compare any existing destination digest against approval expectations;
8. revalidate immediately before rename under an appropriate per-root/path lock.

Avoid check-then-use races. Use existing no-follow/openat-style helpers if the repository has them; otherwise document the platform-safe sequence and test symlink substitution where feasible.

## 7. Collision and replacement policy

First M003 policy is intentionally simple:

- if destination does not exist: allow create;
- if destination exists with identical expected proposal digest and proposal already records that publication: return idempotent success;
- if destination exists with different content: return `SkillAlreadyExists`/conflict and do not overwrite;
- if current effective skill with same normalized name comes from another source, still permit CodeGG-owned create only after the user preview explicitly showed the shadow/precedence consequence; otherwise fail `PrecedenceChangedSincePreview` and require a refreshed approval;
- no `force` flag in the initial publication action.

A future explicit replacement/update milestone may support compare-and-swap replacement of a CodeGG-owned skill, but it is not required to make habit promotion useful.

## 8. Revalidation before write

M003 must not trust M002's historical `Validated` status alone.

Before publication:

- load proposal by ID;
- compare expected revision/digest;
- rerun current portable parser/validator against exact `skill_markdown`;
- rerun generated-skill restriction policy;
- confirm no scripts/resources/`allowed-tools`/unsupported package content was introduced;
- refresh collision/source-precedence view enough to detect material changes since preview;
- confirm target root still belongs to current project/global scope.

If parser/rules changed since proposal creation and the proposal is now invalid, return diagnostics and require redraft/revalidation. Do not publish based on stale parser version merely because the old proposal was valid.

## 9. Atomic publication

### 9.1 Write sequence

Under a per-root or per-destination lock:

1. create package directory only after containment checks;
2. create a temp file inside the same package/filesystem with restrictive normal user permissions;
3. write exact validated `SKILL.md` bytes;
4. flush and `sync_all` the file;
5. recheck destination/collision/symlink state immediately before commit;
6. atomically rename temp -> `SKILL.md`;
7. sync containing directory where supported/consistent with current repository durability helpers;
8. release lock;
9. record publication result in proposal/habit store.

A crash must not leave a partial `SKILL.md`. Stale temp files may be cleaned conservatively on a later publication/reconciliation pass if they match the publisher's own temp naming convention.

### 9.2 Publication record

Persist a bounded result such as:

```rust
struct PublishedSkillRef {
    proposal_id: SkillProposalId,
    target_scope: SkillTargetScope,
    normalized_name: String,
    relative_path: String,
    content_digest: String,
    published_at: i64,
}
```

Prefer relative/root-scoped path in user-visible durable provenance rather than duplicating machine-specific absolute workspace paths where not required.

Transition:

```text
SkillProposal::Validated -> Published
HabitCandidate::Ready -> Promoted
```

Use revision-checked host-only transitions. If proposal state persistence fails after file rename, reconciliation must detect exact destination digest and complete metadata transition idempotently rather than rewriting the file.

## 10. Asset refresh integration

After durable publication/provenance update, request the existing asset refresh for the affected scope.

### Project publication

Use the exact project/workspace `AssetContext`. `AssetRefreshCoordinator` builds a new candidate snapshot outside the publication lock and publishes a new generation only if valid according to existing rules.

Return:

```text
written = true
path = .codegg/skills/<name>/SKILL.md
content_digest = ...
refresh = success | failed
new_generation = optional
resolved_effective = true | false | unknown
shadowed_by = optional source summary
diagnostics = bounded list
active_turn_uses_new_generation = false
```

Never mutate the current in-flight turn's pinned `ProjectAssetSnapshot`.

### Global publication

Global skill publication must trigger the existing global/project refresh behavior used when global runtime assets change. If CodeGG currently requires manual per-project refresh, M003 must either call the canonical affected-project refresh operation or document that publication is durable and subsequent session-open/manual reload is the correctness baseline. Do not invent a second global watcher service merely for promotion.

### Refresh failure

If refresh fails after durable file write:

- keep the file;
- keep prior valid snapshot active;
- proposal remains `Published` with `activation_state = refresh_failed` if the proposal schema supports it, or return a durable diagnostic separately;
- tell the user to fix asset diagnostics/reload;
- retrying refresh must not republish/rewrite the file.

This matches the long-term invariant that failed refresh leaves the previous valid runtime snapshot active.

## 11. Precedence and effective-skill verification

After successful refresh, inspect the resulting registry:

- confirm whether the published digest/name is the effective skill;
- if shadowed, identify source kind and bounded path/provenance;
- if invalid despite prevalidation due to package/environment/source differences, report diagnostic and retain previous valid snapshot rules;
- record only the published file digest as provenance, not a claim that it must be effective forever.

Project `.codegg` normally has high precedence, but explicit session/project configuration may affect resolution. Do not hard-code “published means active”.

## 12. User-facing completion and undo posture

Publication response must clearly distinguish:

```text
Published and active for subsequent turns
Published, but current turn remains on generation N
Published, but refresh failed; previous snapshot still active
Publication rejected due to collision/stale approval/path safety
```

Do not implement automatic deletion/undo in M003 unless the current skill management UI already has a safe explicit delete operation. A generated skill is now a normal user-owned file and may be edited manually. Future replacement/delete operations must honor normal asset refresh semantics.

Marking a candidate promoted should not hide the published skill from ordinary `/skills` inspection.

## 13. Security and authorization

The write action is explicitly user-authorized. A model-facing tool must not expose `publish` as a callable action unless the permission system has a distinct user-confirmation token that cannot be self-issued by the model. The simpler initial design is TUI/native-control user action only.

Requirements:

- derive actor/session/project from the authenticated/local frontend control path;
- do not accept “user approved” as model text;
- target root cannot be widened by skill metadata;
- destination content is the exact approved proposal digest;
- generated instructions remain subject to ordinary tool permissions when later loaded;
- provenance/diagnostics/logs do not expose sensitive memory/session content;
- audit/event metadata, where existing architecture supports it, records structural publish action/digest rather than full skill body unless content retention policy already permits it.

## 14. Expected production-code touch set

Expected areas:

- M001/M002 habit/proposal stores;
- new `src/skills/publish.rs` / `SkillPublicationService` or equivalent owned module;
- `src/skills/source.rs` or a read-only helper for authoritative writable CodeGG roots;
- `src/skills/parser.rs` shared in-memory validation seam;
- `src/agent/asset_refresh.rs` only for invoking existing refresh, not changing its ownership model;
- TUI/native request/command path for preview-confirmed user approval;
- projection/notification result if needed;
- `architecture/skills.md`, `architecture/memory.md`, and `architecture/config.md` only if configuration changes.

Do not add generic writes to all `SourceKind` roots.

## 15. Required tests

### Approval/staleness

- exact proposal revision/digest approval publishes;
- changed proposal after preview -> `StaleApproval`;
- model/tool text claiming approval cannot invoke publisher;
- retry of exact already-published proposal is idempotent;
- changed target/digest on retry fails closed.

### Path/ownership security

- project/global CodeGG roots allowed;
- `.agents`, `.opencode`, `.claude`, absolute, traversal, and foreign configured roots denied;
- malicious skill name cannot escape one package component;
- symlinked root/package/destination escape denied;
- destination type other than expected regular absent file denied;
- TOCTOU-oriented replacement test where supported by the filesystem fixture.

### Collision

- absent destination creates;
- different existing `SKILL.md` never overwritten;
- identical existing content only treated idempotently when provenance/proposal state matches;
- source precedence change after preview causes re-review or explicit diagnostic per implemented policy;
- no force overwrite action exists.

### Generated-content restrictions

- `allowed-tools`, script/resource package attempts, oversized body/frontmatter, malformed YAML, and invalid names rejected again at publication;
- exact validated content digest is the file written.

### Atomicity/reconciliation

- temp write/rename produces complete file;
- simulated persistence failure after rename can reconcile metadata from exact digest without rewriting;
- stale temp file does not become an effective skill;
- concurrent publish attempts for same destination serialize/fail deterministically.

### Refresh/snapshot semantics

- successful project publish triggers normal refresh and new subsequent-turn generation;
- active turn remains pinned to old generation;
- refresh failure keeps prior valid snapshot and published file;
- refresh retry does not republish file;
- effective skill/digest matches published proposal when no shadow exists;
- shadow/invalid diagnostics are surfaced if resolution differs.

### Foreign-root guard

Add a focused source-code/unit guard proving `SkillPublicationService` derives targets only from CodeGG-owned `SourceKind`/scope paths. Prefer a type-level closed enum over string comparisons.

## 16. Verification commands

Required after implementation:

```bash
cargo fmt --all -- --check
git diff --check
cargo test -p codegg-core habit --locked
cargo test --test skills_registry --locked
cargo test --test habit_skill_promotion --locked
```

Run existing asset-refresh tests and the focused publisher path/symlink tests using their exact targets.

Then:

```bash
scripts/verify.sh quick
```

No live provider call is required for publication verification.

## 17. Acceptance criteria

M003 may close only when:

1. Publication requires an explicit user control action tied to exact proposal ID/revision/digest.
2. The model cannot self-issue approval.
3. Only CodeGG-owned project/global skill roots are writable; foreign harness roots are rejected.
4. Destination is host-derived from validated scope/name and survives containment/symlink checks.
5. Proposal is revalidated under current parser/restriction rules immediately before write.
6. Generated scripts/resources/plugins/MCP/`allowed-tools` remain unsupported/rejected.
7. Existing different skill content is never silently overwritten; initial policy has no force overwrite.
8. File creation is atomic/durable and concurrency-safe.
9. Proposal/habit state records exact publication digest/path provenance and can reconcile a crash after file rename.
10. Successful publication invokes the existing asset refresh path rather than mutating runtime registries directly.
11. Active turns remain pinned; subsequent turns use the new generation after successful refresh.
12. Refresh failure retains both the published file and prior valid runtime snapshot with explicit diagnostics.
13. Post-refresh effective/shadowed state is reported truthfully.
14. Existing foreign-skill discovery/precedence and ordinary manually-authored skills remain compatible.
15. Focused tests and `scripts/verify.sh quick` pass.

## 18. Stop conditions

Stop and register a new plan/ADR if:

- publication requires making foreign harness roots writable by default;
- safe path handling requires a new cross-platform filesystem authority rather than existing containment helpers;
- asset refresh cannot consume a newly written CodeGG skill without a broad registry ownership rewrite;
- user approval cannot be represented separately from model/tool calls in the current frontend protocol;
- useful generated skills require scripts/executable resources for the first release;
- publication requires automatic overwrite/merge of existing user-authored skills.

## 19. Closure evidence required

Create `plans/closure/memory-skill-promotion/003-status.md` containing:

- exact implementation revision and M002 dependency revision;
- user-approval/stale-digest authorization evidence;
- writable-root/path/symlink/collision matrix;
- generated-content restriction tests;
- atomicity/reconciliation evidence;
- asset refresh generation/pinning/failure evidence;
- effective/precedence result evidence;
- focused and quick verification outputs;
- unresolved findings and final subsystem disposition.

If M001-M003 are all strictly closed, mark `plans/subsystems/memory-skill-promotion-roadmap.md` closed and update the active planning registry. Existing runtime-assets closure history remains immutable.
