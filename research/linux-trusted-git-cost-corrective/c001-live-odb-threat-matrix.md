# C001 Live ODB Threat and Consistency Matrix

Status: native hosted qualification pending for the current test revision.

Reviewed source baseline: rebased branch commit `0b36c71f` (2026-10-18); test implementation commits `db6033f9` and `5c43a7fd`.
Authority: accepted ADR-0035 permits test-only Linux mechanism probes; proposed ADR-0036 has not been accepted and grants no production selector.

## Security property versus consistency property

| Property | Live read-only object view can establish | Live read-only object view cannot establish |
|---|---|---|
| Host authority | A contained worker can be denied host home/credential files, host processes, host sockets, and host network if the actual M004/M005 dimensions are obtained. | A read-only mount alone does not prevent an external host process from replacing/removing loose objects or pack/index files during the operation. |
| Data consistency | A quiescent repository may produce the same result as Git over a stable object generation. | A multi-object read is not an atomic generation. A clean exit is not proof that every object came from the same generation. |
| Failure behavior | A failed native child may be returned as a typed refusal or Git error without widening authority. | CodeGG must not retry the same operation on the host or silently relabel the result as immutable-snapshot output. |

## Threat surface and current evidence

| Surface | Authority/correctness risk | Current probe | Status |
|---|---|---|---|
| Repository-local Git alias | Runs arbitrary configured shell command in Git's authority context. | C001 test invokes `git authority-probe` through required `ManagedProcessService`; command writes a workspace marker, attempts to read a host-only credential sentinel and host `/proc/<pid>`, attempts host loopback, and records its actual network namespace inode. | Implemented; hosted result pending. |
| `diff.external` | Executes a repository-selected external diff command. | C001 dirties a committed tracked file, runs contained `git diff`, and requires the configured diff executable to execute. | Implemented; hosted result pending. |
| Host user/home files | Credential/config leakage could convert data access into host authority. | Sentinel is created below runner HOME outside the workspace; the configured Git command copies it into the workspace only if readable. | Implemented; hosted result pending. |
| Host process namespace | A Git-configured command might inspect a host PID. | Command probes `/proc/<host runner pid>/cmdline`; host and child network namespace inodes are compared as a positive containment control. | Implemented; hosted result pending. |
| Host TCP/network | A Git-configured command might connect to a host or external endpoint. | Host loopback listener is first positively verified from the parent; the contained config command attempts to connect and writes a marker only on success. Existing M005 test separately covers TCP/UDP, inherited sockets and DNS paths through the same managed-process boundary. | Implemented; hosted result pending. |
| System/Global Git configuration and inherited command variables | May inject helper paths or credentials from the daemon/user environment. | Fixture sets `GIT_CONFIG_NOSYSTEM`, `GIT_CONFIG_GLOBAL`, sanitized environment policy and an explicit inert HOME. | Implemented for this test-only route; production `GitEnvPolicy` matrix remains separate. |
| Hooks and `core.fsmonitor` | May execute repository-configured programs. | Not yet directly probed by C001; alias and external diff prove that arbitrary configured executables remain inside the child boundary, but do not substitute for feature-specific invocation coverage. | Open. |
| Clean/smudge/process filters, textconv, merge drivers, pager, credential helpers, SSH command | May invoke external programs, read secrets, or request network access. | Not yet invoked through the C001 Git route. | Open. |
| External gitdir/common-dir, linked worktree, nested repository, alternates, replace refs, grafts, shallow/promisor objects | May expand the trusted filesystem/object reach or trigger hidden network fetches. | Not yet probed through this route; launch currently derives protected paths from the selected workspace and rejects changed captured identities. | Open. |
| Concurrent `gc`/`repack`, pack/index replacement, loose-object writes | Can produce mixed-generation reads, missing objects, or corrupt temporary views without granting host authority. | C001 implementation does not yet contain a deterministic race fixture. C012 remains authoritative: a CodeGG workspace mutex does not exclude host writers. | Open; no immutable-generation claim is made. |
| Child failure, cancellation, or unsupported namespaces | Must not release target before containment or fall back to host execution. | Required `SandboxRequest` and managed-process barrier are used; existing restricted-userns native test asserts the target marker stays absent when setup fails. | Existing hosted suite; current-source result pending. |

## Decision recommendation before native evidence

Keep live ODB access test-only. A contained worker is a plausible way to prevent host-authority escalation, but it changes the correctness contract from immutable snapshot to potentially inconsistent live reads. Before any production route is considered, close the feature-specific executable matrix, prove external-writer outcomes are detected or explicitly surfaced as possibly inconsistent, obtain M005 offline networking and M004 process/IPC through every Git owner, and obtain independent acceptance of ADR-0036. No clean native read result changes C010's no-admission or C012's closed-negative history.

## Host qualification record

Local host: Linux 6.8.0-147-generic, Bubblewrap 0.9.0 at `/usr/bin/bwrap`, root-owned mode `0755`; `unshare -Ur true` fails with `Operation not permitted`, so this host is not eligible for positive user-namespace proof. Required native evidence is delegated to hosted run `39100634870` (head `5c43a7fd`) and its successor if superseded.
