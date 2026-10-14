# Windows Native Sandbox C050 — Closure Status

Status: closed blocked (host denied restricted-token child creation)
Source implementation plan: `plans/implementation/windows-native-sandbox/050-write-restricted-disposable-root-feasibility.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md`
Repository baseline reviewed: `39f149a15ef529d66787d1d11551872ea22ec384`
Implementation commits: `0db2e5f8`, `7a638dd2`, `9b464bef`, `391a7cca`, `15b221f0`, `74ddf655`, `3e8d77e7`, `829ef60e` — feature-gated disposable-root probe and bounded-launch corrections

## 1. Executive finding

C050's bounded test-only probe compiled on Windows Server 2025 but could not
create its restricted child. `CreateProcessWithTokenW` returned Win32 error 5
(`Access is denied`) on hosted run `38615233054` at exact head
`829ef60e36080f78514f83ef77820678811f5711`. No child ran, so C050 establishes
no WRITE_RESTRICTED filesystem behavior. The milestone closes blocked on the
available runner; it does not qualify a candidate.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Create a real restricted token with a synthetic restricting SID | Hosted C050 test reached `CreateProcessWithTokenW`; no token-creation error | partial | Token creation succeeded, but no process was created |
| Reach child code and write within the new stage | `38615233054`, C050 test output | not run | Child never entered |
| Deny an outside-stage write | Same | not run | No child write attempt occurred |
| Preserve original host objects and compare snapshots | Fixture only edits the newly created test stage; snapshots follow child completion | partial | No populated worktree was targeted; final snapshot assertions were not reached |
| Assign child to Job before resume and reap it | Same | not run | No process handle was returned |
| Verify native toolchain compatibility | No C051 execution | not run | Hard C050 positive prerequisite was unmet |

## 3. Production implementation evidence

Only feature-gated Windows test support was added. No production backend,
profile, process admission path, installer, staging materializer, or public
configuration changed. The fixture creates a temporary root and changes only
the DACL of its newly created stage.

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
- Hosted Windows run [`38615233054`](https://github.com/dbowm91/codegg/actions/runs/38615233054), exact SHA `829ef60e36080f78514f83ef77820678811f5711`: compilation passed; C050 test failed at `CreateProcessWithTokenW` with access denied. C052 then ran independently.
- Runner: Windows Server 2025 Datacenter, version/build `10.0.26100` / `26100`, UBR `33438`, x64; Rust `1.89.0`. Runner principal was `runneradmin`, administrator role true, high-integrity token true, UAC enabled.
- The workflow's ordinary Windows suites were skipped on this opt-in run; this run tested only the C050/C052 probes and the AppContainer filesystem proof.
- `rtk bash scripts/verify.sh quick` passed earlier on the same branch before the final diagnostic-only import/context changes; the final changes were covered by formatting, whitespace and focused Windows CI.

## 5. Invariant review

The production sandbox registry and strict profile semantics are unchanged.
No ACE was applied to an existing user worktree or original repository path.
The test cannot support any claim about reads, writes, aliases, IPC, network,
process memory, or tool execution because its child was not created.

## 6. Failure and recovery review

The failure occurred before process creation returned a handle. No Job, child,
descendant, or teardown path was exercised. The temporary test root is owned
by the fixture; the original-host descriptor post-snapshot assertion was not
reached. No retry or privilege grant is introduced.

## 7. Migration and compatibility review

No schema, protocol, configuration, or migration change was made. The
test-support feature remains guarded from release builds. There is no Windows
compatibility or toolchain claim.

## 8. Security review

The hosted runner's elevated identity did not make restricted-token child
creation available through `CreateProcessWithTokenW`. This is an observed
operational block, not evidence that WRITE_RESTRICTED grants or denies a file
operation. No host-wide privilege, ACL relaxation, or fallback was attempted.

## 9. Documentation and operations

This record, the C050 plan, registry, and subsystem roadmap capture the
blocked result. ADR integrity, sandbox contract, execution ownership,
test-support release, formatting, and whitespace checks passed. Hosted run
logs are linked above.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | The available Windows runner denies `CreateProcessWithTokenW` for the restricted token | No child-level WRITE_RESTRICTED or synthetic-root property is qualified; C051 cannot proceed | Keep C050 blocked. Revisit only with a separately authorized Windows environment that permits the API and records exact child/Job evidence |
| medium | C050's positive/negative filesystem, handle, alias, IPC, descendant, and toolchain matrices were not reached | Broad-read write-confinement candidate remains unqualified | Do not admit or advertise a backend; any successor must scope and authorize a new bounded experiment |

## 11. Roadmap disposition

C050 closes blocked on this runner. C051 remains closed blocked at its hard
positive prerequisite. C046 staging/promotion and C048 production admission
remain independently blocked; no downstream candidate or production work is
unblocked.

## 12. Registry updates

Mark C050 closed blocked with this record in `plans/registry.md` and the
Windows native sandbox roadmap. Keep C051 closed blocked, C046 staging and
C048 admission blocked, and C054 closed with no candidate.
