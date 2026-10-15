# Windows native compatibility successor evidence — 2026-10-14

Status: research / implementation input; no new backend or policy authorization.
Mainline baseline: aa6cbcb1e154a2676325fb654c5b68dc068edfc3. Read alongside ADR-0029 (accepted bounded test-only) and ADR-0030 (proposed/unapproved).

## Source-grounded comparison

| System | Observed source or documentation | Actionable lesson / limit |
| --- | --- | --- |
| OpenAI Codex | codex-rs/sandboxing/src/windows.rs and codex-rs/windows-sandbox-rs/src/{token,launch/process,acl,wfp}.rs; https://openai.com/index/building-codex-windows-sandbox/ | WRITE_RESTRICTED can bound writes via restricting-SID access checks, but does not guarantee read denial; supported elevated principal model uses separate accounts and ACL grants, incompatible with Codegg's current original-tree no-mutation rule without new consent/ADR. |
| Anthropic standalone sandbox-runtime | https://github.com/anthropics/sandbox-runtime/tree/main/vendor/srt-win-src/src (launch.rs, runner.rs, token.rs, logon.rs, winsta.rs, job.rs, self_protect.rs, wfp.rs, acl.rs); README Windows alpha | Two-hop account→own-restricted-token, desktop/window-station, logon SID stripping, explicit handles, Job, SID-scoped WFP are credible comparators for C044. Original-path additive ACL grants, toolchain per-user profile limitations and warning-only runner self-protection cannot be copied unreviewed. Standalone alpha is NOT proof that the Claude Code built-in native Windows sandbox is enabled. |
| MiniMax MCode published TUI | https://github.com/MiniMax-AI/minimax-code/blob/main/packages/local-runtime-v2/src/service/sandbox/initialize.ts and backend/types.ts; config/sandbox-settings.ts | A typed multi-profile sandbox *model* does not imply a native Windows backend. Published implementation registers srt-macos for darwin; full-access/disabled defaults, network enforce=false. Cannot cite it as verified Windows containment. |
| Gemini CLI | https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/sandbox.md | Low-integrity token/relabel technique is write-oriented and risks durable MIC mutation when used on original host directories; synthetic stage-only research is separately possible, not selected. |
| Microsoft MXC | https://github.com/microsoft/mxc and documented fallback.allowDaclMutation | A schema-level fallback switch is not proof that pinned mxc-sdk 1.0.0 ProcessContainerConfig exposes it. Strict possible only on independently verified BaseContainer tier with no DACL fallback and complete handle, loader and cleanup evidence. |
| OpenCode | https://github.com/anomalyco/opencode/security | User tool permissions are not an OS security boundary; do not model Codegg containment as a command/regex authorization list. |

## Codegg negative control evidence

- C011: direct/junction/hardlink synthetic credential read succeeded under prior AppContainer/ACL path. plans/closure/windows-native-sandbox/011-status.md.
- C013: same-file hardlink alias mutation and descendant security descriptor control-bit drift, original populated-tree ACL approach failed. plans/closure/windows-native-sandbox/013-status.md.
- C014: AppContainer stage worked for PowerShell but Git init failed on /dev/null; later hostile probes not run. plans/closure/windows-native-sandbox/014-status.md.
- C038/C043: MXC denies some tested aliases and handles Git/Python/offline Cargo in bounded cases, but SDK fallback prohibition not exposed in pinned Rust API and helper/handle/cleanup remained unqualified. plans/closure/windows-native-sandbox/{038,043}-status.md.
- C044: second-hop restricted static native child resumed in Job but remained STILL_ACTIVE without first entry for 30 s; no filesystem guarantee. plans/closure/windows-native-sandbox/044-status.md.
- C042: typed negotiation is internal only and has no Windows production caller. src/security/sandbox/guarantee.rs.
- C047: test runner, local MCP server and LSP subprocesses bypass ManagedProcessService today. research/windows-native-sandbox/c047-process-tool-coverage.md.

## Candidate separation and decision gates

1. C049: threat model and ADR-0030 operator decision pack; public broad-read mode blocked pending ratification. No child launch.
2. C050: same-user WRITE_RESTRICTED + restricting SID + **new disposable roots**, no original ACL/MIC edits; test-only write semantics and real child/toolchain. Network unrestricted; reads broad; no assertion that an isolated working directory alone restricts writes.
3. C051: if C050 is write-positive, synthetic staged-workspace performance/compatibility and promotion-design feasibility only. C046 needs a new accepted staging ADR and cannot be inferred from C051.
4. C052: same-host differential against Anthropic srt-win and Codex topology to isolate C044 child-entry failure. Account/privileged setup only as disposable explicitly consented native fixtures, no persistent helper installation.
5. C053: desk-verify genuinely new MXC SDK/runtime API and stop unless **both** no-DACL-fallback and helper/handle/lifecycle changes justify a new native experiment.
6. C054: select one candidate or no-admission, reconcile C045/C046/C047/C048 and request separate production ADR/review only after positive evidence.

## Hard boundaries

Existing ReadOnly/WorkspaceWrite stay strict; parent/subagent authority and immutable launch policy unchanged; explicit FullHost is not fallback. Windows SandboxRequest::Required remains unavailable before spawn. C050 broad-read evidence cannot satisfy C042 strict read flag or existing M004/C011 gates. WFP actual socket tests required for offline; an HTTP_PROXY env variable is not egress control. No source/worktree security descriptor, owner or MIC mutation, no agent-controllable elevated helper, no new release/CI lanes or perpetual negative-probe loop. Historical closure statuses remain immutable.

## Operational measurement rule

Record exact SHA, host edition/build/UBR/architecture, toolchain/compiler, actual token and Job, first-instruction proof, allowed and denied operation controls, handle/IPC/alias coverage, elapsed cost, original-object diff and independent process-exit proof. Every gap is explicitly unknown/not-run or unsupported; never infer read secrecy from a denied write or absence of output.
