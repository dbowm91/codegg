# Windows Native Sandbox C052 — Closure Status

Status: closed blocked (both paired launches denied by hosted runner)
Source implementation plan: `plans/implementation/windows-native-sandbox/052-two-hop-native-launch-differential.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md`
Repository baseline reviewed: `39f149a15ef529d66787d1d11551872ea22ec384`
Implementation commits: `7a638dd2`, `9b464bef`, `391a7cca`, `15b221f0`, `74ddf655`, `3e8d77e7`, `829ef60e` — bounded same-host token-flag differential and diagnostics

## 1. Executive finding

C052 ran the paired `CreateRestrictedToken` flag variants on one Windows
Server 2025 hosted runner. Both `CreateProcessAsUserW` attempts returned
`Access is denied`: the C044 control with flags `0` and the candidate with
`WRITE_RESTRICTED`. No child entered user code, so the launch-context
hypothesis was not discriminated. The milestone closes blocked on this
runner, not as a candidate pass or a C044 requalification.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Pin Anthropic and Codex source revisions/licenses | `research/windows-native-sandbox/c052-two-hop-launch-differential.md` | pass | Exact repository heads and Apache-2.0 licenses recorded |
| Same-host paired `flags=0` / `WRITE_RESTRICTED` launch | Run `38615233054`, C052 step | blocked | Both `CreateProcessAsUserW` calls returned Win32 error 5 |
| Child first-instruction marker and process/Job evidence | Same | not run | No process handle was created |
| Identify a single launch-context cause | Same | not established | Both variants failed identically before child creation |
| Verify fixture cleanup and outside-canary stability | Same C052 step | pass, limited | The harness's stage/parent DACL snapshots and outside bytes remained unchanged; no child operation occurred |
| Restricted read/write, aliases, handles, IPC, descendants, toolchain | No child was created | not run | No containment claim follows |

## 3. Production implementation evidence

Only feature-gated test code and an opt-in Windows workflow path changed. No
production launcher, backend registration, profile, installer, persistent
identity, WFP policy, or public contract was added. The workflow runs C052
even when the independent C050 step fails, so this result is not hidden by
the C050 outcome.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk git diff --check
rtk python3 scripts/check_adr_integrity.py
rtk python3 scripts/check_sandbox_contract.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_windows_test_support_release.py
```

### Results

- All listed local guards passed on the recorded revision.
- Hosted Windows run [`38615233054`](https://github.com/dbowm91/codegg/actions/runs/38615233054), exact SHA `829ef60e36080f78514f83ef77820678811f5711`: C052's step completed successfully as an observation harness and emitted both variant outcomes as `process_creation=failed error=Access is denied. (os error 5)`. The overall job failed at C050's equivalent launch failure.
- Runner: Windows Server 2025 Datacenter, version/build `10.0.26100` / `26100`, UBR `33438`, x64; Rust `1.89.0`. Runner principal was `runneradmin`, administrator role true, high-integrity token true, UAC enabled.
- C052's marker is in the Rust test harness function, not a raw/no-CRT first-instruction probe. The account is the hosted runner account, not C044's disposable standard account.

## 5. Invariant review

The two variants used the same host account, source token, executable,
environment, synthetic stage, cwd, API, and Job-before-resume design. The only
requested difference was the `CreateRestrictedToken` flags. Since process
creation failed in both variants, no token restriction or child behavior is
inferred. The production sandbox registry and strict profile invariants are
unchanged.

## 6. Failure and recovery review

Both failures occurred before child creation. No process or thread handle was
returned, so Job assignment, resume, timeout, cancellation, descendant
termination, and child recovery were not exercised. C052 verified that the
synthetic stage/parent DACL snapshots and outside-canary bytes were unchanged
after the denied attempts. No original populated worktree was touched.

## 7. Migration and compatibility review

No migration, protocol, profile, configuration, or launcher behavior changed.
The test-support feature remains unavailable to release builds under the
existing release guard. No compatibility inference is made.

## 8. Security review

The paired experiment provides evidence only that this hosted principal could
not launch either restricted-token variant through `CreateProcessAsUserW`.
It does not show that the flag caused or fixed C044's earlier child-entry
failure. Reads, writes, aliases, handles, IPC, process memory, network, and
host-secret confidentiality remain unqualified.

## 9. Documentation and operations

The source comparison and exact run outcome are recorded in
`research/windows-native-sandbox/c052-two-hop-launch-differential.md`, this
closure, the implementation plan, registry, and subsystem roadmap. ADR,
sandbox, execution ownership, release-boundary, formatting, and whitespace
checks passed.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Both paired `CreateProcessAsUserW` variants are denied on the hosted runner | No differential or child-entry evidence; C044 remains its historical negative | Keep C052 blocked. Any successor requires a separately authorized Windows environment capable of both API calls |
| medium | The paired experiment did not use C044's disposable standard account or raw/no-CRT entry marker | It could not fully requalify C044 even if a child had entered | Preserve these limits in any newly authorized investigation |

## 11. Roadmap disposition

C052 closes blocked because the hosted runner denied both child launches. No
minimal launch-context fix was identified, and no filesystem claim or C045
unlock follows. C049 and C053 remain closed; C050 is separately blocked.

## 12. Registry updates

Mark C052 closed blocked with this record in `plans/registry.md` and the
Windows native sandbox roadmap. C054 records its evidence-based
no-candidate disposition; C045 provisioning, C046 staging/promotion, C047
process-surface integration, and C048 admission remain independently gated.
