# Post-Audit Maintainability and Surface Milestone 002 — Model-Visible Tool-Surface Minimization

Status: blocked

Repository baseline: `15632a0483a8c4b9d573ff2ce43297b29be8f42a`

Source roadmap:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`

Hard dependency:

- M001 — `plans/implementation/post-audit-maintainability-surface/001-compatibility-surface-rationalization.md`

Long-term requirements:

- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Primary class: capability / polish

## 1. Objective

Reduce the number of overlapping tools advertised to a model on every turn while preserving the full registered capability set through CodeGG's existing profile, policy, deferred-loading, catalog, and `tool_search` mechanisms.

The goal is lower tool-selection entropy and clearer canonical intent, especially for models that are competent coders but less reliable at distinguishing near-synonymous tools. This is a progressive-disclosure change, not a capability-removal or routing-framework project.

## 2. Why this milestone is blocked

M001 must first establish the canonical names and disposition literal compatibility aliases. Tool disclosure cannot be stabilized while the repository still has unresolved canonical-versus-alias decisions.

Once M001 closes, all required mechanisms already exist:

- `ToolRegistry::with_options` is the authoritative registration sequence;
- `Tool::defer_loading()` supports tools omitted from the initial definitions;
- `Tool::expose_in_definitions()` supports hidden diagnostic/internal tools;
- model profiles and tool-surface policy already filter/rename/shape definitions;
- `tool_search` exposes deferred capability discovery;
- the catalog/broker own tool metadata and invocation contracts.

No new architecture decision is required after the canonical-name dependency closes.

## 3. Current implementation evidence

The default registry contains broadly useful coding tools and several specialized/research/evidence tools. Known adjacent surfaces include:

- `websearch` and `webfetch`;
- canonical repository search plus the compatibility alias dispositioned by M001;
- `research` and `research_search`;
- `batch_fetch`;
- evidence-oriented wrappers/builders;
- security-oriented search/evidence helpers;
- LSP, repo map/fetch/search, deterministic tools, skill/tool search, and other specialist capabilities.

These tools may be semantically distinct internally, but simultaneous advertisement increases the number of choices the model must make before it can act. The repository already has a dedicated `architecture/agent-tool-surface.md` and profile/policy machinery, so the correct intervention point is definition disclosure rather than backend consolidation.

`ToolRegistryOptions` also contains runtime capability information. The implementation must distinguish:

1. registered and callable;
2. model-advertised immediately;
3. deferred but discoverable;
4. profile/agent-policy prohibited;
5. unavailable because the runtime backend is nonfunctional.

Those states must not be conflated.

## 4. Invariants that must not regress

- Registration remains owned by `ToolRegistry::with_options`; no second registry/router is created.
- Invocation remains owned by the existing broker/tool contracts and scheduler boundaries.
- A tool hidden from initial definitions does not gain authority when discovered later.
- `tool_search` may reveal only tools the current agent/session policy is allowed to know/use.
- Plan mode remains read-only except for its existing permitted planning/state operations.
- Child-agent authority remains the intersection of parent, agent definition, session/project/workspace, broker, and tool policies.
- Tool Program callability is determined by contracts/authority, not by whether a tool is initially advertised to the LLM.
- Runtime-unavailable tools are not advertised as merely deferred.
- Specialist agents may have a broader or different visible palette than the core coding profile when their role requires it.
- No capability is deleted solely to reduce the initial definition count.

## 5. Scope

### In scope

- Define a small canonical core palette for ordinary coding turns.
- Classify every currently model-exposed built-in as `core`, `profile-specific`, `deferred-discoverable`, or `hidden/internal`.
- Make research/evidence/specialist tools the first high-value candidates for deferred/profile-specific disclosure.
- Ensure deferred tools remain searchable with useful descriptions, effect/risk/category metadata, and canonical names.
- Reconcile plan-mode and specialized built-in agent palettes.
- Add contract tests asserting visibility/discoverability/invocability rather than brittle raw count tests alone.
- Update prompt/tool-surface documentation.

### Explicitly out of scope

- Merging research implementations merely because their names are adjacent.
- Replacing `tool_search` with semantic retrieval, embeddings, another router, or automatic tool chaining.
- Changing backend ownership.
- Removing specialist capabilities from CodeGG.
- Per-model online telemetry/adaptive experimentation infrastructure.
- Prompt-token benchmarking infrastructure or a hard numerical tool-count gate.
- Reworking Tool Programs.

## 6. Required production changes

### Core/domain

Create one explicit classification source derived from existing tool metadata/profile configuration, not a second independent list of tools. The exact mechanism may be a small disclosure enum/metadata field or profile defaults if that produces one authoritative representation.

Recommended conceptual states:

```rust
Core
Deferred
ProfileSpecific
Hidden
```

Do not require this exact enum if existing `defer_loading`, exposure, and profile policy can express the states without duplication.

The ordinary coding profile should expose direct primitives needed for the common edit loop: inspect files/repo, edit/write/apply patch, controlled shell/test/Git/task/delegation as appropriate, plus `tool_search`. Specialist synthesis/evidence/fetch variants should generally be discoverable unless they are demonstrably common enough to justify immediate exposure.

### Storage and migrations

No storage migration expected. If profile configuration serializes literal tool names, preserve M001's compatibility reader decisions and canonicalize new writes where appropriate.

### Protocol and DTOs

No protocol change is required. Tool-definition payloads may naturally contain fewer initial entries. ACP/native clients that display tool catalogs must still be able to request/represent the allowed catalog where existing contracts provide it.

### Runtime and concurrency

Deferred discovery must not construct duplicate tool instances or bootstrap backends a second time. Discovery reads metadata from the existing registry/catalog and invocation uses the already registered canonical implementation.

### Frontend or operator surface

`/tool-backends`, diagnostics, and equivalent operator views should distinguish registered capability from currently advertised model definitions if they expose both concepts today. Do not hide capabilities from operators merely because they are deferred from the model prompt.

### Security and authorization

The key property is monotonic authority: discovery can reveal or load a definition only within the already accepted policy set. It cannot turn a prohibited tool into a callable one.

Tool descriptions returned by discovery must not leak secret configuration, endpoints, credentials, hidden reasoning, or unauthorized plugin names.

### Documentation and static guards

Update `architecture/agent-tool-surface.md`, `architecture/tool.md`, prompt/profile documentation, and relevant built-in-agent docs. Prefer behavioral tests over a static source guard.

## 7. Ordered work packages

### Work package A — Measure and classify the current visible surface

Intent: establish the actual prompt surface after M001.

Required actions:

- enumerate default visible definitions for ordinary coding, plan mode, research/security agents, and any other built-in profiles that materially differ;
- note tools already deferred/hidden;
- classify each exposed tool by frequency/necessity and semantic overlap;
- identify capability dependencies (for example, whether a research agent truly needs both primitive search and synthesis tools immediately).

Acceptance evidence:

- a concise classification table;
- canonical names align with M001 closure.

### Work package B — Establish core versus deferred disclosure

Intent: reduce ordinary-turn choice count without capability loss.

Required changes:

- use existing metadata/profile mechanisms to mark specialist tools deferred/profile-specific;
- ensure `tool_search` itself is core where deferred capability exists;
- ensure direct primitives needed to discover/read context do not become recursively hidden.

Acceptance evidence:

- ordinary definition set is materially smaller than baseline;
- no supported capability was unregistered as part of this package.

### Work package C — Make discovery sufficient for correct selection

Intent: ensure a deferred tool is practically usable, not theoretically registered.

Required changes:

- `tool_search` results identify canonical name, purpose, relevant effect/risk/callability information, and enough distinction among related research/evidence tools;
- discovered definition can be supplied/invoked through the existing turn/runtime path;
- avoid returning a huge unfiltered catalog that simply moves prompt bloat to the search result.

Acceptance evidence:

- integration test: a deferred tool is absent initially, found via search, and invoked under allowed policy;
- denied tool remains undiscoverable or non-callable according to the accepted policy contract.

### Work package D — Reconcile specialized profiles and plan mode

Intent: avoid applying the ordinary coding palette indiscriminately.

Required changes:

- research/security specialized agents receive the small role-appropriate palette they need;
- plan mode continues to expose only its allowed inspection/planning surface;
- model-profile overrides continue to work on top of defaults.

Acceptance evidence:

- profile-specific visibility tests;
- plan-mode mutation denial tests remain green.

### Work package E — Documentation and cleanup

Intent: make the distinction between capability registration and prompt exposure explicit.

Required changes:

- update architecture docs and stale prompts/comments;
- remove now-redundant visibility lists if one canonical metadata source can replace them;
- avoid new global count constants.

Acceptance evidence: docs can answer “registered?”, “advertised now?”, “discoverable?”, and “callable?” distinctly.

## 8. Failure, cancellation, restart, and contention semantics

Tool discovery is metadata work and should not spawn independent long-running backend work. A failed search/discovery operation leaves the turn's existing allowed tool surface intact.

A model that never discovers a deferred specialist tool should still be able to complete ordinary coding work using core primitives. Discovery state should not need durable persistence across daemon restart unless an existing tool-definition cache already provides it; rebuilding from the registered catalog is acceptable.

No cancellation semantics change for actual tool invocation. Once invoked, the canonical tool/broker/scheduler path owns cancellation exactly as before.

## 9. Compatibility and migration

M001 owns literal-name migration. M002 preserves its outcome.

Existing explicit profile configurations that request a canonical tool should continue to make it visible/callable according to policy. If a configuration previously depended on “all registered tools are always advertised,” preserve a bounded opt-in/full-palette mode only if current config already provides that contract; do not invent a permanent compatibility switch without evidence.

No stored transcript rewrite is required. Historical tool calls remain historical names/data.

## 10. Required tests

### Focused unit tests

- disclosure classification for representative core/deferred/hidden/profile-specific tools;
- `definitions()` excludes deferred but registered tools;
- catalog retains deferred metadata;
- policy filtering applies before/at discovery.

### Integration tests

- ordinary session: deferred tool absent initially → `tool_search` finds it → allowed invocation succeeds through canonical broker;
- specialized research/security profile receives intended direct tools;
- explicit profile visibility override behaves as documented;
- runtime-nonfunctional backend is not misleadingly exposed.

### Restart and recovery tests

No durable state expected. A fresh registry after restart must deterministically produce the same classifications from the same config/profile.

### Contention and cancellation tests

No new contention contract. Existing invoked-tool cancellation tests remain sufficient.

### Security and negative tests

- prohibited tool cannot be made callable via discovery;
- hidden/internal tools remain absent from model discovery;
- plan mode cannot discover/invoke a mutating tool outside its accepted exceptions;
- discovery output contains no secret backend configuration.

### Migration and compatibility tests

- explicit profile/config references use M001 canonical-name compatibility behavior.

## 11. Required verification commands

After M001 is closed:

```bash
# tool surface / catalog / profile focused tests
cargo test -p codegg agent::tool_surface
cargo test -p codegg tool::tool_search
cargo test -p codegg tool::catalog
cargo test -p codegg model_profile
cargo test -p codegg permission

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Use current exact test names/modules after inspecting repository head; do not add empty tests solely to match these suggested selectors.

## 12. Documentation updates

- `architecture/agent-tool-surface.md` — canonical disclosure states and profile behavior.
- `architecture/tool.md` — registration versus definitions versus discovery versus invocation.
- `architecture/permission.md` — confirm discovery does not widen authority.
- Built-in agent docs/prompts where the role palette changes.
- `AGENTS.md` if its approximate default-tool-count assumptions become stale.

## 13. Acceptance criteria

- M001 canonical names are consumed without reopening compatibility decisions.
- Ordinary coding turns advertise a materially smaller, intentionally selected core tool set.
- Specialist capabilities remain registered and accessible through `tool_search` or role-specific profiles.
- Discovery does not widen permission/authority.
- Plan mode and child-agent restrictions remain intact.
- Tool Programs retain contract-based callability independent of prompt disclosure.
- No second tool registry/router, semantic-routing service, or telemetry framework is introduced.
- Documentation clearly distinguishes registered, advertised, discoverable, and callable states.

## 14. Stop conditions

The agent must stop and report when:

- M001 is not closed or canonical names remain disputed;
- a needed capability can only remain usable by inventing a second routing system;
- changing visibility unexpectedly changes broker authority or Tool Program contracts;
- an external protocol requires every registered tool definition to be advertised eagerly and no compatible negotiation exists;
- specialist profiles are not sufficiently defined to distinguish their intended palettes;
- repository head has materially changed the registry/disclosure architecture.

## 15. Closure evidence required

- implementation commits/PRs;
- before/after default visible tool inventory by relevant profile/mode;
- classification rationale for tools moved out of the default prompt;
- proof that moved tools remain registered;
- successful deferred discovery-and-invocation integration test;
- policy-negative tests;
- plan/profile behavior evidence;
- formatting/lint/quick verification outcomes;
- documentation updates;
- unresolved tools intentionally kept core and why.

## 16. Handoff notes

Do not optimize for the lowest possible tool count. The objective is to remove ambiguity from the common path while keeping direct primitives readily available.

Research is the best initial candidate because CodeGG has both primitive and higher-level evidence/synthesis capabilities. Preserve those distinctions where useful; change when they are advertised, not necessarily what they do.
