# Windows Native Sandbox Milestone C012 — Closure Status

Status: conditionally closed (test-only fail-closed preflight)

Source implementation plan:

- `plans/implementation/windows-native-sandbox/012-existing-tree-deny-lease-correction.md`

Source corrective finding:

- `plans/closure/windows-native-sandbox/011-status.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#m004-corrective-execution-sequence`

Repository baseline reviewed: `20402df11e15ec4072287ab69067b12bffbcb3b7`

Implementation commits:

- `4348773f` — reject populated deny roots before test-only child creation.
- `0a6110d2` — separate empty-root ACL positives from populated-tree rejection.
- `20402df1` — align the ACL fixture with the empty-root policy and verify directory-list denial.
- `6b6e3902` — closure and documentation updates; no runtime behavior change.

## 1. Executive finding

C012 chose the bounded fail-closed option. The test-only managed-process path rejects any deny root with an existing directory entry before AppContainer profile creation, child spawn, or lease DACL mutation. The synthetic C011 fixture includes a credential, alternate data stream, hardlink, and junction; the test asserts typed `SandboxFailed`, absence of the child's start marker, and unchanged ACL snapshots. A separate empty-root fixture verifies allowed workspace behavior, denial of access/listing/writes through the empty deny root, and exact restoration.

C012 is conditionally closed for this test-only preflight. The emptiness check is path-based and does not pin the directory identity through lease acquisition. Concurrent replacement/population remains unqualified, as do C011's broader security matrix and production backend admission. Windows constrained execution remains unavailable.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Populated denied tree fails before child creation | `nonempty_denied_tree_fails_before_child_creation`, exact-SHA run `38223080719` | pass | Synthetic direct/junction/hardlink/ADS fixture is not read by a child; typed `SandboxFailed` and missing child marker are asserted. |
| Rejected tree causes no ACL mutation | Same test, run `38223080719` | pass | Before/after ACL snapshots remain unchanged. |
| Empty-root deny and positive workspace behavior | `windows_acl_lease_allows_owned_roots_and_denies_empty_sibling_then_restores_acl`, run `38223080719` | pass | ACL integration target: 4 passed; allowed workspace read/write, empty-root read/list/write denial, and exact restoration. |
| ACL recovery and failure behavior | Windows ACL lease unit tests, run `38223080719` | pass | 6 passed, including rollback/recovery and conflict retention. |
| Windows launcher, restricted service, AppContainer, and shell/policy regression steps | Full workflow `38223080719` | pass | Launcher 8; suspended-target 1; restricted-stdio 3; environment 1; status-frame 1; restricted service 6; AppContainer ordering 5; shell/policy steps passed. |
| Repository quick verification | `bash scripts/verify.sh quick` on the implementation worktree | pass | Completed after fixture correction and again after closure/documentation updates. |
| Concurrent path replacement between emptiness preflight and lease acquisition | No handle-pinned race test | not qualified | Named condition; no production guarantee follows. |
| Exact DACL cause, recursive protection, and full C011/M004 qualification | Not implemented | blocked | The correction rejects populated roots; it does not diagnose or protect descendants. |

## 3. Production implementation evidence

This is test-support-only. `validate_empty_deny_directory` rejects a populated deny root. `run_windows_test_restricted` performs that check before profile creation and child spawn, mapping rejection to typed `SandboxFailed`. ACL leasing remains limited to eligible empty disposable roots. No production backend, public protocol, or release configuration changed.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
git diff --check
cargo test --locked -p codegg --features windows-sandbox-test-support --target x86_64-pc-windows-gnu --test windows_acl_lease --no-run
cargo test --locked -p codegg --features windows-sandbox-test-support --target x86_64-pc-windows-gnu --test windows_sandbox_adversarial --no-run
bash scripts/verify.sh quick
```

The Windows GNU commands are cross-compilation only. The full native Windows workflow was manually dispatched against implementation SHA `20402df11e15ec4072287ab69067b12bffbcb3b7` and completed successfully: [run 38223080719](https://github.com/dbowm91/codegg/actions/runs/38223080719). Runner: Windows Server 2025 Datacenter, build `26100`; Rust `1.89.0`, target `x86_64-pc-windows-msvc`. The earlier same-policy run [38222911058](https://github.com/dbowm91/codegg/actions/runs/38222911058) also passed on `0a6110d2`.

The earlier attempts preserve the discovery and correction history. Run `38220136385` exposed direct/junction/hardlink reads from a populated deny root. Run `38220719942` at `4348773f` failed because the older positive ACL fixture still populated its deny root after the fail-closed policy was introduced. The fixture was changed to use an empty deny sibling, and exact-SHA run `38223080719` passed. No C012 test was skipped in that final run.

## 5. Invariant review

- No child starts for a populated deny root: verified by the missing marker in the native adversarial test.
- Rejection does not mutate the root or descendant ACLs: verified by exact snapshots; no descendant traversal is performed.
- Empty-root lease recovery retains identity and descriptor conflict checks: C010 regression tests passed.
- Production constrained execution remains unavailable: no production backend or `Enforced` result was added.

## 6. Failure and recovery review

Populated-root rejection occurs before child creation and ACL mutation, so this policy has no partial populated-tree lease to recover. Empty-root normal exit, cancellation, simulated parent-crash recovery, and descriptor-drift tests passed. Recovery of recursive or populated-tree leases is not implemented because those leases are not created.

## 7. Migration and compatibility review

No storage schema, public protocol, or release configuration changed. The test-only feature remains release-disabled. Populated deny roots return a typed failure in this path; callers must provide an empty deny root or fail closed.

## 8. Security review

The correction prevents the observed synthetic existing-tree paths from being read by rejecting the populated root before child start. It does not prove denial while a child runs against populated roots, exact DACL causality, race-safe identity pinning, production journal protection, IPC isolation, or safety for real user data. No non-synthetic credentials were accessed. C011's high-severity finding and remaining qualification gates remain open.

## 9. Documentation and operations

- Updated `docs/windows-acl-leases.md` with the empty-deny-root test policy and remaining C011/M004 blockers.
- Updated `architecture/security.md` and `architecture/testing.md` with the observed leak and bounded guarantee.
- Updated the implementation plan, subsystem roadmap, and `plans/registry.md`.
- No operator repair command exists; do not remove journals or reset ACLs by hand.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | C011 demonstrated direct/junction/hardlink reads from a populated denied tree. | Windows filesystem containment remains unqualified and unavailable in production. | Keep C011/M004 blocked until the remaining native security matrix and independent review pass or a separately reviewed correction is accepted. |
| medium | The emptiness preflight is path-based; directory replacement/population between preflight and lease acquisition is not qualified. Exact DACL cause and recursive protection are absent. | Populated deny roots are unavailable to this test path; race safety is not established. | Any future support for populated roots requires handle-pinned validation, recovery design, and hosted adversarial qualification. |

## 11. Roadmap disposition

C012 is conditionally closed for the bounded test-only fail-closed preflight. The named path-replacement race remains unqualified. C011 remains blocked on its full security and operational matrix; M004, M005, and M006 remain blocked. This closure does not unblock production Windows sandbox admission.

## 12. Registry updates

- Mark C012 conditionally closed and link this record.
- Retain C011 blocked; C012 covers the populated-root pre-spawn rejection slice, while C011's broader security/operational matrix and independent review remain outstanding.
- Retain M004–M006 blocked and keep production backend admission unavailable.
