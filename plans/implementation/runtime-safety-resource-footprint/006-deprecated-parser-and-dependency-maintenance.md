# Runtime Safety, Resource Control, and Footprint Milestone 006 — Deprecated Parser and Dependency Maintenance

Status: ready

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- Milestone 006

Dependency:

- M005 — dependency feature and namespace normalization must establish manifest ownership before this milestone changes parser dependencies.

Repository baseline reviewed: `4d540ce315c9ef2a1c07544cd42df0efc43708e1`

Primary class: compatibility and dependency maintenance

Target closure record:

- `plans/closure/runtime-safety-resource-footprint/006-status.md`

## 1. Objective

Remove active reliance on the deprecated `serde_yaml` 0.9 implementation while preserving currently supported YAML configuration, agent, command, and skill inputs.

The implementation must establish one small parser abstraction and choose one of two bounded outcomes based on current ecosystem evidence at implementation time:

1. adopt a maintained Serde-compatible YAML parser that satisfies CodeGG's required syntax and diagnostic behavior; or
2. retain YAML only as a compatibility import format through a narrowly contained reader while making the existing maintained TOML/JSON5 paths authoritative for new generated or rewritten configuration.

This milestone also codifies a lightweight manual dependency-maintenance procedure. It must not create continuous dependency-update automation or another routine CI lane.

## 2. Explicit non-goals

This milestone must not:

- remove YAML input support abruptly;
- rewrite the configuration subsystem, agent definition format, command format, or skill format;
- introduce a custom YAML parser;
- accept an unmaintained or archived fork merely because it is API-compatible;
- convert every existing user file in place without an explicit backup and compatibility strategy;
- change runtime assets, prompt semantics, agent authority, command authority, or skill precedence;
- add dependency bots, scheduled audits, update matrices, or release automation;
- perform broad dependency upgrades unrelated to the parser boundary;
- require exact preservation of parser wording when a maintained parser provides clearer diagnostics, but compatibility-sensitive error classes and source locations must remain usable;
- add a large corpus, fuzzing framework, or conformance suite to routine CI.

## 3. Current implementation evidence

Inspect at minimum:

- `crates/codegg-config/Cargo.toml` and parser/config loading code;
- `src/agent/mod.rs` and adjacent agent-definition loading;
- `src/command/mod.rs` and command-file loading;
- `src/skills/parser.rs`;
- `src/skills/mod.rs`;
- all `serde_yaml` imports and direct calls;
- repository examples, fixtures, defaults, documentation, and generated files using YAML;
- any YAML serialization/write path;
- TOML and JSON5 parser abstractions already present;
- configuration reload/watch behavior and diagnostics.

The reviewed baseline uses `serde_yaml` across multiple ownership boundaries rather than behind one codec. This makes replacement risk larger than the dependency declaration suggests. The implementation agent must inventory each use as:

- user-authored durable configuration;
- repository-owned agent/command/skill asset;
- generated or rewritten output;
- test fixture;
- migration/import compatibility path.

Before choosing a replacement, inspect current maintenance status, release recency, unsafe code exposure, Serde support, YAML feature coverage, diagnostic quality, MSRV/toolchain compatibility, and transitive dependency impact. Record the decision in the closure record; do not rely on an old planning-time package assumption.

## 4. Invariants that cannot regress

- existing supported YAML files remain readable during the compatibility window;
- parser choice cannot broaden agent, command, or skill authority;
- duplicate-key, type mismatch, unknown-field, alias/anchor, tagged-value, merge-key, and multi-document behavior are explicit rather than accidental;
- source path and useful line/column information remain available in diagnostics where the parser exposes them;
- reload/watch failures do not replace the last valid in-memory configuration with partial data;
- serialization does not silently reorder or discard semantically relevant data where CodeGG currently writes YAML;
- no parser panic is reachable from malformed user input;
- input size/depth limits remain bounded by current surrounding loaders or become explicitly bounded if absent;
- TOML/JSON5 behavior remains unchanged;
- dependency selection is centralized enough to avoid direct parser calls reappearing across subsystems.

## 5. Required parser abstraction

Introduce or normalize one internal codec boundary equivalent to:

```rust
trait StructuredDocumentCodec {
    fn parse<T: DeserializeOwned>(
        &self,
        source_name: &str,
        bytes: &[u8],
    ) -> Result<T, DocumentParseError>;
}

enum DocumentFormat {
    Toml,
    Json5,
    YamlCompatibility,
}
```

Exact design may use functions rather than a trait if simpler. Required properties:

- direct parser-crate APIs are confined to the codec module;
- callers receive one typed error shape with format, source path/name, error class, and optional line/column;
- file discovery/precedence remains outside the codec;
- parser-specific values do not leak into agent, command, skill, or config domain types;
- serialization, if retained, has a separate explicit API and is not implied by read compatibility.

A cheap static guard should reject new direct `serde_yaml` or replacement-parser imports outside the codec and approved tests.

## 6. Parser selection decision

### 6.1 Candidate acceptance criteria

A maintained YAML parser is acceptable only when it:

- has active, non-archived upstream maintenance at implementation time;
- supports the repository's Rust toolchain and Serde data model;
- parses all supported CodeGG YAML fixtures;
- provides bounded failure on malformed input;
- has acceptable diagnostic source-location support;
- does not require a disproportionately large native/runtime dependency;
- has licensing compatible with the repository;
- does not introduce network or code-execution behavior.

The implementation agent must inspect primary upstream documentation/repository status when making the decision and record the exact version and rationale.

### 6.2 Maintained parser outcome

When an acceptable parser exists:

1. add it only to the owning codec/config crate where practical;
2. migrate all direct YAML parse calls to the codec;
3. preserve fixtures and compatibility-sensitive semantics;
4. remove `serde_yaml` from active production dependencies;
5. avoid keeping both parsers in the normal runtime graph after the compatibility tests pass.

A short-lived test-only differential fixture may compare old/new behavior during implementation, but the deprecated parser must not remain a permanent production fallback without a documented time-bounded reason.

### 6.3 Compatibility importer outcome

When no acceptable maintained parser exists:

1. contain existing YAML parsing behind `YamlCompatibilityCodec`;
2. prohibit new YAML serialization and new direct consumers;
3. make TOML or JSON5 the documented format for newly generated configuration/assets;
4. add an explicit import/conversion command or load-and-rewrite path only if CodeGG already owns configuration writes;
5. preserve YAML reads for existing files through a documented compatibility window;
6. mark the deprecated parser as isolated technical debt with one owner and removal condition;
7. do not pretend the dependency was removed when it remains in the compatibility graph.

If the deprecated parser remains, default builds should include it only when existing YAML compatibility actually requires it. Do not add a runtime format feature that silently disables reading existing user files.

## 7. Supported syntax contract

Build a focused fixture set from actual repository/user formats, not generic YAML conformance cases.

Cover at minimum where currently used:

- mappings, sequences, scalars, booleans, integers, floats, nulls;
- quoted and multiline strings;
- nested agent/tool/skill/config structures;
- enum/tag representation used by Serde;
- aliases/anchors or merge keys only if existing supported files use them;
- duplicate keys;
- unknown fields under current Serde annotations;
- malformed indentation and truncated documents;
- multi-document streams;
- non-UTF-8 input rejection;
- excessive nesting/size behavior through surrounding loader limits.

For each edge case, decide and document one of:

- supported and preserved;
- rejected with a typed diagnostic;
- previously accidental/unsupported and now explicitly rejected;
- compatibility-only behavior with a removal note.

Do not broaden YAML support merely because a new parser accepts more syntax.

## 8. Write and migration behavior

Inventory every YAML serialization call.

Preferred target:

- YAML is read compatibility only;
- new generated files use the existing canonical TOML or JSON5 format appropriate to that subsystem;
- existing user files are not rewritten without an explicit user action;
- conversion preserves a backup or writes a distinct destination;
- comments/formatting are not claimed to survive when the chosen representation cannot preserve them.

If a production YAML write path is genuinely required, the maintained parser must support deterministic serialization for the relevant types, and focused round-trip tests must cover semantic—not textual—equivalence.

Do not create a general configuration migration engine in this milestone.

## 9. Lightweight dependency-maintenance contract

Add or update a concise maintainer document describing a manual periodic/release-time process:

1. inspect `cargo outdated` or equivalent locally when desired;
2. inspect RustSec/advisory status through the repository's chosen manual command;
3. review direct dependencies with known deprecation/archive notices;
4. update one bounded dependency group at a time;
5. run focused consumer tests plus `scripts/verify.sh quick`;
6. run package/release checks only during an actual release;
7. record material compatibility decisions in an ADR or subsystem plan, not in CI artifacts.

This document must not prescribe a fixed release cadence, scheduled workflow, automatic pull request bot, all-target matrix, or mandatory evidence bundle.

## 10. Expected production-code changes

Expected areas:

- one codec/parser module in `codegg-config` or the existing shared format owner;
- agent, command, skill, and config loaders migrated to that codec;
- Cargo manifests and lockfile;
- focused fixtures/tests;
- documentation for canonical formats and YAML compatibility;
- a cheap direct-import guard;
- concise dependency-maintenance documentation.

Avoid modifying domain structs solely to accommodate a parser unless their current Serde representation is ambiguous or invalid. Any such change requires explicit compatibility fixtures.

## 11. Storage, protocol, migration, and compatibility effects

Storage:

- no database migration expected;
- user configuration/assets may have an optional file-format conversion path;
- conversion must be idempotent, explicit, and non-destructive;
- existing YAML files remain the source of truth until the user chooses conversion.

Protocol:

- no daemon/client protocol change expected;
- parse diagnostics may gain format/source/line/column fields through backward-compatible additions;
- no agent/tool authority change.

Compatibility:

- supported YAML fixtures must continue to load;
- parser-specific accidental behavior may be rejected only with a documented fixture and rationale;
- TOML/JSON5 continue unchanged;
- reload behavior preserves last-known-good configuration on failure;
- old YAML files are not silently rewritten.

## 12. Ordered work packages

### Work package A — Usage and fixture inventory

1. enumerate every direct YAML call and dependency owner;
2. classify read/write/test/generated use;
3. collect a minimal representative fixture set from repository formats;
4. record edge-case behavior actually relied upon;
5. identify loader size/depth controls.

### Work package B — Codec boundary

1. introduce the format-neutral typed error and codec functions;
2. move existing calls behind the boundary without changing parser first;
3. add the direct-import static guard;
4. verify all current fixtures through the boundary.

### Work package C — Candidate evaluation

1. inspect current maintained parser candidates using primary upstream sources;
2. evaluate toolchain, Serde, diagnostics, feature coverage, dependency graph, and maintenance state;
3. choose maintained replacement or compatibility-importer outcome;
4. record the decision and rejected alternatives concisely.

### Work package D — Parser migration or containment

For maintained replacement:

1. swap codec implementation;
2. resolve only evidenced semantic differences;
3. remove deprecated production dependency;
4. update fixtures and diagnostics.

For compatibility importer:

1. isolate deprecated parser in one module/crate;
2. prohibit new write/direct-use paths;
3. establish TOML/JSON5 generation guidance;
4. add optional non-destructive conversion only if existing product ownership supports it.

### Work package E — Loader and reload correctness

1. preserve discovery/precedence;
2. preserve last-known-good state on parse failure;
3. ensure malformed/oversized input cannot partially apply;
4. add focused reload error tests.

### Work package F — Maintenance documentation

1. add the small manual dependency-maintenance procedure;
2. remove stale claims about parser support/maintenance;
3. keep release checks manual and release-time only;
4. avoid new automation.

## 13. Focused verification

Expected command shape:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test <config codec target> -- --test-threads=1
cargo test <agent loading target> -- --test-threads=1
cargo test <command loading target> -- --test-threads=1
cargo test <skill parser target> -- --test-threads=1
cargo test <reload/last-known-good target> -- --test-threads=1
scripts/verify.sh quick
```

Use current target names. Required focused fixtures include valid representative files and malformed/duplicate/multi-document/unknown-field cases relevant to current formats.

When a replacement parser is selected, record:

- exact direct dependency and version;
- default/added features;
- before/after `cargo tree -e features` excerpt for the parser path;
- supported-fixture result;
- diagnostic location result;
- deprecated dependency absence from the active production graph.

Do not add external parser services or a large conformance download.

## 14. Static guards

Add one cheap guard that permits YAML parser imports only in:

- the codec implementation;
- focused parser tests;
- an explicitly time-bounded compatibility module when that outcome is chosen.

The guard must reject direct imports in agent, command, skill, and unrelated config modules. It must fail closed on matcher failure and use the repository's existing guard conventions.

Do not add a general dependency scanner.

## 15. Acceptance criteria

M006 is complete only when:

- all production YAML parsing flows through one codec boundary;
- current direct `serde_yaml` calls outside that boundary are removed;
- a maintained replacement is adopted, or the deprecated parser is explicitly contained as read-only compatibility debt with a removal condition;
- no archived/unmaintained replacement is accepted without an explicit exceptional ADR;
- representative config, agent, command, and skill YAML fixtures load correctly;
- malformed and compatibility-sensitive fixtures produce typed diagnostics without panic or partial apply;
- last-known-good reload behavior is preserved;
- no unintended YAML rewrite occurs;
- TOML/JSON5 behavior remains unchanged;
- the static guard catches a temporary direct-import violation;
- focused tests, `scripts/verify.sh quick`, and hosted verification pass;
- dependency maintenance remains manual and proportional;
- no release automation, dependency bot, broad parser framework, or feature removal is introduced.

## 16. Stop conditions

Stop and report blocked when:

- no maintained parser satisfies actual supported fixtures and policy requires immediate removal of YAML compatibility;
- YAML semantics currently relied upon cannot be represented by the canonical domain types without a public format decision;
- a production write path requires comment/format preservation not supported by the available parser model;
- parser replacement requires a toolchain/MSRV change outside current policy;
- M005 manifest changes are not stable;
- malformed input exposes a general reload/atomicity defect outside this milestone.

Record one narrow follow-up or compatibility decision. Do not build a parser or migration framework.

## 17. Required closure evidence

`plans/closure/runtime-safety-resource-footprint/006-status.md` must include:

- accepted commit/PR;
- complete YAML usage inventory and final owners;
- parser decision, version, maintenance evidence, and rejected alternatives;
- codec boundary and static guard result;
- valid and malformed fixture outcomes across config/agent/command/skill paths;
- reload/last-known-good evidence;
- serialization/conversion disposition;
- before/after parser dependency feature summary;
- focused commands, quick verification, and hosted run reference;
- unresolved findings by severity;
- explicit statement whether the deprecated parser is removed or remains compatibility-only.
