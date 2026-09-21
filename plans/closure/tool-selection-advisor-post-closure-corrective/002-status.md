# Tool-Selection Advisor Post-Closure Corrective M002 — Closure Status

Status: closed
Source implementation plan: plans/implementation/tool-selection-advisor-post-closure-corrective/002-contextual-encoder-runtime-and-training.md
Source subsystem roadmap: plans/subsystems/tool-selection-advisor-post-closure-corrective-addendum.md# m002--contextual-encoder-runtime-and-local-rust-training
Repository baseline reviewed: 082f7d8
Implementation commits: aa91080 — implement tool advisor contextual encoder; 082f7d8 — tool advisor M002 enter closure review

## 1. Executive finding

M002 is closed. CodeGG now has an optional, pure-Rust contextual advisor
runtime and local trainer distinct from hashed-linear-v1. The runtime loads
auditable binary artifacts, scores bounded context/candidate interactions,
supports unseen textual tool descriptors, and falls back without failing an
agent turn. Training and runtime are feature-isolated and the default build
remains unchanged.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Contextual model | contextual-embedding-v1 learned context/candidate embedding interaction; counterfactual-aware corpus consumed. |
| Capacity points | Small 5,242,881 parameters; medium 15,728,641 parameters; both within the 5M–25M envelope. |
| Local Rust training | contextual-small-training.json and contextual-medium-training.json completed with no Python, download, native sidecar, or remote teacher. |
| Artifact auditability | Binary manifest contains model/version, dimensions, tokenizer, schema versions, dataset fingerprint, weight SHA-256, provenance, license, and parameter count. |
| Unknown tools | Descriptor scoring is text-based and does not depend on canonical tool names; M001 synthetic-name cases are part of the frozen dataset. |
| Baseline comparison | Small held-out MRR 0.939; medium held-out MRR 0.946; hashed-linear-v1 held-out MRR 0.939 on the same frozen split. |
| Feature isolation | Contextual module is behind tool-advisor; autograd/training command path is behind tool-advisor-training; default check and quick verification pass. |
| Authority | ContextualAdvisor implements ToolAdvisor only and receives no broker, permission, registry, scheduler, or execution authority. |

## 3. Production implementation evidence

- src/tool_advisor/contextual.rs owns deterministic hashing, embedding
  interaction scoring, binary artifact validation, atomic writes, local
  training, and capacity reporting.
- advisor_from_config recognizes contextual binary artifacts only when the
  optional runtime feature is enabled; linear artifact loading remains
  supported.
- tool-advisor train and eval support contextual configs/artifacts while
  preserving the existing hashed-linear-v1 path.
- inspect reports contextual manifests without requiring the training feature.
- architecture/tool-advisor-framework-spike.md records the bounded Candle,
  Burn, Tract, and repository-local framework decision.

## 4. Verification executed

- cargo test --workspace --locked — 11,645 passed; 3 ignored across 245 suites.
- cargo test --locked --features tool-advisor -p codegg --lib tool_advisor — 20 passed.
- cargo test --locked --features tool-advisor-training -p codegg --lib tool_advisor — 22 passed.
- cargo test --locked --features tool-advisor-training -p codegg --lib tool_advisor::contextual — 2 passed.
- cargo run --locked --features tool-advisor-training --bin codegg -- tool-advisor train --config assets/tool-advisor/contextual-small-training.json --json — 5,242,881 parameters; 20,972,182-byte artifact.
- cargo run --locked --features tool-advisor-training --bin codegg -- tool-advisor train --config assets/tool-advisor/contextual-medium-training.json --json — 15,728,641 parameters; 62,915,226-byte artifact.
- cargo run --locked --features tool-advisor-training --bin codegg -- tool-advisor eval --model target/tool-advisor/contextual-small.bin --dataset assets/tool-advisor/corpus.jsonl --json — 256-case evaluation completed.
- cargo run --locked --features tool-advisor --bin codegg -- tool-advisor inspect --model target/tool-advisor/contextual-small.bin — contextual manifest loaded and validated.
- cargo clippy --workspace --all-targets --all-features -- -D warnings — no issues found.
- scripts/verify.sh quick — passed.
- cargo fmt --all -- --check — passed.
- git diff --check — passed.

## 5. Invariant review

The contextual model changes only advisory ranking/visibility inputs. It cannot
create tools, widen the resolved surface, bypass permission, or execute
anything. Missing/corrupt artifacts remain a no-op fallback. Default CodeGG
does not require the runtime feature or weights.

## 6. Failure and recovery review

Artifact magic, manifest limits, parameter count, finite values, and weight
digest are validated before scoring. Writes are atomic. Training output is
installed only after the complete local run. Context/candidate limits bound
inference work and malformed artifacts fail closed to NoopAdvisor.

## 7. Migration and compatibility review

The existing JSON hashed-linear-v1 artifact format remains supported. The
contextual binary format has an independent magic/header and schema, so a
linear artifact cannot be silently interpreted as contextual weights. No
provider wire protocol or persisted session schema changed.

## 8. Security review

No model download, remote inference, telemetry, credential access, or private
repository fixture was added. Artifact provenance and license are explicit.
The advisor remains below authority and execution boundaries.

## 9. Documentation and operations

The framework spike, contextual artifact contract, two training configs, model
inspection command, capacity points, and default-off behavior are documented
in architecture/tool-advisor.md and architecture/tool-advisor-framework-spike.md.

## 10. Unresolved findings (severity: critical/high/medium/low)

- None for M002.
- Downstream end-to-end primary-model effectiveness is intentionally not
  claimed here; it remains M004 evidence after M003 closes.

## 11. Roadmap disposition

M002 is closed. M003 remains ready and independently executable. M004 remains
blocked because its hard dependency on M003 pre-turn disclosure is not yet
closed.

## 12. Registry updates

- M002 moved from closing to closed.
- M003 remains ready.
- M004 remains blocked on M003.
