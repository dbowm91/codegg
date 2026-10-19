# Agent Loop Operational Reliability M003 — Workspace Glob and Tool Discovery

Status: closed

Repository baseline: `f37840ef5fca36d44b35daec5da6b26cd049a3ee` (main; verify at implementation start)

Source roadmap: `plans/subsystems/agent-loop-operational-reliability-roadmap.md#7-milestones`

Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `plans/000-long-term-specification.md#28-observability`; `plans/000-long-term-specification.md#29-system-invariants`; `plans/001-terminology-and-domain-model.md#7-agent-terms`.

Applicable ADRs: No new ADR; preserve accepted tool disclosure/authorization contracts and already-closed coding-agent tool surface milestones.

Primary class: polish

## 1. Objective

Make canonical `glob` patterns resolve predictably relative to the authorized workspace and make `tool_search` useful for ordinary multiword task descriptions while preserving bounded, policy-filtered discovery.

## 2. Why this milestone is ready

`GlobTool`, `ToolCatalog`, `tool_search`, `ResolvedToolSurface` and policy-filtering are installed and owned. Closed discovery C001–M003 work need not be reimplemented.

## 3. Current implementation evidence

- `src/tool/glob.rs` builds a relative `globset::GlobMatcher`, walks the canonical absolute root and evaluates `matcher.is_match(&path)` against absolute entry paths; reported `scripts/verify*.sh` incorrectly returned none.
- `src/tool/catalog.rs` defaults to `SearchMode::Keyword`, whose matching condition is `name.contains(full_query) || description.contains(full_query)`. Multiword natural-language prompts rarely match one substring.
- `src/tool/tool_search.rs` rejects empty queries with `no_results`; returns up to 10 results and supports `name` + `detail=schema`. Discovery is policy-filtered and has a bounded advisor fallback; do not introduce another tool index.
- Curated/minimal initial palettes intentionally defer some tools; search should see the authorized deferred set without widening it.

## 4. Invariants

Workspace root is explicit and canonical; no traversal via `..`, symlink or alternate CWD; ignore/gitignore behavior remains documented; globally hidden/denied tools never appear; exact-name schema expansion and current provider palettes still work; bounded output counts and no full catalog/schema dump.

## 5. Scope

In: normalize glob path relative to search root before matching; natural-language query tokenization/fallback to installed BM25 or safe multi-term scoring; exact lookup; bounded authorized paginated/empty-query index with compact descriptors and stable ordering; diagnostics for path/search misses.

Out: wholesale search engine replacement, embedding/vector service, new filesystem scanner, exposing all tools to every model, or changing sandbox semantics.

## 6. Required production changes

**Glob:** strip canonical `walk_root` prefix for matcher input, preserving path separators and legitimate `**/` behavior on Windows/Unix. Define when patterns may be absolute; either document/reject or normalize them securely. Respect `path` search directory and return consistent paths; preserve containment checks before and after canonicalization. Differentiate zero matches from I/O/permission scan error.

**Catalog/search:** tokenize free-text queries and score multi-term name/description matches with deterministic tie-break; use installed BM25 where appropriate. Empty-query listing, if supported, must be bounded and paginated and only after policy filtering. Preserve `name` exact + schema expansion, advisor opt-in and forbidden hidden tools. Do not let a malformed query silently match the entire universe.

**Model-facing:** improve tool-search description/response with `total_matches`, pagination cursor/offset if added, exact operation names and suggestions on no hits. Avoid loading full JSON schemas until requested.

## 7. Ordered work packages

A. Write failing real-workspace fixtures for relative `scripts/verify*.sh`, nested paths, alternate search roots, Gitignore, symlinks and traversal.

B. Normalize match subjects to root-relative form; verify deterministic cross-platform paths.

C. Write failing `tool_search("file write edit tools available")` and `tool_search("run tests compile")` fixtures against registered, allowed tools; implement deterministic, compact multiword fallback and optional bounded listing.

D. Verify exact-schema expansion, advisor fallback and resolved allow-list remain monotonic; update docs.

## 8. Failure, cancellation, restart, and contention

Read-only concurrent queries require no new store; no state leaks across sessions. Invalid glob syntax or inaccessible root returns useful typed error; truncated walk returns explicit indication. Search under a changing registry must be consistent per request and never claim unavailable tools. Cancelled search/walk does not block tool dispatch indefinitely.

## 9. Compatibility and migration

No migration; retain input schemas `glob(pattern,path?)` and `tool_search(query,name?,detail?)`. Optional paging inputs must default to current bounded behavior; no provider-wire changes required.

## 10. Required tests

Focused: `src/tool/glob.rs`, `src/tool/catalog.rs`, `src/tool/tool_search.rs`. Integration: registered real `scripts/verify.sh` fixture and Curated/Minimal resolved surfaces; queries matching multiple verbs, exact lookup, no hits and empty. Restart: catalog recreated with identical ordering. Contention: parallel searches during deferred registration; no lock poisoning. Security negatives: path outside root, symlink escape, hidden/denied tool, stale allow-list, overly long query, oversized result.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Run applicable focused tests from §10 *before* broad verification; use `cargo test -p codegg --lib <module_filter> --locked` and/or the named existing integration target, adjusting command to the actual crate/target at implementation time. Record exact commands and results; never claim verification that was not run. No new CI lane, permanent benchmark, network-dependent provider tests or SBC compile/startup test.

## 12. Documentation updates

Update the relevant `architecture/{agent,tool,context-ledger,provider,permission}.md` sections only where behavior changes; update `docs/tools.md` and `AGENTS.md` if operator/model-visible advice changes. Preserve completed predecessor closure records.

## 13. Acceptance criteria

1. Relative glob of existing `scripts/verify.sh` succeeds when workspace root is elsewhere than process CWD.
2. A natural-language discovery query yields relevant authorized tools; exact-name and per-tool schema remain supported.
3. Empty search either returns a bounded allowed catalog page or explicitly states a single supported listing mechanism; it must not silently fail to discover tools.
4. Full and deferred palettes retain current authority ceilings; no hidden/denied tools exposed, no unbounded schemas.

## 14. Stop conditions

Stop and report rather than broaden scope if the implementation demands a new authorization owner, durable-store migration without a reviewed design, an incompatible provider contract, a cross-boundary bypass, or a new ADR. If source differs materially from pinned baseline, record the exact discrepancy and update the smallest possible plan section before implementation. Do not mark closed until a `plans/closure/agent-loop-operational-reliability/NNN-status.md` record contains commits, failing-then-passing evidence, requirement-to-test matrix, cancellation/contention/security results, residual risks and the exact verified head.

## 15. Handoff

Execute only this milestone's scope. Preserve unrelated working changes, pin the initial commit and provider/model fixture, run the required tests, and register the closure or a named blocker in `plans/registry.md` using the planning skill.
