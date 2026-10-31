# Harness Lab Self-Improvement S013 — Current-Defect Preflight

Status: **blocked** (preflight rechecked 2026-10-31). The authorized
live-campaign request reached the trusted preflight, but the current source
reproduced no eligible CodeGG defect in two
independent native lanes. No live campaign or provider request was started.

Source implementation plan:

- `plans/implementation/harness-lab-self-improvement-resumption/013-first-fresh-authorized-live-causal-campaign.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md` §8

Repository baseline reviewed: `f86020a8bfef55e3cfae313f48e98cdd9ad772f7`

## 1. Executive finding

The user authorized an attempt to assess the improvements. S013 still requires
a current independently reproduced CodeGG defect before any provider proposal.
The fresh S011 trusted intake built the pinned ARM64 CodeGG binary and ran the
controller-owned H007 native contract test in two distinct private lanes. Both
completed with matching test and native-observation digests and no native
contract red. The dossier records `defect_eligible: false`,
`eligible_real_defects: 0`, and `live_authorized: false`.

Therefore the live campaign did not run. No model/provider call, candidate,
patch, billed USD cost, or authorization consumption resulted. The user's
high-level authorization does not waive the defect gate or supply S013's
specific proposer/subject routes, endpoints, and pretransport limits. A future
attempt needs a distinct eligible current defect and a fully frozen bounded
campaign authorization.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Current exact source and native binary | `s013-current-source-preflight.json`; source `f86020a8bfef55e3cfae313f48e98cdd9ad772f7`; ARM64 binary SHA-256 `96bbbcac84d2a7aaa60f4f52ab5090defd7e000acea49e7ca664452a58324cbb` | Pass |
| Two independent trusted reproductions of the frozen native contract | `lane-a` and `lane-b`, both `completed`; same test SHA-256 `7e44498e1c642e6ff8d14df0438ccb77c2f2e9b1a2ab86a11084cfde40f89b60`; no failure signature | Pass; no defect found |
| S013 defect eligibility | Dossier `current-codegg-fac83ef5840336fa0966`; `defect_eligible: false`; eligible count `0` | **Blocked** |
| Live provider campaign and request admission | Not started because the defect eligibility precondition failed | Not run |
| Candidate evaluation, held-out verifier and human review packet | No candidate was proposed or built | Not applicable |
| Provider requests and spend | No provider requests; USD cost not applicable | Pass |

## 3. Verification executed

The first build diagnostic selected the x86_64 Homebrew Rust toolchain and
failed its mixed-architecture link; that result was discarded and did not
enter the dossier. The successful run pinned `rustc 1.98.1`, host
`aarch64-apple-darwin`, ARM64 execution, offline Cargo, and MacPorts
`pkg-config` paths. It ran from a clean temporary worktree at the reviewed
baseline, leaving the user's two fixture edits untouched.

Successful command (paths identify the original temporary clean worktree and
private cache used for this run):

```bash
/usr/bin/arch -arm64 /usr/bin/env \
  PATH=/Users/davidbowman/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin:/Users/davidbowman/.cargo/bin:/opt/local/bin:/opt/local/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin \
  RUSTUP_HOME=/Users/davidbowman/.rustup \
  CARGO_HOME=/Users/davidbowman/.cargo \
  CODEGG_HARNESS_INTAKE_TARGET_DIR=/tmp/codegg-verify-clean-20261031/target \
  CARGO_NET_OFFLINE=true CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 \
  PKG_CONFIG=/opt/local/bin/pkg-config \
  PKG_CONFIG_PATH=/opt/local/lib/pkgconfig \
  PKG_CONFIG_LIBDIR=/opt/local/lib/pkgconfig \
  python3 -m dev.harness_lab intake reproduce \
    --repository /tmp/codegg-s013-intake-20261031 \
    --defect-id s013-current-main-recheck-20261031 \
    --output /tmp/codegg-s013-current-defect-20261031.json
```

Result: `reproduction_validated: true`, `disposition: no_reproducible_defect`,
`eligible_real_defects: 0`, `live_authorized: false`. The exact sanitized
receipt is [`s013-current-source-preflight.json`](s013-current-source-preflight.json).

## 4. Invariants and disposition

No synthetic, historical, H007 task-quality, provider, or setup failure was
promoted to a CodeGG defect. The S010/S011/S012 offline results remain valid;
this result does not measure live-model self-improvement. S013 remains blocked
until a distinct trusted current-source reproduction is eligible and a
separately frozen live authorization supplies route, endpoint/account,
proposer/subject request ceilings, time/iteration/retry bounds, disk floor,
cancellation, retention, and cost-unknown treatment. S014 remains blocked on a
positive accepted S013 result, a second distinct eligible defect, and separate
authorization. `stable_live_evidenced` is not established.

## 5. Cleanup

The intake's private lane roots and temporary build output were owned by this
run. They are removed after preserving the sanitized receipt. The temporary
clean worktree and task-owned Cargo target are also removed; the user's two
uncommitted fixture edits are not modified.
