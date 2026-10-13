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
