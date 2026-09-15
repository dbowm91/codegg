# Dependency Security and Workspace Consolidation M004 — Reusable Crate Boundary Qualification

Status: blocked

Repository baseline reviewed: `b3459640765261057e902a9df05bc71faa6cecc4`

Source subsystem roadmap:

- `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md`

Hard predecessor:

- M002 accepted closure.

Primary class: infrastructure + polish.

## 1. Objective

Qualify existing extracted crates for independent reuse and publication where their ownership boundary is already general, while explicitly rejecting new crate splits that would only relocate CodeGG-specific maintenance.

The milestone does not require publishing anything. It produces package-ready boundaries and explicit dispositions.

## 2. Candidate set and intended disposition

Evaluate these first:

- `egggit` — strong general candidate: read-only Git/worktree facts, status, refs, blame, conflict/operation state and patch validation;
- `eggsentry` — strong general candidate: deterministic command/security classification, secret/unsafe-code scanning, dependency-file classification and structured findings;
- `codegg-protocol` — product-specific but independently useful for third-party/front-end CodeGG clients;
- `eggcontext` — potentially general, but current model-name → tokenizer policy is volatile and must not be presented as universally exact.

Default retain-internal disposition unless contrary evidence appears:

- `codegg-git` — mutation/risk/policy behavior is tightly coupled to CodeGG execution authority;
- `codegg-config` — schema/compatibility semantics are CodeGG-owned;
- `codegg-providers` — provider/auth/connection contracts currently evolve with CodeGG and lack an independent stable consumer.

## 3. Invariants

- No crate depends back on the root `codegg` package.
- Extracted crates must not gain UI/server/daemon ownership merely to become standalone.
- Public APIs expose domain values, not root-private implementation types.
- Publication readiness must not force semver commitments for unstable CodeGG-internal contracts.
- Existing CodeGG imports and behavior remain compatible through re-exports or narrow adapters where necessary.
- No release automation or crates.io publication occurs in this milestone.

## 4. Scope and non-goals

In scope:

- package metadata, README/docs, feature/default review and public API hygiene for the candidate crates;
- package dry-runs and dependency checks;
- small refactors needed to make an existing boundary genuinely standalone;
- `eggcontext` separation of deterministic tokenizer primitives from volatile model-name mapping policy if source review confirms the current API conflates them;
- explicit documented disposition for every candidate.

Out of scope:

- extracting more root modules simply because they are large;
- renaming established crates without a compatibility need;
- publishing or automating releases;
- provider framework redesign;
- moving CodeGG orchestration/permission/daemon logic into generic packages;
- creating `eggnet-policy`, `egghttp`, or other one-consumer microcrates.

## 5. Qualification criteria

A crate is publishable/reusable only if:

1. its purpose can be described without reference to CodeGG runtime internals;
2. it has no dependency on the root application;
3. its public API is coherent enough for an independent consumer;
4. errors/types do not leak unrelated CodeGG ownership;
5. default features are minimal and documented;
6. package metadata/license/repository/readme are complete;
7. `cargo package --allow-dirty` (or equivalent dry-run) succeeds when appropriate;
8. tests exercise the crate through its public boundary.

A package may be marked `internal-generalizable` rather than publishable if API stability is not yet justified.

## 6. Ordered work packages

### WP1 — `egggit`

Audit its read-only contract, Git process/environment policy, public errors/types, package metadata and docs.

Do not absorb mutating `codegg-git` behavior. If a useful API requires CodeGG permissions/workflow context, it belongs outside `egggit`.

Run package-local tests and packaging dry-run. Produce disposition: publishable, internal-generalizable, or CodeGG-specific with named reason.

### WP2 — `eggsentry`

Audit machine-readable finding/category/severity identifiers for stability. Downstream consumers must not need to parse human diagnostic strings as identifiers.

Keep scanner/profile primitives deterministic and library-oriented. CodeGG tool/gate orchestration remains outside the crate.

Run package-local tests and packaging dry-run; document rule/versioning expectations if considered publishable.

### WP3 — `codegg-protocol`

Confirm the crate contains wire/domain protocol contracts rather than runtime services. Ensure docs explain compatibility expectations and that public types can be consumed without root CodeGG.

Because this package is intentionally CodeGG-specific, publication readiness is useful even though it is not a general Eggstack utility.

### WP4 — `eggcontext`

Current token accounting combines tokenizer primitives with a model-name classifier whose policy can age quickly. Review and, if needed, separate:

- explicit tokenizer selection/counting API (`cl100k_base`, `o200k_base`, approximation/provenance);
- convenience model-family/name mapping policy.

Do not claim exact vendor tokenization for Claude/Gemini when the implementation is heuristic. Preserve compatibility through wrappers if root callers depend on current names.

Only mark the crate publishable if the deterministic lower-level API is stable and the volatile mapping policy is clearly documented/replaceable.

### WP5 — Negative-boundary confirmation

Record why `codegg-git`, `codegg-config`, and `codegg-providers` remain internal. Do not modify them merely to make the list symmetrical.

If source evidence reveals a second real consumer and a small stable generic subset, open a separate follow-up rather than expanding M004.

## 7. Verification

For each candidate crate:

```bash
cargo check -p <crate> --all-targets --all-features --locked
cargo test -p <crate> --all-features --locked -- --test-threads=1
cargo clippy -p <crate> --all-targets --all-features --locked -- -D warnings
cargo package -p <crate> --allow-dirty
```

Use `cargo package --list`/dry-run behavior appropriate to the package; do not publish.

Then run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
scripts/verify.sh quick
```

## 8. Acceptance criteria

- Every candidate has an explicit publishable/internal-generalizable/CodeGG-specific disposition with evidence.
- `egggit` and `eggsentry` retain narrow generic ownership and do not absorb CodeGG orchestration.
- `codegg-protocol` can be consumed independently as the product protocol package.
- `eggcontext` no longer conflates low-level tokenizer truth with volatile model-name policy if that conflation remains in the implementation at handoff time.
- Package metadata/docs and package dry-runs are clean for crates marked publishable.
- No new crate is introduced without a second-consumer or clear stable-boundary justification.
- Broad CodeGG verification remains green.

## 9. Stop conditions

Stop a candidate's refactor and mark it internal if publication readiness requires:

- importing root CodeGG services;
- stabilizing large volatile APIs prematurely;
- duplicating provider/auth/permission logic;
- significant semantic redesign unrelated to the crate's current owner boundary.

## 10. Required closure evidence

Include a candidate disposition matrix, public API/package changes, package dry-run results, package-local tests, CodeGG compatibility evidence, and any follow-up explicitly deferred rather than silently folded into the milestone.
