# Windows sandbox successor comparison — Codegg handoff (2026-10-13)

Baseline: main a7b116af3243623099b80cc200b843a36b0ce3d2. Source authority: ADR-0004/0016/0027, current Windows roadmap, closure C011/C013/C014/C032–C038. This note distinguishes verified Codegg results from third-party architecture descriptions. Public upstream designs may change; verify pinned versions when implementing.

| System | Described Windows security architecture | Trade-off for Codegg |
| --- | --- | --- |
| Codex CLI | Dedicated sandbox principals, first-hop alternate logon plus own-token restricted second hop, filesystem grants and account-scoped network controls | Practical ordinary tooling; broad reads and original-user ACE mutations do not prove strict Codegg read confinement |
| Anthropic sandbox-runtime / Claude | Windows alpha dedicated user, two-hop restricted launch, Job ownership, identity-scoped WFP/proxy enforcement, reversible ACL management | Strong reference for WFP/lifecycle; additive original-tree ACL mutations require separate explicit risk acceptance |
| Gemini CLI | Native low-integrity / restricted process plus configurable container alternatives | Low-integrity filesystem label changes can persist; avoid altering original user worktrees as default |
| Grok Build | Workspace/read-only/strict profiles; publicly documented kernel sandbox backends primarily on Linux/macOS | Useful policy separation, not an independently qualified native Windows implementation reference |
| GitHub Copilot / Microsoft MXC | MXC ProcessContainer BaseContainer path on eligible Windows builds | MXC is promising for native read/write policy without original DACL grants, but effective tier/fallback and lifecycle must be measured per host |

Useful upstream starting points (do not treat documentation as a security test):
- https://openai.com/index/building-codex-windows-sandbox/
- https://github.com/anthropics/sandbox-runtime
- https://github.com/google-gemini/gemini-cli
- https://github.com/microsoft/mxc
- https://github.com/microsoft/mxc/blob/main/docs/process-container/os-version-support.md
- https://github.com/microsoft/mxc/blob/main/docs/logging-access-denied.md
- https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw
- https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken

Codegg findings that must survive:
- C011: a real child read denied synthetic host bytes by direct/junction/hardlink operations.
- C013: original populated-tree ACL lease caused shared-object and security-descriptor integrity failures.
- C014: projected AppContainer PowerShell worked but Git init failed on a required device path.
- C032: two-stage restricted process resumed but PowerShell did not reach child entry; no read-access proof.
- C033: MXC BaseContainer denied exercised host-read aliases and ran shell/Git/Python/offline Cargo, but helper startup/handle gate was incomplete.
- C035: compiled native helper entered MXC but its numeric inherited-handle test had no actual transferred handle; path read was denied.
- C036: stamped capture output provenance established, but no child-correlated event; timeout-child cleanup unconfirmed.
- C037: ordinary inheritance controls passed; MXC descendant proof/explicit transfer not established.
- C038: no production candidate selected; stop repeated diagnostic-only cycles.

Implementation implications:
1. No false equivalence: command filters, Job Objects, WRITE_RESTRICTED tokens and output telemetry are not a strict read-isolation boundary.
2. A small native first-instruction helper separates loader failures from security results in C044.
3. MXC native BaseContainer can be evaluated only when pinned SDK/host provides reliable deny-path policy without original-host DACL mutation. Investigate any fallback.allowDaclMutation knob in the **actual pinned schema**; if unsupported, block.
4. Treat network and filesystem as separate policy dimensions; WFP egress is needed for offline claims, and proxy env variables alone do not constrain sockets.
5. Support only observed host/tier/toolchain combinations. A green feasibility GitHub Action is never the production security certificate.
6. Use explicit backend registration/admission gates and prevent accidental production builds with test-only Windows launch support.

Success is one independently verified and supportable Windows backend, not reproducing every possible vendor mechanism.
