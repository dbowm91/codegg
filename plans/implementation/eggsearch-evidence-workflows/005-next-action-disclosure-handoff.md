# Eggsearch Evidence Workflows M005 — Policy-Gated Next-Action Disclosure Handoff

Status: blocked (new M002 + new M004)
Class: capability / integration
Planning baseline: CodeGG main `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`
Source roadmap: `plans/subsystems/eggsearch-evidence-workflows-roadmap.md`
References: `plans/003-planning-process.md`; CodeGG tool visibility/disclosure architecture; `plans/closure/tool-surface-upstream-compatibility/007-status.md`.
Hard dependencies: M002 goal/source compatibility and M004 compact projection strictly closed.

## 1. Objective

Convert eggsearch's structured `next_actions` and `suggested_fetches` into policy-checked, **nonexecuting** follow-up hints that integrate with CodeGG's existing `tool_search` and deferred-tool visibility. Avoid an independent tool discovery registry, and make agents aware of a useful next tool without automatically calling it.

## 2. Audit evidence

- CodeGG `src/tool/disclosure.rs` maintains default/core and deferred tools; `src/tool/tool_search.rs` already hydrates discovery.
- Eggsearch current tool responses include structured `next_actions`/`suggested_fetches`, and discovery metadata can contain `aliases`, `keywords`, `related_tools`, disclosure and capability hints.
- Raw `mcp__eggsearch__*` tools are deliberately hidden; public stable CodeGG wrapper names (such as `repo_fetch` and `evidence_bundle`) remain authoritative.
- Upstream suggestions are **not** instructions, capability grants, permission approvals, or proof a suggested URL is safe.

## 3. Boundaries and non-goals

No auto-following links, batch execution, delegated/background tasks, new permissions, raw MCP exposure, unbounded network crawl, remote code execution, dynamic creation of arbitrary tools or bypass of `tool_search`. No new durable storage, protocol or tool registry. Maintain CodeGG's native local search/file tools and role-based immediate/deferred strategy.

## 4. Expected contract and safety

- Ingest bounded tool suggestions only when present in the successful *structured* result. For errors/partial results, optional suggestions may still be rendered as uncertain suggestions but **never** as prevalidated safe routes.
- Map allowlisted canonical eggsearch tool IDs to existing CodeGG native names using one explicit mapping, e.g. `web_fetch -> webfetch`, `build_evidence_bundle -> evidence_bundle`. Reject/discard all unknown, stale, or unauthorized names; never expose `mcp__eggsearch__*` to the model as executable tools.
- Validate argument shapes and limits against CodeGG native wrapper schemas. Treat suggested URL/repo/path/filter parameters as **untrusted data**; do not preauthorize SSRF, checkout access, policy, sandbox, host, or credential scopes. Do not perform a network fetch during hint processing.
- Check CodeGG policy and current role/surface/tool catalog, *not* an upstream claim about `disclosure`, before a hint becomes visible. Parent/child ceilings, disabled search and network policy remain authoritative at later execution.
- Tool hydration may make a known deferred tool discoverable for the next agent decision; actual execution must still route through the existing broker and approvals. If current CodeGG tool-search hydration only exposes tool definitions upon an explicit lookup, represent suggestions as lookup/query hints rather than bypass the mechanism.
- Bound count, argument size, string length and TTL/scope of hints to the existing response/turn. Prevent session-crossing or parallel-run suggestion leakage; never persist untrusted auto-routing preferences.
- Keep canonical suggested tool names aligned with the exact pinned eggsearch release and explicit stable wrapper translation. Missing metadata is nonfatal and does not alter baseline behavior.

## 5. Ordered work packages

1. Trace `src/tool/disclosure.rs`, `tool_search` and broker permissions. Write a small state diagram for proposed hints -> visible discovery -> normal execution, noting scopes and cancellation.
2. Snapshot upstream suggestion schema and discovery metadata. Implement explicit vetted name mapping and argument validation with deny-by-default semantics, returning a bounded display hint/lookup request.
3. Integrate with current `tool_search` lookup or agent follow-up prompt construction. Do **not** add implicit execution or a second dynamic registry; deliberately test behavior for a policy-disabled tool.
4. Surface concise rationale and source references when safe; untrusted upstream text stays framed as external data rather than a tool-use instruction.
5. Add adversarial fixtures and observability counters for accepted/filtered suggestions; avoid logging full secret-bearing arguments.

## 6. Verification

```bash
cargo fmt --all -- --check
cargo test --test fake_eggsearch_mcp -- --test-threads=1
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo test --test search_backend_arg_mapping -- --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Unit/integration cases MUST include known suggested fetch, unknown tool ID, spoofed `mcp__eggsearch__*` name, malicious prompt-injection arguments, overlarge URLs, off-allowlist host/private-IP URLs (never preauthorized), prohibited tool, disabled search, conflicting roles, cancellation, mixed-run isolation, discovery omission, and “hint without call” proof via fake-MCP call counters. Test that executing an allowed suggested tool later still performs normal policy checks.

## 7. Compatibility, security, concurrency and observability

Existing tool schema/name unchanged; no protocol/storage migration. Suggestion logic must not block the agent loop or spawn background work. Requests remain subject to original tool run/turn cancellation, task authority, current session context, network/SSRF rules and existing budget. No external metadata may mutate policy or persist between unrelated sessions. Diagnostics classify hints accepted vs rejected without exposing credentials.

## 8. Acceptance, stop and closure

**Accept:** only recognized, well-formed, authorized-to-discover suggestions reach the model in bounded form; no suggestion triggers a tool call; later execution retains complete authorization; baseline discoverability and normal search remain unchanged; malicious/unknown suggestions fail closed; tests/quick pass.

**Stop:** if current `tool_search` requires bypassing its central authority to hydrate, or permissions cannot be resolved safely at hint time; implement lookup-only textual suggestions or raise a bounded ADR/corrective rather than create a second broker.

Closure: `plans/closure/eggsearch-evidence-workflows/005-status.md`: mapping matrix, authority/role tests, no-execution proof, negative fixtures, latency/budget observations, focused/quick outcomes and unresolved risks.
