# Post-Audit Correctness, Simplification, and Footprint Milestone 008 — Integration, Measurement, and Closure

Status: blocked

Source subsystem roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`
- Milestone 008

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Primary class: integration, evidence, and closure

Dependencies:

- hard: M001 untrusted HTTP safety and bounded streaming
- hard: M002 daemon stop identity and CLI JSON correctness
- hard: M003 TUI text-layout correctness and render deduplication
- hard: M004 dependency feature slimming and upstream review
- hard: M005 routine CI and static-guard simplification
- hard: M006 test stack/resource root-cause correction
- hard: M007 execution-model pass-through cleanup

Target closure record:

- `plans/closure/post-audit-correctness-simplification/008-status.md`

## 1. Objective

Integrate M001-M007 on one accepted repository revision, reconcile documentation/planning state, capture concise final dependency and release-size evidence, run one proportionate broad verification pass, and close the workstream without creating another evidence-only milestone.

M008 is not permission to reopen implementation scope. It may make only narrow integration/documentation fixes necessary to reconcile already-completed milestones.

## 2. Explicit non-goals

Do not:

- introduce new product features;
- reopen the single-binary decision;
- add a CI lane, matrix, benchmark/size/audit gate, artifact workflow, scheduled job, or release automation;
- perform another broad dependency cleanup beyond M004 evidence;
- run repeated local full-workspace verification solely to create more evidence;
- create M009 for documentation polish or unavailable external evidence;
- make the remaining runtime-safety Landlock Linux evidence condition a blocker for this independent workstream;
- rewrite closure history to imply tests/evidence that were not actually run.

## 3. Required integration review

Before verification, inspect the accepted revisions for M001-M007 and ensure:

- no milestone reverted another milestone's production changes;
- CI/docs agree after M005/M006;
- dependency measurements from M004 reflect the final integrated tree or are repeated if later source/manifest changes materially affect linkage;
- execution-ownership/sandbox guards still reflect final paths after M007;
- no stale references remain to deleted Tokio scanner/baseline or global 32 MiB stack requirement;
- no accidental feature removal, protocol change, storage migration, daemon topology change, or release automation landed.

Correct only concrete integration drift. Do not start a new cleanup sweep.

## 4. Final measurement protocol

Use one fresh release-build environment or isolated target directory.

Record:

- final commit SHA;
- `rustc --version`;
- host/target triple;
- release profile and feature set;
- stripped default release binary bytes;
- feature-tree observations relevant to accepted M004 changes;
- top crate contributors when `cargo bloat` is locally available.

Required commands or repository-equivalent forms:

```bash
cargo tree -e features --locked
cargo tree -d --locked
cargo build --release --locked
```

Optional diagnostic-only:

```bash
cargo bloat --release --crates -n 40
```

Compare against the M004/pre-workstream baseline using the same platform/toolchain when available. If exact baseline reproduction is impossible, state that and compare only measurements that are technically comparable.

Do not introduce a numeric closure threshold. Feature-neutral reductions and code simplification are the goal; measurement provides evidence, not a gate.

## 5. Verification posture

The broad verification for this workstream is intentionally compact.

Required locally on the final integrated revision:

```bash
scripts/verify.sh quick
```

Also run any high-value static guards still part of the resulting routine contract when `verify.sh quick` does not already include them.

Use the existing hosted `verify` workflow as the broad workspace test/Clippy integration result. Do not duplicate an expensive local full workspace run merely because hosted CI will run it.

If hosted CI fails:

- fix a concrete regression within the responsible prior milestone scope;
- rerun the existing workflow;
- do not add resources/lanes/matrices to make a failure disappear without understanding it.

## 6. Documentation reconciliation

Review and update only affected sections of:

- `AGENTS.md`;
- `architecture/testing.md`;
- `architecture/tool.md` / networking-security documentation if M001 changes ownership semantics;
- daemon/client lifecycle documentation if M002 changes legacy stop behavior;
- TUI docs only if they describe changed wrapping/reasoning behavior;
- dependency/footprint documentation only where accepted M004 decisions warrant it;
- execution architecture docs after M007;
- `plans/registry.md` and this subsystem roadmap status.

Do not duplicate detailed implementation evidence into architecture docs.

## 7. Closure-record reconciliation

M001-M007 should each have a compact closure record. M008 closure summarizes the integrated workstream and links to them rather than copying every test result.

M008 closure must classify any residual item as:

- critical/high/medium correctness or security -> workstream cannot close; create one corrective implementation plan only for the concrete defect;
- low polish/measurement preference -> defer without another milestone;
- external/operational evidence unrelated to this roadmap -> record but do not block unless an acceptance criterion explicitly required it.

The workstream should not enter an endless corrective/evidence loop.

## 8. Ordered work packages

### Work package A — Integration audit

1. identify final commits for M001-M007;
2. inspect combined diff for overlap/regression;
3. reconcile CI/stack/docs/guards;
4. correct only integration defects.

### Work package B — Final measurements

1. use fresh/isolated release build;
2. record feature tree and duplicate tree;
3. record default release size and optional top contributors;
4. compare to technically comparable baseline evidence;
5. avoid unsupported causal claims when several milestones changed the same graph.

### Work package C — Final verification

1. run focused checks required by any integration fix;
2. run `scripts/verify.sh quick` once on final revision;
3. obtain one normal hosted `verify` result for the final PR/head;
4. fix concrete failures without expanding workflow architecture.

### Work package D — Documentation and registry closure

1. create/update M001-M007 closure records if implementation agents did not already do so;
2. create `008-status.md` integrated closure;
3. mark roadmap closed only if acceptance criteria are met;
4. move registry row to recently closed and clear implementation-ready entries;
5. archive/supersede interim plans only if current repo convention calls for it; do not delete traceability.

## 9. Storage, protocol, migration, and compatibility review

Explicitly confirm:

- no database/storage schema migration occurred;
- daemon protocol version/schema remains compatible;
- existing config/state/endpoint paths remain unchanged;
- CLI/tool schemas remain compatible except stricter enforcement of already-documented limits/safety;
- TUI capabilities remain present;
- default supported Cargo features remain available;
- manual release cadence remains unchanged;
- one active daemon remains the runtime authority;
- single-binary topology remains accepted.

Any contradiction is a blocking finding, not documentation polish.

## 10. Security review

Confirm integrated behavior for:

- M001 actual-address SSRF pinning and bounded response collection;
- M002 daemon identity verification before signalling;
- no weakening of sandbox/execution ownership in M007;
- no dependency change in M004 introducing a known advisory on the locked accepted tree.

A second generalized security audit is not required. Review the exact changed boundaries.

## 11. Acceptance criteria

M008 and the roadmap close only when:

- M001-M007 are closed with evidence;
- final integrated source contains no unresolved critical/high/medium correctness or security finding from this workstream;
- `scripts/verify.sh quick` passes on the final integrated revision;
- the existing hosted `verify` workflow passes on the final accepted head;
- final release/feature-tree measurements are recorded concisely;
- no supported feature was removed;
- no binary split, new CI lane/matrix, release automation, continuous size/audit gate, broad dependency rewrite, or architecture redesign was introduced;
- CI/documentation no longer references deleted Tokio-flavor machinery or an unjustified global 32 MiB stack workaround;
- execution architecture documentation reflects the actually simplified M007 path;
- closure records state what was not run rather than implying evidence;
- the registry and roadmap are updated to `closed` with one final controlling closure record.

## 12. Stop conditions

Do not mark closed when:

- hosted verify fails for a reproducible code regression;
- an M001 SSRF/body-bound invariant is unproven;
- daemon stop can still signal an unverified PID;
- global stack workaround remains without the explicit narrow fallback evidence allowed by M006;
- dependency slimming removed a supported feature;
- M007 obscured or bypassed execution policy/provenance.

If a concrete blocker exists, create at most one narrowly scoped corrective plan referencing the responsible milestone and this closure audit. Do not create a generic "final final verification" plan.

## 13. Required closure evidence

`plans/closure/post-audit-correctness-simplification/008-status.md` must include:

- final accepted commit/PR;
- links to M001-M007 closure records;
- compact requirement-to-evidence matrix;
- final release measurement environment and results;
- final feature/dependency observations;
- `scripts/verify.sh quick` result;
- hosted `verify` run URL/result;
- security/compatibility/storage/protocol review;
- unresolved findings by severity;
- final recommendation: `closed` or one concrete corrective pass required.
