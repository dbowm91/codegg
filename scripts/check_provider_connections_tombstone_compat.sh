#!/usr/bin/env bash
set -euo pipefail

rg -q 'migrate_v31' crates/codegg-core/src/session/schema.rs \
    || { echo "missing additive provider lifecycle migration"; exit 1; }
for table in provider_connection_lifecycle provider_connection_references \
    provider_connection_tombstones provider_connection_audit_events; do
    rg -q "${table}" crates/codegg-core/src/session/schema.rs \
        || { echo "missing ${table}"; exit 1; }
done

rg -q 'pub async fn purge_eligibility' crates/codegg-core/src/provider_connections.rs \
    || { echo "missing purge eligibility API"; exit 1; }
rg -q 'pub async fn restore' crates/codegg-core/src/provider_connections.rs \
    || { echo "missing restore API"; exit 1; }
rg -q 'ProviderConnectionState::Tombstoned' crates/codegg-core/src/provider_connections.rs \
    || { echo "missing tombstone transition"; exit 1; }

echo "provider-connections tombstone compatibility: ok"
