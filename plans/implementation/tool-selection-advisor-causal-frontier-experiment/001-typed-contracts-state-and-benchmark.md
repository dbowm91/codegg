# Tool-Selection Advisor Causal Frontier M001 — Typed Contracts, State, and Benchmark

Status: closed (positive; see `plans/closure/tool-selection-advisor-causal-frontier-experiment/001-status.md`; implementation `152613de`)

Repository baseline: `aa21cfe1763d7ea00d11e582ed4108233e4088a9`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md#m001--typed-causal-contracts-state-projection-and-benchmark-preregistration`

Primary class: architecture/evidence foundation.

## 1. Objective

Build the smallest trustworthy substrate for causal tool-menu experiments
without changing runtime disclosure behavior.

M001 must deliver:

1. a closed typed state/effect ontology;
2. an additive native causal-contract seam;
3. a deterministic host-owned state snapshot;
4. a bounded pilot set of native tool contracts;
5. a frozen stateful benchmark and M002 gates.

No causal filter affects provider definitions in M001.

## 2. Types

Preferred experiment types live under a narrow module such as
`src/tool_advisor/causal_frontier.rs`.

### 2.1 State facts

Use a closed enum. The initial vocabulary may include only facts that can be
derived without interpreting arbitrary natural language, for example:

```rust
enum CausalStateFact {
    ActiveGoal,
    ActiveWorkPlan,
    ActionableWorkItem,
    InProgressWorkItem,
    BlockedWorkItem,
    ArtifactHandleAvailable,
    TouchedFilesAvailable,
    TestEvidenceAvailable,
    FailedTestEvidence,
    UnresolvedError,
    SecurityFinding,
    LspPreviewAvailable,
    ContextReadAvailable,
    UnmetTestAcceptance,
    UnmetCommitAcceptance,
    UnmetArtifactAcceptance,
    UnmetDelegatedRunAcceptance,
}
```

The exact names may change before the M001 preregistration commit, but every
fact must have one canonical host source.

No fact may be created from an embedding/LLM classifier or hidden reasoning.

### 2.2 Tool outcomes

Use a separate closed enum for coarse causal outcomes, e.g.:

- FilesInspected
- PathsDiscovered
- TextMatchesProduced
- SymbolFactsProduced
- WorkspaceMutationProduced
- VerificationEvidenceProduced
- TestEvidenceProduced
- GitFactsProduced
- CommitEvidenceProduced
- ExternalEvidenceProduced
- DelegatedRunProduced
- ArtifactExpanded
- GoalStateUpdated
- WorkPlanStateUpdated

These are planning metadata, not claims that every invocation succeeds.

### 2.3 ToolCausalContract

Preferred shape:

```rust
struct ToolCausalContract {
    schema_version: u16,
    requires_all: BTreeSet<CausalStateFact>,
    requires_any: Vec<BTreeSet<CausalStateFact>>,
    forbids: BTreeSet<CausalStateFact>,
    produces: BTreeSet<CausalOutcome>,
    provenance: CausalContractProvenance,
}
```

Keep the boolean language intentionally small. No arbitrary expression DSL in
M001.

## 3. Tool trait seam

Add an additive default method:

```rust
fn causal_contract(&self) -> Option<ToolCausalContract> { None }
```

or an equivalent registry-owned seam.

Do not change the semantics of existing `Tool::contract()`,
`ToolContractCatalog`, broker authorization, retry, cache or permission.

A missing contract is a first-class state and must remain safe.

## 4. CausalStateSnapshot

Build one immutable bounded snapshot from existing host state.

Allowed inputs:

- active Goal existence/status;
- active WorkPlan and current item status/dependencies;
- WorkAcceptance disposition/evidence kind;
- context-ledger touched files;
- test results;
- unresolved errors;
- security findings;
- bounded artifact handles;
- turn-local LSP preview registry availability;
- `context_read` availability;
- resolved backend availability already reflected upstream.

The snapshot stores booleans/counts/typed ids only where needed. Do not copy raw
tool output, prompts, file content, secrets or transcript history.

Fingerprint:

- ontology version;
- fact values;
- relevant host-state revision identifiers;
- resolved-surface fingerprint.

## 5. Pilot native contracts

Annotate only tools whose causal metadata can be justified statically.

Required pilot coverage should include representative stateful and ordinary
coding tools, including where available:

- `context_read`;
- Goal tools;
- WorkPlan tools;
- `lsp_preview_apply`;
- `verify` / test surface;
- `task`;
- `read`, `glob`, `grep`;
- `lsp`;
- `write` / edit family;
- Git read/commit surface;
- research/web tools.

Generic tools may have empty preconditions and only outcome metadata.

Multiplexed tools with operation-dependent effects must use conservative union
metadata or remain uncontracted; do not pretend a static contract is more
precise than the tool.

## 6. Contract integrity

Each native contract fingerprint binds:

- canonical tool name;
- implementation id/version when available;
- input-schema fingerprint;
- causal ontology version;
- contract payload.

Tests must fail on:

- unknown enum values;
- contradictory `requires_all` + `forbids`;
- empty/invalid provenance;
- duplicate unstable serialization;
- contract name mismatch.

MCP/plugin tools have no trusted causal contract in M001 unless their existing
native wrapper owns static metadata in source.

## 7. Stateful benchmark

Create a new benchmark rather than reusing the text-only retrieval corpus.

Suggested asset:

- `assets/tool-advisor/causal-frontier-v1.jsonl`
- `assets/tool-advisor/causal-frontier-m001-preregistration.json`

Minimum 160 cases, grouped into semantic/state families.

Each case records:

- stable case id/family;
- explicit `CausalStateSnapshot` facts;
- resolved eligible tool identities;
- contracted/uncontracted identity;
- gold current-step tool set;
- gold premature/inadmissible tools;
- required/never-reduce markers;
- optional structured desired outcome for later M003;
- rationale/provenance.

Required families:

- no Goal/WorkPlan;
- active Goal;
- active WorkPlan pending/actionable/in-progress/blocked;
- unmet test acceptance;
- unmet commit acceptance;
- artifact recovery;
- unresolved error;
- failed tests;
- LSP preview available/unavailable;
- delegation available/unavailable;
- security finding;
- mixed state;
- unknown/uncontracted external tools.

## 8. Split/evidence discipline

Freeze family-aware dev and qualification partitions before M002.

No contract may be edited in response to qualification results without opening a
new corrective/experiment version.

M001 may use dev cases for contract debugging but must reserve a family-balanced
qualification partition untouched until M002 selection is frozen.

## 9. M002 gates frozen by M001

At minimum:

- gold current-step tool preservation = 1.00;
- authority violations = 0;
- uncontracted fallback preservation = 1.00;
- premature/inadmissible contracted exposure reduction >=0.50;
- median causally admissible deferred promotion set <=4 where structured signal
  exists;
- insufficient-state cases abstain/fallback rather than inventing a frontier;
- p95 pure frontier evaluation <=5 ms excluding state I/O.

Record exact formulas/tie-breaking before M002.

## 10. Tests

At minimum:

- each fact has one canonical host-source test;
- state snapshot deterministic/fingerprint stable;
- no raw prompt/output enters snapshot;
- secret-like values never enter snapshot;
- default `causal_contract() == None`;
- missing contract does not mean denied;
- contract serialization deterministic;
- contradiction validation;
- surface fingerprint bound;
- hidden/denied tool cannot appear in benchmark frontier construction;
- benchmark family split has no case-id/family leakage.

## 11. Verification

```bash
cargo test -p codegg --lib tool_advisor::causal_frontier
cargo test -p codegg --lib tool::contract
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Require ordinary hosted CI before positive closure.

## 12. Acceptance

M001 closes positively only when the ontology, contract seam, state projection,
pilot contracts, benchmark and M002 gate receipt are all frozen with no runtime
visibility change.

Positive M001 makes M002 ready.
