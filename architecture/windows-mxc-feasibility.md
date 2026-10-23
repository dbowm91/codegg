# Windows MXC feasibility (M007)

Status: blocked; defer production consideration

Plan: `plans/implementation/windows-native-sandbox/007-mxc-feasibility-spike.md`

Pinned MXC revision: `c6f301d53a1430c4c921a05c57af838f7392348f`

## Disposition

**Defer.** The available evidence is enough to identify material compatibility
and host-preparation questions, but the required isolated SDK build and runtime
matrix was not run. This report makes no security-boundary or enforcement
finding about MXC. CodeGG's Windows sandbox remains unavailable for constrained
execution, and MXC is not linked into production.

## Evidence

| Question | Evidence at pinned revision | Result |
|---|---|---|
| Is the revision fixed and licensed? | Exact revision above; repository license is MIT. | Pass |
| Can it build with CodeGG's supported toolchain? | MXC pins Rust 1.93; CodeGG supports Rust 1.89. The SDK package also has a native build script and target-specific dependencies. No compatible isolated build was run. | Blocked |
| Is the documented Windows floor represented by available runtime evidence? | MXC's pinned ProcessContainer support table is Windows 11-specific. CodeGG's existing native proof ran on Windows Server 2025 build 26100, which is not evidence for a supported Windows 11 client build. | Blocked |
| Does the SDK meet CodeGG's deny-path and fail-closed contract? | No CodeGG adapter or child behavior matrix was run. Unsupported or backend-dependent denied paths cannot be treated as equivalent to CodeGG's required deny roots. | Not established |
| Does use require host-wide or elevated preparation? | The pinned fallback AppContainer+DACL tier documents an administrator-required host-prep executable, persistent ACEs on the system-drive root, and a device security descriptor change. | Material deployment concern |
| Are latency, package size, descendants, network, IPC, ACL recovery, and cleanup qualified? | No qualifying Windows 11 SDK run or measurements. | Not run |

The pinned SDK declares a Rust 1.93 toolchain and builds native assets into the
consuming application. Its ProcessContainer version table names Windows 11
releases and build floors. The documented Tier 3 host-preparation path requires
an elevated binary and includes persistent system-drive ACEs plus a per-boot
device descriptor operation. Those facts make an isolated runtime/build
experiment necessary before any adoption decision; they do not by themselves
prove either secure or insecure runtime behavior.

## Reproduction references

- [Pinned MXC README](https://raw.githubusercontent.com/microsoft/mxc/c6f301d53a1430c4c921a05c57af838f7392348f/README.md)
- [Pinned Rust SDK manifest](https://raw.githubusercontent.com/microsoft/mxc/c6f301d53a1430c4c921a05c57af838f7392348f/src/mxc-sdk/Cargo.toml)
- [Pinned Rust toolchain](https://raw.githubusercontent.com/microsoft/mxc/c6f301d53a1430c4c921a05c57af838f7392348f/src/rust-toolchain.toml)
- [Pinned Windows support matrix](https://raw.githubusercontent.com/microsoft/mxc/c6f301d53a1430c4c921a05c57af838f7392348f/docs/backends/process-container/os-version-support.md)
- [Pinned host preparation contract](https://raw.githubusercontent.com/microsoft/mxc/c6f301d53a1430c4c921a05c57af838f7392348f/docs/backends/process-container/host-prep.md)
- [Pinned Rust SDK API guide](https://raw.githubusercontent.com/microsoft/mxc/c6f301d53a1430c4c921a05c57af838f7392348f/src/mxc-sdk/README.md)
- [Pinned license](https://raw.githubusercontent.com/microsoft/mxc/c6f301d53a1430c4c921a05c57af838f7392348f/LICENSE.md)

## Exact blockers and resumption gate

M007 may resume when an isolated Windows 11 host meeting the pinned
ProcessContainer floor and an experiment toolchain capable of building the
pinned SDK are available. The follow-up must record the exact host build and
commands, then run the same denied-read, outside-write, descendant/cleanup,
network, and IPC probes as the native backend, plus cold-start and package-size
measurements. It must verify host preparation and rollback without touching
unrelated ACLs. Until then, the disposition is defer and no security conclusion
is authorized.

No CodeGG production dependency, backend registration, or default behavior was
changed for this research.

## Successor investigation, without altering M007's historical disposition (2026-10-12)

The above M007 closure is preserved for its pinned early-preview SDK and missing Windows 11 evidence. ADR-0020 now accepts only **test-only** comparison and registers C019–C022, including C020's **new** MXC SDK v1.0.0 feasibility assessment. The 2026-10-07 stable SDK v1 public API and serviced Windows 11 process-isolation capability matrix differ materially from the pinned M007 experiment; `v1.0.0` resolves to upstream commit `7bf210247986cb73b1b314df60c2f1109c479c0b`. The upstream Rust toolchain remains 1.93 (Codegg MSRV 1.89). Windows Server 2025 base build 26100 is not proof of Windows 11 native Process Isolation. Query actual supported host, KB/UBR and selected tier before experiments; never assume an AppContainer+DACL fallback qualifies.

C020 must produce direct running-child observations against C019's denied-read/write aliases, IPC, handles and executable compatibility, plus original-host descriptor preservation. No production MXC integration or new filesystem/network enforcement claim is approved by this append-only successor note. Sources: https://github.com/microsoft/mxc/releases/tag/v1.0.0 ; https://github.com/microsoft/mxc/blob/v1.0.0/docs/backends/process-container/os-version-support.md ; https://code.visualstudio.com/docs/agents/run/agent-sandboxing .

## Successor chain outcome (2026-10-23)

The successor chain that began at C020 has run to its end, and it ended
**negative**. `plans/closure/windows-native-sandbox/054-status.md` closes the
line as a bounded no-candidate disposition:

- **C050** (`WRITE_RESTRICTED` disposable root) — `CreateProcessWithTokenW`
  returned access denied **before** child creation; no write-denial, toolchain,
  or Job evidence.
- **C051** (disposable stage / native toolchain) — closed blocked; C050's
  strict-positive prerequisite was not met, so no stage or promotion probe ran.
- **C052** (two-hop launch differential) — both paired `CreateProcessAsUserW`
  calls returned access denied; no child entry, no cause identified.
- **C053** (MXC changed-API re-entry gate) — negative: the published Rust SDK
  is still `v1.0.0`, and the typed API still lacks the DACL-fallback opt-out
  knob that C043 needed. No native retry was authorized.
- **C054** — no candidate qualifies; no production admission.

**Current posture:** Windows constrained execution stays unavailable and
fail-closed. ADR-0030 (broad-read compatibility profile) remains *proposed and
unaccepted*, so there is no public broad-read profile, no wire/UI surface, and
no backend authorization for one. `mxc-sdk` is still absent from the workspace
manifest: it appears only in the three research harnesses under
`research/windows-native-sandbox/` (`mxc-v1-harness`, `mxc-c033-harness`,
`mxc-c035-harness`), none of which is a workspace member, so nothing MXC
provides can reach a shipped binary.

## CodeGG's own containment posture (verified 2026-10-23)

The Windows gap is not a gap in a Windows *backend* that was merely
unqualified — **no Windows backend is registered at all**:

- `src/security/sandbox/backend.rs::BACKENDS` holds exactly two entries,
  `BackendId::LANDLOCK` and `BackendId::SEATBELT`. Registration order *is*
  preference order, and `backend::select()` returns the first backend whose
  `probe` succeeds. There is no Windows entry, so on Windows no backend can
  ever be selected.
- `BackendId::BUBBLEWRAP` exists as an associated constant but is deliberately
  **not** in `BACKENDS`; it is reached only through an explicit Linux launch
  request, never as a legacy backend fallback.
- `src/security/sandbox/policy.rs::windows_profile_root()` exists only to
  resolve `USERPROFILE` for the deny-root set, and
  `src/security/sandbox.rs::SandboxConfig::launch_spec` refuses a Windows
  launch spec when it is `None`. That is a *deny-path* input, not an
  enforcement mechanism.
- The shared-checkout profile is gated by a compile-time table,
  `codegg_core::run_placement::QUALIFIED_SHARED_CHECKOUT_PLATFORMS =
  &["linux", "macos"]`. Windows is excluded for a reason different from
  macOS's: no filesystem-enforcement backend is registered for it at all, so
  the profile is refused for want of a *capability*, not for want of a
  *measurement* — no CI run could produce the missing evidence.

`src/security/sandbox/guarantee.rs` (C042) holds the typed
requested-versus-obtained contract (`RequestedGuarantees`, `CapabilityFact`)
that a future Windows backend must satisfy. It is internal only: no public
profile, config key, or wire schema was added, and it never reinterprets a
strict `ReadOnly`/`WorkspaceWrite` request as broad-read.

## Source verification

Checked 2026-10-23 against the repository at this worktree.

| Claim | Source checked | Result |
|---|---|---|
| MXC is not linked into production | `Cargo.toml` (workspace manifest), `crates/*/Cargo.toml` | Confirmed — no `mxc-sdk` dependency. The only occurrences are `research/windows-native-sandbox/mxc-v1-harness/Cargo.toml`, `mxc-c033-harness/Cargo.toml`, `mxc-c035-harness/Cargo.toml`, each pinning `mxc-sdk = "=1.0.0"`; none is a workspace member. |
| No Windows containment backend is registered | `src/security/sandbox/backend.rs::BACKENDS`, `::BackendId`, `::select` | Confirmed — two entries, Landlock and Seatbelt only. |
| The Windows gap is a missing capability, not a missing measurement | `crates/codegg-core/src/run_placement.rs::QUALIFIED_SHARED_CHECKOUT_PLATFORMS`, `::shared_checkout_availability` | Confirmed — `&["linux", "macos"]`; the refusal text names the platform and says it has no native containment evidence. |
| Windows deny-root input still resolves `USERPROFILE` | `src/security/sandbox/policy.rs::windows_profile_root`, `src/security/sandbox.rs::SandboxConfig::launch_spec` | Confirmed — `launch_spec` refuses with `ToolError::Permission` when the root is `None`. |
| Codegg MSRV is 1.89, upstream MXC toolchain is 1.93 | `Cargo.toml::rust-version = "1.89"`; `plans/implementation/windows-native-sandbox/020-mxc-v1-native-process-isolation-feasibility.md`; `research/windows-native-sandbox/adr-0020-harness-comparison.md` | Confirmed — the plan and the research note both record the `1.93` vs `1.89` gap and forbid a workspace-wide MSRV bump. |
| The successor chain closed with no qualifying candidate | `plans/closure/windows-native-sandbox/054-status.md`; `plans/implementation/windows-native-sandbox/054-windows-sandbox-successor-disposition-and-handoff.md`; `plans/registry.md` (C049–C054 rows) | Confirmed — C054 closed "no candidate; no admission"; ADR-0030 proposed, C051 blocked, C045/C046/C048 separately gated, Windows Required unavailable. |
| The successor note's ADR mapping is current | `plans/adrs/ADR-0020-windows-native-sandbox-after-appcontainer-feasibility.md`, `ADR-0024-windows-two-stage-native-launch-and-mxc-requalification.md`, `ADR-0025-windows-mxc-helper-diagnostic-corrective.md` | Confirmed — ADR-0024 and ADR-0025 are both accepted **test-only**; neither authorizes a production mechanism. |

Historical line-level claims about the pinned upstream MXC revision are
pinned to that external revision and are **not** re-verified against upstream
here; they stand as recorded on 2026-10-12 and are cited by exact URL.

