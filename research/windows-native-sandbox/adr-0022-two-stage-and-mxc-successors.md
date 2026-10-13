# Windows native sandbox successors — two-stage caller-token and MXC diagnostics (ADR-0024)

Status: new bounded test-only research justification; **no new native qualification** as of 2026-10-13.
Baseline branch: `work/windows-native-sandbox-foundation` at `c7e2dae404d5b58f6bd927aa8d99b4d518c4f23d`.

## Reconciled failure history

C021 proves `CreateProcessWithLogonW` can launch a dedicated standard-user child, use Git/Python/Cargo and deny access to private-DACL fixtures (Windows Server 2025 only); it cannot prove strict read restrictions on files granted to broad groups. C024 confirmed the *normal* account can read such broadly readable file, but host-side `CreateProcessAsUserW`/`CreateProcessWithTokenW` restricted launch failed. C027 corrected UTF-16 ABI and observed Win32 1314 for `CreateProcessAsUserW` and timeout for `CreateProcessWithTokenW`; C028 traced four resumed children to pre-user-entry timeouts. Those results don't prove what happens when the caller is *already* the sandbox principal.

Microsoft `CreateProcessAsUserW`: when hToken is a restricted form of the **caller's primary token**, `SE_ASSIGNPRIMARYTOKEN_NAME` is not required (`SE_INCREASE_QUOTA_NAME` may still be required). OpenAI's Windows Codex report describes a first hop `CreateProcessWithLogonW` into `codex-command-runner.exe` then `OpenProcessToken`/`CreateRestrictedToken`/`CreateProcessAsUserW` inside the new user. Anthropic's Windows sandbox-runtime uses an analogous two-hop runner. These are *architectural precedents*, not positive Codegg evidence: different token flags, DACL assumptions, process environments, helpers and actual host read semantics.

Source: https://openai.com/index/building-codex-windows-sandbox/
Source: https://github.com/anthropics/sandbox-runtime
Source: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw
Source: https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken

## Why C032 is not C030

C030's rejected candidate is an *elevated SCM service* privileged to launch/authorize arbitrary subordinate process lifecycles, with a sensitive named-pipe confused-deputy boundary. C032's first-hop principal can be created only by an explicitly consented disposable test fixture. Its command runner has no elevated SCM installation, special host privilege, arbitrary command service endpoint, or production access; it works on **its own lower-privilege token**. A positive result would still require future provisioning and lifecycle authorization, separate from the privilege question.

## Strictness of the host-read requirement

Normal account denied by a private DACL is insufficient: user/group-readable host files remain accessible with normal access checks. `WRITE_RESTRICTED` only applies restricting-SID check to writes. A true read-restricted token must satisfy both normal and restricting SID access checks for reads. C032 therefore needs same-file positive ordinary-account read and negative restricted read, while simultaneously allowing stage/Git/native shell. A shared hardlink cannot be "fixed" by modifying source ACLs. Built-in groups, runtime dependencies and device/registry objects can cause either unexpected allowed reads or toolchain incompatibility. Record the actual token and per-object access checks, never assume.

## Why revisit MXC separately

C020's native ProcessContainer run found direct/alias denials and descendant cleanup but failed native helper/Python initialization and could not test handle exposure. Current MXC documentation distinguishes supported host tiers and exposes denial capture/learning semantics. C033 must pin the *actual revision and binary* with requested facilities and must prove the selected tier honors them. A later upstream documentation page is not evidence that C020's pinned v1.0.0 build implements a feature. Deny-and-record is enforcement; permissive audit is only diagnostic on trusted synthetic workloads.

Source: https://github.com/microsoft/mxc/blob/main/docs/process-container/os-version-support.md
Source: https://github.com/microsoft/mxc/blob/main/docs/logging-access-denied.md
Source: https://github.com/microsoft/mxc/blob/main/src/core/mxc-sdk/README.md

## Side-by-side qualification and decision

Both candidates consume C019's actual-child oracle and share identical read/write/alias/handle/IPC/Job tests; C032 tests normal -> same-user restricted token, C033 tests selected MXC tier and actual deny policy. Common failure classes are: **launch_failed**, **initialized_no_tool**, **read_exposed**, **filesystem_enforced**, **network_unqualified**, **not_run**, **host_blocked**. No mixed "CI passed" classification. C034 independently decides if a positive candidate deserves a *new* production ADR; no existing closure is reclassified. ADR-0023's broker/guest rejection remains effective.
