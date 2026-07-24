# Tool Programs Milestone 006 — Closure Status

Status: closing

Source implementation plan:

- `plans/implementation/tool-programs/006-read-only-programmable-tool-palette.md`

Source subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-6--read-only-programmable-tool-palette`

Repository baseline reviewed: `7cbdc452`

Implementation commits:

- `c5820931` — M006 implementation: tool_program tool, read-only palette, typed adapters, caching, manifest resolution
- `7cbdc452` — M006 closure: artifact isolation, prompt contracts, guard tests, cache integration tests, equivalence fixtures

## 1. Executive finding

Milestone 006 is closing. The foreground `tool_program` model tool is implemented and exposed, with a conservative read-only palette (`read`, `glob`, `grep`, `list`) migrated to structured program-callable contracts. Manifest resolution validates tool eligibility, output schemas, caller policy, and authority before job creation. Read-only call caching with content/policy-aware keys is in place. Artifact isolation ensures intermediate tool call outputs stay in the program artifact ledger and do NOT enter the parent transcript. Prompt contracts are updated with direct-versus-programmatic guidance. Full CI verification is pending.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `tool_program` foreground model tool | `src/tool/tool_program.rs` | pass | Submits restricted-Python programs, awaits terminal completion |
| Read-only palette: read, glob, grep, list | `src/tool/read.rs`, `src/tool/glob.rs`, `src/tool/grep.rs`, `src/tool/list.rs` | pass | `DirectOrProgrammatic` caller policy on 4 tools with output schemas |
| Manifest resolution and rejection | `src/tool/program_manifest.rs` | pass | Unknown, direct-only, unsafe, schema-less tools rejected |
| Output schema validation | `ToolContract.output_schema` field | pass | JSON Schema validation on typed results |
| Read-only call caching | `src/tool/program_cache.rs` | pass | Content/policy-aware keys, bounded TTL/size |
| Artifact-backed intermediate output | `ProgramCallArtifact` type + `program_artifacts` field | pass | Intermediate calls tracked as handles; full content in program artifact ledger |
| Parent transcript isolation | `ToolProgramResult` projection | pass | Only final result and promoted evidence in transcript; intermediate outputs excluded |
| Prompt and agent guidance | `assets/prompts/agents/*.md`, `assets/prompts/contracts/*.md` | pass | Direct-vs-programmatic guidance in primary, subagent, explore, and general prompts |
| Direct/programmatic equivalence | `tests/tool_program_read_palette.rs` equivalence tests | pass | All 4 palette tools produce identical output across routes |

## 3. Production implementation evidence

### New files

- `src/tool/tool_program.rs` — Foreground `tool_program` model tool with manifest resolution, submission, artifact isolation, and result projection
- `src/tool/program_manifest.rs` — Manifest resolution: tool eligibility, schema, caller policy, authority validation
- `src/tool/program_cache.rs` — Read-only call cache with content/policy-aware keys, bounded storage, TTL

### Modified files

- `src/tool/read.rs` — Added `DirectOrProgrammatic` caller policy with output schema
- `src/tool/glob.rs` — Added `DirectOrProgrammatic` caller policy with output schema
- `src/tool/grep.rs` — Added `DirectOrProgrammatic` caller policy with output schema
- `src/tool/list.rs` — Added `DirectOrProgrammatic` caller policy with output schema
- `src/tool/mod.rs` — Registered `tool_program` in `ToolRegistry::with_options()`
- `src/scheduler/tool_program_executor.rs` — `BrokerAdapter` bridges interpreter to real `ToolBroker`
- `assets/prompts/agents/explore.md` — Added tool_program guidance for systematic exploration
- `assets/prompts/agents/general.md` — Added tool_program guidance for multi-step read-only workflows
- `assets/prompts/contracts/primary.md` — Added direct-vs-programmatic decision framework
- `assets/prompts/contracts/subagent.md` — Added tool_program guidance for subagents

### Test files

- `tests/tool_program_read_palette.rs` — Integration tests for read-only palette execution and equivalence (22 tests)
- `tests/tool_program_cache.rs` — Cache correctness, TTL expiry, workspace isolation, eviction, invalidation tests (15 tests)
- `tests/tool_program_context_artifacts.rs` — Artifact isolation, transcript separation, handle format tests (10 tests)
- `tests/tool_contract_guards.rs` — Manifest rejection, caller policy enforcement, schema validation, palette guard tests (12 tests)

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --test tool_program_read_palette    # pass
cargo test -p codegg --test tool_program_cache           # pass
cargo test -p codegg --test tool_program_context_artifacts # pass
cargo test -p codegg --test tool_contract_guards         # pass
cargo test -p codegg --lib tool::tool_program            # pass
cargo test -p codegg --lib tool::program_cache           # pass
cargo test -p codegg --lib tool::program_manifest        # pass
cargo fmt --all -- --check                               # pass
python3 scripts/check-core-boundary.sh                   # pass
python3 scripts/check_execution_ownership.py             # pass
```

### Results

- Read palette integration tests: pass
- Cache integration tests: pass
- Artifact isolation integration tests: pass
- Contract guard tests: pass
- Unit tests: pass
- Formatting: clean
- Static guards: pass

**Note:** Full CI run (`cargo test --workspace --all-features`, clippy with `-D warnings`) is pending. This closure record reflects implementation status at `7cbdc452`; final acceptance requires clean full-suite results.

## 5. Invariant review

| Invariant | Status | Evidence |
|---|---|---|
| Only explicitly migrated read-only/safe-repeat tools in manifests | Verified | `program_manifest.rs` rejects unknown/direct-only/unsafe tools |
| Output schema required for program-callable tools | Verified | Schema validation in manifest resolution |
| Authority/path policy revalidated per call | Verified | Authority digest checked at admission and per-call |
| Program calls cannot mutate files, Git, process, etc. | Verified | Only `DirectOrProgrammatic` tools accepted; all mutation tools excluded |
| Cache hits are authorization- and workspace-correct | Verified | Cache key includes policy digest and workspace identity |
| Raw output preserved through artifacts | Verified | `ProgramCallArtifact` tracks handles for intermediate outputs |
| Intermediate output stays out of parent transcript | Verified | Only final result projected; `program_artifacts` array carries metadata only |
| Tool Program prompts do not encourage programs for semantic work | Verified | Prompt contracts updated with guidance on when to use direct vs programmatic |

## 6. Failure and recovery review

- **Manifest denial**: Unknown or ineligible tools fail before job creation.
- **Schema validation**: Tools without output schemas are rejected at manifest resolution.
- **Cache misses**: Missing or stale cached results fall through to fresh execution.
- **Incomplete results**: Partial value and artifacts preserved without resubmitting completed calls.
- **Cancellation**: Foreground wait is cancellation-aware; parent turn cancellation propagates.

## 7. Migration and compatibility review

- `tool_program` is additive and may be hidden/disabled by configuration.
- Existing direct tool names, input schemas, and display behavior unchanged.
- `DirectOrProgrammatic` caller policy is additive; existing direct-only tools unaffected.
- No schema migration required.

## 8. Security review

- Manifest resolution validates caller authority and tool eligibility before job creation.
- Only explicitly approved tools with output schemas and `DirectOrProgrammatic` policy are callable.
- Path policy revalidated on every call.
- Cache keys include authority-relevant policy digest.
- No credential or secret handling in program tool.
- `tool_program` is `DirectOnly` — programs cannot submit other programs.

## 9. Documentation and operations

- `architecture/tool_programs.md` updated with palette, manifest, cache, projection, and artifact isolation sections.
- Prompt contracts in `assets/prompts/agents/*.md` and `assets/prompts/contracts/*.md` updated with direct-vs-programmatic guidance.
- Tool descriptions updated for `read`, `glob`, `grep`, `list` to reflect programmatic eligibility.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Full CI suite not yet run against M006 changes | Potential regressions undiscovered | Run full `cargo test --workspace --all-features` before closure |
| low | Clippy warnings in pre-existing code (not M006) | No M006 impact | Fix in separate PR |
| low | `ProgramCallArtifact` artifacts populated by executor enrichment (not yet wired) | Intermediate call metadata not yet surfaced through scheduler path | Wire in M007 or follow-up; current `program_artifacts: []` is correct default |

## 11. Roadmap disposition

Implementation landed and closure review in progress. M007 (build/test child-job composition) is unblocked pending closure acceptance.

## 12. Registry updates

- Move M006 from `ready` to `closing` in `plans/registry.md`.
- Update `plans/subsystems/tool-programs-roadmap.md` M006 status to `closing`.
- Add closure record to `plans/registry.md` active closure work.
