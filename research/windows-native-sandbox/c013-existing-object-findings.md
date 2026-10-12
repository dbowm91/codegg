# C013 Existing-Object Access and Alias Findings

Date: 2026-10-12

Evidence revision: `6028e03f20572b9c4cad53b1614df16a5c8d4182`

Hosted evidence: [Windows process-launcher run 38296752862](https://github.com/dbowm91/codegg/actions/runs/38296752862)

## Scope and environment

The C013 forensic test ran on the repository's Windows Server 2025 hosted runner, Rust 1.89 MSVC, using runner-owned NTFS fixtures and synthetic secret files. The runner principal was Administrator/high integrity, as recorded by the workflow. This is disposable-fixture evidence only; it does not qualify arbitrary-user IPC, production daemon recovery, or a realistic workspace.

Exact C013 command:

```powershell
cargo test --locked -p codegg --features windows-sandbox-test-support --lib c013_forensic_existing_object_acl_and_alias_probe -- --nocapture --test-threads=1
```

Result: 1 passed, 0 failed. The same run passed the test-only restricted managed-process integration (6), journaled ACL lease and recovery integration (4), existing-root adversarial test (1), ACL failure/recovery unit tests (6), and AppContainer ordering tests (5).

## Observed fixture facts

The pre-lease credential file and workspace hardlink had the same NTFS volume (`2c403fba`), file ID (`000200000015406e`), and hardlink count (2). This confirms both paths resolve to one security descriptor. The protected credential child had a distinct file ID and DACL control `9404`; ordinary fixture roots and descendants began with control `8004`.

Applying the existing root-level ReadWrite lease to the workspace changed the existing workspace file and hardlink-backed credential file descriptor. The same credential file ID appeared through both paths. The existing root-level deny lease altered the denied root's DACL. The actual running AppContainer child reported:

| Probe | Result |
|---|---|
| Allowed workspace file read in forensic fixture | denied |
| Direct synthetic credential read | denied |
| Credential through workspace junction | denied |
| Credential through workspace hardlink | denied |
| Protected-DACL credential read | denied |
| Credential alternate data stream read | denied |
| Child added under denied root after lease | denied |
| Create a new file under denied root | denied |

These observations do not establish a usable positive policy: the grant-only forensic setup also denied the allowed workspace read. The deny result is not a successful supported-populated-tree contract because root-level mutation touched a file object that also had an allowed workspace alias.

After root-only lease restoration, ACL bytes in the captured objects returned to their pre-lease values, but the ordinary root and descendant security descriptor control value remained `8404` rather than the original `8004`. The protected child remained `9404`. C013 records the measured finding as descendant descriptor/control drift after root-level restoration. It does not claim exact restoration.

## Policy disposition

| Candidate | Evidence-based disposition |
|---|---|
| Grant-only access to populated existing roots | Not qualified. The actual child could not read the intended allowed workspace file in the forensic fixture. No denied-tree mutation is acceptable as a substitute for proving the allowed contract. |
| Root-level deny lease for populated roots | Rejected for this contract. The lease changes a shared hardlink-backed file object and restore leaves descendant control-bit drift. Path-specific permissions cannot distinguish aliases to one file ID. |
| Bounded per-object deny journal | Not implemented. It would require identity pinning, complete alias discovery, race control, bounded traversal and exact restoration of all descriptor/control state; C013 found no safe bounded implementation to qualify in this pass. |
| Staged workspace projection or isolated identity | Proposed for a future architecture review in `plans/adrs/ADR-0019-windows-sandbox-staged-workspace-projection.md`. No implementation is selected or authorized by this proposal. |
| Current disposition | Keep populated roots unavailable. The ordinary test-support admission path rejects every populated ACL lease root before mutation/spawn; production Windows constrained execution remains unavailable. |

The separate `windows_sandbox_adversarial` test on the same workflow confirms the supported test path returns `SandboxFailed`, starts no child marker, and preserves the before/after snapshots when a populated root is supplied. This does not qualify the path-replacement race between preflight and later lease acquisition; that race remains open and the path is test-only.

## Limits

The forensic bypass is compiled only for Windows test support and operated on a disposable fixture. Its observed restoration drift is contained by the fixture's lifetime, but it is a concrete reason not to admit populated roots through the lease path. C013 did not run adversarial identity replacement between validation and mutation, crash/recovery against populated object journals, an isolated privileged VM, the C011 IPC/descendant/restart matrix, or production filesystem enforcement. C011 and M004 therefore remain blocked.
