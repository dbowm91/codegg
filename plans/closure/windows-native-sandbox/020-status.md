# Windows Native Sandbox C020 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/windows-native-sandbox/020-mxc-v1-native-process-isolation-feasibility.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0020-independent-successor-feasibility-2026-10-12`

Repository baseline reviewed: `cfc5466a78afdb3bd00c090b15d617ebbe40e09d` (`work/windows-native-sandbox-foundation`, exact hosted evidence SHA)

Implementation commits or pull requests:

- `0403c12d`, `eb871f61`, `cf743353`, `5f82cbcd`, `cf22af3f`, `64dc066e`, `af80ee96`, `cfc5466a` — isolated MXC v1 feasibility harness and hosted native probes.
- GitHub Actions run [38335783458](https://github.com/dbowm91/codegg/actions/runs/38335783458), job `115095415457`, passed at the workflow level on the exact evidence SHA. The test report explicitly records the incomplete inherited-handle probe and lifecycle-gate failure; workflow success is not a strict-positive result.

## 1. Executive finding

C020 is closed as a **bounded negative feasibility result** for the pinned MXC v1 ProcessContainer candidate. The Windows 11 Enterprise 25H2 ARM64 host selected `processcontainer` / `base-container` and the child denied all exercised direct and aliased synthetic host-secret reads, outside writes and a named-pipe connection. Git init/status and Cargo version ran; the descendant was gone when the MXC invocation returned; host and secret ACLs and secret bytes were unchanged. However, a Python script failed to initialize its installed runtime, and the direct native inherited-handle helper exited with `-1073741502` (`STATUS_DLL_INIT_FAILED`), so inherited-handle denial could not be measured. The candidate does not satisfy the required runnable-tool and complete probe gate and is not recommended for integration. The positive denials do not convert that missing evidence into a pass.

This finding applies to the observed host, MXC v1.0.0, policy and fixture. It is not a claim that Windows Process Isolation is generally unsafe or that a prohibited read succeeded. No production backend or user-facing enforcement claim was added.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Eligible native host and pinned candidate | Run `38335783458`, `C020_REPORT`, host probe | pass | Windows 11 Enterprise 25H2, build `26200`, UBR `9457`, ARM64; `processmodel.dll` loaded and PSEC export found. MXC v1.0.0 release binary SHA-256 `0bb483b3592d0f663f2c5a7a2ae17db89cc04b82238684b0a1a9384b0fe828ed`; isolated Rust toolchain `1.93.1`. |
| Effective backend and policy | Run `38335783458`, `C020_MXC_BACKENDS`, `C020_MXC_PROBE` | pass | Selected `processcontainer`, tier `base-container`, `CaptureDenials` and `FilesystemDeniedPaths`; no fallback warning; `base_container_supports_deny_paths=true`, `needs_dacl_augmentation=false`. |
| Allowed stage and developer tools | Run `38335783458`, child stdout/stderr | partial | Git init/status and `cargo --version` succeeded. Python `--version` succeeded, but a Python script failed with `Failed to find real location of ... Python\3.13.16\arm64\python.exe`; native helper exited `STATUS_DLL_INIT_FAILED`. Required tool compatibility is incomplete. |
| Running-child read/write/IPC denials | Run `38335783458`, `C020_REPORT.stdout` | pass for exercised probes | Direct, junction, case alias, hardlink, ADS, extended path, sibling and named-pipe read/connect attempts were denied; outside write was denied. All were child observations. |
| Inherited-handle denial | Run `38335783458`, `inherited_handle_probe_execution` | not measured | Native helper exit `-1073741502`; report records `inherited_handle=unavailable`. Do not interpret as denial. |
| Descendant lifecycle | Run `38335783458`, descendant fields | pass, bounded | Python launched a `cmd.exe` descendant; it was no longer alive at return. No independent cancellation or timeout trace ran. |
| No original fixture mutation | Run `38335783458`, ACL/hash fields | pass | Host and synthetic secret ACL strings matched before/after; secret content was unchanged. |
| Network, UNC, independent cancellation | Run `38335783458`, unsupported fields | not run | Network was not configured or claimed; UNC fixture and independent timeout/cancellation trace were unsupported in this run. |

## 3. Production implementation evidence

The changes are confined to the isolated `research/windows-native-sandbox/mxc-v1-harness/` experiment and its test-only GitHub workflow. No MXC dependency was added to the CodeGG workspace or shipped binary. No public API, backend registry, config, storage, installer, MSRV, original workspace, or user ACL was changed. MXC owns its candidate child lifecycle in this experiment; this does not replace or qualify CodeGG's `ManagedProcessService` ownership contract.

## 4. Verification executed

### Commands run

```text
GitHub Actions: Windows MXC v1 host eligibility, run 38335783458
  host probe and pinned MXC v1.0.0 isolated Rust harness
  actual ProcessContainer child matrix and C020_REPORT capture
```

### Results

- Workflow conclusion: success; exact head SHA `cfc5466a78afdb3bd00c090b15d617ebbe40e09d`.
- Selected actual-child fixture count: one primary MXC child matrix plus one native inherited-handle helper attempt. The primary child exited `0`; helper exited `-1073741502`.
- `C020_REPORT` recorded `descendant_alive_at_return=false`, unchanged host and secret ACLs, unchanged synthetic secret, and `inherited_handle=unavailable`.
- The script reported `C020 strict lifecycle gate failed: inherited_handle=inherited_handle=unavailable, descendant_alive_at_return=false`. The workflow's successful report/collection is not evidence that this strict gate passed.
- No local Windows-native execution was available. This is hosted Windows 11 evidence, not Windows Server evidence.

## 5. Invariant review

- Windows production `SandboxRequest::Required` remains unavailable; no backend was registered and no `Enforced` outcome is advertised.
- No fallback to AppContainer+DACL was taken, no original host ACL was changed, and CodeGG MSRV remains unchanged.
- The only exercised resources were synthetic runner-temporary fixtures; their ACL and content snapshots were unchanged.
- No network isolation or offline guarantee is claimed.

## 6. Failure and recovery review

The candidate reaped its observed descendant when the invocation returned. The direct native handle helper and Python script could not initialize under this child environment; these are compatibility failures, not denials. Independent cancellation/timeout, repeated cold/warm startup, concurrent workspaces, crash residue, rollback/uninstall, and complete helper cleanup were not qualified. Because the candidate failed the bounded required-tool/handle-probe gate, these operational measurements were not used to promote it. Any future reconsideration requires a new bounded plan and reproducible successful child initialization before claiming the omitted dimensions.

## 7. Migration and compatibility review

No schema, protocol, config, dependency, installer, toolchain default or runtime migration was introduced. The experiment used the isolated MXC toolchain `1.93.1`; CodeGG's Rust MSRV remains `1.89`.

## 8. Security review

Actual child operations denied the tested direct and alias secret reads, outside write and named-pipe access. Fixture descriptors and secret bytes were unchanged. The inherited-handle outcome is unavailable, and device/registry/UNC/network behavior is unqualified. C020 therefore supports a negative candidate compatibility disposition only; it does not establish a complete threat-boundary proof or an access leak.

## 9. Documentation and operations

- The workflow records host edition/version/build/UBR, PSEC export, MXC backend/tier/capabilities, runtime output, fixture ACLs, process lifecycle and unsupported probes.
- `research/windows-native-sandbox/adr-0020-harness-comparison.md`, the implementation plan, roadmap and registry record the final bounded result.
- M007 remains a separate historically deferred preview experiment.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Python script and native helper initialization failed; inherited-handle denial was not measured. | MXC does not meet the required tool/probe gate on this host, and cannot be considered a positive candidate. | Do not select this candidate. Any re-evaluation must first reproduce reliable Python/native child initialization and collect an actual inherited-handle result. |
| medium | Independent cancellation, timeout, concurrency, latency/cost and crash-residue dimensions were not run. | No operational or production lifecycle claim is supported. | Keep MXC out of production admission; scope any reconsideration in a new plan. |
| low | UNC and network behavior were not tested. | Network/UNC enforcement remains unknown. | Keep network unclaimed; require a dedicated future matrix if reconsidered. |

## 11. Roadmap disposition

C020 is closed as a bounded negative candidate result. C021 remains conditionally closed with positive but incomplete dedicated-principal evidence; C022 records the comparative disposition as insufficient evidence for production selection. C020 does not unblock C011/M004–M006 or C015–C018 and does not alter historical M007.

## 12. Registry updates

- Mark C020 closed (negative bounded feasibility) with exact run `38335783458` and SHA `cfc5466a78afdb3bd00c090b15d617ebbe40e09d` in `plans/registry.md` and the Windows native sandbox roadmap.
- C019's closure unblocked C020 and C021; C020 and C021 written dispositions satisfy C022's comparison dependency. No other blocked plan became dependency-ready. Keep C011/M004–M006 and C015–C018 blocked; no production admission or network claim follows.
