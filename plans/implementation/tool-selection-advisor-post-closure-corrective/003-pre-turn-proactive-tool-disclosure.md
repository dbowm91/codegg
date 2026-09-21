# Tool-Selection Advisor Post-Closure Corrective M003 — Pre-Turn Proactive Tool Disclosure

Status: closing

Repository baseline: `c9087346620988a7c793fb2687732a62e481103a`

Source corrective:

- `plans/subsystems/tool-selection-advisor-post-closure-corrective-addendum.md#m003--pre-turn-proactive-tool-disclosure`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: capability/authority corrective.

## 1. Objective

Implement the proactive disclosure behavior that the predecessor M005 intended but did not wire.

When explicitly configured in `promote` mode, the advisor should inspect bounded current-turn context plus the final policy-allowed tool surface and make a very small number of strongly relevant deferred tools **initially visible to the primary model before provider definitions are finalized**.

This must work even when the primary model never calls `tool_search`.

## 2. Why this milestone is ready

The necessary authority and runtime seams already exist:

- `ResolvedToolSurface` is the immutable policy-filtered authority.
- `ToolAdvisor` and `NoopAdvisor` exist.
- `src/agent/request_preparation.rs` owns initial palette/deferred projection.
- `tool_search` already remains available as deterministic fallback.

M003 can be implemented and tested using `hashed-linear-v1` or a deterministic test advisor. It does not need to wait for the contextual encoder.

## 3. Current evidence and defect

The current production path wires `project_discovery()` only inside `ToolSearchTool`. Therefore:

```text
primary model -> chooses tool_search -> advisor can promote search results
```

This does not solve the target failure:

```text
small/tool-fragile model does not know useful deferred tool exists
-> never calls tool_search
-> advisor never runs
```

The correct seam is in request preparation after final authority resolution and before `project_initial_tool_palette`/`apply_tool_exposure_filter` set `defer_loading`.

## 4. Invariants

- Off/default path is byte/behavior equivalent in tool visibility/order where existing determinism guarantees apply.
- Observe mode records advice but changes no provider-facing definitions.
- Rerank remains a `tool_search` behavior; proactive promotion is a distinct pre-turn disclosure behavior.
- Promotion can only remove deferral from a tool already present in the final `ResolvedToolSurface`.
- Denied/disabled/plan-ineligible/non-callable/parent-ceiling tools cannot be promoted.
- Promotion never creates a definition, capability, broker registration, or permission.
- Core/required/never-reduce tools are never suppressed by the advisor.
- Model/advisor failure leaves the existing palette unchanged.
- No advisor model -> unchanged existing palette.
- Telemetry/capture remains independently disabled unless explicitly enabled.

## 5. Target request-preparation flow

Use the final resolved surface as the source of truth:

```text
native + MCP/plugin definitions
        |
deny/disable/plan/callability/parent ceiling
        |
        v
ResolvedToolSurface
        |
        +--> ordinary contextual_immediate_tools
        |
        +--> optional ToolAdvisor disclosure projection
               input:
                 bounded user/current-task context
                 policy-allowed deferred descriptors
                 surface fingerprint
               output:
                 abstain or <= N canonical names
        |
        v
effective immediate set =
  ordinary contextual immediate
  UNION qualified advisor promotions
        |
        v
project_initial_tool_palette / defer_loading
        |
        v
provider request
```

Do not feed omission records/denied definitions to the advisor.

## 6. Context construction

Build a small host-owned context projection. Prefer already available structured state:

- current user turn/original current objective;
- active goal/current task when available;
- a very small number of recent unresolved/error/task cues if justified;
- no raw full transcript;
- no secrets/credentials/tool arguments.

The exact representation should be versioned with the advisor context schema. Keep it bounded enough that local inference cost is predictable.

## 7. Promotion policy

Configuration should make proactive disclosure unmistakably experimental and explicit. Reuse/extend existing advisor settings rather than adding a second config authority.

Recommended controls:

- mode = `promote`;
- maximum promotions, initial hard cap 2;
- calibrated threshold;
- optional minimum margin over abstention/next candidate;
- maximum added provider-schema/prompt budget.

The host must revalidate every predicted name against the resolved surface immediately before changing visibility.

Promotion should change initial visibility/defer state only. It must not modify tool schemas or invent wrapper tools.

## 8. Interaction with `tool_search`

Preserve `tool_search` as fallback/discovery even in promote mode.

Reactive search reranking may continue to use the advisor, but keep prediction scopes distinct in diagnostics:

- `scope = preturn_disclosure`
- `scope = tool_search_rerank`

A pre-turn promotion must not remove the tool from search results or change callability.

## 9. Ordered work packages

A. Add a pure projection helper over `ResolvedToolSurface` and bounded turn context.
B. Wire observe mode before provider-palette projection with no behavior change.
C. Wire promote mode into effective immediate tool names before `defer_loading`.
D. Add threshold/count/schema-budget caps.
E. Add diagnostics/fallback reason.
F. Optionally emit a training event only through the explicit M001/M004-style capture factory when local capture is enabled.
G. Update architecture docs and qualify with a deterministic test advisor.

## 10. Failure/recovery/concurrency

Scoring is turn-local. Timeout/error/panic boundary/missing artifact -> unchanged palette for that turn.

Do not make promotion durable session authority. Each turn recomputes from the current resolved surface and context.

If caching is added, key by at least model/artifact version + surface fingerprint + bounded-context hash and never cache authority across a changed surface.

## 11. Required tests

### Authority negatives

- denied tool never becomes immediate;
- disabled model tool never becomes immediate;
- plan-ineligible tool never becomes immediate;
- parent-ceiling-excluded tool never enters candidates;
- hidden/internal tool never enters candidates;
- advisor output naming a non-surface tool is ignored/rejected.

### Disclosure behavior

- off -> identical deferred flags;
- observe -> identical deferred flags;
- promote -> one qualified deferred tool becomes immediate;
- low confidence/abstain -> no change;
- promotion cap and schema budget enforced;
- core/required tools unchanged.

### Target regression

Construct a request-preparation test where:

1. a useful specialized tool is deferred;
2. the primary-model path has made **no** `tool_search` call;
3. deterministic advisor returns that tool above threshold;
4. provider-facing definition is initially visible/not deferred;
5. authority/callability are unchanged.

This is the key missing predecessor behavior.

### Failure

- missing/corrupt model -> unchanged palette;
- scorer error/timeout -> unchanged palette;
- telemetry disabled -> no capture/network side effect.

## 12. Verification commands

Use exact targets after implementation. Expected minimum:

```bash
cargo test -p codegg --lib agent::request_preparation
cargo test -p codegg --lib agent::tool_surface
cargo test -p codegg --lib tool_advisor
cargo test -p codegg --lib tool::tool_search
cargo test --test tool_surface_minimization
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

## 13. Documentation updates

- `architecture/tool-advisor.md`: distinguish reactive rerank from proactive disclosure.
- `architecture/agent-tool-surface.md`: document advisor as a visibility projection after authority resolution.
- Config documentation for `off/observe/rerank/promote` semantics and budget.

## 14. Acceptance criteria

M003 closes only when:

1. proactive promotion occurs before provider tool definitions are finalized;
2. it does not require a prior `tool_search` invocation;
3. promoted tools are a strict subset of the current allowed/deferred surface;
4. off/observe/failure preserve existing initial visibility;
5. count/schema budgets are enforced;
6. no authority, permission, execution, or telemetry invariant regresses.

## 15. Stop conditions

Stop if implementation requires moving advisor scoring before policy resolution, giving it raw denied definitions, registering tools dynamically, or making promotion durable/default-on.

## 16. Closure evidence required

- exact request-preparation seam/diff;
- off/observe/promote visibility fixtures;
- authority-negative matrix;
- no-`tool_search` proactive regression;
- prompt/schema budget measurements;
- failure/fallback evidence;
- broad verification output.
