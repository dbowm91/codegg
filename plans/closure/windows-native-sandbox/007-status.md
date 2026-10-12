# Windows Native Sandbox Milestone 007 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/windows-native-sandbox/007-mxc-feasibility-spike.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#7-milestones`

Repository baseline reviewed: `ef5f8a786b7b4d774b3a53eed6a0b1bc3efcfa66`

Implementation commits or pull requests:

- none; evidence-only research is recorded in `architecture/windows-mxc-feasibility.md`

## 1. Executive finding

M007 is blocked under its explicit stop condition. The pinned MXC revision and
its public documentation were inspected, but no isolated SDK build or runtime
security matrix ran on a qualifying Windows 11 host. The disposition is defer;
this record makes no claim that MXC does or does not meet CodeGG's security
contract. MXC remains absent from production and does not affect M004–M006.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Pin exact MXC revision and license | `architecture/windows-mxc-feasibility.md`; exact revision `c6f301d53a1430c4c921a05c57af838f7392348f`, MIT | pass | Source facts only |
| Record SDK/toolchain compatibility | Pinned `src/rust-toolchain.toml` specifies Rust 1.93; CodeGG supports 1.89 | partial | No isolated build run |
| Record Windows availability | Pinned ProcessContainer table specifies Windows 11 release/build floors | partial | No qualifying Windows 11 host available to this experiment |
| Run denied-read/write, descendants, cleanup, network and IPC probes | No Windows 11 SDK execution | blocked | No security conclusion |
| Measure latency and package size | Not measured | blocked | Requires successful isolated build/run |
| Evaluate host preparation | Pinned host-prep docs describe administrator execution and host-wide descriptor changes in the fallback tier | pass | Documentation review only; no operation performed |
| Keep production integration disabled | No production dependency or backend registration added | pass | M004 remains canonical |

## 3. Production implementation evidence

None. No runtime, dependency, protocol, storage, configuration, authority, or
backend-selection change was made. MXC is not registered as a sandbox backend.

## 4. Verification executed

### Commands run

- No MXC build or runtime commands were run; this host is not the qualifying
  Windows 11 runtime required by the plan.
- Pinned official repository sources were inspected and summarized in
  `architecture/windows-mxc-feasibility.md`.

### Results

The document review identified a toolchain mismatch (Rust 1.93 vs CodeGG's
Rust 1.89), a Windows 11-specific support matrix, and elevated host preparation
requirements for the documented fallback tier. These are feasibility concerns,
not runtime security results.

## 5. Invariant review

- No MXC `Enforced` status is reported.
- No production dependency, network requirement, elevation requirement, or
  backend selection change was introduced.
- M001's guarantee matrix and `ManagedProcessService` ownership remain
  unchanged.
- Unsupported deny-path behavior has not been waived.

## 6. Failure and recovery review

No runtime experiment or host mutation occurred. Therefore no cleanup,
cancellation, restart, or recovery behavior is qualified.

## 7. Migration and compatibility review

No migration or compatibility change.

## 8. Security review

No MXC security boundary is qualified. In particular, denied reads, writes,
descendant containment, named-pipe access, network behavior, ACL restoration,
and failure cleanup remain untested.

## 9. Documentation and operations

- Added `architecture/windows-mxc-feasibility.md` with pinned-source findings,
  references, limitations, and an explicit resumption gate.
- Updated the implementation plan, subsystem roadmap, and planning registry to
  mark M007 blocked.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | No qualifying Windows 11 host or isolated SDK runtime evidence | MXC cannot be evaluated against CodeGG's security contract | Resume only when the pinned Windows build and SDK toolchain can be exercised; run the required negative matrix |
| medium | Pinned SDK requires Rust 1.93 while CodeGG supports Rust 1.89 | Direct in-workspace build compatibility is unestablished | Use an isolated research toolchain or reassess after the repository toolchain changes |
| medium | Fallback AppContainer+DACL host preparation is elevated and changes host-wide security descriptors | Deployment and rollback may conflict with CodeGG's operator boundary | Inspect on a disposable host and demonstrate exact reversible state before any future ADR |

## 11. Roadmap disposition

M007 is blocked; defer. No security conclusion or production adoption is
authorized. M004 remains the active Windows capability milestone; M007 neither
blocks nor qualifies it.

## 12. Registry updates

- `plans/registry.md`: M007 is blocked pending a qualifying Windows 11 test host
  and isolated SDK build/runtime experiment.
- `plans/subsystems/windows-native-sandbox-roadmap.md`: M007 is blocked and its
  stop condition/resumption gate is recorded.
