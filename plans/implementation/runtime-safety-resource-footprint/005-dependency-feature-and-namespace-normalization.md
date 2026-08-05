# Runtime Safety, Resource Control, and Footprint Milestone 005 — Dependency Feature and Namespace Normalization

Status: implemented

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- Milestone 005

Repository baseline reviewed: `4d540ce315c9ef2a1c07544cd42df0efc43708e1`

Dependencies:

- no hard dependency;
- may execute in parallel with M001 and M004;
- M006 has a soft dependency on this milestone;
- M007 has a hard dependency on this milestone.

Primary class: dependency ownership, compatibility, and footprint

Target closure record:

- `plans/closure/runtime-safety-resource-footprint/005-status.md`

## 1. Objective

Make workspace dependency features explicit, remove avoidable duplicate capability stacks and umbrella crates, and replace the MD5-based memory namespace with the already available SHA-256 implementation without orphaning durable data.

This milestone targets identified, measurable waste while preserving all current user-visible features and supported build gates.

Required focus areas:

1. reqwest/TLS feature ownership;
2. SQLx default-feature removal and exact feature selection;
3. text-only clipboard feature selection;
4. umbrella `futures` and `grep` dependency narrowing where source imports permit;
5. MD5 namespace removal with compatibility;
6. duplicate or inconsistent workspace dependency declarations exposed during the same audit;
7. feature-tree evidence for default and optional builds.

## 2. Explicit non-goals

This milestone must not:

- remove clipboard, server, image, plugin, LSP, Python, provider, database, or other documented capability;
- remove Wasmtime or change the plugin ABI merely because Wasmtime is large; it is already optional and should remain on its supported LTS line;
- replace SQLx, reqwest, Tokio, rustls, RustPython, tiktoken, syntect, comrak, or other major components wholesale;
- perform the YAML/parser migration assigned to M006;
- split daemon and TUI binaries; that decision belongs to M007;
- chase every transitive duplicate version without material evidence;
- add `cargo bloat`, `cargo machete`, `cargo deny`, or other tools to routine CI;
- add automatic dependency update bots or release automation;
- change network trust roots or TLS behavior accidentally through feature removal.

## 3. Current implementation evidence

Inspect at minimum:

- root `Cargo.toml` and `Cargo.lock`;
- `crates/codegg-core/Cargo.toml`;
- `crates/codegg-providers/Cargo.toml`;
- `crates/codegg-config/Cargo.toml`;
- `crates/egglsp/Cargo.toml`;
- `crates/eggcontext/Cargo.toml`;
- `crates/eggsentry/Cargo.toml`;
- `crates/egggit/Cargo.toml`;
- `crates/codegg-git/Cargo.toml`;
- source imports for reqwest, SQLx, arboard, futures, grep, md5, and sha2;
- feature declarations and optional module gates;
- memory namespace construction and durable storage lookup/migration paths.

The reviewed baseline shows:

- root reqwest is configured with rustls and default features disabled, while `codegg-core` declares plain `reqwest = "0.12"`, which can reactivate default/native TLS and related defaults in the unified feature graph;
- SQLx declarations do not consistently disable defaults, enabling features such as macros, migrate, and JSON whether or not each crate uses them;
- root `arboard` is optional but retains the dependency's image-data default even though current use is text clipboard;
- the umbrella `futures` crate is used where narrower `futures-util`/`futures-core` imports may be sufficient;
- root declares the umbrella `grep` crate alongside individual grep crates even though the implementation can likely use the narrower crates directly;
- memory namespace construction uses MD5 over a path-derived value while SHA-256 is already available elsewhere;
- optional Wasmtime/plugin support is correctly feature-gated and is not a default-footprint defect;
- release profile already enables LTO, stripping, and one codegen unit, so manifest/source feature ownership is the next reduction layer.

The implementation agent must confirm actual imported APIs and feature consumers before editing. A dependency must not be narrowed by assumption.

## 4. Invariants that cannot regress

- HTTPS behavior and certificate validation remain correct for every reqwest consumer;
- only one intended reqwest TLS backend is enabled in the default workspace graph;
- SQLx SQLite/runtime functionality, migrations used by production, macros used by source, and type integrations continue to compile where actually required;
- clipboard text copy/paste remains available under the existing default feature behavior;
- image support elsewhere remains unchanged; disabling arboard image-data must not remove CodeGG's separate image feature;
- asynchronous stream/sink utilities continue to compile with narrower futures crates;
- grep matching/search behavior remains unchanged;
- durable memory remains readable after namespace-hash migration;
- optional feature builds remain explicit and supported;
- no dependency is moved into the default graph solely to simplify manifests;
- Cargo.lock remains reproducible and no unrelated mass upgrade is performed.

## 5. Required baseline evidence

Before editing, record:

```bash
cargo tree -e features -i reqwest
cargo tree -e features -i native-tls
cargo tree -e features -i rustls
cargo tree -e features -i sqlx
cargo tree -e features -i arboard
cargo tree -e features -i futures
cargo tree -e features -i grep
cargo tree -d
```

Use `cargo tree` syntax supported by the repository toolchain. If a package is absent, record that rather than treating the command as failure.

Also record:

- default feature set;
- no-default-features build expectations;
- optional `server`, `plugins`, and `image` feature ownership;
- actual SQLx APIs/features used per crate;
- every call site of the current MD5 namespace.

Do not store a large generated report in the repository. Summarize relevant before/after lines in the closure record.

## 6. Required dependency changes

### 6.1 Reqwest and TLS

Normalize every workspace reqwest declaration to explicit features.

Preferred default contract:

- `default-features = false`;
- rustls-based TLS feature selected consistently;
- enable `json`, `stream`, `multipart`, compression, charset, HTTP/2, or other features only where source actually uses them;
- avoid crate-local plain reqwest declarations that reactivate defaults in the unified graph.

Use workspace dependency inheritance when it reduces divergence without obscuring crate-specific optionality.

Acceptance evidence must show that native-tls/OpenSSL is not enabled by CodeGG's default reqwest graph unless another required dependency independently and intentionally needs it. If it remains, identify the exact owner and do not claim removal.

Do not change root certificate behavior or proxy semantics without focused tests.

### 6.2 SQLx

For each SQLx-using crate:

1. identify database backend(s), runtime, TLS, macros, migrate, chrono/time/uuid/json/type features actually used;
2. set `default-features = false`;
3. enable the exact required feature set;
4. remove duplicated features inherited through another declaration;
5. compile migration and macro call sites explicitly.

Do not remove `migrate` or macros merely because they appear large if production source uses `sqlx::migrate!`, derive macros, query macros, or migration APIs. The goal is exact ownership, not a predetermined feature list.

### 6.3 Clipboard

Current clipboard use is text-only. Configure `arboard` with `default-features = false` and the minimum platform features required for text clipboard operation.

Keep CodeGG's current default clipboard feature enabled unless the repository already makes it optional. Do not conflate arboard's image-data feature with CodeGG's separate image rendering feature.

Add or retain focused text copy/paste tests where the platform abstraction allows deterministic testing. Platform integration smoke may remain manual.

### 6.4 Futures

Inventory imports such as:

- `futures::StreamExt`;
- `futures::SinkExt`;
- `futures::future`;
- `futures::channel`;
- `futures::executor`.

Replace the umbrella crate with `futures-util`, `futures-core`, `futures-channel`, or other narrow crates only when all used APIs are covered. If the executor feature is genuinely used, keep the required crate and document it.

Prefer workspace-level consistency. Do not mechanically rewrite imports without compiling every affected target.

### 6.5 Grep crates

If source uses `grep_regex`, `grep_searcher`, and/or `grep_matcher` directly, remove the umbrella `grep` dependency and import from the narrow crates.

M004 owns search behavior. Coordinate source changes to avoid conflict, but do not change matching semantics in M005.

### 6.6 Namespace hashing

Replace the MD5 dependency with the existing SHA-256 implementation and an explicit domain-separated input, for example:

```text
"codegg-memory-namespace-v1\0" || stable_project_identity_or_legacy_input
```

First determine whether the namespace is:

- durable database/storage identity;
- cache-only/reconstructible;
- external/public contract;
- path-derived legacy compatibility key.

If durable data exists, implement compatibility:

1. compute the new namespace for writes;
2. on lookup miss, compute the legacy MD5 namespace;
3. read legacy data when present;
4. migrate/copy/rename under a transaction or idempotent operation;
5. prevent duplicate merge or repeated migration;
6. retain legacy read support for a documented compatibility window if old stores may be reopened.

If the namespace is demonstrably non-durable and reconstructible, document that evidence and perform a clean replacement without migration.

Do not replace one weak hash with a truncated SHA-256 value that recreates the same collision space without justification.

## 7. Additional manifest normalization

While editing the identified manifests, correct only adjacent issues with direct evidence:

- duplicate dependency declarations with conflicting features;
- optional dependencies accidentally enabled unconditionally;
- target-specific dependencies declared globally;
- unused direct dependencies proven by source/build inspection;
- feature names that enable no consumer or unintentionally enable a broad dependency.

Do not turn this into a workspace-wide dependency modernization campaign. RustPython parser size, tiktoken, syntax rendering, and LSP archive dependencies should be measured in M007 and changed only through a separate bounded plan if a no-feature-loss replacement is clear.

## 8. Expected production-code changes

Expected areas:

- workspace and crate Cargo manifests;
- source imports for narrowed futures/grep crates;
- memory namespace construction and compatibility lookup/migration;
- focused dependency/feature tests;
- optional documentation of feature ownership;
- Cargo.lock updates limited to the intended manifest changes.

No protocol, scheduler, provider, or UI redesign is expected.

## 9. Storage, protocol, migration, and compatibility effects

Storage:

- namespace migration may touch durable memory metadata/data;
- migration must be idempotent and transactional where the store supports transactions;
- no unrelated database schema change;
- closure record must state whether legacy keys remain readable.

Protocol:

- no public protocol change expected;
- namespace hashes must not become user-facing identifiers unless they already are;
- dependency features must not alter API serialization.

Compatibility:

- existing HTTPS endpoints, proxies, provider requests, SQLite stores, clipboard text, grep, and async behavior remain compatible;
- optional feature builds remain supported;
- unsupported clipboard platform behavior must not be newly introduced by disabling defaults;
- old memory stores remain usable.

## 10. Ordered work packages

### Work package A — Feature ownership inventory

1. capture baseline feature trees;
2. map reqwest APIs/features per crate;
3. map SQLx APIs/features per crate;
4. map arboard, futures, and grep imports;
5. trace MD5 namespace durability and consumers.

### Work package B — Reqwest/TLS normalization

1. establish one workspace-level explicit reqwest baseline;
2. update crate declarations to inherit or explicitly match;
3. add only actual crate-specific features;
4. compile/test representative provider/network paths;
5. confirm default graph TLS ownership.

### Work package C — SQLx exact features

1. disable defaults per declaration;
2. enable exact runtime/backend/type/macro/migration features;
3. compile migration and database targets;
4. test store open/migrate/basic operations;
5. record remaining feature owners.

### Work package D — Clipboard/futures/grep narrowing

1. disable arboard image-data/defaults while preserving text;
2. replace umbrella futures imports/dependency where feasible;
3. remove umbrella grep dependency where feasible;
4. compile default and affected optional targets.

### Work package E — Namespace migration

1. introduce domain-separated SHA-256 namespace;
2. implement legacy MD5 read/migrate or document reconstructible data;
3. remove direct MD5 dependency when no runtime call remains;
4. add old/new/idempotency tests;
5. document compatibility window if required.

### Work package F — Feature-tree reconciliation

1. rerun focused `cargo tree -e features` commands;
2. compare only intended changes;
3. inspect `cargo tree -d` for accidental new duplicates;
4. update concise feature ownership docs/closure evidence.

## 11. Focused verification

Required build/test coverage:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo check --workspace --all-targets --no-default-features
cargo check --workspace --all-targets --features server
cargo check --workspace --all-targets --features plugins
cargo check --workspace --all-targets --features image
cargo test <provider/network target> -- --test-threads=1
cargo test <database/migration target> -- --test-threads=1
cargo test <memory namespace migration target> -- --test-threads=1
scripts/verify.sh quick
```

Run feature checks only when those feature names/targets remain valid and were affected. Do not automatically combine every optional feature into one artificial all-features requirement if the repository's supported combinations differ.

Record before/after focused feature trees for reqwest, native-tls/rustls, SQLx, arboard, futures, grep, and MD5/SHA-256 ownership.

Manual platform clipboard smoke is sufficient if deterministic automated clipboard integration is unavailable.

## 12. Static guards

Prefer manifest/source review and focused tests. Add only cheap guards with clear value, for example:

- reject plain `reqwest = "..."` declarations in workspace crates when workspace policy requires explicit defaults;
- reject direct `md5` dependency/use after compatibility code no longer needs it;
- reject arboard image-data reactivation unless a named feature owns it.

Do not create a general dependency policy engine or make `cargo tree` output a routine committed artifact.

## 13. Acceptance criteria

M005 is complete only when:

- every reqwest declaration has explicit feature/default ownership;
- default graph does not unintentionally enable native TLS through `codegg-core` or another CodeGG declaration;
- SQLx defaults are disabled and exact required features compile/test;
- text clipboard remains supported without unnecessary image-data defaults, unless a tested image clipboard requirement is found and documented;
- umbrella futures and grep dependencies are narrowed where source use permits, with any retained umbrella dependency justified;
- MD5 is removed from new namespace writes;
- durable legacy memory remains readable/migrated idempotently or is proven reconstructible;
- Cargo.lock changes are limited and reviewed;
- default, no-default, and affected optional feature builds pass;
- focused provider/database/memory tests pass;
- `scripts/verify.sh quick` and hosted verification pass;
- no feature is removed and no release/CI automation is added;
- closure evidence accurately states any dependency that could not be removed.

## 14. Stop conditions

Stop and report blocked when:

- disabling a default feature reveals an undocumented production requirement with no focused owner;
- TLS normalization would change trust-root/proxy behavior without a compatibility decision;
- SQLx feature use is generated or macro-dependent in a way that cannot be confirmed by compile/tests;
- the MD5 namespace is externally visible or shared with another service and migration requires a protocol decision;
- clipboard text requires a platform feature currently bundled with image-data and no narrow configuration exists;
- source conflicts with an active M004 branch cannot be resolved without mixing search behavior and dependency changes.

Record the narrow unresolved dependency rather than forcing removal.

## 15. Required closure evidence

`plans/closure/runtime-safety-resource-footprint/005-status.md` must include:

- accepted commit/PR;
- before/after dependency feature summary;
- reqwest TLS ownership result;
- SQLx exact feature list per crate;
- clipboard feature result;
- futures/grep umbrella disposition;
- MD5-to-SHA-256 compatibility/migration result;
- default/no-default/affected feature build outcomes;
- focused provider/database/memory tests;
- quick/hosted verification reference;
- unresolved findings by severity;
- explicit confirmation that no user-visible feature was removed.
