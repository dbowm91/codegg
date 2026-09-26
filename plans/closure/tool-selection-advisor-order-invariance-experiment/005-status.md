# Tool-Selection Advisor Order-Invariance Experiment M005 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/tool-selection-advisor-order-invariance-experiment/005-fresh-v4-preregistered-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md#m005--fresh-v4-preregistered-qualification`

Repository baseline reviewed: `7259292ef371ecd116e8911f9c2ddca7032e2a2e`

Implementation commits or pull requests:

- None — hard dependency (positive M004 operating point) remains unsatisfied; qualification did not begin (see §1).

## 1. Executive finding

M005 closes **blocked** without a v4 holdout, preregistration, or
release-mode run, by the plan's own hard-dependency gate (§2: "M005 ...
Hard dependencies: M001-M003 positive closure; M004 positive
retrieval/promotion operating-point closure").

M004 is closed **negatively**
(`plans/closure/tool-selection-advisor-order-invariance-experiment/004-status.md`;
implementations `f04ba608` + `3a9428a9`): the preregistered sweep completed
all three retrieval universes with zero authority violations but best recall
0.9444 misses 0.99/0.98/0.95 at K<=32, so `select_retrieval_operating_point`
returns the specified fail-closed error and no retrieval mode/K, promotion
threshold, or artifact was frozen. Without a frozen operating point there is
no Commit A to preregister (§4), no §12 one-run to execute, and no §13
disposition to record. Creating a v4 holdout now would produce an
unrunnable qualification input with no qualified system to measure.

Historical v3 remains immutable diagnostic evidence; its D disposition is
not rewritten. Live-primary-model M004 remains blocked.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| M001-M003 positive closure | M001 closed (`001-status.md`, `79045a4c`), M002 closed (`002-status.md`, `722d8e7c`), M003 closed positively (`003-status.md`, `c96fbdfa`, span-packed selected) | pass | Hard dep 1 satisfied |
| M004 positive operating-point closure | M004 closed negatively (`004-status.md`): best 0.9444 vs gates 0.99/0.98/0.95; `select_retrieval_operating_point` errors; no `m004-operating-point.json` / `advisor-operating-point.json` frozen; `src/tool_advisor/operating_point.rs:235` fail-closed error retained at this baseline | fail (blocker holds) | Hard dep 2 unsatisfied; plan forbids silent K raise |
| Fresh v4 holdout >=192 cases, >=150 groups, slice minima, >=50 permutation scenarios, balanced positions (plan §2) | `assets/tool-advisor/` contains train/dev/test, v2, v3, M003 selection, retrieval-signal M001 receipt — zero `*v4*`, `*m004-operating*`, or `*advisor-operating*` files; `target/tool-advisor/` absent on this host | not run | Holdout construction is post-M004 work; building it now would be an unrunnable input with no system to qualify |
| Zero leakage vs train/dev/test/v2/v3/M003-dev-augmentation (plan §3) | Not evaluated — no v4 candidate set exists | not run | Vacuously blocked |
| Separate preregistration Commit A with frozen model/encoder/calibration/promotion/retrieval/gates/command/hardware (plan §4) + green hosted CI | No frozen operating point exists to preregister; no Commit A created | not run | Plan §12 one-run discipline cannot start |
| Quality/order/counterfactual/retrieval/promotion/calibration/resource gates (plan §§5-11) | Not measured | not run | No runnable qualification |
| One-run discipline + disposition A-E (plan §§12-13) | Not executed; no machine-readable v4 result | not run | Only disposition A would unblock live M004; no result exists |
| Historical v3 diagnostic-only handling (plan §14) | Holds — v3 untouched | pass | No v3 file modified |

## 3. Production implementation evidence

No production implementation landed, by design for a blocked closure.

State at review:

- Selected model remains the M003 span-packed ranker (frozen selection
  `assets/tool-advisor/order-invariance-m003-selection.json`, sweep
  `011daa5d…`); no v4-tuned weights, thresholds, or K were created.
- `src/tool_advisor/operating_point.rs` retains the M004 preregistered
  protocol (`m004-preregistered-operating-point-v1`), frontier constants
  (`[64,128,256]` × `[16,24,32]`), and fail-closed selection; no operating
  point is frozen in code or assets.
- No `qualification-v4-*`, `order-invariance-m005-*`, or `v4-*` asset was
  added. `git status` shows only this closure batch as pending.
- No ranker, retriever, promotion, authority, storage, protocol, or
  default-surface change was made for M005.

Distinguished clearly: planned-but-absent is the entire v4
holdout/preregistration/qualification stack. Retained-and-frozen is the M003
ranker plus the M004 negative frontier evidence.

## 4. Verification executed

### Commands run

```bash
ls assets/tool-advisor/ | rg -i "v4|m004-operating|advisor-operating"
rg -n "no retrieval operating point clears" src/tool_advisor/operating_point.rs
cargo test --locked -p codegg --lib -- tool_advisor
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

Plan-literal qualification commands (preregistration commit, hosted CI,
exact release binary, one release-mode v4 run) were **not run**: the
prerequisite frozen operating point does not exist.

The feature-gated M004 sweep (`--features tool-advisor-encoder-training`)
was **not re-run** on this host: `candle-core v0.11.0` does not compile
under rustc 1.89.0 on aarch64-darwin (pre-existing NEON `stdarch_neon_f16`
interaction, also recorded in the retrieval-signal M001 closure §4). The
M004 negative verdict is adopted from its accepted closure record rather
than re-proven here; re-running the ~65-minute sweep would not create a
positive operating point.

### Results

- Asset census (local truth): zero v4 / operating-point files in
  `assets/tool-advisor/`; `target/tool-advisor/` absent. No hidden v4 input.
- Code check: `select_retrieval_operating_point` fail-closed error string
  present at `operating_point.rs:235`; M004 closure §4 frontier table (best
  0.9444 everywhere) stands uncontradicted.
- `cargo test --locked -p codegg --lib -- tool_advisor`: 55 passed /
  0 failed (base-feature advisor suite green, including retrieval-signal
  audit reproduction).
- `scripts/verify.sh quick`: passed (fmt, agent schema, core-boundary,
  sandbox, execution-ownership, tui-authority, http-route-disposition,
  audit-coverage, scheduler-bypass, eggwork routing, workspace check).
- `cargo fmt --all -- --check` and `git diff --check`: clean.
- Qualification/hosted scope: not run, truthfully labeled; no qualification
  claim is made.

## 5. Invariant review

Per roadmap §2 durable invariants — all hold because nothing was built or
measured on v4:

- Optional/local/default-off/advisory-only: untouched.
- Normal CodeGG works without any model artifact: untouched.
- No runtime model download: none added.
- In-process Rust inference/tokenization: unchanged.
- `ResolvedToolSurface` authority: untouched; no candidate permutation ran.
- Training augmentation local-only, no telemetry: no training ran.
- Historical train/dev/test/v2/v3 immutable: no corpus file modified; v3
  never loaded for selection.
- v3 diagnostic-only: holds; no v3 number selects anything in this closure.
- Fresh-v4 requirement for any positive claim: holds vacuously; no positive
  claim is made.
- Historical closures immutable: M004 negative and v3 D untouched.

## 6. Failure and recovery review

No new failure modes introduced. Applicability review:

- Duplicate delivery/idempotency: no qualification run exists to duplicate;
  plan §12 retry-only-on-unusable-result rule never triggered.
- Cancellation/restart: no long sweep to cancel or resume; per-universe
  checkpoint logic from M004 untouched.
- Partial persistence: no Commit A, so no partial-preregistration hazard.
- Stale generation/lease, contention, resource release: no daemon, storage,
  or concurrency surface touched (pure planning batch).
- Malformed input: no v4 manifest exists to validate; future unblocked pass
  must still enforce fingerprint/hash tripwires before reading labels.

## 7. Migration and compatibility review

No schema migration, no protocol change, no config change, no artifact
format change. No v4 asset to migrate or roll back. Rollback is a revert of
this planning-only batch. The M003 selection artifact and M004 frontier
checkpoints (where retained under `target/`, gitignored) need no migration.

## 8. Security review

No authorization, secret, network, or privilege surface touched. No model
weights, thresholds, or telemetry added. `#![deny(unsafe_code)]` holds for
the lib. Execution-ownership guard passes (no subprocesses added).

## 9. Documentation and operations

- This closure record is the sole new artifact for M005 in this batch
  (plus registry/roadmap/plan status updates in §12).
- No operator diagnostics, architecture docs, or static guards change: there
  is no new holdout, operating point, or disposition to document.
- Future unblocked pass must still produce: v4 fingerprints/manifest,
  Commit A with all hashes/gates/command/hardware, green hosted CI, one
  release-mode result, and an explicit A-E disposition.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | No K<=32 retrieval operating point exists (M004 negative, best 0.9444) | M005 has no qualified system to preregister or run; live-primary-model M004 stays blocked | Do not build v4 for this ranker/operating-point line; pursue the separately registered retrieval-architecture (closed negative) or retrieval-signal (M006 ready) lines instead |
| low | Feature-gated sweep unrunnable on aarch64-darwin (candle NEON) | M004 evidence cannot be re-proven on this host class | Future M005 implementers must run the release-mode qualification on Linux CI or a compatible host per plan §4/§12 |

No critical findings. No findings indicate a defect in the shipped advisor code.

## 11. Roadmap disposition

- M005: **blocked** — hard dependency on positive M004 unsatisfied; no
  downstream plan is unblocked by this closure.
- Live-primary-model M004 (post-closure corrective): remains **blocked**;
  it requires M005 disposition A plus original provider/operator/trajectory
  prerequisites, none of which exist.
- No subsystem roadmap revision beyond recording this blocked closure; no
  corrective plan is registered because the blocker is a predecessor
  negative verdict, not a new CodeGG defect. The retrieval-signal M006 line
  (ready) remains the only active path that could produce a future positive
  qualification input.

## 12. Registry updates

- `plans/registry.md` dependency-ready row for order-invariance M005:
  `blocked` retained with closure link to this record (hard-blocked on
  negative M004).
- `plans/registry.md` order-invariance gate paragraph: M005-stays-blocked
  text retained with this closure as the current evidence.
- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md`:
  M005 section `blocked` retained with closure link; dependency graph
  unchanged (M004 negative → M005 blocked).
- `plans/implementation/tool-selection-advisor-order-invariance-experiment/005-fresh-v4-preregistered-qualification.md`:
  status line `blocked` retained with closure link.
- Unblock audit: no registered plan lists M005 as a satisfied dependency;
  live M004 stays blocked. Nothing is moved to `ready` by this closure.
