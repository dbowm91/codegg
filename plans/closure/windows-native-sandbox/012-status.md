# Windows Native Sandbox Corrective C012 — Closure Status

Status: conditionally closed (test-only fail-closed preflight)

Source implementation plan:

- `plans/implementation/windows-native-sandbox/012-existing-tree-deny-lease-correction.md`

Source corrective finding:

- `plans/closure/windows-native-sandbox/011-status.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#m004-corrective-execution-sequence`

Repository baseline reviewed: `0a6110d2e45284c9b728eac3b31560cd78af59fc`

Implementation commits:

- `4348773f` — reject nonempty deny roots before test-only child creation
- `0a6110d` — separate empty-root ACL positive coverage from nonempty-tree rejection

## 1. Executive finding

C012 chose the bounded fail-closed option. The test-only managed-process path
rejects any deny root with an existing directory entry before creating the
AppContainer child or mutating a lease DACL. The C011 synthetic fixture contains
a credential, alternate data stream, hardlink, and junction; the updated test
asserts the typed `SandboxFailed`, absence of the child's start marker, and
unchanged workspace and credential-root DACLs. A separate positive fixture uses
an empty deny root while checking allowed workspace writes, read-only behavior,
denied directory listing/writes, and exact restoration.

The exact-revision hosted Windows workflow `38222911058` passed on
`0a6110d2e45284c9b728eac3b31560cd78af59fc`. The run verified the behavior on
Windows Server 2025 Datacenter build `26100`, Rust `1.89.0`, target
`x86_64-pc-windows-msvc`. The disposable runner reported Administrator and
high-integrity tokens with UAC enabled. The earlier attempt `38220719942` failed
because the existing positive ACL fixture still populated its denied root; the
fixture was corrected to test the supported empty-root case separately.

C012 closes conditionally and remains test-support-only. Its emptiness preflight
is path-based and does not pin the directory identity across validation and
lease acquisition. That race, the full C011 privileged security matrix, and
production backend admission remain unqualified. C011 and M004 remain blocked;
Windows constrained execution remains unavailable.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Nonempty denied tree fails before child creation | `nonempty_denied_tree_fails_before_child_creation`, run `38222911058` | pass | The synthetic direct/junction/hardlink/ADS matrix is not executed by a child; the request returns typed `SandboxFailed`. |
| Rejected tree has no partial ACL mutation | Same test, run `38222911058` | pass | Before/after DACL snapshots for the workspace and credentials directories match. |
| Positive empty-root launch preserves allowed workspace read/write | `windows_acl_lease_allows_owned_roots_and_denies_empty_sibling_then_restores_acl`, run `38222911058` | pass | ACL integration target: 4 passed. |
| ACL recovery and failure behavior remains intact | `windows_acl_lease` unit tests, run `38222911058` | pass | 6 passed, including rollback/recovery and conflict retention. |
| C008–C010 Windows launcher and restricted-service regressions | Workflow `38222911058` | pass | Launcher 8; suspended-target 1; restricted-stdio 3; environment 1; status frame 1; restricted service 6; AppContainer ordering 5; shell/policy cases all passed. |
| Repository quick verification | `bash scripts/verify.sh quick` on implementation revision `0a6110d` | pass | Completed after the fixture correction. |
| Concurrent path replacement between preflight and lease | No handle-pinned race test | not qualified | The test-only preflight checks directory entries by path; no production guarantee follows. |
| Full C011/M004 security matrix and production Windows filesystem enforcement | No privileged isolated VM or production backend | blocked | Credentials, daemon IPC, descendants, restart integration, path replacement, and user workspaces remain outside this correction. |

## 3. Scope and invariant review

- A deny root containing a file, directory, junction, hardlink, or other entry
  fails closed before the child starts. Root-only inheritable ACL changes are
  therefore not represented as sufficient for an existing denied tree.
- Positive workspace and read-only behavior remains covered by separate
  disposable fixtures with an empty denied sibling.
- C010 journaling/recovery remains unchanged and test-only; the existing six
  Windows ACL unit tests passed in the same hosted workflow.
- No production backend was registered, no `filesystem=Enforced` status is
  reported, and no Windows network guarantee is made.

## 4. Verification executed

- Hosted Windows run [`38222911058`](https://github.com/dbowm91/codegg/actions/runs/38222911058), exact revision `0a6110d2e45284c9b728eac3b31560cd78af59fc`: success. All workflow steps passed, including the adversarial pre-spawn rejection, ACL integration/recovery, launcher, AppContainer, and shell/policy checks.
- Earlier C012 attempt [`38220719942`](https://github.com/dbowm91/codegg/actions/runs/38220719942), revision `4348773f`: failed in the ACL integration target because the old positive fixture expected a populated deny root to launch. The fixture was updated to use an empty deny root, with nonempty-tree rejection tested separately.
- `bash scripts/verify.sh quick`: passed on `0a6110d`.
- `git diff --check`: passed after documentation and closure updates.

## 5. Remaining condition and roadmap disposition

C012 is conditionally closed for the bounded test-only fail-closed behavior.
Before any production use, a future corrective must pin the checked directory
identity through lease acquisition and prove race-safe behavior. C011 remains
blocked because this narrow fixture does not meet its full privileged
adversarial, daemon IPC, recovery, descendant, and real-user-workspace gates.
M004 remains blocked; M005 and M006 remain blocked on M004. The historical
`plans/closure/windows-native-sandbox/004-status.md` remains unchanged.
