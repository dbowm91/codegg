# Development Verification and Release Milestone 006 — Final Evidence and Release Documentation Closure

Status: ready; Provider Connections dependency closed; strict closure pending DVR review

Repository baseline:

- implementation/evidence review head: `db890ac138fe18c6bae3de991b70dc007789c8a0`
- M005 registration commit: `cc4d69d0243f89062c7fa43ea64be35191864433`
- M005 initial implementation commit: `e90a78e926125aec3b8f472edb96a4b45b9f76f6`

Source roadmap/addendum:

- `plans/subsystems/development-verification-release-roadmap.md`
- `plans/subsystems/development-verification-release-correctness-closure-addendum.md`

Predecessor implementation plan:

- `plans/implementation/development-verification-release/005-green-verification-and-crates-io-closure.md`

Historical closure records retained for traceability:

- `plans/closure/development-verification-release/001-status.md`
- `plans/closure/development-verification-release/002-status.md`
- `plans/closure/development-verification-release/003-status.md`
- `plans/closure/development-verification-release/004-status.md`

Target independent closure record:

- `plans/closure/development-verification-release/006-status.md`

Primary class: evidence and documentation closure

Secondary class: narrow verification-policy correction

## 1. Objective

Close the Development Verification and Release subsystem without expanding its maintenance surface.

M006 must:

1. prove the canonical local verification contract against the final implementation head rather than inheriting evidence from an earlier commit;
2. obtain and record one successful GitHub Actions `verify` run for that same accepted revision;
3. make the Tokio test-flavor guard fully satisfy its stated fail-closed regression contract;
4. regenerate the package inventory from the actual Cargo manifests and package graph;
5. correct the manual crates.io release procedure so its initial-release, ownership, publication-order, and index-propagation commands are operationally truthful;
6. reconcile the addendum, registry, implementation status, and closure evidence without rewriting historical records.

This is a final evidence/documentation corrective pass. It must not become another general test, scheduler, process-management, LSP, Tool Programs, or runtime cleanup series.

## 2. Why this milestone is required

M005 landed the major implementation corrections:

- one bounded read-only routine CI job remains;
- the Tokio flavor check has a checked-in historical baseline;
- local and hosted broad verification use `CARGO_BUILD_JOBS=1`, `RUST_MIN_STACK=33554432`, and `--test-threads=1`;
- scheduler-owned Bash/Python routing no longer relies on the original panic path;
- workspace manifests use publishable metadata and path-plus-version internal dependencies;
- `RELEASING.md` once again describes manual crates.io publication.

Post-implementation review found that strict closure is still unsupported.

### F06 — No accepted current-head verification proof

The repository advanced through multiple runtime/test-fix commits after the first M005 evidence was recorded. The final reviewed head includes scheduler shutdown, managed-process cleanup, Tool Programs artifact handling, LSP test consolidation, and other substantive changes.

Consequences:

- earlier local `quick` or `full` results cannot prove the final head;
- no observable successful hosted `verify` run is attached to the accepted head through the available repository evidence;
- strict closure must remain blocked until both local and hosted verification are tied to one exact revision.

### F07 — The checked-in package inventory is stale and internally inconsistent

`plans/closure/development-verification-release/005-package-inventory.md` does not match current manifests. Examples include incorrect internal dependency relationships for `codegg-core`, `codegg-providers`, and `egglsp`.

Its verification section also claims that full verification passed while recording a failing integration test, and its CI section refers to an earlier implementation commit plus uncommitted fixes.

Consequences:

- the inventory cannot serve as release or closure evidence;
- publication order and registry-sequencing claims must be regenerated from `cargo metadata` and the current manifests;
- no closure record may copy the stale evidence.

### F08 — `RELEASING.md` contains incorrect initial-release and propagation commands

The current procedure:

- runs `cargo owner --list` for crate names that do not yet exist;
- checks registry propagation for packages before those packages are published;
- conflates name availability, existing ownership, authentication, and first publication.

Consequences:

- the documented first-release preflight cannot be completed as written;
- the index-propagation checks do not verify the packages just published;
- a maintainer following the guide could interpret expected first-publication behavior as an ownership failure.

### F09 — The Tokio guard does not completely satisfy the M005 fail-closed contract

The current guard is substantially better than the original, but:

- it skips the entire `examples` directory;
- malformed bare Tokio attributes can become `???UNRESOLVED_*` identities and therefore be represented in the baseline;
- self-tests do not exercise the full production comparison path for accepted historical entries, new violations, stale entries, and malformed source.

Consequences:

- new bare tests under a broad excluded directory may evade the guard;
- malformed source can become baseline-compatible instead of failing closed;
- the guard's regression semantics are not fully demonstrated by tests.

### F10 — Active planning and evidence status remain inconsistent

The registry marks M005 `closing`, while the addendum milestone table still says `ready for handoff`. No independent M005 closure record exists, which is correct, but strict closure ownership has not been transferred to a final evidence pass.

Consequences:

- the planning control surface is ambiguous;
- a later reviewer cannot identify one authoritative final closure milestone;
- historical M005 implementation evidence and final-head evidence are not clearly separated.

## 3. Ownership boundary

M006 owns only:

- focused corrections to `scripts/check-tokio-test-flavors.py` and its focused tests;
- individual baseline updates required by those guard corrections;
- exact-current-head local verification evidence;
- exact-current-head GitHub Actions evidence;
- regeneration/correction of the M005 package inventory;
- correction of `RELEASING.md` commands and explanatory text;
- narrow active-document references that directly conflict with the corrected verification/release contract;
- planning-state transfer from M005 implementation to M006 closure;
- an independent final closure record after implementation lands.

M006 does not own:

- additional scheduler, process-tree, Tool Programs, LSP, TUI, storage, provider, Git, or agent-loop refactors;
- changing product behavior to make an unrelated test green;
- reducing coverage, ignoring tests, broad exclusions, or failure suppression;
- converting all historical bare Tokio tests;
- adding CI jobs, matrices, artifacts, schedules, release automation, audit automation, or cross-target hot-path builds;
- actual crates.io publication;
- renaming crates or selecting a release version;
- adding a release framework, local registry service, task runner, or bespoke evidence generator;
- rewriting M001–M005 historical records to make their original claims appear correct.

If current-head verification exposes a new product/runtime correctness failure, stop. Record the exact command, test, and failure, leave M006 blocked, and create a separate narrowly owned corrective plan. Do not absorb unrelated implementation into M006.

## 4. Required invariants

### 4.1 CI topology

- Exactly one ordinary GitHub Actions workflow/job remains for routine verification.
- The job remains read-only.
- No publication credentials, write permissions, tag/release commands, artifact uploads, matrices, schedules, audit installation, real-server installation, or cross-target builds are introduced.
- The check name remains `verify` unless an existing repository setting makes that impossible and the maintainer explicitly approves a change.

### 4.2 Verification evidence

- All closure evidence names one exact accepted commit SHA.
- `scripts/verify.sh quick` and `scripts/verify.sh full` are rerun after the final M006 code/document changes.
- A result from an earlier commit cannot be reused as final-head proof.
- A nonzero command is never called a pass.
- Test counts, ignored counts, and environment-dependent skips are reported accurately.
- No `|| true`, `continue-on-error`, blanket exclusion, ignored test, or equivalent concealment is introduced.

### 4.3 Tokio regression guard

- All repository-owned Rust source locations are scanned unless a path is generated/vendor/build output and the exclusion is individually justified.
- `examples/` is not excluded as a whole merely because examples are optional.
- Baseline entries remain stable `relative/path.rs::function_name` identities.
- A bare attribute without an unambiguous following function is an immediate parse/guard error and cannot be baseline-accepted.
- A new bare test fails.
- A stale baseline entry fails.
- Duplicate, wildcard, directory, malformed, and unresolved baseline entries fail.
- Normal CI never rewrites the baseline.

### 4.4 Package and release evidence

- The inventory is derived from current `cargo metadata` and manifest inspection.
- Direct internal dependency relationships and topological layers match the manifests exactly.
- Name availability is not described as crates.io ownership.
- Existing-crate ownership checks and first-publication name/authentication checks are documented separately.
- Index propagation checks query packages that were actually published in the immediately preceding step.
- Dependent-package registry failures caused solely by unpublished internal dependencies are recorded as sequencing constraints, not successful dry-runs.
- No actual publication occurs.

### 4.5 Planning and independent review

- M005 remains a historical implemented/conditional milestone whose strict closure was not independently accepted.
- M006 becomes the sole ready implementation plan.
- The M006 implementation agent must not create `plans/closure/development-verification-release/006-status.md`.
- A separate reviewer must inspect the final implementation revision and create the closure record.

## 5. Ordered work packages

### Work package A — Freeze the final implementation baseline

Intent:

Ensure every later evidence item refers to one exact repository revision.

Required actions:

1. Record the starting head and compare it with M005 registration:

```bash
git rev-parse HEAD
git status --short
git log --oneline cc4d69d0243f89062c7fa43ea64be35191864433..HEAD
```

2. Inventory all files modified after M005 registration and classify them as:

- M005 verification/release implementation;
- follow-up verification/test repair;
- unrelated later product/runtime cleanup.

3. Do not treat an earlier M005 test summary as current-head evidence.

Acceptance evidence:

- one exact starting SHA is recorded;
- the worktree is clean before verification;
- later evidence clearly identifies whether it was collected before or after M006 corrections.

### Work package B — Complete the Tokio guard contract

Intent:

Make the baseline-aware guard genuinely fail closed without broad source exclusions.

Required actions:

1. Review `SKIP_PATHS`.

Allowed exclusions are limited to non-source/build/vendor locations such as:

- `.git`;
- `target`;
- `node_modules` only when it contains third-party generated/dependency content.

Remove the blanket `examples` exclusion. If repository examples contain historical bare Tokio tests, add their individual identities to the baseline.

2. Change malformed-attribute handling:

- `#[tokio::test]` not followed by an unambiguous function must produce a dedicated error;
- do not emit `???UNRESOLVED_*` as a baseline-compatible identity;
- reject any existing baseline entry containing unresolved markers.

3. Add focused tests using temporary repositories/files rather than only testing isolated regular expressions.

Required cases:

- explicit `current_thread` passes;
- explicit bounded `multi_thread` passes;
- historical baseline identity passes;
- new bare test fails;
- stale baseline entry fails;
- duplicate baseline entry fails;
- wildcard/directory suppression fails;
- bare test in `examples/` is detected;
- intervening `#[cfg(...)]`, doc comments, and attributes map to the correct function;
- malformed attribute with no following function fails;
- malformed baseline unresolved identity fails;
- deterministic sorted `--emit-current` output;
- missing/unreadable baseline fails.

4. Keep `--self-test` only if it remains useful, but the focused production-path tests are authoritative.

5. Regenerate the baseline deterministically only after the scanner behavior is corrected. Review the diff; do not bulk-add unrelated paths.

Acceptance commands:

```bash
python3 scripts/check-tokio-test-flavors.py --self-test
python3 -m unittest discover scripts/tests -p '*tokio*' -v
python3 scripts/check-tokio-test-flavors.py
python3 scripts/check-tokio-test-flavors.py --emit-current > /tmp/codegg-tokio-current.txt
diff -u scripts/tokio-test-flavor-baseline.txt /tmp/codegg-tokio-current.txt
```

The final diff may account for comments/header lines in the baseline, but the identity sets must match exactly.

### Work package C — Regenerate the package graph and evidence

Intent:

Replace stale hand-written relationships with a truthful current-manifest inventory.

Required actions:

1. Run:

```bash
cargo metadata --format-version 1 --no-deps > /tmp/codegg-metadata.json
```

2. For every workspace package, record:

- package name;
- manifest path;
- version;
- publish policy;
- description/license/repository/readme/rust-version state;
- direct normal internal dependencies;
- direct build internal dependencies;
- dev-only internal dependencies separately;
- topological publication layer;
- crates.io name existence/availability evidence and date;
- package/dry-run result and exact exit code.

3. Derive the publication graph from normal/build dependencies used in packaged artifacts. Do not infer dependencies from the root's aggregate list or copy the old table.

4. Correct `plans/closure/development-verification-release/005-package-inventory.md` in place as implementation evidence, or replace it with a clearly named M006 evidence file if preserving the stale file is preferable for traceability. If replaced, mark the old file historical/stale and link the new file.

5. Remove contradictory statements. A full verification run cannot be described as passing while one included workspace test is recorded as failing.

6. Package verification:

For each leaf package:

```bash
cargo package -p <package> --list
cargo package -p <package>
cargo publish --dry-run -p <package>
```

For packages blocked by unpublished internal dependencies:

- run `cargo package -p <package> --list`;
- inspect the generated normalized manifest with `cargo package -p <package> --no-verify` when needed;
- run the ordinary package/dry-run command and record the exact registry-sequencing failure;
- verify that no local metadata, path-only dependency, missing file, build-script, or package-content defect precedes the expected registry error;
- label the result `blocked until dependency publication`, not `pass`.

7. Inspect package contents for secrets, databases, logs, target output, planning archives, oversized fixtures, and repository-only paths.

Acceptance evidence:

- the table matches all current manifests;
- topological order is internally consistent;
- every leaf package passes local package and dry-run checks;
- every dependent result is honestly classified;
- no actual publication is performed.

### Work package D — Correct `RELEASING.md`

Intent:

Make the manual release guide executable for both initial and subsequent releases.

Required corrections:

1. Separate three concepts:

- Cargo authentication (`cargo login` or credential-provider setup);
- crate-name existence/availability before first publication;
- owner membership for crates that already exist.

2. Initial release:

- do not require `cargo owner --list <name>` to succeed for a crate that does not yet exist;
- document how to check whether a name already exists and what to do if it is owned by another account;
- state that the first successful publisher becomes/establishes ownership according to crates.io behavior;
- treat a name conflict as a maintainer decision blocker; do not rename automatically.

3. Subsequent release:

- use `cargo owner --list` only for crates already published;
- verify the authenticated maintainer has access before irreversible publication.

4. Correct the topological table and commands from the regenerated inventory.

5. Correct index-propagation checks. After publishing a set of packages, query those exact package names/versions before publishing dependents. Do not query a dependent package before it is published as proof that its dependency propagated.

Example shape:

```bash
cargo publish -p codegg-config
cargo search codegg-config --limit 1
# or use an exact registry/API/version check documented by the implementation

cargo publish -p codegg-providers
cargo search codegg-providers --limit 1
```

6. Document bounded retry/wait behavior for index propagation without a tight infinite loop.

7. Preserve:

- manual cadence/version ownership;
- `scripts/verify.sh full` as the canonical pre-release verification command;
- immutable-version and partial-failure handling;
- yanking semantics;
- optional manual tags/GitHub binary releases;
- no hosted publication authority.

8. Ensure examples use actual package names and current publication order.

Acceptance evidence:

- a reviewer can follow the reversible first-release steps without encountering an expected 404 as an unexplained failure;
- every propagation check refers to a package just published;
- initial and subsequent release paths are distinguishable;
- no credential is printed or committed.

### Work package E — Run current-head local verification

Intent:

Obtain one coherent local proof after all M006 corrections.

Required clean-checkout sequence:

```bash
git status --short
bash -n scripts/verify.sh
python3 scripts/check-tokio-test-flavors.py --self-test
python3 -m unittest discover scripts/tests -p '*tokio*' -v
python3 scripts/check-tokio-test-flavors.py
scripts/verify.sh quick
scripts/verify.sh full
```

Record:

- exact commit SHA;
- operating system and architecture;
- Rust/Cargo versions;
- exit code for every command;
- workspace test totals;
- ignored-test totals;
- effective `CARGO_BUILD_JOBS`, `RUST_MIN_STACK`, and test-thread setting;
- duration only as observational information, not a closure criterion.

Rules:

- rerun the complete sequence after the final code/document commit;
- do not patch additional unrelated tests in this milestone;
- if a product/runtime test fails, stop and register a separate blocker;
- environment-dependent optional real-server tests remain out of scope.

Acceptance evidence:

- every required local command exits zero on the final M006 implementation revision;
- no included workspace failure is described as outside scope while still claiming `full` passed.

### Work package F — Obtain hosted `verify` evidence

Intent:

Prove that the retained one-job workflow succeeds in its actual hosted environment.

Required actions:

1. Confirm workflow policy:

```bash
find .github/workflows -maxdepth 1 -type f \( -name '*.yml' -o -name '*.yaml' \) -print | sort
rg --line-number --glob '.github/workflows/*.{yml,yaml}' \
  'cargo publish|gh release create|CARGO_REGISTRY_TOKEN|contents: write|packages: write|id-token: write|upload-artifact|download-artifact|schedule:|matrix:' \
  .github/workflows
```

2. Push the final M006 implementation revision through the normal `main` or pull-request workflow.

3. Record:

- workflow name;
- run ID and URL;
- exact commit SHA;
- trigger;
- job name;
- conclusion;
- step conclusions;
- confirmation that no artifact/release authority exists.

4. The hosted run must correspond to the same code/document revision accepted by closure. A later functional commit invalidates the earlier run and requires rerunning local and hosted verification.

5. If GitHub Actions is disabled, unavailable, not installed for the repository, or branch protection references removed checks:

- record the exact observable condition;
- leave M006 blocked;
- do not add a replacement evidence workflow or broaden CI.

Acceptance evidence:

- one successful `verify` job attached to the accepted implementation SHA;
- no hidden failing or skipped required step;
- no release/artifact workflow added.

### Work package G — Reconcile planning and prepare independent closure

Intent:

Leave one unambiguous final closure boundary.

Implementation-agent updates:

- mark M005 as `implemented; strict closure transferred to M006` in active planning state;
- mark M006 `implemented` or `closing` after implementation lands;
- mark the correctness addendum `closing` with M006 as current milestone;
- update `plans/registry.md` so M006 is `closing` and no implementation plan remains ready;
- preserve `plans/closure/development-verification-release/006-status.md` as absent;
- retain historical M001–M005 records without cosmetic rewriting.

Independent reviewer responsibilities:

- compare the final M006 implementation SHA against this plan;
- inspect the Tokio guard and focused tests;
- verify package inventory against manifests;
- inspect `RELEASING.md` initial/subsequent paths and propagation commands;
- verify local logs and hosted run evidence;
- confirm no unrelated product implementation was absorbed;
- create `plans/closure/development-verification-release/006-status.md` only if no unresolved high or medium finding remains;
- close the addendum/subsystem and update the registry.

## 6. Failure and stop conditions

Stop implementation and leave M006 blocked when any of the following occurs:

- `scripts/verify.sh quick` or `full` fails because of a product/runtime correctness issue outside this plan;
- no successful hosted `verify` run can be obtained for reasons not correctable within the existing one-job workflow;
- a required crate name is already owned by another party or the maintainer lacks required ownership for an existing crate;
- package verification exposes a missing packaged source, invalid normalized dependency, or build requirement needing product/package architecture changes;
- completing the pass would require another CI job, release automation, a local registry service, package rename, actual publication, or broad runtime refactor;
- the implementation agent cannot distinguish expected registry sequencing from a local packaging defect.

A stop-condition report must name:

- exact commit SHA;
- exact command;
- exit code;
- minimal failure output;
- owning subsystem;
- proposed next plan boundary.

## 7. Required final evidence matrix

The independent closure record must contain a table with at least:

| Requirement | Command/source | Revision | Result | Notes |
|---|---|---|---|---|
| Workflow count/topology | workflow inventory/search | final SHA | pass/fail | one read-only verify job |
| Tokio focused tests | self-test + unittest suite | final SHA | pass/fail | includes examples/malformed cases |
| Tokio repository baseline | guard command | final SHA | pass/fail | no new/stale entries |
| Quick verification | `scripts/verify.sh quick` | final SHA | pass/fail | exact exit code |
| Full verification | `scripts/verify.sh full` | final SHA | pass/fail | test/ignored totals |
| Package graph | metadata + manifests | final SHA | pass/fail | exact dependency layers |
| Leaf package checks | package/dry-run commands | final SHA | pass/fail | package-by-package |
| Dependent package checks | package/normalized manifest/dry-run | final SHA | pass/blocked/fail | sequencing only if applicable |
| Release guide correctness | document inspection | final SHA | pass/fail | initial/subsequent paths |
| Hosted verify | GitHub Actions run | final SHA | pass/fail | run ID/URL/conclusion |
| Planning state | registry/addendum | final SHA | pass/fail | M006 closing before review |

No row may say `pass` when the referenced command returned nonzero.

## 8. Completion definition

M006 is complete only when:

- the guard scans repository-owned example source and fails closed on malformed bare attributes;
- focused production-path guard tests cover accepted/new/stale/malformed behavior;
- the package inventory exactly matches current manifests and contains no contradictory verification claims;
- `RELEASING.md` has correct first-release, subsequent-release, ownership, publication-order, and propagation instructions;
- `scripts/verify.sh quick` exits zero on the final implementation SHA;
- `scripts/verify.sh full` exits zero on the same SHA;
- one successful hosted `verify` job is recorded for that SHA;
- the one-job, read-only, non-release CI architecture remains intact;
- no unrelated runtime implementation is included;
- planning state identifies M006 as the final closure owner;
- a separate reviewer creates `plans/closure/development-verification-release/006-status.md` and finds no unresolved high or medium issue.

## 9. Handoff notes for a smaller implementation model

Execute work packages in order. Do not begin with broad test fixes.

1. Correct and test the Tokio guard.
2. Regenerate the package graph from metadata/manifests.
3. Correct `RELEASING.md` from that graph.
4. Run all local verification on the resulting head.
5. Push and obtain hosted evidence.
6. Update planning to `closing` without creating the closure record.

When a command fails, determine whether it is:

- a defect introduced by the narrow M006 changes;
- an expected unpublished-dependency sequencing constraint;
- an unrelated product/runtime failure;
- an external GitHub/crates.io blocker.

Only the first category is directly fixable inside M006. Record and stop for the others according to this plan.
