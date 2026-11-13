# Sandbox Containment Audit Corrective Milestone C001 — macOS Credential Deny-Set Completion

Status: closed

Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/sandbox-containment-audit-corrective-roadmap.md#milestone-c001--macos-credential-deny-set-completion`

Long-term requirements:

- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs:

- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`
- `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`

Primary class: invariant

## 1. Objective

Make the macOS credential exclusion boundary cover the material an agentic coding
tool can actually reach, by completing `policy::sensitive_deny_paths()` with a
per-path classification and a recorded breakage assessment, and by proving each
addition is enforced by the kernel rather than by the generated profile text.

## 2. Why this milestone is ready

No hard dependency. The denial mechanism already exists and is already exercised
end to end on macOS: `sandbox_containment.inc` runs a real contained child on both
platforms, pins `BackendId::SEATBELT` on the macOS arm, and asserts observed
denials for injected deny roots. This milestone adds entries to an existing table
and one fixture in the same family.

The one interface it needs — "a denied root is denied for read and for write, and
a deny inside a writable root wins" — is delivered and tested at
`sandbox_containment.inc:286` and `:825`.

## 3. Current implementation evidence

**The deny list is the only read protection on macOS.**
`seatbelt::sbpl_profile` emits `(version 1)`, `(allow default)`,
`(deny file-write*)`, a `(allow file-write* (subpath …))` per write root, and then
`(deny file-read* …)` plus `(deny file-write* …)` per denied root. It never emits
`BackendPolicy::read_roots`; those are computed and validated in
`SandboxConfig::launch_spec` and then discarded on this backend. Read
confidentiality therefore reduces entirely to the contents of the deny list.

**The read-broad posture is deliberate and pinned, not a defect.**
`sandbox_containment.inc` asserts `READ-OUTSIDE-LEAK` for `BackendId::SEATBELT` and
`READ-OUTSIDE-DENIED` for `BackendId::LANDLOCK` on the same probe, with the
comment that reads are deliberately broad for tool compatibility. This milestone
does not change that; it completes the deny list that the posture depends on.

**The guarantee the list is supposed to deliver.**
`SEATBELT_GUARANTEES` claims "credential material denied by path". That claim is
currently true only for the enumerated entries.

**The current unix list** (`policy.rs`, `sensitive_deny_paths`) contains
`/etc/sudoers`, `/etc/shadow`, `/etc/gshadow`, `/root/.ssh`, `/var/db/dslocal`,
`/var/db/dhcpd_leases`, and under `$HOME`: `.ssh`, `.aws`, `.gnupg`, `.gpg`,
`.kube`, `.docker`, `.netrc`, `.npmrc`, `.pypirc`, `.gem/credentials`,
`.cargo/credentials`, `.cargo/credentials.toml`, `.cargo/config`,
`.cargo/config.toml`, `.config/gh/hosts.yml`, `.config/gcloud`, `.azure`,
`.terraform.d`, `.config/codegg`, `.codegg`, `Library/Keychains`,
`Library/Application Support/codegg`, plus the Rustup credential files.

**What it omits.** Verified absent from the list at the baseline:
`~/.git-credentials`, `~/.config/git/credentials`, `~/.m2/settings.xml`,
`~/.gradle/gradle.properties`, `~/.yarnrc.yml`, `~/.password-store`,
`~/.local/share/keyrings`, `~/.config/op`, `~/.vault-token`, `~/.pgpass`,
`~/.my.cnf`, `~/.bash_history`, `~/.zsh_history`, `~/Library/Messages`,
`~/Library/Mail`, `~/Library/Safari`, `~/Library/Cookies`, and browser profiles
under `~/Library/Application Support/`.

`~/.git-credentials` is the plaintext store written by
`git config --global credential.helper store`. Git is the tool the agent invokes
constantly, and macOS has no network isolation, so the read and the exfiltration
are one command each.

**Existing deny additions are not free.** `policy::validate_denied_paths` refuses
a launch when an existing denied **regular file** has `nlink > 1`, because a
path-based backend cannot distinguish two names for one inode. It is called from
`managed_process` before every `SandboxRequest::Required` launch. Adding paths
therefore widens the set of hosts on which a constrained launch refuses, and that
is fail-closed and stays.

**Existing coverage is membership-only for this list.**
`policy.rs` asserts the vector contains `~/.ssh` and the Codegg store, and
`policy.rs` denies-normalisation tests cover carving. No test asserts that a
curated entry is unreadable by a real kernel; every observed-denial test injects
its own `deny_paths`.

## 4. Invariants that must not regress

- A constrained request never degrades to host authority; an unavailable backend
  refuses.
- A deny applies to read **and** write, and a deny that sits inside a writable
  root wins because it is emitted last.
- `ReadOnly` still denies every persistent write while keeping the device nodes
  writable, and still permits reading the workspace.
- The toolchain still runs: `/etc` stays readable and the shell still resolves a
  command.
- A denied regular file with hardlink aliases still refuses the launch.
- No path is added that makes an ordinary supported command fail without a
  recorded justification.

## 5. Scope

### In scope

- `policy::sensitive_deny_paths()` (unix arm) — new entries, with classification.
- `policy.rs` test module — membership assertions for the new entries.
- `tests/security_permission/sandbox_containment.inc` — one kernel-asserted
  fixture over the curated list, with a positive control.
- `architecture/security.md` — the accepted breakage and the list's rule.

### Explicitly out of scope

- Converting macOS to a deny-first read allowlist. That is a tool-compatibility
  redesign and needs its own ADR.
- Any change to `sbpl_profile`'s rule order or to `BackendPolicy::read_roots`.
- Any change to the Linux read allowlist (that is C002).
- Any change to the Windows deny list.
- Any change to `validate_denied_paths`' hardlink refusal.
- Any change to `SEATBELT_GUARANTEES` wording unless the list's meaning changes.

## 6. Required production changes

### Security and authorization

Extend `sensitive_deny_paths()` with the entries that pass the classification
rule in work package A. The list is shared by both backends; on Landlock the
entries are outside the read allowlist and cost nothing, and on Seatbelt they
become the read protection.

### Core/domain

None.

### Storage and migrations

None. `SandboxConfig.deny_paths` is an in-memory `Vec<String>`; no persisted key
changes and no `STORAGE_LAYOUT_VERSION` bump.

### Protocol and DTOs

None. `SandboxLaunchSpec.deny_paths` keeps its shape.

### Runtime and concurrency

None. The list is built per launch and normalised in `BackendPolicy`.

### Frontend or operator surface

None directly. The operator-visible effect is that the descriptor's guarantee
list becomes true for the paths it names.

### Documentation and static guards

`architecture/security.md` records the classification rule and the accepted
breakage. No new guard: the fixture is the guard, and the existing membership
assertions keep the list from silently shrinking.

## 7. Ordered work packages

### Work package A — Classify every candidate path

Intent: decide what may be denied before denying it, because a deny is total
(read and write) and a wrong one breaks a supported toolchain.

Required changes: for each candidate path, record three facts in the plan's
completion evidence — (1) **class**: `credential-only` (the file exists only to
hold secrets), `config-with-embedded-secret` (legitimate build configuration that
contains credentials), or `private-user-store` (personal data with no toolchain
role); (2) **what breaks** if it is denied, named as a concrete command; (3)
**decision**, with the reason. The plan's proposed starting classification:

| Path | Class | Expected breakage if denied |
|---|---|---|
| `~/.git-credentials` | credential-only | `git push/pull` over HTTPS with `credential.helper=store` cannot read the token; the user re-authenticates or uses a helper with a socket |
| `~/.config/git/credentials` | credential-only | same as above for XDG users |
| `~/.pgpass`, `~/.my.cnf` | credential-only | `psql`/`mysql` prompt for a password |
| `~/.vault-token` | credential-only | `vault` CLI needs a re-login |
| `~/.password-store` | credential-only | `pass` cannot read the store |
| `~/.local/share/keyrings` | credential-only | desktop keyring unavailable to CLI tools |
| `~/.config/op` | credential-only | 1Password CLI cannot read its session config |
| `~/.yarnrc.yml` | config-with-embedded-secret | Yarn Berry reads `npmAuthToken` from it; denial breaks authenticated installs |
| `~/.m2/settings.xml` | config-with-embedded-secret | Maven loses server credentials **and** mirrors/proxies; denial can break the build, not just the auth |
| `~/.gradle/gradle.properties` | config-with-embedded-secret | Gradle loses signing and repository credentials **and** ordinary build properties |
| `~/.bash_history`, `~/.zsh_history` | private-user-store | none for toolchains; history may contain pasted secrets |
| `~/Library/Messages`, `~/Library/Mail`, `~/Library/Safari`, `~/Library/Cookies` | private-user-store | none for toolchains; these are personal data with no development role |

The classification is a proposal, not a verdict. `.m2/settings.xml` and
`.gradle/gradle.properties` are the two entries where the deny is likely to break
a build rather than only an authenticated fetch, and `.npmrc` is the existing
precedent that the repository has already accepted that trade. The agent must
justify each entry against the breakage column, and **must not add an entry whose
breakage has no recorded justification**.

Acceptance evidence: a completed table in the closure record's evidence, with
every row either `add` or `skip` plus a reason.

### Work package B — Add the accepted entries

Intent: implement exactly what work package A approved, nothing more.

Required changes: extend the `$HOME`-relative list in `sensitive_deny_paths()`.
Keep the existing style: `$HOME`-relative entries only, because the list is
already resolved against `home_dir()` and the host-wide entries are named
individually to avoid covering a read allowance root. Do **not** deny a directory
that contains legitimate configuration — deny `~/.config/git/credentials`, never
`~/.config/git`.

Acceptance evidence: `cargo test -p codegg --lib security::sandbox::policy`
passes, and the membership assertions added in work package C cover every new
entry.

### Work package C — Prove each entry with a real kernel denial

Intent: turn the list from a claim into observed behaviour, which is what the
suite currently lacks for curated paths.

Required changes: add one fixture to `sandbox_containment.inc`, platform-neutral
in shape like its neighbours, that for each **credential-only** and
**private-user-store** entry which exists on the host asserts a real read denial
with a positive control. The fixture must not read or write the user's real
credential files: create a fixture root, and drive the curated path through
`SandboxConfig::deny_paths` exactly as the production assembly does. Where a
curated path cannot be exercised without touching real user data, assert the
rendered profile names it and record that as a structural check, not as kernel
evidence — and say so in the test name.

Add the membership assertion alongside the existing ones in `policy.rs` for every
new entry.

Acceptance evidence: the fixture fails if an entry is removed from the list
(prove it by removing one locally and observing the failure), and passes on
macOS.

### Work package D — Document the rule and the accepted breakage

Intent: the next reader must not have to re-derive why a path is denied.

Required changes: in `architecture/security.md`, record the three-class rule, the
list's purpose ("on Seatbelt the deny list *is* the read protection"), and the
accepted breakage for each `config-with-embedded-secret` entry. Correct any prose
that implies `BackendPolicy::read_roots` participates in the Seatbelt profile.

Acceptance evidence: the document states the rule and names the breakage; no
claim in it contradicts `sbpl_profile`.

## 8. Failure, cancellation, restart, and contention semantics

The change is a table plus a fixture; there is no new runtime state. A denied path
that is absent is retained by `BackendPolicy::normalized_denies`, which resolves
the nearest existing canonical parent, so a credential file created after policy
construction stays denied. A denied regular file with hardlink aliases makes the
launch refuse before process creation — unchanged, and the reason is recorded in
the roadmap §10. No cancellation or restart path changes.

## 9. Compatibility and migration

No persisted configuration, durable payload or protocol change. A host that has
one of the newly denied files with `nlink > 1` starts refusing constrained
launches; that is the existing fail-closed rule applied to a larger set and is
recorded in the roadmap §10 risk 4. No migration. No removal of a legacy path.

## 10. Required tests

### Focused unit tests

- Membership: every new entry is present in `sensitive_deny_paths()`.
- Normalisation: a new entry that does not exist on the host still appears in
  `BackendPolicy::denied_roots` under its nearest existing canonical parent.
- Profile structure: the generated SBPL contains a `(deny file-read* (subpath …))`
  and a `(deny file-write* (subpath …))` for each new entry, emitted after every
  re-allow rule.

### Integration tests

- The new kernel-asserted fixture in `sandbox_containment.inc`, on macOS, with a
  positive control.

### Restart and recovery tests

Not applicable; no durable state.

### Contention and cancellation tests

Not applicable.

### Security and negative tests

- The fixture is non-vacuous: removing an entry makes it fail.
- A deny nested inside a writable root still wins (already covered at
  `sandbox_containment.inc:286`; the new entries are outside write roots, so this
  is a regression guard, not new coverage).

### Migration and compatibility tests

- The toolchain probes still pass on macOS:
  `a_sandboxed_shell_can_still_read_the_system_and_resolve_a_command` and the
  Cargo metadata probe.

## 11. Required verification commands

```bash
# focused
cargo test -p codegg --lib security::sandbox::policy
cargo test -p codegg --lib security::sandbox::seatbelt

# real contained child (macOS host)
cargo test -p codegg --test security_permission -- sandbox_containment

# static guards that cover this surface
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_comment_budget.py

# formatting and lint
cargo fmt
cargo clippy -p codegg --all-targets --locked -- -D warnings
```

Do not claim a command that was not run. A Linux host cannot produce the macOS
kernel evidence; record that as platform-scoped rather than as passing.

## 12. Documentation updates

- `architecture/security.md` — the three-class deny rule, the accepted breakage,
  and the correction that Seatbelt does not consume `read_roots`.
- The roadmap §12 status table and `plans/registry.md` on closure.

## 13. Acceptance criteria

1. Every candidate path in work package A has a recorded class, breakage
   assessment and decision.
2. Every `add` decision is present in `sensitive_deny_paths()` and in a membership
   assertion.
3. A real contained child on macOS cannot read each newly denied
   credential-only path, proved with a positive control, and the fixture fails
   when an entry is removed.
4. The generated profile emits each new deny **after** every re-allow rule.
5. The macOS toolchain probes still pass.
6. `architecture/security.md` states the rule and the accepted breakage and no
   longer implies `read_roots` reaches Seatbelt.

## 14. Stop conditions

Stop and report rather than improvise when:

- a candidate path's denial breaks a supported toolchain **and** no narrower entry
  (a specific file rather than a directory) restores it;
- a path would have to be denied as a directory because the credential file's name
  is not stable, which would break ordinary configuration reads;
- an addition would make a majority of hosts refuse constrained launches through
  the hardlink rule;
- the only way to prove a path is to read the user's real credential material;
- the work would require converting macOS to a deny-first allowlist.

## 15. Closure evidence required

- The completed classification table with every row decided.
- The exact commands run and their results, labelled by host platform.
- The non-vacuity demonstration: the entry removed, the failure observed, the
  entry restored.
- A statement of which entries were structurally asserted rather than
  kernel-verified, and why.
- The toolchain-probe results on macOS.
- The roadmap §12 and registry updates, including the unblock audit.

## 16. Handoff notes

- The list is shared by both backends. An addition is free on Landlock (the path is
  outside the read allowlist) and load-bearing on Seatbelt. Do not add a
  Linux-only or macOS-only arm to `sensitive_deny_paths()` without a reason
  recorded in the plan's evidence.
- `policy.rs` already has two `sensitive_deny_paths` definitions, one `#[cfg(unix)]`
  and one `#[cfg(windows)]`. Edit the unix arm only.
- Do not touch `sbpl_profile`'s rule order. The "deny after every re-allow" property
  is load-bearing and is asserted by `credential_denies_follow_writable_reallow_rules`.
- This plan and **C002** both edit `policy.rs` allowance/deny tables. Do not run
  them concurrently.
- This plan and **C004** both extend `sandbox_containment.inc`. C004 owns the
  strict-mode marker and the non-vacuity control; this plan owns the curated-list
  fixture. Coordinate the shared helper rather than duplicating it.
- `~/.npmrc` is already denied wholesale, which is the precedent for denying
  `config-with-embedded-secret` entries. If that precedent is wrong, it is a
  separate finding, not a reason to skip an entry here.
