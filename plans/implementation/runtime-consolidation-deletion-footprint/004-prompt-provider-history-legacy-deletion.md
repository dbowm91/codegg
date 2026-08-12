# Runtime Consolidation, Deletion, and Footprint M004 — Prompt, Provider-Compatibility, and History Legacy Deletion

Status: implemented

Source roadmap:

- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Relevant references:

- `plans/000-long-term-specification.md`
- `plans/003-planning-process.md`
- `architecture/agent.md`
- runtime-assets roadmap/closure records
- agent-runtime model-adaptation/ACP roadmap and closure records

Repository baseline reviewed: `bd9b3b610af0fa72ce3fe5a8b8f59222659f006d`

Primary class: simplification / correctness preservation

Dependencies:

- hard: none;
- interface: `PromptCompiler`, `ProjectAssetSnapshot`, `RuntimeAssetPin`, model adapter/profile registry, `ContextPlan`;
- soft: M003 benefits from this deletion landing before or during loop decomposition.

Target closure record:

- `plans/closure/runtime-consolidation-deletion-footprint/004-status.md`

## 1. Objective

Remove superseded prompt/instruction/history compatibility implementations now that the canonical PromptCompiler/runtime-asset/model-adapter architecture is established.

The target is one production prompt compiler and explicit provider wire projection. Compatibility may alter provider-facing representation, but canonical prompt/history state must not keep alternative owners solely for historical callers that no longer exist.

## 2. Current implementation evidence

Inspect at minimum:

- `src/agent/prompt.rs`;
- `src/agent/instructions.rs`;
- `src/agent/asset_*`;
- model-profile/model-adapter modules;
- provider request construction and history hardening in `src/agent/loop.rs` or its M003 successor;
- tests and external/public Rust callers for deprecated prompt helpers.

Known baseline legacy surfaces include:

- deprecated `find_instructions_file()` and `find_all_instruction_files()` using process-global current directory;
- deprecated `load_agent_prompt()` / `load_agent_prompt_async()` compatibility paths;
- `compile_legacy_prompt()`;
- remote instruction fetching from prompt assembly compatibility code despite the runtime-assets architecture requiring refresh ownership;
- `select_provider_prompt()` and provider/model-name prompt selection beside declarative model adapter/profile behavior;
- wrappers that compile a prompt then append custom instructions outside the typed block pipeline;
- canonical-history repair that synthesizes missing tool results or preserves malformed provider sequences instead of projecting provider-specific compatibility at the wire boundary where feasible.

## 3. Explicit non-goals

Do not:

- redesign the PromptCompiler block schema;
- change project instruction precedence without explicit evidence;
- fetch remote instructions synchronously during active turn compilation;
- add new giant provider-specific prompt templates;
- remove a public supported Rust API without caller/compatibility evidence;
- rewrite provider clients broadly;
- weaken immutable runtime-asset snapshot/pin behavior;
- introduce process-global CWD fallback to preserve a deprecated helper;
- change model-visible semantics simply to reduce prompt size unless current text is demonstrably redundant and tests cover the behavior.

## 4. Invariants that cannot regress

- active daemon turns use explicit execution/workspace context;
- runtime assets are resolved/published before turn compilation and pinned for the turn;
- PromptCompiler is the single production system-prompt compiler;
- compiler output remains deterministic for equivalent typed inputs;
- model adapter/profile configuration may specialize behavior but must not create a second prompt assembly pipeline;
- remote instruction bodies do not bypass refresh/snapshot ownership;
- child/root turns retain the same authority and compiler contract;
- provider compatibility does not become canonical durable state.

## 5. Ordered work packages

### A. Production caller audit

For each legacy/deprecated helper and provider prompt template selector:

1. enumerate Rust callers, tests, examples, docs, and any exported/public API usage;
2. classify as production, compatibility with a named supported caller, test-only, or dead;
3. identify the canonical replacement path;
4. delete dead/test-only compatibility code rather than marking it deprecated again.

A repository search result showing only definitions/plans/docs is evidence for deletion but must be confirmed against compilation/public API exposure.

### B. Delete process-CWD prompt authority

Remove active compatibility paths that search `std::env::current_dir()` for project instructions.

If a source-compatible helper must remain temporarily:

- it must be clearly outside daemon production;
- it must not be used by factory/turn runtime;
- tests must prove production prompt assembly requires explicit `AssetContext`/snapshot;
- closure evidence must name the external/embedded caller requiring it and a deletion condition.

Prefer deleting the helper over adding more static guards around it.

### C. Consolidate remote instruction ownership

Remote instructions/configured URLs must be resolved by the bounded runtime-asset refresh owner before becoming effective prompt content.

Delete prompt-assembly-time network fetching and placeholder insertion where no supported compatibility caller requires it.

Do not add another downloader. Reuse current asset-refresh security/bounds semantics.

### D. Remove alternative provider prompt ownership

Audit `select_provider_prompt()` and bundled provider prompt templates.

For each template:

- if model adapter/profile blocks fully supersede it, delete it and references;
- if a template remains required for a supported adapter, make the adapter the explicit owner and avoid generic model-name checks in prompt compilation;
- ensure model identification comes through resolved adapter/profile rather than scattered string-prefix checks.

The goal is small adapter deltas around invariant harness semantics, not one large prompt per provider family.

### E. Eliminate post-compile system-string mutation

Audit helpers that append custom instructions or control text after `PromptCompiler::compile()`.

Production prompt content should enter as typed `PromptBlock`/runtime input before compilation so:

- fingerprint identity is truthful;
- ordering/cache classification is deterministic;
- context planning does not observe a different prompt than the provider receives.

Delete or adapt post-compile mutation paths accordingly.

### F. Move history compatibility to provider projection where safe

Audit `harden_history()` and related canonical message mutation.

For provider requirements such as missing-tool-result pairing:

1. determine whether canonical stored/turn history is malformed because of a real internal bug or merely unacceptable to a specific provider wire grammar;
2. real internal invariant violations should fail/repair at the owning write boundary with tests;
3. provider-only requirements should be projected at request serialization/adapter time without rewriting canonical history when feasible;
4. do not fabricate durable facts that a tool completed when the result is actually unknown.

If a canonical repair remains necessary, document the invariant and keep it provider-independent.

### G. Tests and docs

Add/adjust tests proving:

- production prompt compilation from the same typed snapshot/profile/tools is deterministic;
- prompt fingerprint includes all effective system content and no post-compile append changes provider text invisibly;
- process-global CWD changes cannot change an already-bound production turn prompt;
- remote instructions become effective only through asset refresh/snapshot publication;
- root and descendant turns use the same compiler contract;
- adapter specialization does not require generic provider-name prompt branching;
- provider history projection satisfies strict wire pairing without corrupting canonical history where the adapter approach is adopted.

Update `architecture/agent.md` and runtime-assets/model-adapter docs to describe only the retained path.

## 6. Compatibility, protocol, storage, migration

Compatibility:

- internal deprecated functions may be deleted after caller audit;
- public crate API removals require explicit evidence that Codegg does not promise that API or a narrow deprecation window;
- model-visible prompt semantics should remain stable unless duplicate/conflicting instructions are intentionally removed and regression tests cover the resulting harness behavior.

Protocol:

- no ACP/native/provider wire protocol change intended;
- provider request projection may change internally while preserving externally valid messages.

Storage:

- no schema change.

Migration:

- no user action;
- remove dead bundled prompt assets/files when no adapter references them.

## 7. Security and correctness review

Verify that deletion does not:

- permit remote instructions to bypass bounded refresh/security policy;
- reintroduce path authority through CWD;
- allow child agents to receive a broader prompt/tool authority than parent policy permits;
- allow provider adapter content to silently bypass prompt fingerprint/context-plan identity;
- leak local absolute paths or secrets into adapter diagnostics.

## 8. Verification

Expected focused commands:

```bash
cargo test -p codegg --lib agent::prompt -- --nocapture
cargo test -p codegg --lib agent::instructions -- --nocapture
cargo test -p codegg --lib agent::asset_snapshot -- --nocapture
cargo test --test agent_loop_harness -- --test-threads=1
scripts/verify.sh quick
git diff --check
```

Run model-adapter/runtime-assets focused tests appropriate to modified files. Broad hosted verification is reserved for M007.

## 9. Explicit acceptance criteria

M004 is complete only when:

1. PromptCompiler/runtime assets are the sole production prompt compilation path.
2. No production turn prompt is discovered from process-global CWD.
3. Remote instruction fetching does not occur inside prompt compilation/turn assembly.
4. Effective prompt content is represented before compiler fingerprinting; unexplained post-compile system-string mutation is absent.
5. Generic prompt code no longer owns provider/model-name-specific giant prompt selection when the model adapter can own the specialization.
6. Every retained legacy/deprecated prompt helper has a named supported caller and explicit deletion condition; otherwise it is removed.
7. Dead provider prompt assets are removed when no adapter/caller references them.
8. Provider-only history grammar repair is moved to adapter/request projection where feasible; canonical history is not mutated merely for wire compatibility.
9. Any remaining canonical history repair corresponds to a provider-independent invariant and is covered by tests.
10. Runtime-asset snapshot/pin and child/root compiler semantics remain unchanged.
11. Focused prompt/asset/loop tests and `scripts/verify.sh quick` pass.
12. Architecture documentation contains no stale claim that deprecated CWD/legacy prompt loaders are production paths.
13. No new network fetcher, prompt framework, CI lane, or static source guard is added.

## 10. Stop conditions

Stop and document a blocker before deleting a surface if a supported external embedding/API relies on it and compatibility cannot be preserved with a thin adapter. Do not silently break public consumers for cleanup aesthetics.
