# Build provenance catalog

Implement the workspace as a small production-shaped catalog, not as five
independent examples. `catalog-store` depends on normalized records;
`catalog-index` derives indexes from the durable store; `catalog-query` depends
on the index; and `catalog-cli` composes the query and storage APIs.

## Required behavior

1. Parse v1 and v2 pipe-delimited build records, validate the FNV-1a checksum,
   normalize both schemas into one record, reject unsupported versions and
   malformed fields, and canonicalize package/dependency identities.
2. Import transactionally and idempotently. Re-importing identical records is
   a no-op. Conflicting `(package, build)` identities fail without changing the
   prior catalog. Recovery accepts a truncated final journal line but rejects
   corruption in the interior. Persist through temporary-file plus rename.
3. Build deterministic forward/reverse dependency indexes, report missing
   references and cycles, and support updating one package without changing
   unrelated results.
4. Provide deterministic transitive lineage and stable between-snapshot diffs.
   Incomplete provenance must be explicit.
5. Implement CLI commands `import`, `verify`, `lineage`, `diff`, and `export`.
   Export is byte-stable and diagnostics go to stderr with nonzero exit status.

Record rows have six fields: `schema|package|build|revision|dependencies|checksum`.
The checksum is eight lowercase hexadecimal digits over the first five fields,
joined by `|`, using 32-bit FNV-1a. Dependencies are comma-separated package
identities; `-` means none. The bundled corpus is deliberately small (96 rows)
and exercises shuffled input, repeated imports, one schema upgrade, invalid
checksum, missing references, a cycle and an incremental update. A separate
deterministic test generates 250,000 compact records to exercise dependency
index ordering and memory behavior; it is a bounded integration check, not a
duration target. The fixed corpus size is not intended to create a long-running
test.

Run `cargo test --offline --locked --workspace` and the CLI contract tests. The
provided tests and `contract.json` are frozen inputs. No dependencies or
network access are permitted. Preserve a durable phase note in the workspace
root so a continuation can identify finished phases and the next dependent
phase without replaying writes.
