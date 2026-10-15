# Windows Native Sandbox C037 — Closure Status

Status: **closed — bounded negative/incomplete; no production admission**

Source implementation plan: `plans/implementation/windows-native-sandbox/037-mxc-handle-propagation-semantics-and-bypass-matrix.md`

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0024-capture-and-handle-semantics-follow-up`

Implementation evidence SHA: `1803105fbf2434eaaf9ba673f77e2b124c4eb151`.

Native evidence: [Windows MXC C037 run 38429625036](https://github.com/dbowm91/codegg/actions/runs/38429625036). The workflow completed successfully as evidence collection; `qualified_test_only=false` because the descendant probe did not produce proof and the child returned `Exited(44)`.

## Finding

The ordinary Win32 controls behaved as intended: inheritance-on proved same-file handle identity and read the synthetic bytes; inheritance-off did not receive the same-file handle, while the broadly readable path remained readable. The MXC BaseContainer child entered with token and nonce proof, wrote its stage proof, used stdin/stdout/stderr, and received `ERROR_ACCESS_DENIED` (`5`) opening the same synthetic host path. Its attempted use of the numeric parent handle did not identify a present same-file handle and did not read the secret. This supports only the observed narrow result that this MXC launch did not expose that ambient handle value to this child.

Pinned MXC v1.0.0 `ContainerRequest` has no arbitrary handle-transfer field; the public launcher constructs an explicit handle list for its standard I/O endpoints. Thus intentional arbitrary transfer is unsupported by this API and remains unavailable, not a tested denial. The descendant process was created and synchronously reaped, but it exited with `0xC0000008` (`STATUS_INVALID_HANDLE`) before producing descendant probe proof (`descendant_probe_result=null`). Cleanup is confirmed; descendant handle confidentiality is not. DuplicateHandle/cross-process authority, named-pipe IPC beyond SDK stdio, host-version matrix, and network enforcement were not tested.

## Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Pinned SDK and eligible host | Run `38429625036`, exact SHA above; Windows 11 Enterprise 25H2, build `26200`, UBR `9457`, ARM64; SDK 1.0.0 checksum `e4866ca589a68edc8e931eba58eed9253ad1ce670e974aedc4830a1d94bca6cd`, VCS `a846837d827b496fc4a08dfe60554515758c6647` | pass for this host only |
| Ordinary inheritance-on control | Exit 0; same-file identity true; exact synthetic secret read | pass |
| Ordinary inheritance-off control | Exit 0; same-file identity false; no secret read through the handle; path remains readable | pass |
| MXC child entry and path boundary | Child PID 7248, observed token/integrity, nonce stage proof; host path open returned error 5 | pass for this child and path operation |
| MXC ambient numeric-handle attempt | Source owner PID 3784; child handle absent, same-file false, secret false; probe exception `0xC0000008` | bounded observation only; not proof of explicit transfer denial |
| Standard I/O functionality | stdin pipe write and stdout/stderr proof succeeded | pass for SDK stdio |
| Descendant cleanup and probe | Process creation succeeded; wait completed; `descendant_cleanup_confirmed=true`; descendant exited `0xC0000008`, with no proof result | cleanup pass; descendant confidentiality unqualified |
| Explicit arbitrary handle transfer | No public request field or supported safe transfer route | unavailable/unsupported, not denied |
| Other channels and wider qualification | DuplicateHandle authority, named pipes beyond stdio, other Windows hosts, and network enforcement absent from tested dimensions | not run/unavailable |
| Fixture and host integrity | Only disposable fixture ACL granted; fixture content and setup ACL stable; original host ACL unchanged | pass |
| Strict-positive qualification | `qualified_test_only=false`; failed gates `descendant-handle` and `mxc-exit` | fail; no admission |

The hosted artifact retains the machine report and console output. No credential or populated host-object path is reproduced here.

## Verification

The isolated harness formatting, C036/C037 contract tests, ARM64 Windows cross-check, native helper build and execution, sandbox contract guard, execution-ownership guard, `git diff --check`, and `bash scripts/verify.sh quick` passed. The workflow passing means the bounded evidence job completed; it does not override the report's failed qualification gates.

## Security disposition

- C037 establishes an ordinary inheritance-on/off control and a bounded MXC child observation only. It does not establish that already-valid explicit handles are blocked, that descendants cannot access handles, or that alternate IPC is contained.
- C036's separate closure records verified capture provenance but no child-correlated event and incomplete timeout cleanup in the final exact-SHA run.
- Windows constrained execution remains unavailable/fail-closed. No filesystem or network `Enforced` claim is made; C011/M004–M006/C015–C018 remain blocked, and C030/C031 remain terminally blocked.
- C038 is now eligible to make the final no-admission disposition from C036/C037 evidence. It may not turn these bounded negative/incomplete observations into production approval.
