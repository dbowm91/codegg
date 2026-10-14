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

| Dimension | CodeGG C044 | Anthropic srt-win source | Codex source | Differential status |
|---|---|---|---|---|
| First hop | `CreateProcessWithLogonW`, disposable account, suspended, explicit environment; outer Job assignment before resume | Disposable sandbox account and runner; runner establishes the second-hop boundary | Separate command-runner account/launcher topology in documented design | Different topologies; not isolated in C044 run |
| Second-hop token | Runner calls `CreateRestrictedToken` on its own primary token with one restricting SID; flags `0` | Runner token policy includes restricted-token setup, group/privilege handling and self-protection checks | `WRITE_RESTRICTED` policy maps writable roots; read denial is explicitly unsupported for split-read rules | Candidate one-variable hypothesis: change only C044's flags from `0` to `WRITE_RESTRICTED`; not yet run |
| Child creation | `CreateProcessAsUserW`, `CREATE_SUSPENDED`, no inherited handles; verifies Job then resumes with previous suspend count `1` | `CreateProcessAsUserW`, suspended launch, Job assignment, explicit handle-list and mitigation attributes | Windows sandbox launcher has a dedicated native launch layer | Handle/mitigation context differs; no C044 child-entry variant tested |
| Desktop / station | `STARTUPINFO.lpDesktop` unset; inherits runner context | Runner checks non-default desktop and starts under a private non-interactive desktop | Not enough source evidence in the inspected policy module to compare this field | Plausible context difference, but not shown causal |
| Environment / cwd | Narrow environment and staged cwd | Runner-controlled environment and launch directory | Launcher-controlled child configuration | Exact per-variable paired evidence absent |
| Job / descendants | Outer runner and inner child verified in Job; kill-on-close | Locked Job, breakaway/descendant policy | Dedicated process ownership implementation | C044 Job membership passed; entry still absent |

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
upstream implementation. The bounded differential therefore did not select or
run a corrective variant. No broad-read, strict-read, tool compatibility,
network, or production claim follows. A future experiment needs one
consented disposable Windows job that runs both variants under the same
account, host image and marker binary, after selecting a falsifiable single
variable and instrumenting token groups, desktop, handles, mitigation policy
and process start state.

C044's fixture makes one candidate especially concrete: the probe image's
explicit file DACL grants read/execute to the temporary account but not the
restricting SID, while the token is created with flags `0`. A paired
`WRITE_RESTRICTED` run would preserve the restricting SID's write check while
allowing normal-token reads needed for image/runtime loading. This is a
falsifiable single-variable hypothesis; the paired launch has not been run in
this closure, so it remains unverified and does not explain the C044 result.

## Source references

- Anthropic revision: <https://github.com/anthropics/sandbox-runtime/tree/9a277e5e04af1ab1b04feea4400a6ed8a5b72039/vendor/srt-win-src/src>
- Codex revision: <https://github.com/openai/codex/tree/2b21129c6b8120e63f9ac510fe99e8b02ef43292/codex-rs/windows-sandbox-rs/src>
- C044 exact hosted evidence: `plans/closure/windows-native-sandbox/044-status.md`
