# Windows ACL lease status

Constrained process execution on Windows is not available as a production
filesystem-enforcement backend. The ACL lease path is compiled only for the
debug/test-only `windows-sandbox-test-support` feature, which is rejected in
release builds. It is test support, not a production sandbox claim.

The tests exercise ACL changes on disposable directories. A journal records
the directory identity and owner SID, along with its original DACL and the
expected leased DACL. Release or recovery restores the original DACL only when
the directory identity and owner are unchanged and the current DACL matches
either the recorded original or lease image. If identity or descriptor state
differs, the operation fails closed and retains the journal instead of
replacing the changed ACL.

Read and write roots can contain fixture data. The normal test path requires a
deny root to be empty because leasing only its root does not establish that
existing descendants have safe ACLs. A separate test-only forensic probe can
populate disposable roots to examine that limitation; it does not qualify
denial of an existing tree.

The test journal is stored under the per-user daemon root at
`sandbox/acl-leases`; `CODEGG_DAEMON_HOME` can override that root. There is no
production caller, operator command, or manual repair procedure. Do not delete
a retained journal or reset a directory DACL by hand.

## Source verification

Previously checked 2026-10-23 against the source tree. Rechecked 2026-11-15
against `Cargo.toml`, `src/lib.rs`, `src/managed_process.rs`,
`src/managed_process/windows_acl_lease.rs`, and
`crates/codegg-client/src/paths.rs`.

- `windows-sandbox-test-support` is restricted to debug builds, and the ACL
  lease module is gated to Windows test support.
- Journal records contain the root owner SID, volume and file identity, and
  original and expected DACLs. Restoration rechecks those values and preserves
  the record on drift.
- The normal test path rejects populated deny roots; a separate test-only
  forensic probe exercises disposable populated roots. The journal root is
  `sandbox/acl-leases` below the daemon root, which honors
  `CODEGG_DAEMON_HOME`.
- This update did not re-run hosted Windows tests or requalify a production
  backend.
