#!/usr/bin/env bash
set -euo pipefail

for sym in rotate refresh disable enable delete restore; do
    rg -q "pub (async )?fn ${sym}" src/core/provider_connections.rs \
        || { echo "missing ConnectionManager::${sym}"; exit 1; }
done

rg -q 'Tombstoned' crates/codegg-core/src/provider_connections.rs \
    || { echo "missing ProviderConnectionState::Tombstoned"; exit 1; }
rg -q 'Error' crates/codegg-core/src/provider_connections.rs \
    || { echo "missing ProviderConnectionState::Error"; exit 1; }
rg -q 'ConnectionRotateBegin' src/server/ws.rs \
    || { echo "missing server WS guard for ConnectionRotateBegin"; exit 1; }

for variant in ConnectionRotateBegin ConnectionRotateCancel ConnectionRotateStatus \
    ConnectionRefreshBegin ConnectionRefreshCancel ConnectionRefreshStatus \
    ConnectionEnable ConnectionDelete ConnectionRestore ConnectionPurge \
    SessionLifecycleGet; do
    rg -q "${variant}" crates/codegg-protocol/src/core.rs \
        || { echo "missing CoreRequest::${variant}"; exit 1; }
done

echo "provider-connections lifecycle coverage: ok"
