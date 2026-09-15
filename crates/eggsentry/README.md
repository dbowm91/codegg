# eggsentry

Deterministic security scanning primitives for Rust tools.

`eggsentry` classifies shell commands, scans text and files for secret
and unsafe-code patterns, classifies dependency files, and produces
structured findings. All scanners are deterministic library primitives:
the same input bytes produce the same findings and stable finding IDs,
with no network, clock, or host-service dependency.

## Stable identifiers

Match on the typed values, not on human diagnostic strings:

- `SecurityCategory`, `Severity`, `Confidence`, `FindingSource`,
  `FindingMode`, `CommandRisk`, `DependencyEcosystem`, and
  `SecurityProfile` serialize as `snake_case` strings. Those strings
  (and `SecurityCategory::label` / `SecurityProfile::as_str`) are the
  stable wire identifiers.
- `evidence`, `reasons`, `recommendation`, and `summary` are human
  diagnostics, not identifiers.
- Finding `id`s are stable hashes over
  `(prefix, category, evidence/context, line)`.

## Versioning

Adding a new category, rule, ecosystem, or profile variant is minor.
Renaming or removing an existing wire identifier, or changing the
meaning of an existing risk mapping, is major. Rule-pattern refinements
that keep the same identifiers are patch-level.

Host orchestration (tools, gates, approvals, daemon wiring) lives
outside this crate.
