# Tool-Advisor M003 Receipt CI Corrective C001 — Reproduce Fingerprint Drift and Restore Hosted Gate

Status: **implemented** — closed after exact-head hosted verification and merge of the repair PR.
Planning baseline: Codegg `main` at `90ab44775a36ebe30466a0d6e340f584324de114`, async-jobs PR #119 head `247edec5f37851ca3f67db5943675c896d19e9d9` (2026-10-15 review). Re-evaluate on execution.
Corrective addendum: `plans/subsystems/tool-advisor-m003-receipt-ci-corrective-addendum.md`.
Historical closure: `plans/closure/tool-selection-advisor-causal-frontier-experiment/003-status.md` (M003 disposition **D**, negative; M002 remains selected). Do **not** replace its historical result.
Affected integration dependency: async-jobs `plans/implementation/async-background-jobs-corrective/007-mainline-pr119-hosted-qualification-and-merge-readiness.md` (blocked until verified root CI green).

## 1. Exact observed failure and ownership

On PR #119, commit `247edec5f37851ca3f67db5943675c896d19e9d9`, `CI / verify` workflow run **38650792114**, job `verify` **116148778906**, failed in "Workspace tests" at `src/tool_advisor/causal_frontier.rs:5787`:

- Test: `tool_advisor::causal_frontier::tests::m003_checked_in_receipt_matches_recomputation`.
- Compared deterministic field: `stored.effect_catalog_fingerprint` from checked-in `assets/tool-advisor/causal-frontier-m003-result.json` versus fresh `qualify_m003(...)`.
- Stored fingerprint: `94ef4d745c79450f8d4b27a0985551c82fb2f572d7f8eba6aa4718c5edfd7649`.
- Fresh fingerprint: `a9c5afe90f2ad6ee7a165ed31d136b1087ff800d81964f0c27362722e228d9fb`.
- 4,267 tests passed; **1 failed; 9,522 not run** because nextest stopped early. Thus root hosted CI is red, regardless of whether the failure predates C005. Current head's Desktop E2E run **38650792103** and Windows process-launcher **38650792129** succeeded. Previous claims "pre-existing" and "not in async diff" identify likely code ownership; they do **not**, alone, reproduce the failure on an exact `main` workflow head. This plan must establish provenance rather than assume it.

The receipt includes an intentionally **implementation/schema-bound effect catalog**. The M003 closed-negative disposition D is scientifically meaningful; a green test obtained by simply replacing the stored hash would destroy the distinction between frozen benchmark evidence and current implementation identity. Do not silently regenerate `assets/tool-advisor/causal-frontier-m003-result.json`, weaken the comparison, skip/ignore the test, exclude it from nextest, or make CI `continue-on-error`.

## 2. Required diagnosis / reproducibility

### WP-A — Establish baseline, provenance and responsible diff

Run `cargo test tool_advisor::causal_frontier::tests::m003_checked_in_receipt_matches_recomputation -- --exact --nocapture` and the local `src/tool_advisor::causal_frontier` filtered suites on both current `main` and the PR branch, same pinned toolchain and a fresh workspace. Capture the exact stored/fresh values and the earliest known commit where they diverge. Inspect M003 historical closure (`82fc5b40`), M004/M005 followups, `assets/tool-advisor/causal-frontier-m003-result.json`, effect-binding default tool registry, native contract identity/version/input schema, and dependency changes. Diff the *inputs* to `effect_catalog_fingerprint` on each ref (sorted tool identities, source/provenance, versions and canonical parameter schemas) rather than only the final hash. Compare exact main and PR with real CI logs if available; if main CI lacks successful prerequisite toolchain setup, record "main re-run unavailable" rather than an unsupported "main reproduces".

### WP-B — Fix at the semantic boundary without falsifying the frozen negative experiment

Choose an explicit disposition based on findings:

**Case 1: legitimate post-M003 tool registry drift.** A historical frozen receipt must be compared against the correct historical bound snapshot or fixture identity, not today's registry. Fix the test fixture/binding selection or guard so it distinguishes (a) frozen experiment identity, pinned at historical M003 input; and (b) current runtime catalog compatibility, which may drift and should fail closed to M002 if M003 were active. New deterministic regression proves both: frozen benchmark receipt remains byte-identical in deterministic fields, live identity mismatches trigger `StaleCatalogFallback`, and M002 is still selected. Do not mask a genuine live deployment staleness condition.

**Case 2: unintended implementation/schema mutation.** Correct the specific tool-contract identity regression (or register an approved versioned contract migration where needed), preserving current legitimate API changes. Verify exact catalog bindings and stable fingerprints on the target toolchain.

**Case 3: unstable canonicalization / unordered iteration / toolchain environment.** Remove the nondeterminism from canonical fingerprint input with order-independent, versioned canonicalization; assert macOS/Linux and stable Rust deterministic identity on the **same** frozen tool registry. Preserve the prior receipt's provenance; if a schema-format migration is unavoidable, write a **new** versioned requalification record/receipt and a deliberate roadmap decision rather than mutating the historical D result in place.

If one case cannot be distinguished, stop with a quantified blocker and exact input diff. Do not pick an expedient "fresh hash equals stored" rewrite.

### WP-C — Regression and hosted evidence

Add a narrow test that prints or structurally diff-reports **which** implementation ID/version/schema entry changed (secrets omitted) when the fingerprint differs, with stable canonical ordering. Pin the historical M003 negative disposition D (0.41 preservation, 32 false exclusions, M002 selected) and the new current catalog's fail-closed behavior. Run:

```bash
cargo fmt --all -- --check
cargo test tool_advisor::causal_frontier::tests::m003_checked_in_receipt_matches_recomputation -- --exact --nocapture
cargo test tool_advisor::causal_frontier
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

Then obtain an actual **green exact-head hosted `CI / verify`** on the appropriate tool-advisor/mainline repair PR. Record workflow run ID/commit SHA, test totals, and cross-toolchain reproducibility. If integration PR #119 still needs the fix after main is green, merge or cherry-pick the reviewed repair through ordinary PR flow, then rerun `CI / verify` on the **new integration SHA**. An unrelated CI failure is not exempt from the hard merge gate merely because its ownership is elsewhere.

## 3. Constraints, acceptance and closure

- Preserve M003 negative disposition D, M002 selected runtime frontier and original M003 closure/receipt historical meaning; no retroactive claim M003 is safe, no relaxed test. Do not modify async-jobs agent admission, watcher, or sandbox simply to clear this failure.
- **Pass:** exact original mismatch reproduced or convincingly source-diagnosed; cause classified with annotated catalog identity diff; smallest correct semantic repair; frozen test and current stale fallback semantics verified; all targeted tests and guards green; green **hosted `CI / verify`** on exact repair merge/integration SHA.
- **Fail/block:** any solution that deletes/ignores the test, updates receipt in place without governed requalification, skips root CI, or has no reproducible cause. If main remains red for another unrelated error, record it separately and update the integration gate honestly.
- Closure file `plans/closure/tool-advisor-m003-receipt-ci-corrective/001-status.md` must contain baseline refs, precise compared fields/hashes, root cause, regression test names and counts, original negative disposition preservation, patch diffs, CI run IDs, remaining severity-classified findings, and unblock audit for async C007. Update dedicated addendum/registry only after real evidence.

**Ownership / integration:** This is the **tool-advisor** CI correctness track. Implement as a focused independent patch against current `main` (or an isolated tool-advisor branch), not by folding unrelated tool-advisor runtime changes into the asynchronous job feature patch without review. The planning files may reside on the async plan branch for registration; the eventual repair must be independently attributable.
