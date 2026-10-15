# ADR-0024 — Windows two-stage native launch and MXC requalification

Status: accepted — **bounded, test-only successor feasibility only; no production mechanism selected**
Date: 2026-10-13
Decision owners: Codegg maintainers
Baseline: `work/windows-native-sandbox-foundation` at `c7e2dae404d5b58f6bd927aa8d99b4d518c4f23d` (refresh before execution).

References: `plans/000-long-term-specification.md` sections 4, 17, 27, 29; `plans/001-terminology-and-domain-model.md` section 8; `plans/003-planning-process.md`; ADR-0004, ADR-0016, ADR-0019, ADR-0020, ADR-0023; `plans/subsystems/windows-native-sandbox-roadmap.md`.
Research: `research/windows-native-sandbox/adr-0022-two-stage-and-mxc-successors.md`.

## Context

M001–M003 implement launch supervision and Windows command compatibility. C011 exposed readable denied secrets via direct paths/junctions/hardlinks; C013 showed ACL mutation and descriptor-restoration drift on populated host trees. C014 AppContainer staging ran PowerShell but not minimum Git. C020 MXC v1 produced some direct/alias denials and Job containment but failed Python/native helper startup and did not test an inherited handle. C021 launched a separate standard account via `CreateProcessWithLogonW` and passed private-DACL tests but did not confine reads of broadly group-readable files. C024/C027 tried restricted token-taking creation from the original operator/runner context and did not create a working restricted child; C028 verified four children resumed then stalled before user entry. C029/ADR-0023 rejected **selection of a privileged service broker or native Windows guest** and recorded no admission. Those results and closures remain valid.

New external design evidence changes the list of *candidates*, not those old results: OpenAI Codex and Anthropic sandbox-runtime independently use a first-stage `CreateProcessWithLogonW` to run a command runner as a separate standard account, then construct a restricted token **from that runner's own primary token** and spawn the actual process. Microsoft documents that `CreateProcessAsUserW` does not require `SE_ASSIGNPRIMARYTOKEN_NAME` when the supplied token is a restricted version of the caller's primary token; it may still require `SE_INCREASE_QUOTA_NAME` and other actual launch prerequisites. Therefore this alternative is **not** equivalent to an elevated SCM broker or a host-side token launch. It does not prove read isolation or fix C028 automatically.

MXC now exposes documented denial-capture and OS-tier diagnostics worth a separate, pinned feasibility check. A green API or toolchain smoke test still does not establish the requested read/write boundary.

## Decision

Approve only the *research/implementation of test-only feasibility harnesses* for two parallel candidates and one subsequent comparative disposition:

- **C032: two-stage unprivileged dedicated-principal runner** — the host controller launches an immutable runner as the distinct sandbox user; inside that account the runner restricts **its own** primary token without `WRITE_RESTRICTED`, then launches a Job-supervised child. No privileged arbitrary-command broker, service, automatic installer, persistent account or product backend. Any administrative provisioning is limited to explicit operator-approved, disposable synthetic VM/test fixture setup. A negative launch or tool-compatibility result closes the experiment negative; it never counts as denied read.
- **C033: MXC denial telemetry and compatibility requalification** — isolate pinned SDK/binary/MSRV, qualify an OS-supported tier, verify *requested versus honored* filesystem operations from a real child, and diagnose essential native toolchain failures using deny-and-record rather than permissive access. Permissive logging may run only on synthetic trusted fixtures and **must not** be used as evidence of confinement.
- **C034: independent comparative no-admission/admission-proposal decision** — conclude with positive/negative/blocked evidence for both candidates, including separate per-dimension filesystem read, write, network, process and compatibility facts. A positive test can recommend a **future separately authorized** production ADR, but cannot register a backend or close C011/M004–M006 itself.

C032 and C033 may begin in parallel, each dependent on C019's common oracle and accepted ADR-0024, with operational gates for a disposable consented Windows host and a serviced eligible Windows 11 host respectively. C034 depends on written dispositions from both (negative/blocked is an admissible disposition); positive recommendation requires independent Windows security review and a strict-positive actual-child test. No implementation agent may infer blanket admission from user approval of *this feasibility ADR*.

ADR-0023 **remains rejected** for its *privileged SCM broker/guest* alternatives; C030/C031 remain terminally blocked. This new ADR is additive, not a silent reopening or historical rewrite of ADR-0023 or C024–C029, and does not override ADR-0004 or ADR-0016.

## Hard security and ownership invariants

1. Original user workspace, profile, credentials, populated host files and hardlink-sharing objects MUST NOT have DACL/SACL/MIC/owner/descriptor-control bits changed. Test-only synthetic *newly created* stage objects may receive grants. No shared broad user-profile/runtime cache grant, global device permission or service privilege.
2. For *strict read confinement*, first prove a normal dedicated-account child **can** read an unchanged synthetic `Users`/`Authenticated Users`/`Everyone`-readable file. Then prove the **same file** cannot be read by the running truly read-restricted child; a private-DACL fixture alone or `WRITE_RESTRICTED` does not qualify.
3. The trusted fixed executable and command data must be bound to a nonce/run identity and immutable authorized manifest; no untrusted executable substitution, operator credential inheritance, arbitrary command authority beyond a test fixture, writable executable locations or unsafe inherited handles.
4. Actual child token identity, restricting SIDs, Job containment before resume, descendants, termination, retries and cleanup are observed separately. For first-stage Job ownership, validate Windows nested/assigned Job behavior; prohibit breakaway and Job-handle inheritance. Never declare access denied because spawn failed.
5. Status remains test-only: production `SandboxRequest::Required` is unavailable/fail-closed, with no `filesystem=Enforced` or `network=Enforced`; no FullHost fallback. No changes to production `src/managed_process.rs`, security backend registration, daemon approvals or installer are authorized by this ADR.
6. Do not conflate **write confinement**, protected-secret read denial and strict read allowlisting. If a later design wishes to offer a weaker write-only mode, it requires a separate explicit policy/approval/API security decision, with no silent weakening of existing `ReadOnly`/`WorkspaceWrite` requests.
7. Network remains unclaimed until a separately qualified kernel-enforced policy (e.g. identity-bound WFP or supported MXC network rule) demonstrably blocks IPv4/IPv6, DNS, direct sockets, loopback escape and descendants; firewall/M005 is out of scope here.
8. Record not-run/unavailable/fail/denied distinctly. Hosted Windows Server evidence cannot establish Windows 11 compatibility; SDK presence does not establish effective MXC backend tier.

## Alternatives and consequences

- **Unrestricted separate user:** useful for process ownership and private-DACL protection, but broad-group files remain readable; insufficient.
- **Host-side restricted-token launch:** already negative/inconclusive under C024–C028; do not repeat an unbounded flag/privilege loop.
- **Original-tree ACL mutation or copied cwd without an enforcing principal:** rejected by C011/C013; not a confidentiality boundary.
- **Privileged service broker or Windows guest:** not selected; ADR-0023's explicit no-admission remains intact. A new decision would be needed to reopen either.
- **Two-stage runner:** avoids the *specific* cross-account privilege transition, but may still fail on quota privilege, interactive logon, session/profile/loader, DLL/runtime access, restricting SID access checks or attack surface.
- **MXC:** consolidates OS primitives but is tier-/build-/schema-dependent and may be incompatible with important Win32 tooling. It must fail closed on unsupported policy operations.

No released binary, user setting, protocol, schema, installer, system service, durable account, host-ACL policy or security guarantee changes in this planning decision.

## Evidence, admission and stop conditions

Accept C032/C033 as implementation-plan handoffs **only for synthetic native feasibility**. C032 must supply real two-process same-file contrast, stage/Git/shell functionality, token/Job attestation, alias and inherited-handle/IPC probes; C033 must supply a selected enforcing tier, actual denial and toolchain/handle matrix with capture mode disclosed. Stop any experiment that requires an unsafe host grant, real user data, undeclared elevation, uncontained child, or silently relaxed policy.

C034 must issue a recorded recommendation of either strict-positive candidate for *future production proposal*, no candidate, or still blocked. Even a strict-positive requires independent review plus new production implementation/lifecycle plans before any M004/C011 re-entry. C015–C018, M004–M006, C030/C031 remain blocked. Historic closure records are immutable.

## Authoritative references

- OpenAI Windows Codex engineering: https://openai.com/index/building-codex-windows-sandbox/
- Anthropic sandbox-runtime Windows security model: https://github.com/anthropics/sandbox-runtime
- Microsoft `CreateProcessAsUserW` token and privilege contract: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw
- Microsoft `CreateRestrictedToken`: https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken
- Microsoft MXC host tier support: https://github.com/microsoft/mxc/blob/main/docs/process-container/os-version-support.md
- Microsoft MXC denial capture: https://github.com/microsoft/mxc/blob/main/docs/logging-access-denied.md
