# Windows ACL lease status

Windows constrained process execution remains unavailable in production. The
ACL lease code currently exists only behind the debug/test-only
`windows-sandbox-test-support` feature and is used with newly created disposable
directories on the hosted Windows runner.

The test lease records a directory's volume/file identity and exact original
and expected DACL before applying an AppContainer ACE. Release or startup
recovery restores only when the directory identity is unchanged and the current
DACL exactly matches the recorded lease image. If either check fails, the
journal is retained for inspection; the lease does not overwrite a changed
descriptor. A leftover test journal is under the runner's temporary
`CodeGG-test-acl-leases` directory and must only be reconciled by the test
recovery path against the disposable roots it created.

There is no production journal location, operator command, or manual repair
procedure yet. Do not remove a journal or reset a directory DACL by hand: C010
is still qualifying durable ownership, restart recovery, ACL contention, and
conflict handling. C011 requires a privileged isolated Windows VM for the
remaining host-path, credential, IPC, reparse-point, hardlink, and alternate
data stream tests. Neither milestone enables a production backend until all
required evidence is accepted.
