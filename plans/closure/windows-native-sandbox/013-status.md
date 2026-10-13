# Windows Native Sandbox Corrective C013 — Closure Status

Status: closed (negative feasibility; successor deferred pending ADR review)

Source implementation plan:

- `plans/implementation/windows-native-sandbox/013-c011-existing-object-access-and-alias-containment.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#m004-corrective-execution-sequence`

Source finding and mitigation:

- `plans/closure/windows-native-sandbox/011-status.md`
- `plans/closure/windows-native-sandbox/012-status.md`

Repository baseline reviewed: `841d6a51d7a0e7b03c58aa47d78b703801fb80fe` (closure revision; implementation code is unchanged from `6028e03f20572b9c4cad53b1614df16a5c8d4182`)

Implementation commits:

- `6d965e03` through `6028e03f` — test-only forensic fixture, handle-aware object snapshots, fail-closed populated-root admission, and native workflow qualification.
- `841d6a51` — closure, research, proposed ADR and planning disposition; no runtime change.

Native evidence: [Windows process-launcher workflow 38299806236](https://github.com/dbowm91/codegg/actions/runs/38299806236), run against the exact closure revision above. Earlier implementation-SHA run [38296752862](https://github.com/dbowm91/codegg/actions/runs/38296752862) also passed.

## 1. Executive finding

C013 closes as a **negative feasibility/defer result**. The current root-level ACL lease is not a safe populated-existing-tree solution: the hosted native probe confirmed that the credential and workspace hardlink resolve to the same NTFS object, showed that the lease mutates that shared object, and found descendant security descriptor control-bit drift after root-only restoration. The grant-only forensic path also failed to read the intended allowed workspace file. No per-object lease mechanism passed the required identity, alias, race, and exact-recovery gates.

The ordinary test-support admission path rejects populated ACL lease roots before mutation and child creation; the same-revision Windows adversarial test passed this fail-closed contract. A proposed ADR-0019 scopes review of staged workspace projection or isolated identity as possible future directions. It is not accepted and selects no implementation. Production Windows constrained execution remains unavailable. C011 and M004 remain blocked.

This is a completed decision about the current lease strategy, not a claim that populated-tree containment works.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Capture object identity, link count, owner, DACL bytes and descriptor control before/after the lease | `c013_forensic_existing_object_acl_and_alias_probe`; workflow `38296752862` | pass | Disposable NTFS fixture. Direct credential and workspace hardlink shared volume `2c403fba`, file ID `000200000015406e`, link count 2. |
| Observe a running AppContainer child against direct, junction, hardlink, protected child, ADS, post-lease child, create, and allowed workspace paths | Same named C013 test | pass as diagnostic | Direct, junction, hardlink, protected, ADS, post-lease reads and creation were denied. The allowed workspace read was also denied; no positive supported-populated-tree contract is claimed. |
| Determine whether root lease restoration exactly restores descendant state | Same C013 test | fail; contained by disposable fixture | Captured DACL bytes returned to their original bytes, but ordinary root/descendant `sd_control` was `8404` after restore versus `8004` before. Protected child remained `9404`. The bypass is test-only; the normal path rejects populated roots before lease mutation. |
| Preserve allowed and denied semantics for an object shared by allowed and denied hardlink paths | Same C013 test | fail / infeasible under current path-level lease | Both aliases share one file ID and security descriptor. Per-path different ACLs cannot encode the requested policy. |
| Reject populated-root request before mutation and child creation | `nonempty_denied_tree_fails_before_child_creation`, workflow `38296752862` | pass | Test result 1 passed; typed `SandboxFailed`, no child marker, unchanged snapshots. |
| Decide a bounded future mechanism | Findings report and proposed ADR-0019 | negative/deferred | Root lease rejected; per-object journal not qualified; staged projection or isolated identity requires maintainer review and a new bounded plan. |
| Qualify C011/M004 full security matrix | Not run by C013 | blocked | Privileged IPC, full descendant/process matrix, restart and realistic-workspace review remain independent requirements. |

Detailed measured evidence and the support matrix are in `research/windows-native-sandbox/c013-existing-object-findings.md`.

## 3. Production implementation evidence

No production filesystem enforcement was added. C013 code remains within `windows-sandbox-test-support` or test-only fixtures. The test-support service validates every ACL lease root as empty before proceeding. The forensic test bypass is explicitly limited to disposable test-owned fixture analysis; it is not an admission path.

No production backend registration, security status, public protocol, or Windows `SandboxRequest::Required` behavior changed. No filesystem `Enforced` claim is made.

## 4. Verification executed

### Commands run

Cross-host on the implementation worktree at `6028e03f`:

```bash
cargo fmt --all -- --check
git diff --check
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
bash scripts/verify.sh quick
cargo test --locked -p codegg --features windows-sandbox-test-support --target x86_64-pc-windows-gnu --test windows_acl_lease --no-run
cargo test --locked -p codegg --features windows-sandbox-test-support --target x86_64-pc-windows-gnu --test windows_sandbox_adversarial --no-run
```

Hosted Windows Server 2025 x86_64 MSVC, Rust 1.89.0, Administrator/high-integrity disposable runner, exact SHA `6028e03f`:

```powershell
cargo test --locked -p codegg --features windows-sandbox-test-support --lib c013_forensic_existing_object_acl_and_alias_probe -- --nocapture --test-threads=1
cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_restricted_managed_process -- --nocapture --test-threads=1
cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_acl_lease -- --nocapture --test-threads=1
cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_sandbox_adversarial -- --nocapture --test-threads=1
cargo test --locked -p codegg --features windows-sandbox-test-support --lib windows_acl_lease -- --nocapture --test-threads=1
cargo test --locked -p codegg --lib managed_process::windows::tests::appcontainer_ -- --nocapture
```

The workflow also ran Windows managed-process launcher tests, suspended-target failure injection, restricted stdio failure injection, environment-block control, status-frame rejection, and Windows shell/policy tests.

### Results

- `cargo fmt --all -- --check`, `git diff --check`, sandbox and execution-ownership guards, `verify.sh quick`, and both Windows GNU cross-compilation commands passed on the implementation worktree before the final documentation-only changes.
- Workflow `38299806236` completed successfully against closure revision `841d6a51d7a0e7b03c58aa47d78b703801fb80fe`; it repeated the same C013 result and all Windows process-launcher workflow steps.
- Earlier workflow `38296752862` completed successfully against implementation revision `6028e03f20572b9c4cad53b1614df16a5c8d4182`.
- Selected Windows counts: C013 1; restricted managed process 6; ACL integration 4; existing-root adversarial 1; ACL recovery unit tests 6; AppContainer ordering 5; launcher 8; suspended-target failure 1; restricted stdio 3; environment block 1; status frame 1. Shell/policy targets also passed (6 plus seven single-test filters).
- Windows GNU commands were compile-only, not runtime evidence.
- The final planning/closure-only changes require formatting/diff/static planning checks again; they do not change Rust code or workflow behavior.

## 5. Invariant review

- **No production Windows constrained admission:** preserved. Test-support code remains isolated; no production registration or `Enforced` status changed.
- **No child for populated roots in the normal test-support path:** same-SHA adversarial test passed, asserting failure before child marker and unchanged snapshots.
- **No broad host ACL mutation:** the forensic ACL mutations ran only against disposable runner-owned NTFS fixtures. The nonempty-root admission path does not perform them.
- **No path-specific hardlink fiction:** the shared file ID and link count were captured; the alias conflict is documented and the root-level strategy rejected.
- **No claim of exact recursive restoration:** the observed control-bit drift is retained as a finding; no positive populated-root support claim is made.
- **Unix behavior and authority boundaries:** no Unix or shared production policy behavior changed; repository quick verification and execution-ownership guards passed.

## 6. Failure and recovery review

The diagnostic intentionally exercised the existing root-level lease and observed control-bit drift after restore on a disposable fixture. The fixture was not user data and was discarded after the test. This finding is contained by excluding populated roots from normal test-support admission. The existing empty-root lease/recovery suite passed (4 integration and 6 unit tests), but that does not qualify populated-object recovery.

Identity replacement between preflight and mutation, concurrent population, descendant reparse replacement, parent crash with a populated-object journal, and full C011 restart/recovery behavior were not qualified. They remain unsupported and must fail closed; this closure does not claim race safety for path-based preflight.

## 7. Migration and compatibility review

No storage migration, public API, protocol, or configuration change was made. Proposed ADR-0019 is `proposed`; it does not change the current compatibility contract. Any future accepted projection or isolated-identity implementation requires a new plan covering data freshness, links/reparse points, output reconciliation, interruption cleanup and migration.

## 8. Security review

C013 confirms that root-level directory ACL leasing is insufficient evidence for a populated tree and can affect an alias reachable from an allowed workspace path. The runner child denied all measured fixture reads in the forensic run, but it also denied the intended allowed workspace read; this is not a usable security contract. C013 does not explain every aspect of the original C011 leak and does not replace the C011 independent review.

The supported test-only disposition is fail-closed rejection of populated lease roots. No real credentials were accessed. C011 remains blocked; M004, M005 and M006 remain blocked. Network remains unrestricted and unclaimed.

## 9. Documentation and operations

- Added `research/windows-native-sandbox/c013-existing-object-findings.md` with object facts, child observations, strategy comparison and evidence limits.
- Added proposed `plans/adrs/ADR-0019-windows-sandbox-staged-workspace-projection.md`; maintainer review is required before any successor implementation.
- Updated the C013 implementation plan, this closure, the Windows native sandbox roadmap and `plans/registry.md`.
- No operator repair command exists or is needed for the disposable fixture. Do not manually alter user ACLs or remove durable journals.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Existing root-level ACL leasing is not a safe populated-tree mechanism: hardlink identity conflicts with allowed workspace access, and restoration left descendant control-bit drift. | Windows populated-root filesystem containment remains unavailable. | Keep populated roots unavailable; review proposed ADR-0019 before registering a new architecture plan. Keep C011/M004 blocked. |
| medium | Test-support populated-root preflight is path-based and races with replacement/population are not qualified. | This preflight has no production security claim and is not safe evidence for a broader support envelope. | Any future support must pin/revalidate identity and pass native race and recovery qualification. |
| high | C011's independent privilege/IPC/descendant/restart/realistic-workspace security matrix and review are outstanding. | M004 cannot close; M005/M006 cannot proceed. | Continue to block C011 and M004–M006 until separately accepted evidence exists. |

## 11. Roadmap disposition

C013 is closed as a negative feasibility result for the current populated-root ACL lease strategy. No enforcement capability or downstream dependency is unblocked. A staged workspace projection or isolated identity is only a proposed direction in ADR-0019 and is deferred pending maintainer decision. C011 remains blocked; M004, M005 and M006 remain blocked; M007 remains optional/deferred. Historical closure records `004-status.md`, `011-status.md`, and `012-status.md` are unchanged.

## 12. Registry updates

- Mark C013 closed (negative feasibility; successor deferred) and link this closure.
- Remove C013 from dependency-ready work; add it to recently closed work.
- Retain C011 blocked on its independent matrix and review. Its C013 existing-object corrective is complete as a negative/defer result, not a successful enforcement mechanism.
- Retain M004–M006 blocked and Windows production constrained execution unavailable.
- No registered plan was unblocked. ADR-0019 remains proposed and no successor implementation plan is registered pending its review.
