# Tool-Selection Advisor Post-Closure Corrective M001 — Closure Status

Status: closed
Source implementation plan: plans/implementation/tool-selection-advisor-post-closure-corrective/001-dataset-split-and-consent-integrity.md
Source subsystem roadmap: plans/subsystems/tool-selection-advisor-post-closure-corrective-addendum.md# m001--dataset-split-and-consent-integrity-corrective
Repository baseline reviewed: 0b67e88
Implementation commits: 66bf63e — tool advisor M001 dataset consent integrity; 0b67e88 — tool advisor M001 enter closure review

## 1. Executive finding

M001 is closed. The advisor qualification corpus is no longer smoke-scale: it
has deterministic provenance, semantic-group isolation, counterfactual pairs,
unknown/synthetic names, tool-family metadata, and frozen split fingerprints.
Training-data transport now requires a host-owned consent snapshot and a
current sink-policy check; caller-authored booleans remain audit fields only.
No default advisor capture or network behavior was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Corpus floor | 256 cases; 128 semantic groups; 64 hard-negative; 32 no-tool; 56 multi-tool; 32 unknown-tool; 10 task/tool families. |
| Leakage-resistant split | Semantic groups are the split key; report fingerprints are dev 1b87257f8e29bd3b5a8e52f985c845bb3c814d224433054b7550d8f1b239817b, test 73c79bfa5872e54edd3d8978bc1a4c948fcfa71c009d200ddbc05abd5b857f3b, train 50c25e5664cdd1ff3059651d3b436c3c39531e6d8055fdd12003411d87d9b476. |
| Counterfactual coverage | 112 valid pairs with identical candidate sets and changed task/relevance labels. |
| Tool-family/unknown holdouts | Plugin-family holdout fingerprint 9bc1be331d068f10d0968a3bd98fab019996e9997fd860915c995bd0f228d14e; 32 synthetic-name cases. |
| Provenance/import integrity | All generated cases declare generated-local-template-v1; case validation rejects missing/invalid labels and provenance. |
| Consent authority | TrainingConsentSnapshot, v2 event construction, local sink checks, remote sink checks, and current-policy revocation checks. |
| Default locality | tool-advisor data status reports Disabled; no automatic teacher, download, or remote transport path exists. |

## 3. Production implementation evidence

- ToolAdvisorCase now carries semantic group, task family, tool family,
  generated-variant family, and optional local teacher probabilities.
- coverage_report, validate_qualification_corpus, and tool-advisor lint make
  floors, split fingerprints, holdouts, and counterfactual validation
  executable.
- Training uses semantic_group as the split boundary while preserving the
  hashed-linear-v1 baseline.
- Event schema v2 carries TrainingConsentSnapshot; v1 records remain
  readable but cannot authorize remote transport.
- Remote submission requires both event and current host-policy metadata
  consent; content requires both independent content grants.

## 4. Verification executed

- cargo test -p codegg --lib tool_advisor — 18 passed.
- cargo test -p codegg --features tool-advisor-training --lib tool_advisor — 20 passed.
- cargo run --locked --bin codegg -- tool-advisor lint --dataset assets/tool-advisor/corpus.jsonl --json — floors and metadata passed.
- cargo run --locked --bin codegg -- tool-advisor bench --dataset assets/tool-advisor/corpus.jsonl --json — 256-case baseline; dataset fingerprint df24a36d40337b355026382716317e414b9359fde0468c74f30978578ff94c9d.
- cargo run --locked --bin codegg -- tool-advisor data status — Disabled.
- cargo fmt --all -- --check — passed.
- cargo clippy --workspace --all-targets --all-features -- -D warnings — no issues found.
- scripts/verify.sh quick — passed.
- git diff --check — passed.

## 5. Invariant review

Semantic groups cannot cross splits, generated variants retain provenance,
unknown names are synthetic, and advisor data remains advisory-only. Resolved
tool authority is untouched. Remote content never follows from metadata-only
consent. Historical predecessor closure records were not edited.

## 6. Failure and recovery review

Malformed cases fail validation before use. Local writes remain atomic and
corrupt spool records are quarantined. Remote policy is revalidated on every
send, so revocation prevents queued-event transmission. Consent/sink failures
remain non-fatal to agent execution.

## 7. Migration and compatibility review

The case schema remains version 1 with backward-compatible default metadata for
programmatic callers; repository fixtures use explicit metadata. Training
events are version 2, and v1 records remain readable for inspection but have no
transport authority without a v2 host snapshot.

## 8. Security review

No private repository content was added. Candidate/context redaction remains in
place. Remote endpoints still require explicit HTTPS configuration. Event
booleans cannot self-authorize a remote send, and consent is not mixed with
security/audit storage.

## 9. Documentation and operations

architecture/tool-advisor.md documents the corpus contract, lint command,
holdouts, counterfactuals, and consent authority. The operator-visible data
status command remains disabled by default.

## 10. Unresolved findings (severity: critical/high/medium/low)

- None for M001.
- The lexical hashed-linear-v1 baseline, proactive disclosure wiring, and live
  primary-model trajectory evidence remain explicitly owned by M002, M003, and
  M004 respectively; they are not being claimed by this closure.

## 11. Roadmap disposition

M001 is closed. It unblocks M002 because the expanded dataset, frozen split
fingerprints, counterfactual pairs, and consent contract are now stable.
M003 remains independently ready. M004 remains blocked until M002 and M003
close.

## 12. Registry updates

- M001 moved from closing to closed.
- M002 moved from blocked to ready in the same closure change.
- M003 remains ready.
- M004 remains blocked on M002 and M003.
