# Tool Program Capability Expansion Roadmap

Status: closed

Long-term references:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#29-system-invariants`

Related ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md` is authoritative. This roadmap extends eligible contracts through the existing broker/manifest/ledger/runtime; it does not create another program executor or bypass broker authority.

## 1. Purpose and ownership boundary

This roadmap increases the practical value of the closed Tool Program subsystem by admitting additional carefully classified read operations. It owns caller/effect/schema/replay/cache eligibility for new programmatic calls and, where broad multiplexed tools mix reads and mutations, narrow programmatic-only adapters that delegate canonical read implementations.

It does not broaden mutation authority, change the restricted Python language, create a new search/LSP/Git engine, or make all read-looking tools program-callable automatically.

## 2. Work classification

### Invariants

- Tool Programs invoke tools only through existing `ToolBroker`/contract/manifest/authority machinery.
- `DirectOrProgrammatic`/`ProgrammaticOnly` classification is explicit and effect-correct.
- A broad tool containing mutation-capable operations is never marked wholesale programmatic merely because some operations are reads.
- Workspace/path/principal/effect/budget authority cannot widen in a program.
- External/untrusted/nondeterministic provenance is preserved and replay/cache semantics never claim determinism falsely.
- New programmatic adapters contain no independent execution logic.

### Capabilities

Programs can perform a more useful set of deterministic local analysis, repository search, and selected structured Git/LSP reads without repeated model round trips.

### Infrastructure

Eligibility matrix and narrow programmatic adapter contracts where per-operation effects cannot be represented by an existing multiplexed tool contract.

### Polish

Documentation/diagnostics expose why a tool or operation is/is not programmatic.

## 3. Non-goals

Programmatic file mutation, Bash/terminal/process execution, LSP mutation/preview application, Git mutation, arbitrary web/network tools, bypassing tool disclosure/permission, hosted Tool Program transport, parser rewrite, blanket `ReadOnly => programmatic` rule.

## 4. Current state

The Tool Program roadmap M006-M020 is closed with restricted-Python compile/IR verification, brokered execution, manifests, metering, cache/replay, durable ledger, recovery and notifications. Production `DirectOrProgrammatic` contracts found at baseline are the local `read`, `glob`, `grep`, and `list` tools. `diff` is read-only but uses a legacy string result and path root local to the tool. `repo_search` is read-only and already returns structured external-untrusted provenance through an explicit `SearchRuntimeContext`, but lacks a programmatic caller contract. `git` and `lsp` are multiplexed surfaces with both read and mutation-capable operations, so blanket promotion is unsafe.

## 5. Target architecture

A documented eligibility checklist governs programmatic admission: effect class, caller policy, bounded schemas/results, immutable execution context/path policy, deterministic or explicitly nondeterministic semantics, broker authority, cache/retry/replay declaration, provenance and no hidden mutable global state.

Deterministic local reads may become `DirectOrProgrammatic`. External repository search is a separately classified read with external-untrusted provenance and conservative cache/replay semantics. Multiplexed Git/LSP expose only selected reads via `ProgrammaticOnly` narrow adapters or an equivalent contract mechanism that delegates the canonical backend and is hidden from ordinary model disclosure.

## 6. Dependency graph

```text
M001 deterministic local read expansion
        |
        v
M002 external repo-search programmatic seam
        |
        v
M003 operation-scoped Git/LSP read adapters
```

M001 is ready on the closed Tool Program system. M002 hard-depends on the eligibility contract from M001. M003 hard-depends on M001/M002 so adapter policy/provenance/replay rules are stable.

## 7. Milestones

### M001 — Deterministic local read contract expansion
Class: capability. Objective: establish executable eligibility rules and promote `diff` after structured output/workspace-policy correction; admit any other candidate only if it satisfies the same evidence without enlarging scope. Exit: at least `diff` is safely callable from a program; mutation/network tools remain denied; manifest/cache/replay tests pass.

### M002 — External repository-search programmatic seam
Class: capability. Objective: make canonical `repo_search` available to programs only with explicit external-untrusted/nondeterministic/cache/replay semantics and existing runtime context. Exit: brokered program query works; no global service or trust upgrade; replay/ledger behavior is truthful.

### M003 — Operation-scoped Git/LSP read adapters
Class: capability. Objective: expose a small high-value selected read subset through hidden programmatic-only contracts that delegate canonical Git/LSP owners, without marking multiplexed mutation tools programmatic. Exit: selected status/diff/log/symbol/diagnostic/navigation reads work; all mutation operations are structurally unavailable.

## 8. Cross-cutting requirements

No storage migration beyond existing Tool Program manifest/ledger schema unless versioned additive metadata is necessary. Contract changes must invalidate/rehash manifests/cache correctly. Network search requires existing SSRF/backend policy and provenance. LSP/Git adapters must use explicit workspace execution context and existing backend methods, never shell subprocess duplication.

## 9. Verification strategy

Focused contract/broker/program runtime/cache/replay/authority tests plus negative mutation matrices and normal repository verification. No new benchmark/CI lane.

## 10. Risks and decision points

`diff` currently uses tool-local cwd/root assumptions; it must be bound to program execution context before promotion. Search results change over time; replay/cache must not imply deterministic recomputation. Git/LSP wrappers can recreate tool overlap; keep them `ProgrammaticOnly`, narrow and delegating, or stop if the contract system cannot express this cleanly.

## 11. Completion definition

M001-M003 accepted: Tool Programs have materially broader useful read capability through existing authority, external trust is preserved, and no mutating or duplicate backend path has been introduced.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/tool-program-capability-expansion/001-deterministic-local-read-contract-expansion.md` | `plans/closure/tool-program-capability-expansion/001-status.md` | — |
| M002 | closed | `plans/implementation/tool-program-capability-expansion/002-external-search-programmatic-read-seam.md` | `plans/closure/tool-program-capability-expansion/002-status.md` | — |
| M003 | closed | `plans/implementation/tool-program-capability-expansion/003-operation-scoped-lsp-git-read-adapters.md` | `plans/closure/tool-program-capability-expansion/003-status.md` | — |
