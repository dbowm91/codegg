# Agent Runtime Correctness, Autonomy, and Simplification M002 — Textual Tool-Call Repair Safety

Status: ready

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- Milestone M002

Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`

Primary class: security/correctness invariant

Dependencies:

- hard: none
- interface: current model-profile/adapter resolution, provider-normalized `ChatEvent`/`ToolCall`, resolved tool surface
- soft: M005 must consume the normalized provider outcome produced by this milestone

Relevant references:

- `plans/000-long-term-specification.md`
- `plans/003-planning-process.md`
- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md` — historical model-adaptation ownership
- `architecture/provider.md`
- `architecture/agent.md`

Target closure record:

- `plans/closure/agent-runtime-correctness-autonomy-simplification/002-status.md`

## 1. Objective

Preserve compatibility with tool-fragile/local models without allowing arbitrary assistant prose to become executable merely because it contains JSON, XML, a fenced block, or an `invoke(...)` example that resembles a tool call.

Move textual-tool parsing/repair from an unconditional generic-loop fallback into an explicit model/provider adaptation contract. The canonical path remains structured provider tool calls.

## 2. Explicit non-goals

Do not:

- delete textual-tool compatibility for models that demonstrably require it;
- force every provider/model to use the same textual grammar;
- add a general natural-language action parser;
- infer executable intent from phrases such as "I will run", "next I should", or "use bash";
- parse arbitrary Markdown/JSON examples in an otherwise final answer;
- introduce provider-specific branches throughout `AgentLoop`;
- change ordinary native tool names/schemas;
- redesign the provider abstraction beyond the minimal normalized repair capability;
- add a second permission bypass: repaired calls still go through the same normal tool-surface validation and authorization as structured calls;
- make textual parsing more permissive in order to preserve obscure undocumented formats.

## 3. Current implementation evidence

Inspect at minimum:

- `crates/codegg-providers/src/text_tool_parser.rs`;
- all call sites of `parse_text_as_tool_calls()`;
- `src/agent/loop.rs` no-tool-call handling and stop-reason logic;
- `crates/codegg-core/src/model_profile/` or current model-adapter/profile types;
- provider capability resolution and provider-normalized stream events;
- current tests for Hermes/XML, raw JSON, fenced, `invoke(...)`, local/open models, and tool-fragile profiles;
- resolved tool-surface validation before execution.

Known risk at baseline:

- when `EventProcessor` reports no structured tool calls, the generic loop calls `parse_text_as_tool_calls(processor.text())` without first requiring an explicit adapter/profile capability;
- the parser recognizes raw JSON embedded in prose such as `{"name":"bash","arguments":{...}}` as an executable call;
- it also accepts fenced blocks where the fence language becomes the tool name, XML forms, and `invoke(...)` forms;
- repaired calls are assigned new call IDs and emitted as normal tool calls, so accidental parser matches can become real execution;
- this behavior is global rather than limited to provider/model families known to emit textual tool syntax.

## 4. Invariants that cannot regress

- structured provider tool calls are canonical and take precedence over textual repair;
- a final assistant text response is never scanned for executable calls unless the resolved adapter/profile explicitly enables textual-tool repair for that response condition;
- repaired calls must reference a currently exposed/authorized model-facing tool name;
- repaired arguments must satisfy basic JSON/object/schema requirements before permission/execution;
- textual repair never bypasses normal permission evaluation;
- repair is bounded: one provider response cannot create an unbounded parse/retry cascade;
- explanatory examples remain text;
- adapter-specific syntax lives in provider/model adaptation code or data, not generic orchestration conditionals;
- models without textual-tool repair enabled behave exactly as structured-tool-only models;
- unsupported or ambiguous textual syntax becomes a typed malformed-tool-protocol outcome or ordinary final text, not guessed execution.

## 5. Target repair contract

Introduce or reuse an explicit adapter/profile capability representing textual tool-call repair. The exact field/type is implementation-dependent; examples:

```text
ToolCallEncoding::StructuredOnly
ToolCallEncoding::StructuredWithTextRepair(TextRepairProfile::HermesXml)
```

or a resolved adapter capability carrying:

- whether text repair is allowed;
- accepted grammar/profile;
- which stop reasons permit repair;
- whether mixed prose plus tool syntax is accepted;
- maximum repaired calls per provider response.

Do not derive this solely from ad hoc model-name substring checks at execution time. Existing model-profile inference may assign the capability centrally when necessary, but `AgentLoop` should consume a resolved capability rather than re-infer it.

## 6. Repair triggering requirements

A textual parse attempt should require all of the following unless repository evidence demonstrates a safer equivalent:

1. no valid structured tool calls were emitted;
2. the resolved adapter/profile explicitly permits text repair;
3. the provider stop/finish state is compatible with malformed/textual tool output rather than an ordinary completed final answer;
4. the response is within a bounded parse size;
5. the accepted grammar is the one assigned to the adapter/profile;
6. every parsed tool exists in the current resolved model-facing tool surface;
7. arguments pass shape/schema validation before normal authorization.

For a model that historically emits a textual tool call with a normal `stop` reason, compatibility may require profile-specific allowance. Encode that exception in the profile and cover it with a fixture; do not re-enable broad global scanning.

## 7. Grammar contraction requirements

Audit each existing parser grammar against actual supported-model evidence.

Preferred dispositions:

- Hermes/XML grammar: retain if used by a supported model/profile and test with exact fixtures;
- `invoke("tool", {...})`: retain only if an explicit supported profile emits it;
- fenced ` ```tool {json} ``` `: retain only with evidence; otherwise delete because ordinary documentation/examples commonly use fenced JSON/tool names;
- raw JSON embedded anywhere in prose: remove from generic repair. If a supported model requires raw JSON, accept only an exact response-level envelope under that model's repair profile, not a regex substring search;
- multiple calls: retain only when the profile/provider contract supports multiple textual calls and the parser can identify them unambiguously.

Prefer exact parsers/balanced JSON extraction over regex patterns that search arbitrary prose for executable fragments.

## 8. Normalized provider outcome

M002 should leave M005 with a simpler contract. Consider a normalized internal result such as:

```text
ProviderTurnOutcome::ToolCalls(Vec<ToolCall>)
ProviderTurnOutcome::FinalText(...)
ProviderTurnOutcome::MalformedToolProtocol { repairable: bool, ... }
ProviderTurnOutcome::ProviderFailure(...)
```

This type need not be public or large. The purpose is to prevent the generic loop from repeatedly asking "are there tool calls? can I regex the text? does the stop reason look odd?" in several branches.

Do not fold autonomous continuation/recovery policy into this type; M005 owns what to do after normalization.

## 9. Ordered work packages

### Work package A — Inventory supported textual encodings

1. list every parser grammar and its tests;
2. identify which model/provider profiles actually need each grammar;
3. inspect adapter/profile configuration for current textual-tool/tool-fragile capabilities;
4. classify each grammar: retain profile-specific, tighten, or remove;
5. record unsupported historical compatibility separately rather than preserving speculative grammar forever.

### Work package B — Add explicit adapter/profile capability

1. add the smallest resolved capability needed to represent textual repair;
2. assign it in one model-adaptation layer;
3. ensure default/frontier structured-tool profiles disable text repair;
4. preserve existing tool-fragile/local profiles only where evidence supports repair;
5. expose the resolved profile to the provider-outcome normalization path without model-name branching in the generic loop.

### Work package C — Harden parser behavior

1. remove arbitrary raw-JSON substring execution from the generic parser path;
2. constrain each retained grammar to exact/profile-specific envelopes;
3. enforce bounded input size and maximum call count;
4. reject missing/unknown tool names;
5. require argument objects where the tool schema requires an object;
6. preserve balanced nested JSON handling where required;
7. do not silently turn malformed arguments into strings for a tool expecting structured arguments.

### Work package D — Validate against resolved tool surface

1. pass or otherwise consult the current resolved tool names/definitions during repair;
2. reject repaired calls for tools hidden by plan mode, profile policy, agent denial, deferral, or unavailable backend;
3. keep normal permission evaluation after repair;
4. ensure generated tool IDs cannot collide with existing structured IDs in the same turn.

### Work package E — Normalize loop integration

1. replace unconditional `parse_text_as_tool_calls(processor.text())` with the explicit repair contract;
2. return/record a typed malformed-protocol outcome when a repair-enabled profile fails parsing;
3. do not decide continuation/bootstrap here; M005 owns that policy;
4. keep current behavior for valid structured calls.

### Work package F — Documentation

Update:

- `architecture/provider.md` with canonical structured-call and adapter-repair ownership;
- `architecture/agent.md` to remove claims that generic arbitrary text parsing is normal orchestration;
- model-profile docs/config comments if new capability is user-configurable.

## 10. Security and authorization effects

This milestone reduces execution authority from model text.

Required adversarial cases:

- ordinary final answer containing `{"name":"bash","arguments":{"command":"rm ..."}}` as an example -> remains text for structured-only profiles;
- Markdown fenced example named `bash` -> remains text unless an explicit profile grammar treats the whole response as a textual call and all triggering conditions are satisfied;
- `<tool_call>` example inside quoted/documentation text -> no execution for profiles without that grammar;
- repair-enabled profile emits unknown tool -> rejected before permission/execution;
- repair-enabled profile emits known mutating tool -> still reaches normal `Ask`/deny policy;
- malformed nested JSON -> typed parse failure, not best-effort string execution.

Do not include destructive real commands in live tests; use fake/mock tools.

## 11. Compatibility effects

- structured-call providers/models: no behavioral change expected;
- supported fragile/local profiles: textual repair remains available through explicit profile selection;
- undocumented accidental parsing of arbitrary prose examples is intentionally removed;
- if a retained model requires a relaxed stop-reason trigger, encode and test that exception narrowly.

No storage or public protocol migration is expected.

## 12. Focused verification

Add focused tests covering:

```text
structured call bypasses textual repair
structured-only profile + raw JSON prose -> final text
structured-only profile + XML/fence -> final text
repair-enabled Hermes fixture -> ToolCall
repair-enabled raw-json profile, if retained -> exact-envelope only
unknown/hidden tool rejected
malformed args rejected
repaired mutating tool still enters normal permission path
one response cannot exceed repair call/count/size bounds
```

Then run:

```bash
scripts/verify.sh quick
```

Run provider crate tests if parser/normalized provider types move into `codegg-providers`. Do not require broad workspace tests unless shared provider DTOs materially change.

## 13. Static guards

Do not add a regex guard prohibiting `parse_text_as_tool_calls` in the loop. The desired invariant should be expressed through type/API ownership and regression tests.

If the generic parser remains public for compatibility, document it as an adapter utility and ensure the production call path requires an explicit repair profile.

## 14. Acceptance criteria

M002 closes only when:

- generic no-tool-call handling no longer scans arbitrary model prose unconditionally;
- textual repair is enabled only by a resolved adapter/profile capability;
- accepted grammars are tied to supported-model evidence and tightened accordingly;
- raw JSON substring parsing is removed or restricted to an exact profile-specific response envelope;
- repaired calls are bounded and validated against the current resolved tool surface;
- repaired calls continue through ordinary permission and broker execution;
- structured-call behavior is unchanged;
- adversarial prose/example fixtures remain non-executable for structured-only profiles;
- supported fragile-model fixtures still repair correctly;
- `scripts/verify.sh quick` and focused provider/parser tests pass;
- no provider-specific branches are added throughout the generic agent loop.

## 15. Stop conditions

Stop and record a compatibility blocker if:

- a currently supported model relies on an ambiguous textual grammar that cannot distinguish explanation from execution;
- provider-normalized stop reasons are insufficient to safely decide whether repair is intended;
- moving the behavior behind an adapter requires a public provider API redesign larger than this milestone.

Prefer a documented profile-specific compatibility limitation over restoring unconditional arbitrary-prose execution.

## 16. Required closure evidence

`plans/closure/agent-runtime-correctness-autonomy-simplification/002-status.md` must include:

- implementation commit/PR;
- grammar inventory with retain/tighten/delete disposition;
- model/profile mappings that enable repair;
- adversarial non-execution test results;
- supported fragile-model repair fixture results;
- resolved-tool-surface validation evidence;
- focused test and `scripts/verify.sh quick` outcomes;
- any known model compatibility exceptions and severity.