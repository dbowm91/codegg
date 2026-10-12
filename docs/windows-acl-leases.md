# Windows ACL lease status

Windows constrained process execution remains unavailable in production. The
ACL lease code currently exists only behind the debug/test-only
`windows-sandbox-test-support` feature and is used with disposable directories on
the hosted Windows runner. It is not a production filesystem backend.

The Windows lease engine records a directory's owner SID, volume/file identity,
and exact original and expected DACL before applying an AppContainer ACE. A lease
root must be owned by the current token user or its configured default owner SID
(which can be the Administrators group for an elevated GitHub runner). Release or
startup recovery restores only when the directory identity is unchanged and
the current DACL exactly matches the recorded lease image. If either check
fails, the journal is retained for inspection; the lease does not overwrite a
changed descriptor. The current test-only managed-process path stores its
journal under the per-user daemon root at `sandbox/acl-leases` (the root can be
overridden with `CODEGG_DAEMON_HOME`). Read and write roots may contain fixture
data. A deny root must be empty: the test-only path rejects a deny root with any
entry before creating the child because a root-only inheritable ACE allowed an
AppContainer child to read a synthetic existing credential directly and through
a junction and hardlink (hosted run `38220136385`). This is a fail-closed test
limitation, not a qualified policy for existing denied trees.

There is no production caller, operator command, or manual repair procedure
yet. Do not remove a journal or reset a directory DACL by hand. C010 qualifies
the lease transaction and recovery behavior on disposable roots. C012's
fail-closed preflight passed exact-SHA hosted Windows run `38223080719`, but the
path-based check is not qualified against replacement races. C011 remains
blocked on the hosted existing-file read finding and its broader security
matrix. Neither milestone enables a production backend until all required
evidence is accepted.
