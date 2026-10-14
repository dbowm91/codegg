# Windows Native Sandbox C043 — Closure Status

Status: closed (negative bounded adapter qualification)
Source implementation plan: `plans/implementation/windows-native-sandbox/043-mxc-basecontainer-managed-process-adapter-feasibility.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0027-capability-stratified-successor-handoff-post-pr-111-merge-2026-10-13`
Repository baseline reviewed: `eb89715c1`
Implementation commits: none for the adapter (no adapter code landed; negative rests on backend-level decisive stops documented below)

## 1. Executive finding

C043 is closed negative: the pinned MXC backend cannot satisfy the
adapter's hard acceptance prerequisites, so no ManagedProcessService
adapter was built around it. Two independent stops, both measured:

1. **Schema stop (work package A).** Pinned `mxc-sdk` 1.0.0
   `ProcessContainerConfig` exposes `learning_mode`, `capabilities`,
   `capture_denials`, `ui`, `filesystem`, `network` — and no
   `allowDaclMutation` / fallback-prohibition knob or equivalent
   explicit original-DACL-mutation prohibition. The plan requires
   validating that knob and failing the tier if it silently uses an
   AppContainer/DACL fallback; the knob does not exist in the pinned
   schema, so the explicit-prohibition half of package A fails.
2. **Backend-behavior stop (work packages C/D).** Fresh hosted runs on
   the eligible tier reproduce the decisive C038 gaps exactly: the
   native helper still fails to initialize
   (`STATUS_DLL_INIT_FAILED`, exit `-1073741502`) and inherited-handle
   transfer proof remains `unavailable`, while the full read/alias/
   outside-write/pipe denial matrix, ACL stability, descendant
   cleanup, and Git/Python/Cargo compatibility are preserved. Handle/
   IPC reconciliation (package D) is therefore impossible on this
   backend, independent of any adapter wrapper.

Building work packages B–E (adapter + matrix + evidence) around a
backend with known, reproduced, adapter-independent decisive gaps
would not change the backend's init/handle behavior. The honest
disposition is negative, not another verification loop. Windows
`SandboxRequest::Required` stays unavailable/fail-closed; no backend
registered; C038's no-candidate decision stands, re-confirmed on the
current runner image.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| C042 stable contract (hard dep) | `042-status.md`, `src/security/sandbox/guarantee.rs` at `eb89715c1` | pass (predecessor) | Closed before this assessment |
| Pinned eligible serviced Windows 11 host | Run `38459652871` (host-eligibility, head `eb89715c1`): Win11 Enterprise 25H2 b26200 UBR9457 ARM64, `processmodel.dll` loads, `CreateProcessSecurityEnvironment` present, `native_candidate_host_eligible=true` | pass | Old "host gate outstanding" blocker is retired by measurement |
| `fallback.allowDaclMutation=false` schema validation (A) | Local source inspection of pinned `mxc-sdk` 1.0.0 `src/configs/process_container.rs`: no such field or equivalent prohibition | fail | Explicit-prohibition requirement unsatisfiable on pinned SDK |
| BaseContainer deny-path fidelity without host mutation (A/C) | Run `38459657632` (C033 requal, head `eb89715c1`): `processcontainer`/`base-container` selected, exit 0, `direct/junction/case_alias/hardlink/ads_read=denied`, `outside_write=denied`, `pipe=denied`, 21 block-mode denials, host+secret ACLs and bytes unchanged | pass | Behavioral half holds; does not cure the schema or handle stops |
| Native child entry + minimum tools (C) | Same run: child entered, `same_file_normal_child_baseline=allowed`, git/python/cargo allowed, descendant gone after cleanup | pass | Unchanged from C033/C037 |
| Native helper init + inherited-handle transfer (C/D) | Same run: `inherited_handle=unavailable`, probe `Exited(-1073741502)` (`STATUS_DLL_INIT_FAILED`); `unsupported` still lists UNC/timeout-trace/standard-account baseline | fail (decisive) | Adapter-independent backend property; reproduces C038 exactly |
| ManagedProcessService-compatible adapter (B) | not built | not executed (justified) | Backend-level stops precede adapter construction; building it cannot change init/handle behavior |
| JSON evidence with raw IDs + SHA (E) | C033 `C033_REPORT` re-collected at head `eb89715c1` (run `38459657632`) | pass (backend level) | No C043-adapter evidence exists because no adapter was built |

## 3. Production implementation evidence

None for C043. No source, config, storage, protocol, runtime,
frontend, guard, or docs changes. The assessment reused the standing
C033 harness and host-eligibility workflow via `workflow_dispatch` on
this branch — no repo changes were needed for the evidence. The C042
contract the adapter would have consumed exists and is closed.

## 4. Verification executed

### Commands run (hosted, exact-SHA)

- `gh workflow run windows-mxc-v1-host-eligibility.yml --ref planning/windows-sandbox-capability-successors` → run `38459652871`, head `eb89715c1`, conclusion `success`. `C020_HOST.native_candidate_host_eligible=true`; disposition line: host prerequisite set present.
- `gh workflow run windows-mxc-c033-requalification.yml --ref planning/windows-sandbox-capability-successors` → run `38459657632`, head `eb89715c1`, conclusion `success`. `C033_REPORT.result=Exited(0)`, all seven required denial probes present, ACLs unchanged, `inherited_handle_probe_execution=Exited(-1073741502)`.

### Commands run (local, darwin)

```bash
git diff --check
python3 scripts/check_adr_integrity.py
```

Local: no C043 files changed; guards pass. Schema inspection is a
deterministic source read of the pinned registry copy
(`mxc-sdk-1.0.0/src/configs/process_container.rs`), not host-dependent.

### Result details

Hosted Windows 11 ARM runner (windows-11-arm pool, image
`windows-11-arm` per prior C020 runs): edition `Microsoft Windows 11
Enterprise`, version `10.0.26200`, build `26200`, UBR `9457`,
`DisplayVersion` `25H2`, arch `Arm64`, MXC SDK `1.0.0` (Cargo
pinned). No Windows Server inference is made; no production admission
is claimed by either run (both workflows record evidence only).

## 5. Invariant review

All invariants preserved: no silent BaseContainer selection, no DACL
fallback assumed (behaviorally unchanged ACLs observed, but no
explicit knob claimed), no MSRV uplift (isolated 1.93.1 toolchain
only in CI), no production `Required` path, no original-host mutation
(ACLs byte-identical before/after), C011/M004–M006, C015–C018,
C030/C031, C029/C034/C038 dispositions unchanged.

## 6. Failure and recovery review

No adapter, no new fixtures beyond the C033 harness's own synthetic
set (removed by that harness's teardown). The negative rests on
measured backend behavior, not on a failure to clean up.

## 7. Migration and compatibility review

None (no changes, no new surface).

## 8. Security review

No new authority, surface, or secret handling. Closing negative
rather than building an adapter around a backend that cannot prove
handle confinement is the secure disposition: an adapter cannot
upgrade backend guarantees it does not have.

## 9. Documentation and operations

This closure record is the documentation. A future MXC retry would
need a new SDK revision resolving (a) the explicit DACL-mutation
prohibition schema gap and (b) native helper init + handle-transfer
provability — i.e., a new implementation plan, not a reopening of
C043.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Pinned backend cannot initialize native helpers under confinement (`STATUS_DLL_INIT_FAILED`) and cannot report handle transfer | No strict-positive backend exists to adapt; C043 acceptance unsatisfiable | New SDK/backend revision + new plan; C043 stays negative |
| medium | No explicit DACL-mutation prohibition in pinned schema | Enforcement rests on per-run behavioral observation, not contract | Same as above |
| low | UNC/timeout-trace/standard-account dimensions still unsupported in C033 harness | Matrix incomplete in the same places as C033 | Future harness work if a new backend ever qualifies |

No finding reopens C015–C018 or C030/C031.

## 11. Roadmap disposition

Milestone closed (negative bounded adapter qualification). It does
not gate C044 (parallel two-hop line, active), the C047 audit slice
(closed via C042), or C041/C042 (closed). C045/C046/C048 are
unaffected: they remain blocked behind strict-positive-backend and
new-ADR gates that this negative does not satisfy. The MXC lane
terminates here on the pinned SDK; any retry is new-plan work.

## 12. Registry updates

C043 row `blocked (C042 satisfied; host gate outstanding)` →
`closed (negative bounded adapter qualification)`, closure
`plans/closure/windows-native-sandbox/043-status.md`. Implementation
plan `043-*.md` status: `blocked` → `implemented (negative bounded
adapter qualification)`. Unblock audit: this closure unblocks
nothing; C045 remains blocked (still needs a strict-positive backend
plus a separate provisioning ADR), C048 remains blocked (no
strict-positive backend). The old host-gate blocker is retired by
measurement (run `38459652871`), replaced by backend-level negative
grounds that no host change can cure on the pinned SDK.
