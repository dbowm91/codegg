# C052 — Two-hop launcher source comparison and bounded differential disposition

Review date: 2026-10-14. This is test-only research; it does not admit a
backend or alter Windows profile guarantees.

## Frozen revisions and license

| Comparator | Exact revision | Scope inspected | License |
|---|---|---|---|
| Anthropic sandbox-runtime | `9a277e5e04af1ab1b04feea4400a6ed8a5b72039` | `vendor/srt-win-src/src/{launch,runner,token,logon,winsta,job,self_protect}.rs`; `vendor/srt-win-src/Cargo.toml` | Repository `Apache-2.0`; no source copied |
| OpenAI Codex | `2b21129c6b8120e63f9ac510fe99e8b02ef43292` | `codex-rs/sandboxing/src/windows.rs`, `codex-rs/windows-sandbox-rs/src/{process,token,desktop}.rs` | Repository `Apache-2.0`; no source copied |
| CodeGG negative control | `76e9e9c9` | C044 `scripts/windows_c044_two_hop_native.ps1`, static `/MT` native probe, and closure | CodeGG repository revision |

The upstream hashes are exact public `main` heads observed on the review date;
they are evidence pins, not release tags. Anthropic's `srt-win` crate declares
edition 2024 and is not a CodeGG dependency. The C044 hosted negative is run
`38468468249`, exact head `76e9e9c9`, Windows Server 2025 Datacenter build
26100, UBR 33438, x64.

## Launch-context comparison

| Dimension | CodeGG C044 | Anthropic srt-win source | Differential status |
|---|---|---|---|
| First hop | `CreateProcessWithLogonW` starts a disposable standard account runner suspended with an explicit narrow environment; C044 assigns it to a kill-on-close Job and verifies membership before resume | `CreateProcessWithLogonW` starts the dedicated `srt-sandbox` runner with `LOGON_WITH_PROFILE`, a broker-created desktop, and piped stdio; the broker assigns its Job before resume | Both use a distinct account and a logon-service first hop, but differ in desktop, profile environment, and stdio setup |
| Window station / desktop | Both C044 `STARTUPINFO.lpDesktop` fields are zero/null. No private desktop is created or granted by this harness, so the runner and child inherit the caller's window-station/desktop context | Broker creates a per-exec desktop on `WinSta0`, passes `WinSta0\srt-sb-*` to the first hop, and grants the sandbox **user SID** station attach rights. Runner fails closed if it lands on `Default`; the child inherits the isolated desktop with `lpDesktop = NULL` | Concrete source-level difference. No C044 paired desktop variant ran, so this is not established as the cause of C044's child-entry failure |
| Logon SIDs | C044 creates a restricted token from the runner token but supplies no disabled-SID list. The source does not enumerate or strip `SE_GROUP_LOGON_ID` groups | srt-win enumerates every `SE_GROUP_LOGON_ID` group and disables them in the lockdown token. Its source documents that seclogon leaves the broker's interactive logon SID in the runner's `TokenGroups`; it grants desktop/BNO access to the sandbox **user SID**, not an assumed new per-logon SID | Explicit and important token-policy difference; no C044 token-group snapshot was obtained |
| Token groups / restricting SIDs | `CreateRestrictedToken` uses flags `0`, no disabled groups, no deleted privileges, and adds one synthetic restricting SID | `CreateRestrictedToken(LUA_TOKEN)` disables Builtin Administrators plus all logon SID groups, deletes privileges except `SeChangeNotifyPrivilege`, and deliberately supplies no restricting SID; it then sets medium integrity and a default DACL | Different restriction mechanisms: CodeGG's restricting-SID check versus srt-win's disabled groups, privilege deletion, integrity and DACL policy |
| Child process creation | `CreateProcessAsUserW` with `CREATE_SUSPENDED`, `bInheritHandles = FALSE`, null environment and null `lpDesktop`; C044 verifies the child is in a Job then resumes and expects suspend count `1` | `CreateProcessAsUserW` with `CREATE_SUSPENDED`, `CREATE_UNICODE_ENVIRONMENT`, `CREATE_NO_WINDOW`, `EXTENDED_STARTUPINFO_PRESENT`, conditional job breakaway, a restricted explicit handle list, and mitigation attributes; `lpDesktop = NULL` inherits the runner's isolated desktop | Both suspend, establish Job ownership and then resume. srt-win has explicit stdio/handle and mitigation setup absent from the C044 harness |
| Privilege and GUI failure handling | C044 records process creation, Job membership, resume count, entry marker and bounded wait; no desktop fail-closed check or token privilege/logon-SID snapshot is recorded | Runner checks desktop isolation before second-hop launch and refuses `Default`; source comments explain that Job UI-handle limits do not block low-level keyboard hooks | Source shows srt-win guards a GUI-object boundary that C044 did not test |
| Environment / cwd | First hop receives a manually constructed environment rooted at the disposable account and stage; second hop inherits runner environment and uses the stage cwd | First hop uses the sandbox profile environment; broker PATH/proxy values are overlaid by the runner for the child, which inherits cwd | Different environment construction; no same-host variable-by-variable result |
| Job / descendants | C044 runner and inner child were each verified in a Job; outer Job is kill-on-close | srt-win uses broker→runner and runner→child Job ownership, verifies the child Job, configures descendant policy and conditionally breaks away from inherited jobs | C044 established membership, not descendant equivalence; C044 child never reached entry |

This is a source comparison, not a runtime side-by-side. The frozen source
reveals several coupled design differences, especially private desktop setup,
logon-SID removal, and privilege deletion. It does not identify which, if any,
of those differences explains C044: C044's recorded child-entry failure was
not reproduced under srt-win, and no controlled C044-versus-srt-win launch
was run.

## Codex cross-check

Codex remains the secondary source comparator required by the C052 plan; it is
not presented as a runtime control for C044. At the frozen revision:

- `windows-sandbox-rs/src/desktop.rs` creates a private
  `Winsta0\CodexSandboxDesktop-*` desktop. Its ACL gives the sandbox SID
  participant access and preserves caller ownership; process setup carries
  that desktop name rather than falling back to `Default`.
- `windows-sandbox-rs/src/token.rs` reads the current token's logon SID and
  includes it with capability SIDs and `Everyone` as restricting SIDs. It
  calls `CreateRestrictedToken` with
  `DISABLE_MAX_PRIVILEGE | LUA_TOKEN | WRITE_RESTRICTED`, sets a logon-scoped
  default DACL, and explicitly enables only `SeChangeNotifyPrivilege`.
- `windows-sandbox-rs/src/process.rs` uses `CreateProcessAsUserW` with an
  extended startup structure, explicit environment and cwd, and an explicit
  inherited-handle list for stdio. This is a separate launcher design, not
  evidence that C044's `CreateProcessAsUserW` failure would be fixed by copying
  one Codex setting.

The source-level token contrast is useful: Codex makes the launch logon SID a
restricting SID, while srt-win disables logon SID groups in its lockdown token.
Both also build a private desktop and narrow process access around the logon
context. C044 does neither in the inspected harness source. No effectiveness
or causality claim follows without a controlled Windows launch and token
snapshot.

## C044 control and disposition

At run `38468468249`, the unrestricted control and first-hop setup succeeded.
The second-hop static native child was created suspended, verified in its Job,
and resumed with previous suspend count `1`. It remained alive with exit code
`259` (`STILL_ACTIVE`) for the full 30-second bound without writing its
first-instruction entry marker. The outer runner, account, profile and fixture
were removed; original fixture bytes and descriptors were unchanged. No
restricted-child token snapshot, filesystem read/write result, or handle/IPC
result was produced. This remains `blocked-before-entry`, not a read denial.

The pinned sources show multiple launch-context differences at once. None of
the captured C044 evidence identifies a single changed variable as the cause,
and the existing C044 run is not a same-host paired control for either pinned
upstream implementation. In particular, srt-win has isolated desktop and
explicit logon-SID/privilege treatment that C044 does not show. C044's
`CreateRestrictedToken` flags (`0`) and the
probe image DACL make one variable falsifiable: change only the flags to
`WRITE_RESTRICTED`, so reads use the ordinary account check while writes also
require the synthetic restricting SID. The C052 test attempted those two
flag variants on one hosted Windows host, with the same source token, account,
test executable, environment, cwd, stage ACL, Job-before-resume path and token
SID verification. It uses `CreateProcessAsUserW` and no inherited handles in
both variants. Hosted run `38615233054`, exact head
`829ef60e36080f78514f83ef77820678811f5711`, emitted
`process_creation=failed` with Win32 error 5 (`Access is denied`) for both
`c044_flags_zero` and `write_restricted`. Neither child ran. The experiment
did not discriminate the flag hypothesis; the detailed disposition is in the
C052 closure.

This is a token-flag differential using the hosted runner account, not a
reproduction under C044's separately provisioned standard account. Its marker
is emitted by the Rust test harness entry function, not by a raw/no-CRT first
instruction. Those limits are retained in the result; no strict-read,
toolchain, network, or production claim follows.

C044's fixture makes the candidate especially concrete: the probe image's
explicit file DACL grants read/execute to the temporary account but not the
restricting SID, while the token is created with flags `0`. A paired
`WRITE_RESTRICTED` run preserves the restricting SID's write check while
allowing normal-token reads needed for image/runtime loading. Whether it
explains C044 remains unknown because the hosted runner denied both paired
process-creation calls before entry. The hosted principal was `runneradmin`
(administrator role and high integrity, UAC enabled); it was not C044's
disposable standard account. No filesystem behavior or strict-read conclusion
is derived from these launch failures.

## Source references

- Anthropic revision: <https://github.com/anthropics/sandbox-runtime/tree/9a277e5e04af1ab1b04feea4400a6ed8a5b72039/vendor/srt-win-src/src>
- Codex revision: <https://github.com/openai/codex/tree/2b21129c6b8120e63f9ac510fe99e8b02ef43292/codex-rs/windows-sandbox-rs/src>
- C044 exact hosted evidence: `plans/closure/windows-native-sandbox/044-status.md`
