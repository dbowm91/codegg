# Tool Programs Milestone 006 — Read-Only Programmable Tool Palette

Status: blocked pending Milestone 005 closure

Repository baseline: `2f715941516a1d49be578fdef56714ad3ddfe8bf` (`main`)

Source roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-6--read-only-programmable-tool-palette`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/001-terminology-and-domain-model.md` — execution context, run, artifact

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Primary class: capability

## 1. Objective

Expose the foreground `tool_program` model tool and migrate a conservative read-only/safe-repeat palette to structured program-callable contracts. Programs must support useful multi-read, search, filtering, aggregation, and validation workflows while keeping intermediate output artifact-backed and outside the parent model transcript by default.

## 2. Readiness boundary

Hard dependency: M005 closure. The runtime must already prove bounded termination, cancellation, replay, and fault convergence using fixture tools.

## 3. Current implementation evidence

- ToolRegistry contains read, glob, grep, list, deterministic helpers, LSP, repository search/map, Git, context retrieval, web/research, and many mutation tools.
- Most tools return model-oriented strings and lack output schemas.
- `context_read` and artifact stores provide partial expansion seams, but not every tool result is content-addressed.
- Context-cache work distinguishes stable/slow/volatile blocks, yet live semantic reduction and transcript mutation remain limited.
- Tool search/deferred loading can reduce model-facing definitions, but a running program must use a frozen manifest and cannot discover new tools.

## 4. Invariants that must not regress

- Only tools with explicit output schema, caller policy, safe effect class, implementation version, and tested typed execution may be program-callable.
- Program capability manifests are frozen before execution.
- Current authority/path policy is revalidated on every call, cache hit, and replay.
- Raw file/tool output remains available through artifacts; compact projections never replace evidence.
- Program calls cannot mutate files, Git state, process state, todo/goal state, permissions, sessions, agents, or external systems.
- Web/research tools remain direct-only until citation/source-preservation contracts are independently proven.
- Tool Program prompts do not encourage using programs when semantic judgment or approval is required.

## 5. Scope

### In scope

Initial candidates, subject to contract review:

- `read`, `glob`, `grep`, `list`;
- deterministic/eggsact operations;
- safe read-only LSP queries;
- repository map/search and code search with bounded structured hits;
- Git read operations such as status, diff, log, show, and branch metadata through safe subcommands;
- `context_read` or equivalent artifact expansion with strict bounds;
- cached read-only metadata calls.

Also in scope:

- model-facing `tool_program` foreground schema;
- manifest selection and result schema declaration;
- structured output migration for the selected palette;
- read-only call caching;
- context projection and prompt contracts;
- direct-versus-programmatic evaluation fixtures.

### Explicitly out of scope

- write/edit/replace/apply_patch, Bash/terminal, Git mutation, commit/push, Python transform, subagent/task, question, permission, goal/todo, web/research, and any tool with external side effects.
- Background execution.
- Build/test child jobs.
- Hosted provider programs.

## 6. Required production changes

### Structured tool adapters

For each selected tool:

- define a stable JSON output schema independent of display prose;
- implement `execute_value` or equivalent typed broker path;
- include artifact handles for large bodies;
- record implementation/version and cache identity;
- mark `DirectOrProgrammatic` only after focused parity and negative tests;
- retain current direct-call display through a renderer over the typed value.

Example read result fields should include path identity, byte/range metadata, content or artifact handle, digest/revision context, truncation, and diagnostics. Search results should be bounded arrays with path, line/range, snippet or artifact, score/source, and truncation metadata.

### `tool_program` tool

Expose a normal function tool with bounded fields:

- `language` fixed to restricted Python v1;
- `source` or immutable source reference;
- declared final `result_schema` with bounded schema complexity;
- requested tool names or capability profile;
- optional narrower limits;
- execution fixed to foreground/await in this milestone;
- intent/description.

The tool submits one program through the program service and waits for terminal/incomplete completion. It returns one compact structured projection with program/job/run/artifact handles, budget usage, selected final value, and recovery information.

### Manifest resolution

- Resolve requested tools against Tool Broker contracts and effective authority.
- Reject unknown, direct-only, unsafe, schema-less, versionless, or denied tools before job creation.
- Include exact contract hashes and current implementation versions.
- Do not allow program source to request tool search or expand its manifest.

### Cache and workspace identity

Cache only explicit read-only/safe-repeat typed results. Cache key must include:

- tool implementation/version and contract hash;
- normalized arguments;
- project/workspace identity;
- Git revision where available;
- dirty-overlay or relevant file digest identity;
- capability/authority-relevant policy digest;
- output schema version.

Cache hits still pass authorization, path policy, result bounds, artifact availability, and contract checks. Define bounded TTL/size/eviction and do not cache errors by default.

### Context and projection

- Keep nested call outputs in program artifacts/call ledger.
- Parent transcript receives only final result projection, explicitly promoted evidence, and handles.
- Compact successful outputs first; preserve failures and source/test evidence more fully.
- Track input/output tokens, nested-call count, cache hits, artifact bytes, and transcript bytes avoided.
- Ensure shared session/workspace artifact access; do not create isolated unexpandable pseudo-handles.

### Prompt and agent guidance

Update stable prompt contracts to state:

- use programs for predictable bounded control flow, parallel reads, deterministic filtering/aggregation, and mechanical dependent arguments;
- use direct tools for semantic judgment, approvals, mutation, final source/citation validation, and uncertain next steps;
- declare finite stopping condition and result schema;
- handle incomplete results by narrowing or continuing explicitly;
- never poll manually for work the program runtime already awaits.

Keep volatile manifest/tool names outside stable provider-cache prefixes where possible.

## 7. Ordered work packages

### Work package A — Select and migrate the first palette

- Audit each candidate for actual read-only behavior, path policy, output stability, and hidden side effects.
- Define schemas and typed adapters.
- Leave questionable tools direct-only with recorded reasons.

### Work package B — Foreground submission tool

- Add schema, manifest resolution, submission identity, scheduler wait, result projection, and permission/error handling.
- Prevent source/result schema/palette payload amplification.

### Work package C — Artifact and context integration

- Store raw/intermediate output once.
- Add shared expandable handles and bounded final projection.
- Verify parent transcript excludes nested outputs unless promoted.

### Work package D — Read-only cache

- Add content/policy-aware keys, bounded storage, invalidation, metrics, and negative tests for stale dirty workspaces.
- Verify replay ledger and cache identity are distinct: replay proves prior execution; cache is an optional new-call optimization.

### Work package E — Prompting, evaluations, and guards

- Update primary/subagent prompt contracts and tool descriptions.
- Add direct-versus-programmatic fixtures for repository inspection tasks.
- Add guards that only approved contracts use `DirectOrProgrammatic`.

## 8. Failure, cancellation, restart, and contention semantics

- Manifest denial or output-schema absence fails before job creation.
- Foreground wait is cancellation-aware; cancelling the parent turn requests program cancellation and joins terminal delivery.
- Parent provider disconnect does not orphan the durable program; session policy decides whether the foreground job is cancelled or retained, and the outcome is explicit.
- Cache corruption/missing artifacts causes a miss or typed failure, never fabricated output.
- Read-only transient calls may retry according to persisted contract policy; deterministic validation/not-found results do not loop.
- Program parallelism is bounded independently from agent parallel tool-call limits and consumes broker/resource budgets.
- Incomplete results preserve partial value and artifacts without resubmitting completed calls.

## 9. Compatibility and migration

- Existing direct tool names, input schemas, and display behavior remain compatible.
- Typed adapters may coexist with string `execute` until all direct consumers migrate.
- `tool_program` is additive and may be hidden/disabled by configuration or model profile.
- Providers need only ordinary function calling for the native backend.
- Tool catalog/deferred loading must not imply program eligibility; caller policy is authoritative.

## 10. Required tests

### Focused unit tests

- output schemas and typed renderers for each selected tool;
- manifest selection/rejection;
- result-schema complexity and value validation;
- cache key/invalidation/eviction;
- prompt/tool definition snapshots.

### Integration tests

- foreground programs performing multi-file read, grep/filter, repo-map aggregation, Git read, LSP read, and context expansion;
- direct/programmatic typed-result equivalence;
- artifact expansion and transcript isolation;
- exact job/program/call/tool/run correlation.

### Restart and recovery tests

- restart during read, parallel group, cache population, and final projection;
- completed calls replay without duplicate tool execution;
- missing cached artifact handled safely.

### Contention and cancellation tests

- bounded large file/search sets;
- cancel foreground parent during program execution;
- many concurrent read-only programs respecting call/task/memory limits.

### Security and negative tests

- attempted direct-only tool call;
- forged manifest/contract hash;
- path traversal and sensitive path access;
- stale cache after dirty file change;
- oversized result schema/value/artifact expansion;
- prompt injection in file contents cannot alter runtime manifest or caller policy.

### Evaluation tests

Measure correctness, source/evidence retention, model turns, tool calls, context bytes/tokens, latency, and cache effects for direct and programmatic routes. Token reduction without correctness/evidence equivalence is not acceptance.

## 11. Required verification commands

```bash
cargo test -p codegg --test tool_program_read_palette
cargo test -p codegg --test tool_program_context_artifacts
cargo test -p codegg --test tool_program_cache
cargo test -p codegg --test agent_loop_harness
cargo test -p codegg --test tool_contract_guards
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
```

## 12. Documentation updates

- tool eligibility matrix in `architecture/tool_programs.md`
- typed output schema documentation
- context/artifact/cache architecture docs
- primary/subagent prompt contracts
- user/operator guide for foreground programs and incomplete results

## 13. Acceptance criteria

1. An ordinary function-calling model can submit and await a restricted-Python program.
2. Only explicitly migrated read-only/safe-repeat tools can be called.
3. Selected tools return stable schema-validated values and preserve raw evidence through artifacts.
4. Intermediate nested outputs do not enter the parent transcript by default.
5. Cache hits are authorization- and workspace-correct.
6. Direct and programmatic routes are equivalent on the accepted evaluation corpus.
7. Cancellation, restart, and bounded parallel execution converge without repeated completed calls.
8. No unresolved high or medium finding remains.

## 14. Stop conditions

Stop and report if M005 is not closed, a candidate tool has hidden mutation/external side effects, a stable output schema cannot be defined, cache identity cannot distinguish dirty state, or prompt changes would make programs the default for semantic/approval-sensitive work.

## 15. Closure evidence required

Create `plans/closure/tool-programs/006-status.md` with tool eligibility inventory, schema/version matrix, direct/program parity results, transcript/artifact evidence, cache correctness tests, prompt snapshots, restart/cancellation/contention results, evaluation metrics, and residual findings.

## 16. Handoff notes

Start with a small high-confidence palette. Do not enable a tool merely because its category currently says read-only. Inspect real implementation effects and preserve evidence handles before granting programmatic caller policy.
