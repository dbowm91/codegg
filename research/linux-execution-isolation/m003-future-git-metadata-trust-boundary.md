# M003 future Git metadata trust boundary

Date: 2026-10-14

## Question

Does strict M003 require denying creation of every future nested `.git`, or
must it ensure newly created repositories cannot cause trusted daemon Git
operations to execute repository-controlled code or escape their selected
worktree?

## Finding

The second contract is the relevant trust boundary. A static Bubblewrap mount
graph cannot deny every future nested `.git` while preserving an ordinary
writable source tree. That limitation does not, by itself, require staging or
copy-on-write for the whole workspace. A newly created nested repository is
workspace data. It must not be treated as trusted executable metadata merely
because Git can discover it.

Trusted non-contained Git operations must therefore be bound to the daemon's
selected worktree and must neutralize repository configuration that can
redirect reads or execute helpers. The canonical `egggit::process::GitEnvPolicy`
now canonicalizes the selected worktree, supplies it as Git's explicit
`--work-tree`, caps repository discovery at its parent, disables system/global
configuration, and retains the existing command-bearing local-config
overrides. Native regression tests cover a hostile local `core.worktree` and
the existing fsmonitor/hook execution probes.

Git operations that can invoke clean/smudge filters, merge drivers, hooks,
external diff/textconv, remote helpers, credentials, or network remain
classified as requiring contained execution or separate authorization. They
remain unavailable until their own contained route is qualified. This keeps
ordinary status/diff/log and safe local metadata inspection usable without
claiming arbitrary Git mutation parity.

## Why blanket nested-repository denial is not the selected policy

- Nested repositories and submodules are ordinary developer workspace
  structures; hiding all future `.git` paths would make their creation and
  inspection fail unpredictably.
- Creation alone does not grant a repository authority over the daemon. The
  authority comes from which typed Git operation the daemon admits and which
  worktree it selects.
- The security goal is to prevent untrusted metadata from executing code,
  redirecting trusted reads outside the selected worktree, or broadening
  filesystem/network/credential authority. Those properties can be enforced
  at the trusted Git process boundary and independently tested.

## Remaining evidence

This change does not qualify the opt-in Bubblewrap mount view for production.
Linked worktree/common-dir race coverage, the full protected-path matrix,
toolchain/LSP/PTY compatibility, and contained local mutations remain open.
The native C006 mount candidate remains opt-in; no default or production
backend promotion follows from this finding.

## Verification

- `cargo test -p egggit --locked process::tests::trusted_git_status_cannot_be_redirected_by_local_core_worktree`
- `cargo test -p egggit --locked process::tests::trusted_git_status_does_not_run_repository_fsmonitor`
- `python3 scripts/check_git_forbidden_patterns.py`
- Final merge branch native and cross-platform CI qualification is recorded in
  `plans/closure/linux-execution-isolation/014-status.md` and the CI run link
  added after the reconciled head is pushed.
