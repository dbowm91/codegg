# Architecture Convergence M003 — Git Ownership Convergence Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/architecture-convergence-incomplete-verticals/003-git-ownership-convergence.md`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Repository baseline reviewed: `3c4890035513cd4d74430b6f64523c8be676024e`

Implementation commit:

- [`83c0f74`](https://github.com/dbowm91/codegg/commit/83c0f74) — converge Git process, workflow-result, and root adapter ownership.

## 1. Executive finding

M003's in-scope production implementation is complete. Generic Git process
construction and local environment hardening now have one owner in
`egggit::process`; portable mutation workflow results have one owner in
`codegg-git::workflow`; CodeGG durable lifecycle, RunStore persistence,
authorization, network overlays, and projections remain in their existing
CodeGG owners. Historical public paths are retained as compatibility
re-exports where callers still use them.

The status is conditionally closed because this host cannot link the root
CodeGG test binary: the x86_64 Rust target selects incompatible arm64
MacPorts `liblzma`/`libiconv` artifacts. The focused `egggit`, `codegg-git`,
and core worktree suites passed, and the quick verification compile/static
posture passed. The root focused runtime suite and strict all-features
Clippy need a corrected host/CI toolchain; strict Clippy also reports one
unrelated existing `needless_late_init` in `crates/egglsp/tests/real_server_smoke.rs`.
No corrective implementation pass is required.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Every major Git operation has one documented owner | Ownership matrix in `architecture/git.md` covers discovery, reads, process construction, network, mutation, worktree, lineage, provenance, recovery, and projection | pass |
| Generic mechanics are not duplicated | `egggit::process::GitEnvPolicy` is used by all production `egggit` read helpers, core worktree/lineage probes, and root local execution; the old policy implementation and repeated read builders were removed | pass by source and compile evidence |
| CodeGG workflow result boundary is stable | `codegg_git::workflow::{RepoSnapshot, StateDelta, MutationOutcome, MutationResult}` is consumed by mutation, recovery, projection, integration, network, and RunStore adapters | pass |
| Root code is adapter-oriented | Root mutation execution retains CodeGG-specific authorization/network/RunStore integration; projector and persistence consume the crate workflow types directly | pass |
| Worktree/run/mutation ownership remains explicit | `codegg-core::worktree_service` remains durable lifecycle authority; `codegg-core::repository_lineage` and worktree helpers use the shared process owner; integration remains parent-side | pass |
| `codegg-git` remains justified | It owns typed argv/risk/path/ref/sensitive contracts plus portable workflow results; it is not a forwarding-only crate | pass, evidence-backed |
| M005 has one stable Git boundary | M005 can consume `codegg_git::workflow` results and the typed render/process policy without reconstructing root snapshot/result types | pass by interface evidence |
| Security invariants remain covered | Existing redaction, audit-safe argv, policy drift, worktree safety, and Git mutation tests remain; the Git guard now rejects literal production `git` construction outside `egggit::process` | pass |

## 3. Production implementation evidence

- Added `crates/egggit/src/process.rs` as the generic, shell-free Git process
  and local environment-policy owner.
- Rewired `egggit` status, diff, log, blame, refs, worktree, and rich-status
  production helpers to that owner. Patch validation now uses the same
  hardened synchronous builder.
- Replaced `codegg-git::process_policy`'s policy tables with compatibility
  re-exports and added `codegg-git::workflow` for portable mutation result
  types.
- Removed the duplicate root `GitEnvPolicy` implementation and the duplicate
  root mutation result definitions; root consumers now use the canonical
  crate-level types.
- Rewired network policy to layer its reviewed network environment overlay on
  the canonical base policy instead of rebuilding local hardening.
- Rewired core worktree and repository-lineage probes to the shared process
  builder.
- Strengthened `scripts/check_git_forbidden_patterns.py` with a production
  direct-`git` constructor guard and moved its policy source check to
  `egggit::process`.
- Updated Git and worktree architecture documentation with the final matrix,
  dependency-direction rationale, and retained compatibility boundaries.

## 4. Deleted forwarding/duplicate paths

- Repeated `env_clear`/PATH/editor/pager/command-bearer setup in six
  `egggit` production modules.
- The root-local `GitEnvPolicy` implementation; its historical export now
  points to `egggit::process`.
- Root-owned duplicate definitions of `RepoSnapshot`, `StateDelta`,
  `MutationOutcome`, and `MutationResult`; historical root exports now point
  to `codegg-git::workflow`.
- Network policy's duplicated reconstruction of base environment hardening.
- Core worktree's hand-built command environment; `hardened_git_command` is
  now a narrow adapter over the generic owner.

Retained intentionally: root CodeGG mutation/recovery/network orchestration,
RunStore persistence, projection formatting, and durable worktree service.
Those paths depend on CodeGG authority and cannot move into either leaf crate
without changing dependency direction or ownership semantics.

## 5. Final ownership diagram

```text
typed argv/risk/path/ref + workflow results
              codegg-git
                    |
                    v
generic Git process + read facts ----> egggit
                    |
       root/core CodeGG adapters
     /          |             \
 mutations   worktrees      RunStore/projections
 network     lineage        rerun/integration consumers
```

`egggit` remains read-only and CodeGG-agnostic. `codegg-git` remains a
portable typed contract crate. `codegg-core` owns durable worktree/lineage
state because it owns the relevant identity and lifecycle types. Root owns
model-facing authorization and projection/persistence adapters.

## 6. Verification executed

Successful:

```text
rtk cargo fmt --all -- --check
rtk cargo check -p egggit -p codegg-git -p codegg-core -p codegg --all-targets
rtk cargo test -p egggit                 # 75 passed
rtk cargo test -p codegg-git             # 358 passed
rtk cargo test -p codegg-core worktree --lib # 12 passed
rtk python3 scripts/check_git_forbidden_patterns.py
rtk python3 scripts/check_execution_ownership.py
rtk bash scripts/check-core-boundary.sh
rtk bash scripts/verify.sh quick
rtk git diff --check
```

Attempted but blocked outside the changed implementation:

```text
rtk cargo test -p codegg git_mutations::policy_drift_tests --lib
```

The root test binary failed at link time because the x86_64 host toolchain
ignored arm64 MacPorts native libraries and could not resolve x86_64 `lzma`
symbols. The required command also reported:

```text
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
```

one unrelated pre-existing lint at
`crates/egglsp/tests/real_server_smoke.rs:2236`. These are the explicit
conditions for this conditional closure; no changed Git source emitted a
Clippy or compile diagnostic.

## 7. Invariant, failure, and security review

- No shell is introduced: typed Git argv remains rendered by
  `codegg-git::render_argv` and process construction is direct argv.
- Local inspection/mutation defaults retain environment clearing, reviewed
  allowlist restore, command-bearer stripping, editor/pager pinning,
  noninteractive prompting, and process kill-on-drop.
- Network operations retain their explicit reviewed overlay and re-apply the
  hard-deny set.
- URL redaction and `AuditSafeArgv` persistence behavior are unchanged.
- Worktree containment, generation-fenced leases, base/result validation,
  conflict/recovery fail-closed behavior, and RunStore compatibility are
  unchanged.
- No durable schema, protocol, identity, scheduler-authority, or frontend
  change was made.

## 8. Compatibility and migration

No migration is required. `codegg_git::process_policy` continues to export
the old policy names, root `git_mutations` continues to export its historical
policy/result names, and serde shapes for the moved workflow result types are
unchanged. Existing historical RunStore records remain readable.

## 9. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| critical/high/medium | None in M003 scope | closed |
| low / operational | Root focused tests cannot link on this host because of the x86_64/arm64 native-library mismatch | Rerun on CI or corrected host toolchain; does not require corrective code |
| low / pre-existing verification | Strict all-features Clippy reports `needless_late_init` in `crates/egglsp/tests/real_server_smoke.rs:2236` | Out of scope; fix or suppress under the owning LSP work only |

## 10. Roadmap disposition

M003 is conditionally closed. The subsystem roadmap remains active because
M004, M005, M007, and M008 are future architecture-convergence milestones.
M003's implementation plan is marked `implemented` and points to this
record.

## 11. Downstream dependency audit

The registry's active, ready, and blocked sections plus the dependency text
in all architecture-convergence implementation plans were audited:

- M004's hard dependencies M001, M002, and M003 are now satisfied; M004 is
  promoted from `blocked` to `ready`.
- M005's sole hard dependency M003 is now satisfied; M005 is promoted from
  `blocked` to `ready`.
- M006 remains blocked on M004.
- M007 and M008 remain independently ready and unchanged.
- No other registered plan names M003 as a hard/interface dependency.
- No corrective plan or ADR is required.

## 12. Registry updates

The closure commit:

- marks this implementation plan `implemented`;
- records this closure record and M003 as conditionally closed;
- removes M003 from dependency-ready work;
- promotes M004 and M005 to dependency-ready work;
- removes M004 and M005 from blocked work;
- preserves M006's blocker; and
- records M003 under recently completed control points.

Final disposition: conditionally closed pending only the named host-toolchain
test rerun and unrelated all-features Clippy finding.
