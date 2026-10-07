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

# The Connection lifecycle is a CoreDaemon concern, not a WS-inline one:
# src/server/ws.rs only maps a handful of JSON-RPC methods onto CoreRequest
# and forwards them. Assert the request is actually *implemented* where it is
# dispatched (src/core/daemon_providers.rs) and routed to the Providers
# family (src/core/daemon_family.rs), rather than in the WS layer.
for variant in ConnectionRotateBegin ConnectionRotateCancel ConnectionRotateStatus \
    ConnectionRefreshBegin ConnectionRefreshCancel ConnectionRefreshStatus \
    ConnectionEnable ConnectionDisable ConnectionDelete ConnectionRestore ConnectionPurge; do
    rg -q "CoreRequest::${variant} \{" src/core/daemon_providers.rs \
        || { echo "missing daemon handler for CoreRequest::${variant}"; exit 1; }
    rg -q "CoreRequest::${variant} \{ \.\. \} => Self::Providers" src/core/daemon_family.rs \
        || { echo "missing Providers routing for CoreRequest::${variant}"; exit 1; }
done

for variant in ConnectionRotateBegin ConnectionRotateCancel ConnectionRotateStatus \
    ConnectionRefreshBegin ConnectionRefreshCancel ConnectionRefreshStatus \
    ConnectionEnable ConnectionDelete ConnectionRestore ConnectionPurge \
    SessionLifecycleGet; do
    rg -q "${variant}" crates/codegg-protocol/src/core.rs \
        || { echo "missing CoreRequest::${variant}"; exit 1; }
done

echo "provider-connections lifecycle coverage: ok"
