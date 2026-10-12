# Windows ACL lease status

Windows constrained process execution remains unavailable in production. The
ACL lease code currently exists only behind the debug/test-only
`windows-sandbox-test-support` feature and is used with newly created disposable
directories on the hosted Windows runner.

The Windows lease engine records a directory's owner SID, volume/file identity,
and exact original and expected DACL before applying an AppContainer ACE. A lease
root must be owned by the current token user or its configured default owner SID
(which can be the Administrators group for an elevated GitHub runner). Release or
startup recovery restores only when the directory identity is unchanged and
the current DACL exactly matches the recorded lease image. If either check
fails, the journal is retained for inspection; the lease does not overwrite a
changed descriptor. The current test-only managed-process path stores its
journal under the per-user daemon root at `sandbox/acl-leases` (the root can be
overridden with `CODEGG_DAEMON_HOME`) and applies it only to newly created
disposable roots.

There is no production caller, operator command, or manual repair procedure
yet. Do not remove a journal or reset a directory DACL by hand. C010 qualifies
the lease transaction and recovery behavior on disposable roots; C011 owns
the privileged host-path, credential, IPC, reparse-point, hardlink, and
alternate data stream matrix. Neither milestone enables a production backend
until all required evidence is accepted.
