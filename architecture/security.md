# Security Module

The `security` module provides URL validation, IP checking, filesystem
sandboxing, security scanning, command classification, and an
evidence-based security review workflow.

## Purpose

Prevent SSRF attacks, confine subprocess filesystem access via Landlock,
classify shell commands and tool calls by risk, scan code for secrets
and unsafe patterns, and run a conservative evidence-based security
review that never mutates files.

## Where It Lives

| Artifact | Location |
|----------|----------|
| SSRF protection (`is_internal_ip`, `validate_url_host`, etc.) | `src/security/ssrf.rs` |
| Containment policy + allowance sets | `src/security/sandbox/policy.rs` |
| Backend registry (`BackendId`, `SandboxBackend`, `BACKENDS`) | `src/security/sandbox/backend.rs` |
| Landlock backend (Linux) | `src/security/sandbox/landlock.rs` |
| Opt-in Linux mount-view compiler | `src/security/sandbox/linux_mount_view.rs` |
| Seatbelt backend (macOS) | `src/security/sandbox/seatbelt.rs` |
| `SandboxConfig`, `SandboxLaunchSpec`, capability, status protocol | `src/security/sandbox.rs` |
| Security policy (`action_for_command`, `action_for_findings`) | `src/security/policy.rs` |
| High-level `SecurityService` facade | `src/security/service.rs` |
| Security review workflow (diff parsing, evidence synthesis) | `src/security/workflow/` |
| Security review runtime (`prepare_security_review`, `validate_report`) | `src/security/runtime.rs` |
| LSP security context executor adapter | `src/security/lsp_executor.rs` |
| Sensitive path matching | `src/security/mod.rs` (`matches_sensitive_path`) |
| Re-exports of eggsentry types for backward compat | `src/security/mod.rs` |
| Deterministic security scanning (secrets, commands, deps) | `crates/eggsentry/src/` |
| `codegg-sandbox-helper` binary (backend dispatch + Seatbelt FFI) | `src/bin/codegg-sandbox-helper/` |

## How It Works

### SSRF Protection (`ssrf.rs`)

Prevents requests to internal infrastructure via a multi-stage
validation pipeline:

1. **`validate_url_target(raw_url)`** (`ssrf.rs:126`) — parses URL,
   checks scheme (http/https only), resolves DNS once, validates all
   resolved addresses. Returns `ValidatedUrlTarget` with a pinned
   `SocketAddr` set for Eggfetch `RequestBuilder::resolved_addresses`.
2. **`validate_url_host(url)`** (`ssrf.rs:179`) — convenience wrapper
   returning just the normalized host string.
3. **`validate_host_ip(host, port)`** (`ssrf.rs:85`) — DNS resolution +
   internal IP check on all resolved addresses.
4. **`revalidate_dns(host, port, validated_ips)`** (`ssrf.rs:152`) —
   re-resolves DNS and verifies IPs haven't changed (DNS rebinding
   protection). Handles IPv4-mapped IPv6 equivalence.
5. **`is_internal_ip(ip)`** (`ssrf.rs:22`) — checks against all
   reserved ranges (loopback, private, link-local, multicast, CGNAT,
   benchmark, IPv4-mapped IPv6, IPv6 unique local, etc.).
6. **`ipv6_segments_to_ipv4(ipv6)`** (`ssrf.rs:57`) — converts
   IPv4-mapped and IPv4-compatible IPv6 to IPv4 for range checking.

Used by:
- `src/mcp/remote.rs` — MCP remote URL validation
- External web fetch is owned by the eggsearch subprocess. CodeGG's adapter
  checks that fetch URLs are non-empty, bounded, and HTTP(S) before dispatch;
  destination resolution and SSRF enforcement remain in Eggsearch.

### Filesystem containment (`sandbox/`)

Enforcement is performed **only** by the private one-shot
`codegg-sandbox-helper` process. The daemon never restricts itself.

The module is split into a platform-neutral half and a platform-specific
half:

| Layer | File | Owns |
|---|---|---|
| Policy | `src/security/sandbox/policy.rs` | `BackendPolicy` — readable roots, writable roots, denied roots, plus the curated tool-compatible allowance sets |
| Registry | `src/security/sandbox/backend.rs` | `BackendId`, `SandboxBackend`, the `BACKENDS` table, preference-ordered `select()` |
| Backends | `src/security/sandbox/landlock.rs`, `seatbelt.rs` | Compiling a `BackendPolicy` into an LSM ruleset / SBPL profile |
| Facade | `src/security/sandbox.rs` | Profiles, `SandboxConfig`, capability, enforcement, execution path, helper status protocol |

**Adding a backend** is one `SandboxBackend` entry in `BACKENDS` plus its
`probe`/`apply` functions. Capability probing, enforcement descriptors,
the execution path, the helper, the audit token, and the operator text all
read backend identity out of the registry, so none of them change.

Linux enforcement uses the maintained `landlock` crate.

The trusted CodeGG Git subprocess policy is a separate boundary: it pins
repository-configured hooks/fsmonitor/external diff/textconv/credential helper
and signing behavior, binds Git to the canonical worktree selected by the
daemon, disables system/global config, then returns a typed capability outcome before spawn.
Requests needing repository-controlled filters, merge drivers, executable
aliases, or remote transport require a contained/mediated backend and do not
fall through to a raw Git command. This does not make writable `.git`
metadata immutable and does not establish network isolation. Future nested
repositories are permitted as workspace content; they must not redirect a
trusted operation outside its selected worktree or introduce executable Git
metadata on a trusted route. See
`architecture/git.md` and the Linux execution-isolation corrective closure
records for current evidence and open items.

Trusted `egggit` preflight rejects a repository whose `.git/objects` or
linked-worktree common object store already contains `objects/info/alternates`.
For the exact trusted read allowlist, it also creates a command-scoped private
object directory containing copies of regular loose and packed object files,
then sets `GIT_OBJECT_DIRECTORY` to that directory. The private view omits
`objects/info`; Git therefore cannot follow an alternate created in the
repository after admission. Copying object bytes (rather than hard-linking
them) also prevents later writes to a source object from changing the child's
view. The temporary view is removed when the command exits or is cancelled.
Unsupported object-store entries and setup failures return typed
`BackendUnavailable[RepositoryAlias]` before spawn. This contract applies only
to the explicit trusted read allowlist; object-writing mutations such as
commit/tag creation remain on the separate C002 contained-Git path and are not
qualified by this snapshot. A local measurement on a 105 MiB, 3,945-file
object store on ext4 without reflink support took 2.95 seconds for view setup
plus trusted `git status`; this is a material performance limit for fragmented
repositories, and no production Git routing is enabled. The view does not make
the repository's `.git` metadata immutable or establish network isolation.

**Architecture:**
- Parent resolves helper as canonical regular-file sibling of the
  running binary (inherited env/PATH/cwd cannot select it)
- Parent serializes a bounded `SandboxLaunchSpec` to an owner-only
  ephemeral file in the system temp directory
- Helper adds every Landlock rule, requires ABI 3+ and `no_new_privs`, writes
  a versioned typed status frame to a private
  pipe
- Helper marks the status writer close-on-exec before exec'ing the
  target
- Parent accepts only the expected setup/exec state sequence; fails
  closed on malformed, duplicate, oversized, or contradictory frames

**Key types:**
```rust
pub enum SandboxMode { ReadOnly, WorkspaceWrite, DangerFullAccess } // DangerFullAccess is deprecated compat-only

pub struct SandboxConfig {
    pub enabled: bool,
    pub mode: SandboxMode,
    pub allowed_paths: Vec<String>,
    pub deny_paths: Vec<String>,  // operator-declared; merged with policy::sensitive_deny_paths()
}

pub struct SandboxLaunchSpec {
    pub target: PathBuf,
    pub args: Vec<String>,
    pub read_paths: Vec<PathBuf>,
    pub write_paths: Vec<PathBuf>,
    pub deny_paths: Vec<PathBuf>,  // never readable/writable, regardless of profile
}
```

`SandboxLaunchSpec` names **paths, not a mechanism**, so the same spec
serves every backend.

**Linux launch contract (M002 interface slice):**
`src/security/sandbox/linux_launch.rs` models requested filesystem, network,
process/IPC, syscall, and resource dimensions under an immutable parent
ceiling. `ManagedProcessService` routes constrained Linux launches through
that contract. Existing requests select the private Landlock helper. An
explicit internal `SandboxConfig::with_linux_protected_mount_view(true)`
request selects the Bubblewrap protected-mount path; the option defaults off
and is not a persisted user setting. Its
read-only host probe reports namespace objects/limits, seccomp and proc
kernel inputs, a fixed-location root-owned Bubblewrap candidate, Landlock,
cgroup v2, and AppArmor/SELinux facts. These are host observations only; they
do not prove a child obtained enforcement. The mount path builds a fresh root
from explicit read/write grants, opens bind sources without following links,
checks captured device/inode identities, and overlays Git metadata read-only.
It omits host `/proc`, `/run`, and `/sys` and rejects nested mounts beneath
granted roots. The opt-in Bubblewrap candidate also refuses deny paths beneath
writable grants: a mask on a host-backed writable directory entry can be
detached by renaming that entry after mount setup. Denies outside writable
grants remain absent or masked according to the compiled path scope.
Bubblewrap's private JSON status is
not used as proof that the requested command has not already started. The
trusted in-sandbox gate reports a framed setup receipt over the private status
pipe and waits for an acknowledgment from `ManagedProcessService` before it
execs the requested command. The helper clears close-on-exec on both protocol
descriptors before starting Bubblewrap; the in-sandbox gate consumes the
acknowledgment descriptor and marks the status descriptor close-on-exec before
the requested exec. Missing receipts, malformed status, unsupported options,
or setup failure refuse the launch. The obtained result records the exact path
scope. Network, PID/IPC, seccomp, and resource limits are not inferred from
mount setup.

An additional internal `SandboxConfig::with_linux_process_isolation(true)` opt-in
composes the mount view with Bubblewrap's PID and IPC namespaces and a private
`/proc` mount. It requires Landlock ABI 6 signal and abstract-Unix-socket scopes
before starting Bubblewrap, which keeps the existing outbound-network policy
separate from M005's offline intent. This scoped path is intentionally
fail-closed on hosts without those ABI features. The helper closes inherited
descriptors except stdio and its private status/ack descriptors before exec,
and passes an architecture-checked `seccompiler` denylist to Bubblewrap for
installation before target exec. The filter blocks namespace and legacy/new
mount changes, tracing, selected kernel control interfaces, and namespace flags
passed to `clone`; it does not implement network denial. Exact-head hosted run
[`38760659016`](https://github.com/dbowm91/codegg/actions/runs/38760659016)
passed the M004 host-PID/proc, signal, abstract/pathname socket, descriptor,
seccomp and descendant-cancellation probes, plus protected-mount and trusted-Git
regressions. The runner relaxed its ephemeral unprivileged-userns AppArmor
sysctl. PTY service routing, broader proc/ptrace probes, toolchain compatibility
and other host configurations remain unqualified. The process tier is not
production-admitted while those M004 and M003 gates remain open.

ADR-0031 accepts Bubblewrap-based Linux namespace/mount composition as the
selected architecture. The existing Landlock-only default and unrestricted
network behavior are unchanged. The opt-in mount path is implemented but
remains unqualified until the native protected-mount, descriptor, race,
cancellation, and tool-compatibility matrix passes on supported Linux hosts.
No default backend selection or rollout is authorized by this internal
opt-in.

Ordinary command syntax is not an additional authority boundary: once the
workspace and requested action are authorized, quotes, wrappers, environment
assignments, pipes, and substitutions alone do not trigger review. Explicit
operator policy and concrete risk signals remain on the normal approval path;
neither risk review nor approval mode changes the filesystem/network guarantees
actually obtained from the backend.

Command audit events retain the accepted permission receipt/source and policy
revision, requested sandbox profile, safe execution-route label, advisory risk
level, and the obtained filesystem/network result. For raw shell, the filesystem
value comes from the completed managed-process result (for example,
`enforced:landlock:abi=4`); a timeout is marked as unavailable rather than
reported as an enforcement result. Network remains explicitly `unrestricted`
for Landlock-only launches. The event records the dispatched command digest and
bounded labels, not command text, arguments, output, or generated script
contents. Completed raw-shell events also record exit code, output-truncation
status, cleanup status, and the fixed helper identity when the helper ran.
Route provenance does not claim a complete trace of nested syscalls.
See `architecture/audit.md` and the M003 validation record.

`SandboxConfig::enforce()` refuses to restrict the calling process —
callers must use the child launch path.

**M005 production policy (execution-reliability):** the requested
`SandboxProfile` (`ReadOnly`/`WorkspaceWrite`/`FullHost` in
`codegg-core/src/approval.rs`) is threaded through
`ToolRegistryOptions`/`SessionToolContext`/`TurnRunInput` into
`BashTool` via `sandbox_config_for_profile()` using the authoritative
workspace root. `WorkspaceWrite`/`ReadOnly` build an enabled config;
`FullHost` intentionally builds no config (explicit no CodeGG
filesystem containment, auditable). Obtained enforcement is reported
separately as `SandboxEnforcement` (`FilesystemEnforcement` +
`NetworkEnforcement`): Landlock is filesystem containment only, so
normal shell always reports `network=Unrestricted`. `ApprovalMode`
never mutates `SandboxProfile`. `DangerFullAccess` is retained only as
a compat parser name mapping to `FullHost` vocabulary; it must never be
constructed for new execution. Child sandboxes narrow via
`resolve_child_sandbox()` and a worktree child's writable root is its
leased worktree. Guard: `scripts/check_sandbox_policy_wiring.py`.

**Scheduler-owned Tool Program binding:** `ToolProgramExecutor` is registered
during `CoreDaemon` construction, which routinely happens before any frontend
connects, so no session workspace root exists at that point. It therefore holds
no dispatch registry at construction: `job_tool_context()` builds the registry
and broker once per job from the scheduler-owned lease root on
`JobExecutionContext`, and applies the same conservative `WorkspaceWrite` ceiling
the subagent executor uses — never a frontend-selected profile. The static
contract catalog used by `validate` and frozen contract snapshots is
materialized lazily and only ever answers `lookup_contract`. Because Tool Program
manifests admit only read-only, non-`DirectOnly` tools, `bash` is unreachable
through this registry regardless of profile. Guards: `job_tool_context_confines_reads_to_the_job_workspace`,
`default_construction_defers_registry_building`.

**Platform outcomes:**
- Linux with Landlock ABI >= 3: `Enforced { backend: landlock, abi: Some(v) }` — the
  delegated-scratch grant path is qualified natively; see "Native Linux scratch
  containment" below
- macOS: `Enforced { backend: seatbelt, abi: None, .. }`
- Windows: managed finite processes receive Job Object lifecycle
  supervision, but no filesystem or network enforcement backend is registered;
  constrained requests fail before launch.

Windows policy inputs enumerate system/toolchain reads, user tool caches,
temporary/cache writes, and sensitive user-profile credential roots for future
backend compilation. Raw Windows shell invocations receive a unique per-run
TEMP/TMP directory and remove it after process completion; this is cleanup and
environment isolation, not an OS access-control boundary. Existing policy
paths reject UNC/device/extended-length
namespaces, alternate data stream syntax, parent traversal, and reparse-point
components before canonicalization. These checks are pre-launch diagnostics;
they do not bind a later filesystem operation to the checked identity, prevent
hardlink aliases, or close a replacement race. Windows constrained execution
remains unavailable until M004 supplies and qualifies enforcement.
The debug-only `windows-sandbox-test-support` feature exercises the AppContainer
process lifecycle through `ManagedProcessService` with a restricted three-handle
stdio list, pre-resume token/Job verification, bounded output, cancellation, and
a private status pipe. It returns a distinct `TestOnlyAppContainer` result and
never `Enforced`; release builds fail compilation if this feature is enabled.
Its explicit Unicode environment block retains drive-current-directory entries,
uses the requested cwd for its drive entry, and supplies `LOCALAPPDATA`, `TEMP`,
and `TMP` from the AppContainer package-local profile instead of the host user's
profile paths. Its disposable-root fixture can add an AppContainer ACE after
journaling the exact original and expected descriptors, then restore only when
path identity and the expected descriptor still match. The fixture does not
establish production journal protection, existing-workspace authority,
adversarial path safety, or backend availability. The test-only lease rejects a
deny root that already contains an entry before child creation: root-only DACL
leasing allowed a child to read a synthetic denied file through its direct
path, a junction, and a hardlink in hosted run `38220136385`. The
populated-root preflight and separate empty-root fixtures passed exact-SHA
Windows workflow `38223080719`, but the path-based check is not qualified
against replacement races. This fail-closed restriction is not a production
capability or complete existing-tree policy. See
`tests/windows_restricted_managed_process.rs`, `tests/windows_acl_lease.rs`, and
`docs/windows-acl-leases.md`.
- Any host with no registered backend: constrained requests report
  `FilesystemEnforcement::Unavailable` and are refused before launching
  constrained Bash or Python (never `FullHost`). Portable post-execution
  snapshots are defense in depth, not a substitute for OS containment.

`SandboxLaunchOutcome::Enforced` carries `guarantees` and `limits` alongside
the backend id. Those lists are the honest boundary of each mechanism and
are asserted in `tests/sandbox_containment.rs`, so a backend can never be
read as stronger than it is. Capability-stratified successor negotiation
(C042, ADR-0029 bounded test-only) lives in
`src/security/sandbox/guarantee.rs`: the pure `negotiate()` function takes
an immutable `RequestedGuarantees` plus attested `AttestedCapabilities`
and either returns an exact `SatisfiedContract` or refuses before any
untrusted target starts. Strict `ReadOnly`/`WorkspaceWrite` are never
satisfied by broad host reads, `Unrestricted` network never satisfies
offline, unknown/downgraded tiers never satisfy the no-host-mutation
prohibition, test-only candidates never report production `Enforced`,
and a child broader than its parent ceiling is refused. No public
profile is added; ADR-0030 remains proposed, not authorized:

| Backend | Guarantees | Limits |
|---|---|---|
| landlock | deny-first allowlist, ABI 3+ write and truncate rights, ABI 2+ refer rights, `no_new_privs` privilege drop, inherited across `exec` | no network isolation; pre-open fds remain usable; filesystem permissions still apply |
| seatbelt | writes confined to workspace + scratch, credential paths denied, inherited across `exec` | no network isolation; **no `no_new_privs` equivalent**, so a setuid binary in an allowed read root may regain privilege; pre-open fds remain usable; Apple Events / Launch Services are a cross-process channel; reads are broadly allowed — containment is write-confined, not read-allowlisted |

**Seatbelt (`src/security/sandbox/seatbelt.rs`).** `sandbox_init` accepts an
arbitrary SBPL string **in-process** and is available on macOS 26.6.2
(verified: a `deny file-read*` subpath rule blocks reads, rules are
last-rule-wins, and restrictions survive `exec`). No `sandbox-exec` wrapper
is needed; the helper calls `sandbox_init` and then `exec()`s the target,
exactly as it does for Landlock. The `sandbox.h` header is deprecated in
favour of App Sandbox, which is entitlement-gated and meaningless for a CLI
— deprecated-but-shipped Seatbelt is the only OS-provided, non-entitlement
filesystem confinement a terminal process can get, and it is what Claude Code
and the OpenAI Codex CLI ship.

The `sandbox_init` FFI is `unsafe` and the root library denies
`unsafe_code`, so the helper installs it through
`seatbelt::install_apply_hook` at startup. An uninstalled hook is a hard
error, never a fallback to an uncontained exec.

`policy.rs` grants read access to system runtimes and selected read-only Rust
toolchain inputs. It does not allow broad `~/.config`, cache, Cargo, or
Rustup ancestors. Landlock grants are split around denied descendants, so a
workspace or tool-root allowance cannot re-grant a nested credential. A
missing denied child is preserved relative to its nearest existing parent.
On Seatbelt, credential read/write denials are emitted after workspace write
re-allow rules because SBPL evaluation is order-sensitive.

Persistent writes are restricted to approved workspace roots, authorized
session scratch roots, and stateless device nodes. A model process with a
trusted session identity receives an owner-private directory under the
per-user CodeGG cache root (`dirs::cache_dir()/codegg`) as
`CODEGG_SCRATCH_DIR`, `TMPDIR`, `TMP`, and `TEMP`. This is kept outside the
macOS credential-denied daemon data root; scratch contents are disposable and
may be removed by host cache maintenance. The Required launch adds the exact
path as a read/write root. Durable child runs receive a separate owner-private
root keyed by `AgentRunId`; parent-created shared roots enter that run's scope
only through an immutable direct-child grant validated in `AgentRunStore`.
Shared roots are explicit sandbox/tool roots and never replace the child's
private temp variables. Scheduler attempt leases preserve admitted access
through child completion; revocation blocks later admission. Unix session and
run roots use mode 0700; Windows inherits
the per-user cache-root ACL and has no native isolation claim while Required
launches remain unavailable. Constrained
launches also retain a separate collision-resistant 0700 per-launch staging directory,
which is reaped with the process tree. Python's temporary script is held in
its own private run directory. Shared `/tmp` and global HOME caches are not
writable grants. Revoked shared directories are removed after the last attempt lease releases.
Terminal delegated-run private roots are reclaimed only after the supervised
child returns. A soft-deleted session's scratch tree is reclaimed only after
durable runs and jobs become terminal; active work defers cleanup, and
restoring the session prevents deletion. Cleanup rechecks the deleted-session
row under the SQLite writer lock before claiming the tree. Startup keyset-scans
deleted sessions and re-arms cleanup after daemon restart. Transcripts,
durable run results, and managed Git worktrees are outside the scratch tree.

**Native Linux scratch containment (qualified).** The delegated-grant path is
qualified on a native Linux host with Landlock ABI >= 3 by
`tests/linux_native_scratch_containment.rs`. The fixture admits a delegated
run through the real `TaskTool` → `JobSubmissionService` → `JobScheduler`
path, rebuilds the effective scope from the durable `AgentRunRecord` exactly
as the delegated worker does, and launches it through the real
`ManagedProcessService` and the real `codegg-sandbox-helper`. The launch spec
it hands over names only the workspace: every scratch write root is added by
the production owner from the authorization scope, so the observed result
proves the owner composed the grant rather than the test writing one. The
child may write its run-private root and its explicitly granted shared root,
and the kernel refuses an ungranted sibling, another session's run-private
root, the scratch parent, an unrelated host temporary directory, a
pre-existing file outside the granted roots, a cross-boundary hard link or
rename, and a symlink planted inside the workspace that points out of it. The
obtained status is asserted as Landlock with ABI >= 3 and `no_new_privs`; a
host that cannot obtain Landlock **fails** the fixture rather than skipping
it, so an unavailable qualification can never be counted as a satisfied one.
The fixture also carries a defect-injection control: an artificially broad
grant makes the same ungranted sibling writable, which is what makes the
denials above non-vacuous. This qualifies the **filesystem** dimension only.
It makes no claim about process, IPC, or network isolation, and it says
nothing about a Windows host, where the Required backend is still absent.
Closure: `plans/closure/managed-agent-scratch-native-qualification-corrective/001-status.md`.
On Unix and Windows, shared-resource reclamation first atomically renames the
managed directory to an opaque tombstone, validates its owner marker, and walks
entries without following links. Unix quarantines symlinks, non-regular
entries, and multiply-linked regular files. Windows checks the stable reparse
attribute and quarantines reparse points; hardlinks are file names rather than
recursive edges, so deleting a link under the tombstone only removes that link
and leaves the outside file intact. Windows recursive removal uses the Rust
standard library's handle-based implementation. A later retry recovers an
orphaned tombstone after daemon interruption, and concurrent reclamation is
serialized. Other platforms remain fail-closed. Session and run lifecycle
reclaimers use this same owner-validated tombstone path.
Existing denied regular files with hardlink aliases cause the required launch
to fail because path-based backends cannot distinguish the aliases. As before,
paths are canonicalized so Seatbelt matches resolved paths such as
`/private/tmp` correctly.

**CANONICAL_PATHS_CACHE:** Static cache with 300s TTL and 100-entry
cap (`sandbox.rs:568`-`:573`: `CACHE_TTL = 300s`, `MAX_CACHE_ENTRIES = 100`).
Entries older than 300s are evicted on
access.

### Security Policy (`policy.rs`)

Decides `Observe | Ask | Deny` based on `CommandClassification` and
`SecurityConfig`:

```rust
pub enum SecurityAction { Observe, Ask, Deny }

pub struct SecurityDecisionHint {
    pub action: SecurityAction,
    pub reason: String,
    pub finding: Option<SecurityFinding>,
}
```

**Decision logic** (`action_for_command`):
1. Disabled/off → `Observe`
2. Explicit deny list match → `Deny`
3. Review mode: Critical/High risk → `Ask` (never Deny); else `Observe`
4. Critical + explicitly configured `deny_critical_commands = true` → best-effort `Deny`; omitted defaults to advisory behavior
5. Critical + `ask_on_high_risk_command` → `Ask`
6. Network exfiltration category → `Ask`
7. Secret exposure category → `Ask`
8. High risk + `ask_on_high_risk_command` → `Ask`
9. Strict mode + Medium risk → `Ask`
10. Otherwise → `Observe`

`denied_commands` and an explicitly enabled `deny_critical_commands` are
operator-selected invocation-text filters. They deny matching text but cannot
guarantee that equivalent behavior is absent or provide OS containment.

`action_for_findings` uses the worst-severity finding with similar
logic. Review mode always observes (reports findings but never denies).

### SecurityService (`service.rs`)

High-level facade used by the agent loop:

```rust
pub struct SecurityService { config: SecurityConfig }
```

Methods: `classify_tool_call(name, args)`, `classify_bash(command)`,
`classify_git(subcommand)`, `classify_raw(classification)`,
`format_prompt_hints(findings)`.

### Security Review Workflow (`workflow/`)

A read-only, evidence-based security review pipeline:

1. **Diff parsing** (`diff.rs`) — parses unified diffs into
   `ChangedHunk` structs with file paths and line ranges
2. **Preset selection** (`enrichment.rs`) — selects review presets
   (dependency_review, unsafe_review, web_backend, rust_cli,
   rust_server) based on file path and content
3. **Target building** — deduplicates review targets from hunks,
   excludes vendor/target/node_modules/third_party/dist/build
4. **Preflight checks** (`preflight.rs`) — deterministic checks
   (secret filename hints, content scans)
5. **Evidence synthesis** (`evidence.rs`) — conservative: risk markers
   alone never produce findings; requires 2+ evidence dimensions
   (e.g., RiskMarker + ChangedHunk, or Preflight + ChangedHunk)
6. **Report assembly** (`report.rs`) — produces `SecurityReviewReport`
   with targets, prompts, findings, and notes

Key invariants:
- Findings are defensive review outputs, never proof of exploitability
- Same-file scoping only
- Severity/confidence are deterministic enums
- Malformed or oversized model output is rejected; unsupported
  findings become evidence gaps

### eggsentry Crate (`crates/eggsentry/`)

Deterministic security scanning primitives:

| Module | Purpose |
|--------|---------|
| `command.rs` | `classify_bash_command`, `classify_git_subcommand`, `classify_tool_call` — risk classification with regex patterns |
| `scanner.rs` | `inspect_text`, `inspect_file` — secret/unsafe-code pattern scanning with 15+ regex rules |
| `finding.rs` | `SecurityFinding`, `SecurityReport`, `Severity`, `Confidence`, `SecurityCategory` — 16 categories |
| `profile.rs` | `ProfileRunner` with 4 profiles: Ambient, DependencyDelta, PreCommit, SecurityReview |
| `dependency.rs` | `detect_dependency_file`, `recommended_audit_commands` — 7 ecosystems |

**Secret detection patterns** (scanner.rs): AWS keys, GitHub tokens
(ghp/gho/ghu/ghs/ghr/github_pat), Slack tokens, npm tokens, PyPI
tokens, GCP service accounts, OpenAI keys, private key blocks,
passwords, API keys, secrets. Also scans for unsafe blocks, unsafe
functions, `danger_accept_invalid_certs`, `Command::new("sh"/"bash")`,
CORS wildcards, and bind-all addresses.

**Command risk levels** (command.rs):
- Critical: `rm -rf /`, fork bombs, `mkfs`, `dd of=/dev/`,
  `shutdown`, `chmod 777 /`, curl-pipe-sh, private key piping
- High: `git push --force`, `git reset --hard`, `git clean -fdx`,
  docker privileged/root-mount/socket, kubectl/terraform/ansible,
  scp/rsync/netcat/ftp, ssh remote exec, env exfiltration
- Medium: `rm`, `mv`, `cp`, package manager installs, env dumps,
  chmod, sed -i, perl -pi, git push
- Low: cargo test/check/build/clippy, git read-only, ls, cat, grep

### Sensitive Path Matching (`mod.rs:64`)

```rust
pub fn matches_sensitive_path<'a>(
    file_path: Option<&str>,
    sensitive_paths: &'a [SensitivePathConfig],
) -> Option<&'a SensitivePathConfig>
```

Matches file paths against configured glob patterns with
canonicalization.

### Untrusted HTTP body limits (Eggfetch-owned)

```rust
client
    .get(url)?
    .resolved_addresses(validated_addresses)
    .max_decoded_body_size(limit)
    .send()
    .await?
    .bytes()
    .await
```

Generic response-size enforcement is owned by Eggfetch's request-scoped
decoded-body limit (`Error::DecodedBodyTooLarge`), which applies to
identity bodies with absent, misleading, chunked, or exact-boundary
`Content-Length`. CodeGG owns SSRF validation, the accepted address
snapshot attached via `resolved_addresses()`, no-follow redirect policy
on security-sensitive routes, and secret-safe mapping of
`DecodedBodyTooLarge` into the owner-domain body-limit error. No
CodeGG-owned generic chunk accumulator remains.

## Key Types & APIs

### SSRF (`ssrf.rs`)

- `is_internal_ip(ip: &IpAddr) -> bool` (:25)
- `ipv6_segments_to_ipv4(ipv6: &Ipv6Addr) -> Option<Ipv4Addr>` (:60)
- `validate_host_ip(host, port) -> Result<Vec<IpAddr>, String>` (:88)
- `validate_url_target(raw_url) -> Result<ValidatedUrlTarget, String>` (:129)
- `validate_url_host(url) -> Result<String, String>` (:182)
- `revalidate_dns(host, port, validated_ips) -> Result<(), String>` (:155)

### Sandbox (`sandbox.rs`)

- `SandboxConfig::enforce()` — refuses to restrict parent (:76)
- `SandboxConfig::launch_spec(target, args, cwd)` — builds child
  launch description (:89)
- `sandbox_helper_path()` — resolves trusted helper binary via the shared
  `crate::install` sibling rule (canonical exe dir, no PATH/cwd/env; :290)
- `validate_path_safety(path, allowed_paths)` — symlink check +
  canonicalization (:509)
- `platform_sandbox_capability()` — probes every registered backend
- `apply_backend(spec)` — dispatches to the host's backend (helper only)
- `probe_landlock()` / `apply_landlock(spec)` — Landlock specifically
  (install doctor, mechanism-specific callers)

### Policy (`policy.rs`)

- `action_for_command(classification, config) -> SecurityDecisionHint` (:23)
- `action_for_findings(findings, config) -> SecurityDecisionHint` (:189)

### Runtime (`runtime.rs`)

- `prepare_security_review(input) -> Result<(SecurityEvidenceBundle, SecurityReviewOutput), String>` (:119)
- `validate_report(report, bundle) -> (SecurityReviewReport, Vec<String>)` (:209)

### eggsentry (`crates/eggsentry/src/`)

- `classify_bash_command(cmd) -> CommandClassification` (command.rs:201)
- `classify_git_subcommand(sub) -> CommandClassification` (command.rs:505)
- `classify_tool_call(name, args) -> CommandClassification` (command.rs:509)
- `inspect_text(path, text) -> Vec<SecurityFinding>` (scanner.rs:319)
- `async inspect_file(path, max_bytes) -> Result<Vec<SecurityFinding>>` (scanner.rs:402)
- `ProfileRunner::inspect_paths(profile, paths) -> SecurityReport` (profile.rs:48)

## Configuration Surface

Security configuration lives in `SecurityConfig` (from
`config::schema`):

```toml
[security]
enabled = true
mode = "ambient"           # Off | Ambient | Strict | Review
prompt_hints = true
max_findings_in_prompt = 5
denied_commands = []       # explicit deny list

[security.gates]
deny_critical_commands = true # explicit opt-in best-effort text denial; omitted defaults to advisory
ask_on_high_risk_command = true
ask_on_network_exfiltration = true
ask_on_secret_exposure = true
```

**Security modes:**
- `Off` — all checks return Observe
- `Ambient` — the default. High-risk and Critical commands Ask when the ask
  gate is enabled. Set `gates.deny_critical_commands = true` to explicitly opt
  into best-effort text denial.
- `Strict` — Medium-risk commands also Ask
- `Review` — Critical/High Ask (never Deny); everything else Observe

Tool backend configuration:

```toml
[tool_backends.security]
backend = "native"         # native | mcp | disabled
fallback_to_native = true  # only for mcp backend
```

## Internal IP Ranges Blocked

| Range | Description |
|-------|-------------|
| `127.0.0.0/8` | Loopback |
| `10.0.0.0/8` | Private |
| `172.16.0.0/12` | Private |
| `192.168.0.0/16` | Private |
| `169.254.0.0/16` | Link-local |
| `0.0.0.0/8` | Current network |
| `100.64.0.0/10` | Carrier-grade NAT |
| `198.18.0.0/15` | Benchmark |
| `224.0.0.0/4` | Multicast |
| `::1` | IPv6 loopback |
| `fc00::/7` | IPv6 unique local |
| `fe80::/10` | IPv6 link-local |
| `ff00::/8` | IPv6 multicast |
| `::ffff:x.x.x.x` | IPv4-mapped IPv6 |

## Invariants & Gotchas

1. **Sandbox is child-process-only.** `SandboxConfig::enforce()`
   returns an error if called on an enabled config. The daemon is never
   confined.
2. **Security review never mutates files.** The workflow is read-only;
   findings are defensive outputs, not proof of exploitability.
3. **Finding synthesis requires 2+ evidence dimensions.** Risk markers
   alone produce review prompts, never findings. Same-file scoping only.
4. **DNS rebinding protection.** `revalidate_dns` re-resolves and
   compares; IPv4-mapped IPv6 equivalence is handled.
5. **Bounded HTTP body.** Eggfetch's request-scoped decoded-body limit
   fails closed on declared-length, chunked/unknown-length, and
   exact-boundary bodies; CodeGG maps `DecodedBodyTooLarge` to the
   owner-domain body-limit error.
6. **eggsentry is deterministic.** No network calls, no file mutations.
   Regex-based scanning with `LazyLock` compiled patterns.
7. **Backends report obtained facts, not claims.** `SandboxLaunchOutcome::
   Enforced` carries the backend id plus `guarantees`/`limits`. Seatbelt
   cannot claim Landlock's `no_new_privs` drop, and does not. A backend with
   an empty `limits` list fails `tests/sandbox_containment.rs`.
8. **Landlock ABI V3 minimum.** The helper hard-requires write/truncate
   rights from ABI 3 and refer rights from ABI 2, then requests newer rights
   best-effort. A partial status is accepted only when the effective ABI is
   at least 3 and `no_new_privs` is set; older kernels fail before target
   execution.
9. **Local daemon endpoints are not principals.** Unix socket permissions and
   the Windows pipe DACL restrict transport admission; endpoint names do not
   authorize CoreFrame requests. The Windows pipe uses an explicit protected
   DACL for the current user and LocalSystem, rejects remote clients, and
   fails closed if descriptor construction fails. Live Windows access-control
   and lifecycle qualification is tracked in the desktop foundation plan.

## Testing

```bash
# SSRF tests
cargo test -p codegg --lib security::ssrf

# Sandbox tests (backend registry, policy, profile generation)
cargo test -p codegg --lib security::sandbox

# End-to-end containment: runs the real helper and asserts observed
# enforcement on whichever backend this host provides
cargo test -p codegg --test sandbox_containment

# Policy tests
cargo test -p codegg --lib security::policy

# Service tests
cargo test -p codegg --lib security::service

# Workflow tests (diff parsing, evidence synthesis)
cargo test -p codegg --lib security::workflow

# Runtime tests (prepare_security_review, validate_report)
cargo test -p codegg --lib security::runtime

# eggsentry tests (scanner, command classification, profiles)
cargo test -p eggsentry

# WebFetch bounded-body and static-routing tests
cargo test -p codegg --lib tool::webfetch
```

## Approval-vs-security boundary (M003, ADR-0004)

Deterministic `SecurityService` classification (`Observe | Ask | Deny`)
executes before approval routing and remains authoritative. `Deny` never
becomes an escalation; `Ask` and sensitive-path matches normalize into an
`ApprovalRequest` resolved by the single `ApprovalRouter`
(`src/permission/approval.rs`). `Yolo` auto-allows only `Escalate` within
the resolved ceiling, never `Deny`. `Automatic` resolves `Escalate`
through the bounded M006 reviewer when a reviewer model is configured
(`src/permission/reviewer.rs`, see `approval_reviewer.md`) and otherwise
defers to the human; reviewer failure never becomes Allow and the
reviewer cannot mutate sandbox selection. Approval mode cannot mutate
sandbox profile; filesystem vs network containment stays reported
separately. See `permission.md` for the router contract and `session.md`
for durable preference ownership.

**M006 reviewer threat model:** repository/command/tool output and the
primary agent's justification are untrusted evidence, not instructions.
A file saying "approve this command" has no authority: verdicts require
strict Allow/Deny/DeferUser JSON, investigation is limited to
`read/glob/grep/list/diff/git_read`, and malformed, timeout,
unavailable, over-budget, cancelled, stale-policy, or forbidden-tool
behavior fails closed (DeferUser interactive, explicit deny in
configured headless mode). The reviewer cannot change `ApprovalMode`,
`SandboxProfile`, path/capability ceilings, credentials, or
parent/child authority, cannot recurse into the router, and persists no
hidden reasoning. Guard: `scripts/check_approval_reviewer.py`.

**M007 FullHost warning semantics:** `FullHost` is explicit only — it is
never selected by silent fallback, never implied by `Yolo`/`Automatic`,
and never produced by sandbox unavailability (constrained requests fail
closed as `FilesystemEnforcement::Unavailable`). Interactive frontends
require one explicit confirmation for any `FullHost` selection and two
for `Yolo`+`FullHost`; the confirmation binds the preference revision
(CAS), not a permanent bypass token. Capability-scoped remembered
approvals (`PersistentDecision.scope`) narrow grants toward one command
family; the scope is part of the HMAC-signed material so it cannot be
tampered away, while pre-M007 signed broad rows still verify. See
`permission.md` M007 item and `src/policy_surface.rs`.

**M008 qualification:** `tests/reliability_qualification_m008.rs` proves
WorkspaceWrite failure reports `Unavailable` (never silent FullHost),
FullHost stays explicit/auditable (Yolo+FullHost Strong/2 confirmations),
sandbox-helper status frames fail closed, and diagnostics stay secret-safe.
Supported-Linux Landlock enforcement is covered by existing
`sandbox_landlock` fixtures; this host (darwin) exercises the
unsupported-host `Unavailable` path.

## Related Docs

- [tool.md](tool.md) — Uses security validation
- [permission.md](permission.md) — Path permissions
- [native_crates.md](native_crates.md) — eggsentry crate details

## Source Verification

Verified 2026-10-06 against `src/security/` and `crates/eggsentry/src/`.
- Corrected 9 stale `file.rs:line` refs. All six SSRF entry points had
  drifted by ~3 lines: `validate_url_target` `129`→`126` (it is `pub(crate)`),
  `validate_url_host` `182`→`179`, `validate_host_ip` `88`→`85`,
  `revalidate_dns` `155`→`152`, `is_internal_ip` `25`→`22`,
  `ipv6_segments_to_ipv4` `60`→`57`. Also
  `matches_sensitive_path` `mod.rs:65`→`:64` and the eggsentry list
  `classify_git_subcommand` `503`→`505`, `classify_tool_call` `507`→`509`.
- Corrected the canonical-paths cache ref: `sandbox.rs:453-458` pointed at
  `resolve_executable`. The cache is `CANONICAL_PATHS_CACHE` at
  `sandbox.rs:568` with `CACHE_TTL = 300s` (`:573`) and
  `MAX_CACHE_ENTRIES = 100` (`:572`) — the documented 300s TTL and 100-entry
  cap were correct, only the line range was stale.
- Marked `inspect_file` as `async` (`crates/eggsentry/src/scanner.rs:402`).

Confirmed accurate, no change: `classify_bash_command`
(`command.rs:201`), `inspect_text` (`scanner.rs:319`), and
`ProfileRunner::inspect_paths` (`profile.rs:48`). The eggsentry items live in
`crates/eggsentry/src/`, not `src/security/` (whose `workflow/` submodules are
`context`, `diff`, `enrichment`, `evidence`, `mod`, `preflight`, `receipt`,
`report`, `types`).
