# Agent Loop Operational Reliability M006 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-loop-operational-reliability/006-provider-tool-schema-compatibility.md`

Source subsystem roadmap: `plans/subsystems/agent-loop-operational-reliability-roadmap.md`

Repository baseline reviewed: `f37840ef5fca36d44b35daec5da6b26cd049a3ee`

Implementation commit: `c409e1e012d9e599abe67e975c28f4fac9dd66de`

Verified implementation head: `c409e1e012d9e599abe67e975c28f4fac9dd66de`

## 1. Executive finding

Provider capability declarations are conservative for direct OpenAI Chat Completions. Non-deferral requests strip deferral fields, and one typed, bounded pre-output compatibility retry removes unsupported `defer_loading` fields without changing the selected model, messages, or authorized tools.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Capability is wire-compatible | provider deferred capability test | pass | Direct OpenAI Chat Completions is conservative; Anthropic stays enabled. |
| Safe request downgrade | fake-gateway `AgentLoop` integration | pass | Retry is bounded and occurs before visible streamed output. |
| Non-deferral wire shape is clean | request-preparation tests | pass | All deferral fields removed. |
| Broad regression coverage | workspace nextest, quick and clippy | pass | 14,614 default-feature tests passed. |

## 3. Production implementation evidence

Provider capability now reflects direct OpenAI Chat Completions support. The final request preparation strips unsupported defer fields. The provider-turn retry recognizes typed HTTP 400 schema rejection language, retries once with only deferral fields removed, and remains under the existing retry chain/deadline.

## 4. Verification executed

```bash
rtk cargo test -p codegg-providers --locked deferred_tool_support_is_conservative_for_openai_compatible_chat
rtk cargo nextest run -p codegg --test agent_loop_harness --locked --profile ci
rtk cargo nextest run --workspace --locked --profile ci --no-fail-fast
rtk scripts/verify.sh quick
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
```

Provider capability passed (1 passed, 274 filtered); agent-loop harness 61 passed; default workspace 14,614 passed; quick and clippy passed. The feature-configured full command's existing TUI fixture failure is recorded in M007.

## 5. Invariant review

Retry preserves model, messages, and policy-authorized tool set, does not repeat an executed side effect, and ends after one compatibility downgrade. Visible output prevents this compatibility retry.

## 6. Failure and recovery review

Only typed API 400 errors with bounded unsupported-field evidence qualify. Other provider errors follow existing bounded retry rules. Request deadline and cancellation remain controlling.

## 7. Migration and compatibility review

No provider protocol or stored configuration migration. Existing support for Anthropic deferral remains unchanged; unsupported gateways receive the compatible standard tool list.

## 8. Security review

The downgrade only removes schema metadata; it does not add tools or expand authorization. Error classification is bounded and does not log credentials.

## 9. Documentation and operations

Updated `architecture/provider.md` with wire capability and compatibility fallback behavior.

## 10. Unresolved findings

None within M006.

## 11. Roadmap disposition

Milestone closed. M001–M006 closure requirements are now satisfied and M007 was unblocked for final qualification.

## 12. Registry updates

M006 is closed; no successor in this line remains blocked.
