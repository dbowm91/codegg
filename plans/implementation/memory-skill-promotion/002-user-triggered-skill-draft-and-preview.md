# Memory-to-Skill Promotion M002 — User-Triggered Skill Draft and Preview

Status: ready for handoff

Repository baseline: `1bee32578566cc6cdf4025002af781309d8f29f4`

Source subsystem roadmap:

- `plans/subsystems/memory-skill-promotion-roadmap.md`

Hard dependency:

- M001 `plans/implementation/memory-skill-promotion/001-habit-observation-and-candidate-store.md` must be strictly closed.

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Current authority:

- M001 habit records are structural evidence, not model instructions;
- `src/skills` / `AssetRegistry` remains the authoritative portable skill parser/validator;
- normal AgentLoop/provider context remains the preferred model execution path;
- M002 creates no effective skill file and invokes no asset refresh.

Primary class: capability / safety boundary

## 1. Objective

Let a user explicitly select a ready habit candidate and have the current agent turn it into one bounded portable `SKILL.md` proposal that can be validated, persisted, previewed, revised/rejected, and approved later. M002 stops before publication.

Target flow:

```text
ready HabitCandidate
      |
user explicitly requests promotion
      |
      v
bounded candidate evidence exposed to current agent
      |
      v
model drafts portable SKILL.md
      |
      v
host proposal submission service/tool
      |
      v
existing skill parser validation
      |
      v
SkillProposal { Draft | Validated | Rejected }
      |
      v
user preview / reject

NO `.codegg/skills` write in M002
```

The key safety property is that model participation begins only after user intent is explicit and ends at a non-effective proposal artifact.

## 2. Explicit non-goals

M002 must not:

- write any file into a runtime skill root;
- trigger `AssetRefreshCoordinator` because no effective asset changed;
- publish into `.agents`, `.opencode`, `.claude`, or `.codegg/skills`;
- create executable scripts/resources, plugins, MCP definitions, binaries, dependencies, or Tool Programs;
- let `allowed-tools` in a generated draft grant or imply permission;
- automatically draft a proposal when M001 marks a habit ready;
- add a background per-session model call;
- mark a candidate `Promoted`;
- overwrite or replace an existing skill;
- make an unvalidated draft visible to normal skill discovery;
- make proposal content ambient system-prompt context.

## 3. Re-inspection required before implementation

After M001 closes, inspect:

- M001 closure and exact `HabitStore`/candidate APIs;
- `src/skills/parser.rs`, candidate/effective types, resource inventory, name validation, size limits, and portable/native schema detection;
- `src/skills/registry.rs` for how candidate validation/diagnostics can be reused without writing a file;
- `src/tool/skill.rs` for current skill model-facing tool shape and compatibility risk of extending it;
- `src/tool/catalog.rs`/deferred tool behavior if a separate proposal-submission tool is appropriate;
- TUI command registry/prompt follow-up mechanisms for a user command that can expose a selected habit to the next agent turn;
- provider/request-context rules only if implementation chooses a dedicated model call instead of the preferred current-AgentLoop flow;
- project/global config-root resolution for target-scope metadata, without writing it yet.

Prefer extracting an in-memory skill validation seam from the existing parser rather than implementing a second parser in the proposal service.

## 4. User initiation contract

A proposal must have an explicit user initiation record or state transition.

Provide a discoverable operation equivalent to:

```text
/skill-promote <habit-id>
```

or a habit detail action that says “Draft skill”. Exact TUI syntax may follow current command conventions.

On initiation, the host:

1. loads the exact candidate by project/scope and ID;
2. requires status `Ready` (or an explicit user override from `Observing` only if separately designed; default reject);
3. captures candidate revision/fingerprint;
4. constructs a bounded `HabitPromotionContext`;
5. records a short-lived/session-scoped `PromotionDraftRequest` so a later model tool call can prove it corresponds to current user intent;
6. injects a bounded user/control message into the current agent turn asking it to draft a skill and submit it through the proposal tool/service.

Do not infer initiation from the model spontaneously calling the proposal tool after seeing `/habits` output.

The initiation token/request must be bounded, expire or be consumed after one successful proposal submission, and be scoped to the exact session + habit candidate revision.

## 5. Bounded promotion context

Expose only information useful for drafting reusable instructions:

```rust
struct HabitPromotionContext {
    habit_id: HabitId,
    workflow_fingerprint: String,
    action_skeleton: Vec<WorkflowAction>,
    successful_occurrences: u32,
    distinct_sessions: u32,
    related_memories: Vec<BoundedMemoryRef>,
    existing_skill_names: Vec<String>,
    target_scope_hint: SkillTargetScope,
}
```

`related_memories` should contain only explicitly selected or safely retrieved current `Memory` summaries/IDs. Do not copy entire historical sessions.

The host may include current project instruction context already available to the agent normally; do not duplicate the complete system prompt into a proposal record.

Existing skill names/descriptions can help avoid duplicates, but cap the list or use current catalog search rather than dumping the full registry.

## 6. Proposal submission surface

### 6.1 Preferred shape

Add a narrow model-facing host tool/service such as `skill_proposal` with a `submit` action, rather than giving the model filesystem-write authority for the final skill path.

Representative input:

```json
{
  "action": "submit",
  "promotion_request_id": "...",
  "habit_id": "...",
  "name": "rust-focused-verification",
  "description": "Run focused verification after Rust changes.",
  "skill_markdown": "---\nname: ...\n..."
}
```

The tool is proposal-state mutation only, not repository/workspace mutation. Categorize it consistently with other safe host state updates; it must not bypass the explicit initiation check.

If extending the existing `skill` tool is cleaner and backward-compatible, add an explicit action while preserving old load-by-name behavior. Do not silently reinterpret an existing call shape.

### 6.2 Submission authorization

Host validation must require:

- exact active promotion request ID;
- matching session/project/habit ID;
- habit fingerprint/revision still equals the value previewed at initiation;
- request not expired/consumed;
- one bounded proposal per initiation unless user explicitly asks for another revision.

A custom agent/subagent cannot submit a proposal for an unrelated habit just because it can call the tool. Subagent access should default denied unless the promotion request explicitly delegates drafting to that exact child and preserves the user-initiation provenance.

## 7. Restricted generated skill schema

M002 generated proposals support one portable `SKILL.md` only.

The host accepts:

- portable required frontmatter `name`, `description`;
- optional `license` only if user/project policy permits and it is a simple bounded string;
- optional safe `metadata` entries, preferably CodeGG provenance inserted by the host rather than invented by the model;
- Markdown body instructions.

The host rejects for generated proposals:

- `allowed-tools` until a later explicit authority-neutral design is accepted;
- embedded executable script/resource declarations or package side files;
- absolute or traversal paths;
- instructions to auto-install dependencies as an asset-side effect;
- malformed/oversized frontmatter/body;
- names invalid under the existing portable parser;
- duplicate/reserved built-in identifiers according to current skill registry rules.

The Markdown body is still untrusted instructions when later activated. Existing runtime tool permissions remain authoritative; M002 does not attempt to prove semantic safety of every sentence.

## 8. Reuse existing parser/validator

Extract or expose an in-memory validation entry point from `src/skills/parser.rs` if one does not exist, for example:

```rust
validate_skill_document(
    source: &str,
    limits: AssetDiscoveryConfig,
    origin: ProposalOrigin,
) -> Result<ValidatedSkillDocument, Diagnostic>
```

The exact API may differ. Requirements:

- one parsing implementation for discovered and proposed portable skills;
- same name/description/frontmatter/body size rules;
- generated-proposal-specific restriction layer applied after portable parse and before proposal becomes `Validated`;
- no need to create a temporary file solely to reuse parsing;
- no bundled-resource enumeration for proposal documents because M002 does not accept package resources.

Add drift tests proving a document accepted/rejected in proposal mode gets the same base portable parse result as the ordinary registry parser, with only the intentional generated-skill restrictions differing.

## 9. Skill proposal domain/store

Add bounded proposal state, preferably near the habit store rather than the runtime skill registry:

```rust
enum SkillProposalStatus {
    Draft,
    Validated,
    Rejected,
    Published,   // reserved for M003 host transition
    Superseded,
}

struct SkillProposal {
    id: SkillProposalId,
    habit_id: HabitId,
    habit_fingerprint: String,
    candidate_revision: u64,
    project_namespace: String,
    target_scope: SkillTargetScope,
    name: String,
    description: String,
    skill_markdown: String,
    content_digest: String,
    status: SkillProposalStatus,
    diagnostics: Vec<BoundedDiagnostic>,
    created_at: i64,
    updated_at: i64,
    revision: u64,
}
```

Target scope is metadata only in M002 (`Project` default; `Global` only if the user explicitly selects it). It is not a model-provided filesystem path.

Use a file-backed atomic/locked store consistent with M001. Proposal body size must be <= the existing accepted skill file bound; proposal count/file size also needs a hard cap.

Store validated content exactly as previewed, plus its digest/revision, so M003 can reject stale approval if anything changes.

## 10. Host-inserted provenance

The host may add safe namespaced portable metadata at validation/finalization time, for example:

```yaml
metadata:
  codegg:
    origin: habit-promotion
    habit_fingerprint: <bounded digest>
```

Only do this if the existing portable metadata schema/parser preserves nested values safely and deterministically. Otherwise keep provenance solely in the proposal store and avoid modifying model content invisibly.

Never include raw session IDs, prompts, command text, secret values, or user-identifying information in the eventual portable skill merely for provenance.

## 11. Preview and revision behavior

User-facing operations should support:

```text
/skill-proposals
/skill-proposal <id>
/skill-proposal reject <id>
```

and one explicit way to request a new draft/revision from the same habit.

Preview must show:

- proposed name/description;
- complete bounded `SKILL.md` body;
- target scope (`project`/`global`), not raw arbitrary path;
- validation diagnostics;
- habit evidence summary/counts/fingerprint;
- content digest/revision;
- explicit statement that the proposal is not installed/effective yet.

A revision creates a new proposal revision or supersedes the prior draft through revision-checked store operations. Do not mutate content after the user has selected it for M003 approval without changing digest/revision.

Rejecting a proposal does not automatically dismiss the underlying habit unless the user chooses that separately.

## 12. Existing-skill collision awareness

M002 should warn before publication if a validated proposal's normalized name already resolves to an existing skill in the current `AssetRegistry`.

This is advisory in M002 because no file is written. Record diagnostics such as:

```text
existing effective skill with same name
source kind/path
whether proposed project CodeGG root would have higher/lower precedence
```

Do not tell the model to choose a foreign directory to avoid the collision.

M003 decides publication conflict behavior; the initial policy will fail closed rather than overwrite a different CodeGG-owned skill.

## 13. Model/provider execution choice

Preferred implementation uses the already-running current agent to draft and call the proposal submission tool. Benefits:

- no new direct `Provider::stream()` caller;
- existing provider request/session context and model adaptation are reused;
- user can conversationally steer the draft;
- proposal remains typed host state.

If implementation instead adds a dedicated drafting model call, requirements are:

- initiated only by explicit user action;
- one stable owning session/provider request context, consistent with Provider M009;
- strict bounded response parsing with one repair at most;
- no background periodic drafting;
- same host proposal validation/store and no direct file write.

Document the selected path in architecture and closure evidence.

## 14. Expected production-code touch set

Expected areas:

- M001 habit domain/store;
- new proposal domain/store near `crates/codegg-core/src/memory` or a narrowly named promotion module;
- `src/skills/parser.rs` in-memory validation extraction;
- generated-proposal restriction validator;
- `src/tool/skill_proposal.rs` or backward-compatible `src/tool/skill.rs` extension;
- tool registry/factory/catalog metadata as needed;
- TUI command/prompt-follow-up path for explicit promotion initiation and preview/reject;
- optional bounded projection DTO for proposal summaries;
- `architecture/memory.md`, `architecture/skills.md`, `architecture/tool.md` if a new tool is added.

Do not add a skill filesystem publisher or asset refresh invocation in M002.

## 15. Required tests

### Explicit initiation

- model/tool submission without an active user promotion request is denied;
- wrong session/project/habit request ID is denied;
- stale habit fingerprint/revision is denied and asks user to re-review;
- one accepted initiation cannot create unlimited proposals;
- expired/consumed request cannot be replayed.

### Parser/restriction consistency

- valid portable name/description/body accepted;
- malformed/oversized frontmatter/body rejected identically to registry parser;
- `allowed-tools` rejected for generated proposal mode;
- attempts to include scripts/resources/package paths rejected;
- proposal content cannot escape size bounds;
- parser and proposal validator share the same base parsing implementation.

### Proposal store

- atomic/locked round trip;
- digest/revision deterministic;
- changed content changes digest/revision;
- rejection/supersession lifecycle is monotonic/validated;
- project scope and target scope are host enums, not arbitrary paths;
- malformed/oversized persisted proposal fails safely.

### Privacy/provenance

- proposal evidence context contains action skeleton/counts and selected memory summaries only;
- raw historical commands/tool outputs/hidden reasoning absent;
- host provenance metadata, if emitted, contains no raw session/user data.

### Runtime non-effect

- creating/validating/rejecting a proposal does not modify any discovered skill directory;
- `AssetRegistry` effective skill set/generation remains unchanged;
- current turn prompt/skill activation is unchanged except for the explicit drafting interaction.

### Collision diagnostics

- existing same-name skill produces diagnostic with source provenance;
- no overwrite or shadow manipulation occurs in M002.

## 16. Verification commands

Required after implementation:

```bash
cargo fmt --all -- --check
git diff --check
cargo test -p codegg-core habit --locked
cargo test --test skills_registry --locked
cargo test --test habit_skill_promotion --locked
```

Run the focused tool/TUI tests introduced for promotion initiation/proposal submission.

Then:

```bash
scripts/verify.sh quick
```

No live provider call is required; if drafting is exercised in an integration test, use the repository's mock/capture provider.

## 17. Acceptance criteria

M002 may close only when:

1. Proposal drafting cannot begin automatically when a habit becomes ready.
2. An explicit user promotion request is scoped to one session/project/habit candidate revision.
3. The current agent or dedicated user-triggered drafter receives only bounded habit evidence and selected current memory summaries.
4. Model output must pass a host proposal submission/validation boundary; it cannot write the runtime skill path.
5. Portable parsing is reused from the existing skill subsystem rather than duplicated.
6. Generated proposals are limited to one `SKILL.md`; scripts/resources/plugins/MCP and `allowed-tools` are rejected in the initial feature.
7. Proposal name/description/body/diagnostics are bounded and digest/revision tracked.
8. Proposal store is atomic, locked, path-safe, and restart-safe.
9. User can preview complete proposed content, validation state, scope, provenance summary, and collision diagnostics.
10. Reject/revise operations are explicit and do not mutate the underlying habit unless separately requested.
11. No skill file is written and no runtime asset refresh/effective skill generation changes in M002.
12. Existing skill discovery/activation and memory behavior remain compatible.
13. Focused tests and `scripts/verify.sh quick` pass.

## 18. Stop conditions

Stop and register a follow-up if:

- safe proposal validation requires writing temporary files into live skill roots;
- the existing parser cannot be reused without a broad skills-registry rewrite;
- proposal drafting requires giving a model arbitrary filesystem write authority;
- the product cannot enforce explicit user initiation at the model/tool boundary;
- supporting revisions requires an unbounded transcript/proposal history store;
- foreign harness directories must be written to make the feature useful.

## 19. Closure evidence required

Create `plans/closure/memory-skill-promotion/002-status.md` with:

- implementation revision and M001 dependency revision;
- explicit-initiation authorization tests;
- parser/restriction drift matrix;
- proposal store/digest/revision evidence;
- proof that no runtime skill root changed during proposal flow;
- collision/privacy/provenance evidence;
- focused and quick verification output;
- unresolved findings and recommendation.

Only accepted M002 closure moves M003 to `ready`.
