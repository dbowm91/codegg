# Dependency Security and Workspace Consolidation M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/dependency-security-workspace-consolidation/004-reusable-crate-boundary-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md#m004--reusable-crate-boundary-qualification`

Repository baseline reviewed: `b1aa4d50d8867b24249c4e6aadc6d41e73e2b4a3` (M002/M003 closure; plan snapshot baseline `b3459640`)

Implementation commits or pull requests:

- `b95d37ec` — M004: qualify reusable crate boundaries without new splits

## 1. Executive finding

M004 is complete. The four candidate crates have explicit dispositions
backed by package, test, and docs evidence; the three CodeGG-owned
crates have explicit retain-internal dispositions with named reasons; no
new crate was introduced; nothing was published. `egggit` and
`eggsentry` retain narrow generic ownership without absorbing CodeGG
orchestration; `codegg-protocol` is independently consumable as the
product protocol package; `eggcontext` no longer conflates the
deterministic tokenizer layer with the volatile model-name policy —
`count_with_tokenizer` / `estimate_for_tokenizer` /
`TokenizerType::as_str` / `TokenizerType::encoding_name` are now the
explicit tokenizer-selection API, while `for_model` plus multipliers
remain a documented replaceable convenience policy. Package metadata,
READMEs, and `cargo package` verification are clean for all crates
marked publishable. Broad verification is green; no new
medium-or-higher finding introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Every candidate has an explicit disposition with evidence | §3 disposition matrix below (4 publishable, 3 CodeGG-specific retain-internal) | pass | No `internal-generalizable` needed; every boundary was already stable enough to qualify or explicitly retain |
| `egggit` retains narrow generic ownership, absorbs no `codegg-git` mutation | `crates/egggit/src/lib.rs` scope docs; structured read APIs unchanged; `process::run` documented as unprivileged plumbing with no permission policy; no new dependency on root/`codegg-git` | pass | WP1; forward `cargo tree -p egggit` shows only `serde`/`thiserror`/`tokio` (+`tempfile` dev) |
| `eggsentry` machine-readable identifiers stable; scanners deterministic and library-oriented; orchestration stays outside | `src/lib.rs` stability/versioning docs; `finding.rs` enum docs + `label()` == serde `snake_case`; deterministic sha256 IDs; `ProfileConfig` is a host-decoupled placeholder (no `codegg::` type dependency); no tool/gate/daemon code in crate | pass | WP2; consumers match on typed values, never on `evidence`/`reasons`/`summary` strings |
| `codegg-protocol` consumable independently as product protocol | `src/lib.rs` crate docs (data contracts only, no runtime services; third-party consumption without root); compatibility section (`PROTOCOL_VERSION = 2` + per-surface versions, additive-minor / breaking-major, bounded payloads); forward `cargo tree -p codegg-protocol` shows only `serde`/`serde_json`/`thiserror` | pass | WP3; intentionally CodeGG-specific but publishable as the product protocol package |
| `eggcontext` separates deterministic tokenizer truth from volatile model-name policy | New `count_with_tokenizer`, `estimate_for_tokenizer`, `as_str`, `encoding_name`; `estimate_with_provenance` delegates to the deterministic layer after `for_model` mapping with historical GPT-4 pinning preserved; crate docs define the two layers; no exact-vendor claims (Claude/Gemini documented heuristic, `approximate` flag) | pass | WP4; 3 new tests pin the separation and compat |
| Package metadata/docs and dry-runs clean for publishable crates | Per-crate `README.md` + `readme`/`keywords`/`categories` in each `Cargo.toml` + minimal-default comments; `cargo package -p <crate> --allow-dirty` with verification passes for all four | pass | §4 |
| No new crate without second-consumer/stable-boundary justification | No new `[package]` added; change is docs + metadata + 4 small `eggcontext` API additions; WP5 recorded below without modifying the three internal crates | pass | `git diff --stat` shows 4 READMEs + edits only |
| Broad CodeGG verification green | fmt check, per-crate check/test/clippy, workspace clippy all-features, `verify.sh quick`, root `context::compaction` suite | pass | §4 |

### Candidate disposition matrix

| Crate | Disposition | Reason |
|---|---|---|
| `egggit` | publishable | Read-only Git/worktree facts, status, refs, blame, conflict/operation state, patch validation; no root dependency; local `EgggitError`; minimal defaults documented; README + metadata complete; package verify pass; 75 tests through the public boundary |
| `eggsentry` | publishable | Deterministic command/security classification, secret/unsafe scanning, dependency-file classification, structured findings with stable `snake_case` identifiers and deterministic IDs; no root/host-type dependency; rule/versioning expectations documented; package verify pass; 159 tests |
| `codegg-protocol` | publishable (product-specific) | Wire/domain DTOs only, no runtime services; consumable without root; compatibility expectations documented; package verify pass; 177 tests. Not a general Eggstack utility by design |
| `eggcontext` | publishable | Deterministic `cl100k_base`/`o200k_base` BPE layer explicitly separable from volatile `for_model`/multiplier policy; approximation honestly documented with provenance; compat preserved; package verify pass; 21 tests |
| `codegg-git` | CodeGG-specific, retain internal | Mutation/risk/policy behavior tightly coupled to CodeGG execution authority; correctly depends on generic `egggit` facts (right direction); no second consumer |
| `codegg-config` | CodeGG-specific, retain internal | Schema/compatibility semantics CodeGG-owned (`notify`/`toml`/`json5`/`serde_norway`); no independent stable consumer |
| `codegg-providers` | CodeGG-specific, retain internal | Provider/auth/connection contracts evolve with CodeGG (`codegg-config`, `sqlx`, `eggfetch-core`, crypto); no independent stable consumer |

## 3. Production implementation evidence

Landed changes (M004 implementation commit `b95d37ec`):

- `crates/egggit/src/lib.rs`: crate docs rewritten to a generic
  read-only-facts contract. Mutation stays with the host application and
  its permission policy; `process` is documented as unprivileged
  shell-free plumbing. No API signature changed.
- `crates/eggsentry/src/lib.rs`: generic deterministic-library docs plus
  a stable-identifiers section (typed `snake_case` values are wire IDs;
  `evidence`/`reasons`/`recommendation`/`summary` are human diagnostics)
  and a versioning section (additive-minor, rename/remove-major,
  pattern refinement-patch).
- `crates/eggsentry/src/finding.rs`: stability docs on `Severity`,
  `SecurityCategory`, and `label()` (canonical wire string == serde
  representation; match on typed values, never on evidence text).
- `crates/eggsentry/src/profile.rs`: comment decoupled from
  `codegg::SecurityConfig` (now "host security-scan limits"); no type
  dependency existed or was added.
- `crates/codegg-protocol/src/lib.rs`: new crate docs (data contracts
  only; independent third-party consumption; compatibility: core version
  2 + per-surface versions, additive-minor / breaking-major, bounded
  payloads). No type changed.
- `crates/eggcontext/src/lib.rs`: two-layer docs plus new explicit
  deterministic API (`count_with_tokenizer`, `estimate_for_tokenizer`,
  `TokenizerType::as_str`, `TokenizerType::encoding_name`);
  `estimate_with_provenance` refactored to map-then-delegate with the
  historical GPT-4 `encoding_for_model` pinning preserved
  byte-identically; 3 new tests
  (`explicit_tokenizer_bypasses_model_mapping`,
  `estimate_for_tokenizer_matches_model_provenance`,
  `tokenizer_identifiers_are_stable`).
- Package metadata (all four candidates): `README.md` added;
  `Cargo.toml` gains `readme`, `keywords`, `categories`, and a
  minimal-default comment. No versions, dependencies, features, or
  dependency sources changed.
- `codegg-git`, `codegg-config`, `codegg-providers`: intentionally
  untouched (WP5 negative boundary).
- Planning: implementation plan `ready` → `active` → `implemented`;
  registry M004 `ready` → `active` (closure moves it to `closed`).

### Deliberately absent

Publishing or release automation; provider framework redesign;
renaming crates; extracting more root modules; moving orchestration,
permission, daemon, scheduler, or network policy into generic packages;
creating `eggnet-policy`, `egghttp`, or other one-consumer microcrates;
stabilizing volatile model-tokenizer mappings as exact vendor truth.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo check -p egggit --all-targets --all-features --locked
cargo check -p eggsentry --all-targets --all-features --locked
cargo check -p eggcontext --all-targets --all-features --locked
cargo check -p codegg-protocol --all-targets --all-features --locked
cargo test -p egggit --all-features --locked -- --test-threads=1
cargo test -p eggsentry --all-features --locked -- --test-threads=1
cargo test -p eggcontext --all-features --locked -- --test-threads=1
cargo test -p codegg-protocol --all-features --locked -- --test-threads=1
cargo clippy -p egggit --all-targets --all-features --locked -- -D warnings
cargo clippy -p eggsentry --all-targets --all-features --locked -- -D warnings
cargo clippy -p eggcontext --all-targets --all-features --locked -- -D warnings
cargo clippy -p codegg-protocol --all-targets --all-features --locked -- -D warnings
cargo package -p egggit --allow-dirty
cargo package -p eggsentry --allow-dirty
cargo package -p eggcontext --allow-dirty
cargo package -p codegg-protocol --allow-dirty
cargo test --lib context::compaction -- --test-threads=1
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
scripts/verify.sh quick
cargo tree --locked -i codegg@0.1.0 --depth 1
cargo tree -p egggit --locked --depth 1
cargo tree -p eggsentry --locked --depth 1
cargo tree -p eggcontext --locked --depth 1
cargo tree -p codegg-protocol --locked --depth 1
cargo tree -p codegg-git --locked --depth 1
cargo tree -p codegg-config --locked --depth 1
cargo tree -p codegg-providers --locked --depth 1
```

`cargo package` ran with verification enabled (not `--no-verify`), so
each packaged copy was compiled. No publish was performed.

### Results

- `cargo fmt --all -- --check`: pass.
- Per-crate `cargo check --all-targets --all-features --locked`: pass
  (all four).
- Per-crate `cargo test --all-features --locked`: `egggit` 75 passed,
  0 failed; `eggsentry` 159 passed, 0 failed; `eggcontext` 21 passed
  (18 pre-existing + 3 new), 0 failed; `codegg-protocol` 177 passed,
  0 failed. Tests exercise the crates through their public boundary
  (read-only git ops incl. non-repo error paths; classifier/scanner/
  dependency/profile suites incl. deterministic-ID stability;
  tokenizer mapping/multiplier/provenance suites; protocol DTO/
  projection/reducer suites).
- Per-crate `cargo clippy --all-targets --all-features --locked --
  -D warnings`: pass (all four).
- `cargo package -p <crate> --allow-dirty` (with build verification):
  pass for all four (`egggit` 15 files; `eggsentry` 10 files;
  `eggcontext` 5 files; `codegg-protocol` 27 files — counts include the
  new READMEs).
- CodeGG compatibility: `cargo test --lib context::compaction` — 33
  passed, 0 failed (root `eggcontext` wrapper behavior preserved);
  reverse `cargo tree -i` shows root/`codegg-core` consume the
  candidates while no candidate depends back on root `codegg`.
- `cargo clippy --workspace --all-targets --all-features --locked --
  -D warnings`: pass.
- `scripts/verify.sh quick`: passed (fmt, agent schema, core-boundary,
  sandbox, execution-ownership, tui-authority guards, workspace check).

## 5. Invariant review

- No crate depends back on the root `codegg` package: forward trees for
  all four candidates show only external leaves; reverse trees show the
  consumption direction is root/core → candidates. Pass.
- Extracted crates gained no UI/server/daemon ownership: no new
  `axum`/`ratatui`/`daemon`/`scheduler` dependency; `Cargo.toml` diffs
  are metadata-only. Pass.
- Public APIs expose domain values, not root-private types: no new
  imports of root-private types; `eggsentry` host config remains a
  crate-local placeholder; errors stay crate-local (`EgggitError`,
  `EggsecError`, `EggcontextError`, protocol `thiserror` types). Pass.
- No forced semver commitments for unstable contracts: volatile
  `eggcontext` model mapping documented as replaceable policy, not
  exact truth; `eggsentry` additive-vs-breaking versioning documented;
  protocol additive-vs-breaking documented. Pass.
- Existing CodeGG imports/behavior compatible: `eggcontext` refactor
  preserves historical GPT-4 pinning byte-identically and root wrapper
  tests pass; other crates changed docs/metadata only. Pass.
- No release automation or publication: dry-runs only, no publish, no
  new CI lane or bot. Pass.

## 6. Failure and recovery review

- `eggcontext` refactor is pure computation: invalid/unknown model
  hints still fall back to `cl100k_base`; empty text still yields 0;
  saturating `usize` conversion retained. Covered by the 21-test suite
  plus the 33-test root compaction suite.
- Docs/metadata-only changes (`egggit`, `eggsentry`,
  `codegg-protocol`, all `Cargo.toml`s) cannot fail at runtime; the
  packaged-copy builds prove the manifests are well-formed.
- No storage, protocol wire change, scheduler, daemon, authorization,
  or migration path touched. No new failure mode introduced. Rollback
  is `git revert` of the M004 commits; downstream consumers see no wire
  change.

## 7. Migration and compatibility review

- No schema, config, protocol, or data migration. No MSRV change (all
  manifests still inherit workspace `rust-version = 1.89`). No public
  API removal: the `eggcontext` change is purely additive (4 new
  items) with existing signatures and byte-identical counts preserved.
- `codegg-protocol` wire surface unchanged; compatibility expectations
  are now written down but no version was bumped.
- `eggsentry` wire identifiers unchanged (`label()` output equals the
  pre-existing serde `snake_case`); the docs now forbid parsing human
  strings, which matches existing typed usage.
- Reverse-dependency check confirms CodeGG compatibility: root and
  `codegg-core` still resolve to the same candidates with no feature
  change (candidates define no optional features).

## 8. Security review

- No advisory silenced; no audit-ignore touched; no dependency
  version/feature/source changed in any manifest (metadata-only diff
  outside `eggcontext`'s additive API).
- `egggit` hardened env policy untouched; the docs now state explicitly
  that `process::run` enforces no permission policy so hosts do not
  mistake plumbing for authorization.
- `eggsentry` patterns, severities, and ID hashing untouched; no
  secret, network, sandbox, Landlock, SSRF, archive, plugin, or
  redaction behavior changed.
- `eggcontext` touches only local BPE counting; no secret or network
  surface.

## 9. Documentation and operations

- Added: `crates/egggit/README.md`, `crates/eggsentry/README.md`,
  `crates/eggcontext/README.md`, `crates/codegg-protocol/README.md`;
  crate-level docs for all four candidates as described in §3.
- Updated: the four `Cargo.toml`s (readme/keywords/categories +
  minimal-default comments); implementation plan status
  (`ready` → `active` → `implemented` in the implementation commit).
- Checked and intentionally left unchanged: `README.md` (no per-crate
  claims to narrow), `architecture/` (no ownership boundary moved),
  `.opencode/skills/` (no contract change), prior closure records
  (history preserved), `codegg-git`/`codegg-config`/`codegg-providers`
  (WP5 negative boundary), second-consumer microcrate proposals
  (rejected per non-goals).
- No new CI lane, scanner, bot, size gate, or release automation added.
  Temporary diagnostics (`cargo tree`, packaged file counts) remain
  closure evidence only.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| info | `eggcontext` Claude/Gemini multipliers (1.4/1.2) remain heuristics, not vendor measurements | Callers treating them as exact would mis-budget context | None in this workstream; docs + `approximate` flag + replaceable `for_model` policy already record this. A future consumer needing exact vendor tokenizers should vendor those encodings rather than reopening M004. |
| info | `codegg-protocol` is publishable but intentionally CodeGG-specific | A general Eggstack consumer should not mistake it for a generic utility | None; README and §2 matrix state this explicitly. |

No critical, high, or medium findings. No unexplained memory-safety
advisory in the supported graph from this change (no dependency
changed).

## 11. Roadmap disposition

Milestone M004 closed. M005 remains blocked solely on the external
generalized updater interface (its M002 hard dependency is satisfied;
M004 closure adds no updater contract and unblocks nothing new). No
previously registered plan was unblocked by M004 beyond what M002
already unblocked: audit of the registry's Blocked work section and the
subsystem dependency graph shows no registered plan lists M004 as a
hard or interface dependency, and the remaining unrelated conditional
blockers (architecture-convergence M009 compatible-host Clippy
evidence, runtime-safety C002 Landlock fixture evidence) are untouched
by this workstream. If source evidence later reveals a genuine second
consumer plus a small stable generic subset for any retained-internal
crate, that is a separate follow-up, not a silent M004 expansion.

## 12. Registry updates

- `plans/registry.md`: M004 implementation plan `active` → `closed`;
  subsystem current milestone `M003 closed, M004 active` → `M004
  closed, M005 blocked (external updater interface only)`; Blocked-work
  M005 row retained with the external-interface blocker narrowed as
  before; this closure added under Recently closed work with the M004
  implementation commit `b95d37ec`.
- Subsystem roadmap: M004 status updated to closed; M005
  externally-blocked note retained; completion criteria now satisfied
  for M001-M004 (M005 may remain blocked per the roadmap's explicit
  allowance); no baseline history rewritten.
- Implementation plan: Status `implemented` (closure record is the
  gate; this record marks it closed).
