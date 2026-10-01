# Tool-Selection Advisor Retrieval-Signal Closure Corrective C001 — Closure Status

Status: closed

Source implementation plan:
`plans/implementation/tool-selection-advisor-retrieval-signal-closure-corrective/001-terminal-roadmap-and-ci-reconciliation.md`

Source corrective addendum:
`plans/subsystems/tool-selection-advisor-retrieval-signal-closure-corrective-addendum.md`

Repository baseline reviewed: `87a7da18`

Implementation commits: `f4e00752` — terminal roadmap/registry
reconciliation plus registration of the separately-owned toolchain fix
(no retrieval/model code in this line).

## 1. Executive finding

The retrieval-signal experiment is repository-clean and terminally negative.
The subsystem roadmap describes completed history (closed-negative) instead
of preregistration-era readiness; the registry agrees; hosted run
`36866669727` is exactly classified as stable-toolchain drift owned by the
separately registered toolchain corrective (closed alongside, fix
`877666be`, hosted green `36912603806`). No retrieval/model work was
reopened, reinterpreted, or re-tuned. M004/M005 remain terminally
blocked/not reached.

## 2. Requirement-to-evidence matrix

| Requirement (plan) | Evidence |
|---|---|
| Reproduce hosted Clippy (§3) | Exact CI command run locally on 1.89.0 (pass) and on installed stable 1.99.0 (17-error reproduction, same `double_must_use` sites as hosted log) |
| Ownership classification (§4/§5) | Transient/toolchain + unrelated-current-head: new 1.99.0 lint on `#[must_use]` injected by async-trait 0.1.89 (`expand.rs:69`); flagged files byte-identical `7b509faf`..head with zero authored `must_use`; M002/M003 touch only `src/tool_advisor/*` |
| Fix only if owned | Not owned → no source edit in this line; separate corrective registered and closed |
| Roadmap terminal (§5) | `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md`: closed (negative), completed-history dependency graph, M004/M005 blocked/not reached, future work is a separate workstream |
| Registry terminal (§6) | Experiment row closed-negative; corrective row active→closed; C001 row ready→active→closed; M004/M005 rows terminal; gate prose records classification + owner |
| Closure record (§9) | This file |
| No new architecture (§11) | Nothing in `f4e00752` touches scoring, models, gates, or v4 |

## 3. Production implementation evidence

No production code changed in this corrective by design. Planning-only
commit `f4e00752`:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md`
  (37-line terminal reconciliation);
- `plans/subsystems/toolchain-clippy-drift-corrective-addendum.md` (new,
  separately-owned fix track);
- `plans/implementation/toolchain-clippy-drift-corrective/001-async-trait-double-must-use-suppression.md`
  (new);
- `plans/registry.md` (subsystem rows, implementation rows, gate prose,
  terminal M004/M005 blockers).

## 4. Verification executed (commands + results; label local vs CI truthfully)

Local (darwin aarch64):

- `cargo clippy --workspace --all-targets --locked -- -D warnings`
  (1.89.0): exit 0 — the routine-CI command reproduces green on the floor
  toolchain, proving the hosted failure is toolchain-version-specific.
- `cargo +stable clippy ...` (1.99.0): reproduced the hosted failure
  exactly (same lint, same two initial sites; full enumeration found 35
  macro sites + 1 deprecation, all fixed in `877666be`).
- `cargo fmt --all -- --check`, `git diff --check`: clean.
- `scripts/verify.sh quick`: passed (includes
  `check_execution_ownership.py` and all routine guards).
- `rustc --version --verbose` / `cargo --version` /
  `git rev-parse HEAD` recorded the 1.89.0 vs 1.99.0 split behind the
  classification.

Hosted (CI / verify, PR #88):

- Run `36912603806`, verify job `110557954083`: **success** in 20m32s —
  Workspace Clippy green on stable 1.99.0 (the toolchain fix verified
  where it counts) and workspace tests green (one transient
  `asset_refresh` coalescing timeout on first attempt, green on `--failed`
  rerun without code change).

## 5. Invariant review

ADR-0009 holds: no scoring, representation, fusion, K, gate, label, or
universe change; no retraining; no frozen artifact; M002/M003 not
reinterpreted; v4 untouched; historical closures untouched;
`ResolvedToolSurface` authority and default-off/local-only behavior
untouched.

## 6. Failure and recovery review

The stop conditions (§10) were evaluated: the CI failure belongs to other
subsystems (egglsp/providers/codegg-core/root) and the toolchain, so a
separate corrective was registered instead of widening C001 — exactly the
prescribed path. No retrieval/model behavior was at risk at any point.

## 7. Migration and compatibility review

None: planning-only line. The toolchain fix it points to preserves the
1.89 MSRV floor (verified) and changes no contracts.

## 8. Security review

None applicable: no authorization, execution, network, or storage surface
touched in either this line or the referenced fix.

## 9. Documentation and operations

Roadmap and registry now permanently describe the terminal negative state;
stale "only ready handoff" / "M001R blocked" / "M002 blocked" prose is
gone. Future architecture attempts must register separately (the
late-interaction experiment is the first such workstream).

## 10. Unresolved findings (severity: critical/high/medium/low)

None. The main-branch CI failure at `36919438717` (Workspace Clippy, same
lint family) is the same toolchain drift on code that has not yet merged
this branch's fix; it is resolved by merging PR #88, not by new work.

## 11. Roadmap disposition

`plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md`
is **closed (negative)**. M001 blocked/closed hard stop; evaluation
corrective C001 closed B; M001R closed positive; M002 closed
negative-but-valid; M003 closed negative; M004/M005 blocked/not reached
(terminal — no eligible retrieval candidate ever existed).

## 12. Registry updates

- Retrieval-signal experiment + closure-corrective rows move to closed
  with this record; C001 implementation row moves to closed.
- Blocked-work audit: M004/M005 terminal blockers stay explicit; the
  late-interaction M001 row is unaffected by this closure (its blocker was
  "C001 finishes terminal cleanup/CI classification" — now satisfied, so
  the M001 close below moves it to ready→closed in the same push sequence).
- Live-primary-model trajectory stays blocked (unchanged prerequisites).
- No other registered plan lists this corrective as a dependency; nothing
  else is unblocked.
