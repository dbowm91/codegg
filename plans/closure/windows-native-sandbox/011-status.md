# Windows Native Sandbox Corrective C011 — Qualification Status

Status: blocked (full privileged M004 security matrix outstanding)

Source implementation plan:

- `plans/implementation/windows-native-sandbox/011-m004-native-adversarial-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#m004-corrective-execution-sequence`

Repository baseline reviewed: `6efc0f92ab50fbadd10f3e1188e3639a4202afcd`

Implementation commits:

- `657587b9` — reject final-component reparse roots before canonicalization
- `6efc0f92` — add the synthetic existing-root direct/junction/hardlink/ADS adversarial test and hosted workflow step

## 1. Executive finding

C011 did not qualify Windows filesystem containment. The privileged GitHub-hosted
Windows Server 2025 runner met the operational Administrator/high-integrity gate,
and the bounded synthetic fixture exercised an existing disposable workspace and
credential directory through `ManagedProcessService`. The child successfully read
the synthetic credential by its direct path, through a junction, and through a
hardlink, despite the credential directory being listed in `deny_paths`. The ADS
read was denied. This is a high-severity failure of the requested deny guarantee.

The exact-revision Windows workflow `38217933472` failed at the new adversarial
step with output
`leaked|leaked|leaked|denied|workspace-readable|workspace-write`. The launcher,
failure-injection, C009 restricted-service, and C010 ACL lease/recovery steps before
it passed. No repository or runner secret was read. Production Windows backend
admission remains disabled; M004 remains blocked.

The same adversarial test failed again on revision `361bd00fe447adb4564f585d2e4bbb2298d37492`
in Windows workflow `38220136385`, with the same output. That run passed the
privilege probe and every preceding Windows launcher, restricted-service, and
ACL lease/recovery step. The host was Windows Server 2025 Datacenter build
`26100`, Rust `1.89.0`, `x86_64-pc-windows-msvc`; the runner reported
Administrator and high-integrity tokens with UAC enabled. The repeat confirms
the leak after final-root and ancestor reparse checks; those checks do not repair
the denied existing-file case.

C012 now rejects a nonempty deny root before child creation in the test-only
path, but C011 remains blocked. This correction does not qualify a race-free
existing-tree lease, daemon IPC, descendants, restart recovery, real user
workspaces, or the remaining privileged M004 matrix. The historical M004
record remains unchanged.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Hosted runner has disposable elevated privilege | Workflow privilege probe in `38217933472` | pass | Administrator and high-integrity checks passed; UAC remains enabled. |
| Existing-root workspace reads and writes remain usable | `windows_sandbox_adversarial` in `38217933472` | pass | Synthetic workspace read and write succeeded; exact workspace DACL restoration assertion was not reached because the deny assertion failed. |
| Denied credential file cannot be read directly | `windows_sandbox_adversarial` in `38217933472` and `38220136385` | fail | Child read the synthetic credential. |
| Denied credential cannot be reached through a junction | `windows_sandbox_adversarial` in `38217933472` and `38220136385` | fail | Child read the same credential through the workspace junction. |
| Denied credential cannot be reached through a hardlink | `windows_sandbox_adversarial` in `38217933472` and `38220136385` | fail | Child read the hardlinked credential file. |
| Denied alternate data stream cannot be read | `windows_sandbox_adversarial` in `38217933472` and `38220136385` | pass | ADS access was denied in this fixture. |
| Final-component reparse root fails before child creation | Workflow `38217662482`, implementation `657587b9` | pass | The symlink-root regression test passed on the same Windows Server 2025 runner class. This does not qualify nested reparse paths or credential denial. |
| C009 process control and C010 lease/recovery regressions | Earlier steps of `38217933472` | pass | Launcher, failure injection, managed-process integration, and ACL lease/recovery tests passed before the adversarial step. |
| Production Windows filesystem enforcement and M004 closure | No production backend/admission | blocked | C011/C012 evidence is test-only; no `filesystem=Enforced` claim is permitted. |

## 3. Stop-condition and follow-up

The C011 plan explicitly requires stopping on any credential leak. The observed
result therefore prevents C011 closure and M004 requalification. The likely area
to investigate is how the root-only inheritable deny ACE interacts with existing
descendant file DACLs and explicit ACE ordering; the hosted result establishes the
leak, not that particular root cause. Microsoft documents that ACE inheritance
and direct ACE precedence affect child DACLs: [automatic propagation of
inheritable ACEs](https://learn.microsoft.com/en-us/windows/win32/secauthz/automatic-propagation-of-inheritable-aces).

C012 (`plans/implementation/windows-native-sandbox/012-existing-tree-deny-lease-correction.md`)
implements the bounded rejection path; its conditional closure is recorded at
`plans/closure/windows-native-sandbox/012-status.md`. The corrected synthetic
test passes by proving that a populated deny root returns a typed failure before
child creation and leaves the root DACLs unchanged. C011 remains blocked until
the remaining privileged matrix is available and independently accepted.

## 4. Roadmap disposition

- C010 remains closed for test-only ACL lease/recovery infrastructure.
- C011 remains blocked by its high-severity existing-file read finding and the
  outstanding full privileged matrix.
- C012 is conditionally closed for bounded test-only rejection; path-replacement
  races remain unqualified.
- M004 remains blocked; M005 and M006 remain blocked on M004.
- No change is made to `plans/closure/windows-native-sandbox/004-status.md`.
- Windows constrained execution remains unavailable/fail-closed.
