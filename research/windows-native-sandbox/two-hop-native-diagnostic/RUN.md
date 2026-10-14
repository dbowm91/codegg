# C044 disposable-host runbook (consented VM only)

> Do not run on a developer workstation, a hosted CI runner without
> administrator consent, or any machine holding real credentials. All
> fixtures are synthetic and disposable.

## 0. Hosted CI path (primary)

Workflow `.github/workflows/windows-c044-two-hop-native.yml`
(`windows-latest`, disposable + admin-gated) orchestrates
`scripts/windows_c044_two_hop_native.ps1`:

1. Records runner metadata + admin/high-integrity consent evidence.
2. Builds `native_probe.c` with `cl.exe /MT /O1 /GS /sdl` (`/MT`
   static CRT: the restricted child loads only the probe file plus
   kernel32/advapi32 KnownDLLs) and pins the
   SHA-256 + compiler version (`C044_NATIVE_PROBE`,
   `C044_PROBE_SHA256`, `C044_PROBE_COMPILER`).
3. Runs the portable harness unit tests (`cargo test --locked`).
4. Runs the bounded ps1 fixture: disposable local user, explicit ACL
   topology (broad-readable host fixture, writable stage, read-only
   immutable runner + probe), LogonW first hop, restricted
   own-primary-token inner child (`native_probe.exe` suspended +
   Job-verified before resume), 30 s entry poll, exit wait, token/read/
   write/alias proof, restricted `cmd.exe` + `git` compatibility,
   DACL/hash stability, full teardown (`C044_CLEANUP`).
5. Uploads `codegg-c044-report.json` + console (`windows-c044-
   two-hop-native-<run_id>`, 30-day retention) and fails the job only
   on control failure or missing report — a `negative` or
   `blocked-before-entry` outcome is recorded evidence, not a job
   failure.

Trigger: push touching the workflow/script/harness/C044 plan, or
`gh workflow run windows-c044-two-hop-native.yml --ref <branch>`.
Evidence contract: `codegg.two_hop_c044.v1` (`C044_REPORT=` + `outcome`
of `strict-positive-candidate` | `partial-positive` | `negative` |
`blocked-before-entry` | `control-failed`).

## 1. Manual provision (administrator, consented VM)

1. Record OS edition/version/build/UBR/arch, toolchain (`cl.exe`,
   Rust 1.89 harness build), and MXC non-involvement.
2. Create one temporary standard local user with a random password
   (no admin group, no remote desktop). Record its SID.
3. Create one runner-owned stage directory owned by that user
   (e.g. `C:\c044-<nonce>\`), plus `broad\` (first-hop readable),
   `host\` (restricted-denied), and `stage\` (restricted-writable)
   fixtures. Hash every fixture file.

## 2. Build

1. `cl.exe /O1 /GS /sdl native_probe.c /link /out:stage\native_probe.exe`
   inside the stage. Record `cl.exe` version + output SHA-256.
2. `cargo build --locked` in this harness directory with Rust 1.89.
   No product installer, no PATH mutation, no service install.

## 3. Run

1. First hop: `CreateProcessWithLogonW` as the temporary user, cwd =
   stage, suspended, assigned to a fresh Job before resume.
2. Verify Job membership + suspend count, then resume.
3. Second hop (from inside the first hop): `CreateRestrictedToken`
   on the first hop's own primary token (disable admin/power-user SIDs,
   add restricting SIDs), then `CreateProcessAsUserW` for
   `native_probe.exe` suspended + Job-assigned before resume.
4. Wait for the entry marker (30 s bound). **No entry marker →
   stop, record loader/privilege/environment diagnostics, close
   negative/blocked. Never count as read-denied.**
5. On entry, collect: token SID/restricting groups, same-file
   broad-read allowed (first hop) vs denied (restricted child),
   direct/alias/hardlink/ADS/reparse routes, stage-write allowed,
   host-write denied, inherited stdio/handle behavior, descendant
   cleanup via OS process query after timeout/cancel.

## 4. Tear down

1. Terminate the Job tree, verify descendants gone via OS query.
2. Re-hash fixtures; any drift fails the run.
3. Delete the temporary user + profile + stage. Record negative
   teardown evidence (user gone, profile gone, fixtures gone).
4. Publish the JSON evidence with exact SHA, host metadata, probe
   hash, per-operation allowed/denied/error classification, and
   unsupported/not-run dimensions marked honestly.
