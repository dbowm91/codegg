# Stable-Toolchain Clippy-Drift Corrective C001 — Closure Status

Status: closed

Source implementation plan:
`plans/implementation/toolchain-clippy-drift-corrective/001-async-trait-double-must-use-suppression.md`

Source corrective addendum:
`plans/subsystems/toolchain-clippy-drift-corrective-addendum.md`

Repository baseline reviewed: `87a7da18`

Implementation commits: `877666be` — toolchain-clippy C001 source fix
(35 macro-site suppressions + scoped `deprecated` allow; planning registration
in `f4e00752`).

## 1. Executive finding

Hosted run `36866669727` is classified as stable-toolchain drift, unrelated
to the retrieval-signal line, and its entire failure surface is fixed with
zero behavior change. Default-feature Workspace Clippy is green on both the
1.89.0 floor and current stable (1.99.0) locally, and hosted `CI / verify`
is green on the fix head. The corrective closes with no successor work.

## 2. Requirement-to-evidence matrix

| Requirement (plan §2/§6) | Evidence |
|---|---|
| Suppress `double_must_use` at all affected macro sites | 35 `#[allow(clippy::double_must_use)]` attributes with drift comments: 1 egglsp, 1 providers, 14 codegg-core, 19 root crate |
| `fetch_update` deprecation without breaking MSRV 1.89 | Scoped `#[allow(deprecated)]` on `Gauge::dec` with revisit note; no rename |
| No logic/signature/schema/contract change | `git diff 7b509faf..877666be --stat`: attribute/comment lines only; no executable line touched |
| Green on 1.89.0 and 1.99.0 | Local `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0 on both toolchains |
| Hosted green | CI run `36912603806`, verify job `110557954083`, success in 20m32s |

## 3. Production implementation evidence

- `crates/egglsp/src/evidence_collector.rs` — `LspEvidenceProvider`.
- `crates/codegg-providers/src/provider_core.rs` — `Provider`.
- `crates/codegg-core`: `ConvergenceStore`, `AgentRunStore`,
  `AgentRunControlStore`, `AgentRunGroupStore`, `JobStore`,
  `ScheduleStore`, `OccurrenceMaterializer`, `ProjectionArtifactRegistry`,
  `RunStore`, `SessionSummaryProvider`, `BrokerCallback`,
  `WorkspaceStore`, `WorktreeStore`, `WorktreeOwnerResolver`.
- Root crate: `TurnRuntime`, `Hook`, `JobDispatcher`,
  `ReviewerModelBackend`, `ReviewerInvestigator`,
  `TypedHunkSourceContextTarget`, `SecurityContextExecutor`,
  `HunkSourceContextExecutor`, `ContextArtifactStore`, `CoreClient`,
  `ConnectionStore`, `PluginRuntime`, `EggworkNodeClient`,
  `JobProgressSink`, `JobExecutor`, `SearchProvider`, `RemoteTransport`,
  `Tool`, `TtsEngine`.
- `src/util/metrics.rs` — `Gauge::dec` scoped deprecation allow.

Root cause (verified, not inferred): the flagged `#[must_use]` is injected
by `async-trait` 0.1.89 macro expansion (`expand.rs:69`), not authored in
repository source (zero `must_use` hits in every flagged file; flagged files
byte-identical between `7b509faf` and the fix head). The new
`clippy::double_must_use` lint (stable 1.99.0, 2026-09-28) fires because the
desugared `Pin<Box<dyn Future>>` return is already `#[must_use]`. Local
1.89.0 predates the lint, hence the local/hosted split. Retrieval-signal
M002 (`eff890ec`) and M003 (`2df9a5f7`) touch only `src/tool_advisor/*` plus
advisor assets and never the flagged crates.

## 4. Verification executed (commands + results; label local vs CI truthfully)

Local (darwin aarch64):

- `cargo +stable clippy --workspace --all-targets --locked -- -D warnings`
  (rustc 1.99.0): exit 0, zero diagnostics.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
  (rustc 1.89.0): exit 0 — the suppression names are inert on the floor
  toolchain (no `unknown_lints` breakage).
- `cargo fmt --all -- --check`: clean. `git diff --check`: clean.
- `scripts/verify.sh quick`: passed (includes execution-ownership and all
  routine guards plus workspace `cargo check`).
- Focused tests: attribute-only change; no touched module needed new tests.
  The existing suites covering the touched crates pass in hosted CI below.

Hosted (CI / verify, PR #88):

- Run `36912603806`, verify job `110557954083`: **success** in 20m32s.
  Workspace Clippy step green on stable 1.99.0; workspace tests green
  (11,830-test sweep; one transient `asset_refresh` coalescing timeout on
  the first attempt, green on `--failed` rerun with no code change).

## 5. Invariant review

No retrieval scoring, representation, fusion, K, gate, label, universe,
model weight, training, or calibration was touched. `ResolvedToolSurface`
authority and advisor default-off/local-only behavior are untouched. No
toolchain pin, dependency bump, or CI-command change. No historical closure
record was rewritten.

## 6. Failure and recovery review

The only failure mode introduced would be a future lint rename/removal
turning the targeted allows into `unknown_lints` noise. Mitigation: each
allow carries a drift comment naming the exact cause, so removal is
mechanical when the toolchain or `async-trait` moves on.

## 7. Migration and compatibility review

None: comments and lint attributes only. MSRV 1.89 floor preserved
(explicitly verified, and the `fetch_update` rename was deliberately NOT
applied for that reason).

## 8. Security review

None applicable: no authorization, crypto, network, or sandbox surface
touched. `DescriptorTokenCache`-style concerns do not arise here.

## 9. Documentation and operations

Drift comments live at all 36 sites. The `fetch_update` site records the
1.99 rename and the revisit condition (MSRV floor past 1.99).

## 10. Unresolved findings (severity: critical/high/medium/low)

None.

## 11. Roadmap disposition

The corrective addendum is terminally satisfied; no successor milestone
exists in this track. The fix unblocks hosted CI greenness for the
retrieval-signal closure corrective C001 (which owns classification, not
the fix) and for all downstream advisor work.

## 12. Registry updates

- `plans/registry.md`: toolchain corrective rows move to closed with this
  closure and implementation `877666be`; hosted run `36912603806` recorded
  as the green evidence.
- Blocked-work audit: no registered plan lists this corrective as a hard
  dependency; the retrieval-signal C001 closure it enables is recorded
  separately. Nothing else is unblocked or reblocked.
