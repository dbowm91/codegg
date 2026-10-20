//! A synthetic Gregg host-telemetry provider for the fixture node.
//!
//! The node's telemetry owner polls a **loopback** provider and normalizes
//! the answer into `eggwork_core::HostTelemetry`. This module stands in for a
//! real Gregg daemon so the live qualification path can exercise the whole
//! chain — provider, node, client — without installing Gregg.
//!
//! The payload is built from the real `gregg_protocol` types, so the fixture
//! serves the same wire contract a production provider serves. Nothing here
//! is a CodeGG-facing shape: the provider's vocabulary stops at the node's
//! boundary, exactly as upstream requires.
//!
//! The provider is deliberately *not* a fake measurement of the fixture
//! host: it reports fixed synthetic numbers, so a test can assert on exact
//! values. The one field that must be live is the observation timestamp — the
//! node refuses a sample whose clock is more than 30 s away from its own.

use std::time::{SystemTime, UNIX_EPOCH};

use gregg_protocol::v2::{
    CpuMetricsV2, DiskIoMetrics, DiskIoPayload, DriveMetrics, MetricCapabilitiesV2,
    NetworkInterfaceMetrics, NetworkPayload, StatusPayloadV2, StatusSnapshotV2, SwapMetrics,
    SCHEMA_VERSION_V2,
};
use gregg_protocol::{LoadAverage, MemoryMetrics, SystemIdentity};

/// The only path the provider protocol serves a status payload on.
const PROVIDER_STATUS_PATH: &str = "/v2/status";

/// Largest request head the provider reads before answering.
const MAX_REQUEST_HEAD_BYTES: usize = 8 * 1024;

/// A running synthetic provider.
pub struct SyntheticProvider {
    /// The loopback port the provider listens on.
    pub port: u16,
}

/// Build the synthetic provider payload for the current instant.
///
/// The provider serves a `StatusPayloadV2` — the status document, not the
/// health envelope. The health envelope is what a *warming* provider answers
/// on the same path with a `503`, and the node reads it as a distinct state.
fn payload() -> StatusPayloadV2 {
    let observed_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default();
    let capabilities = MetricCapabilitiesV2::new(true, true, true, false);
    StatusPayloadV2 {
        snapshot: StatusSnapshotV2 {
            schema_version: SCHEMA_VERSION_V2,
            observed_at_unix_ms,
            sample_interval_ms: 1_000,
            capabilities,
            system: SystemIdentity {
                name: "fixture".into(),
                hostname: "fixture.invalid".into(),
                os_name: "linux".into(),
                os_version: "0".into(),
                kernel_name: "Linux".into(),
                kernel_release: "0".into(),
                architecture: "x86_64".into(),
            },
            cpu: CpuMetricsV2 {
                logical_cores: 8,
                usage_pct: 12.5,
                iowait_pct: capabilities.cpu_iowait.then_some(1.5),
            },
            load: capabilities.load_average.then_some(LoadAverage {
                one: 0.5,
                five: 0.25,
                fifteen: 0.125,
            }),
            memory: MemoryMetrics {
                used_bytes: 4_000,
                total_bytes: 8_000,
                usage_pct: 50.0,
            },
            swap: capabilities.swap.then_some(SwapMetrics {
                used_bytes: 0,
                total_bytes: 2_000,
                usage_pct: 0.0,
            }),
            commit: None,
        },
        drives: Some(vec![DriveMetrics {
            name: "fixture-volume".to_string(),
            used_bytes: 1_000,
            total_bytes: 4_000,
            available_bytes: Some(3_000),
        }]),
        cpu_frequency_hz: Some(2_400_000_000),
        disk_io: Some(DiskIoPayload {
            aggregate_read_bytes_per_sec: 3,
            aggregate_write_bytes_per_sec: 4,
            devices: vec![DiskIoMetrics {
                id: "fixture-disk".to_string(),
                name: "fixture-disk".to_string(),
                read_bytes_per_sec: 3,
                write_bytes_per_sec: 4,
                drive_name: Some("fixture-volume".to_string()),
            }],
        }),
        network: Some(NetworkPayload {
            aggregate_rx_bytes_per_sec: 5,
            aggregate_tx_bytes_per_sec: 6,
            aggregate_rx_capacity_bps: Some(1_000_000_000),
            aggregate_tx_capacity_bps: Some(1_000_000_000),
            interfaces: vec![NetworkInterfaceMetrics {
                id: "fixture-nic".to_string(),
                name: "fixture-nic".to_string(),
                rx_bytes_per_sec: 5,
                tx_bytes_per_sec: 6,
                rx_capacity_bps: Some(1_000_000_000),
                tx_capacity_bps: Some(1_000_000_000),
                is_loopback: false,
                aggregate_member: true,
            }],
        }),
    }
}

/// Start the synthetic provider on an ephemeral loopback port.
///
/// The provider answers `GET /v2/status` with the synthetic payload and
/// refuses every other path. It binds `127.0.0.1` only: the node's telemetry
/// owner validates the provider endpoint as a loopback literal, and a
/// provider reachable from anywhere else would be a different (and wrong)
/// fixture.
pub async fn start() -> Result<SyntheticProvider, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("bind synthetic telemetry provider: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("read synthetic provider address: {e}"))?
        .port();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let _ = answer(&mut stream).await;
            });
        }
    });
    Ok(SyntheticProvider { port })
}

/// Answer one provider request.
async fn answer(stream: &mut tokio::net::TcpStream) -> std::io::Result<()> {
    use tokio::io::AsyncReadExt;

    // Read the request head only; the provider never consumes a body.
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while head.len() < MAX_REQUEST_HEAD_BYTES {
        let read = stream.read(&mut byte).await?;
        if read == 0 {
            break;
        }
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let request = String::from_utf8_lossy(&head);
    let path = request
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    if path != PROVIDER_STATUS_PATH {
        write_response(stream, 404, b"{\"error\":\"not found\"}").await?;
        return Ok(());
    }
    let body = serde_json::to_vec(&payload())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_response(stream, 200, &body).await
}

/// Write one bounded HTTP/1.1 response.
async fn write_response(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    body: &[u8],
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let reason = if status == 200 { "OK" } else { "Not Found" };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The node validates the provider's payload before it normalizes it,
    /// so a payload the provider's own protocol would reject never becomes
    /// a ready observation. This test names the offending field instead of
    /// leaving it to be discovered by a live fixture run.
    #[test]
    fn the_synthetic_payload_satisfies_the_provider_protocol() {
        if let Err(violations) = payload().validate() {
            let described: Vec<String> = violations
                .iter()
                .map(|v| format!("{} at {}", v.kind, v.field))
                .collect();
            panic!("the synthetic provider payload violates the protocol: {described:?}");
        }
    }

    /// The whole provider contract, end to end, without the node: the
    /// upstream client fetches from the synthetic provider and must come
    /// back with a validated observation.
    ///
    /// This is the seam the node's polling owner uses. A defect here makes
    /// the node report no observation at all, which the live qualification
    /// can only show as "the node must report a ready observation" — it
    /// cannot say which side of the contract broke.
    #[tokio::test]
    async fn the_upstream_client_reads_an_observation_from_the_synthetic_provider() {
        use eggwork_server::telemetry::{
            GreggTelemetryProvider, HostTelemetryConfig, HostTelemetryProvider, ProviderOutcome,
        };

        let provider = start().await.expect("start the synthetic provider");
        let endpoint = format!("http://127.0.0.1:{}/v2/status", provider.port);
        let config = HostTelemetryConfig::new(&endpoint, 1, 5).expect("a valid endpoint");
        let client = GreggTelemetryProvider::new(&config);

        match client.observe().await {
            Ok(ProviderOutcome::Observed(observation)) => {
                // The observation carries the provider's own timestamp, so a
                // non-zero value means the payload survived the round trip
                // through the client's decode and the node's translation.
                assert!(
                    observation.observed_at_unix_ms > 0,
                    "the observation lost its timestamp"
                );
            }
            Ok(ProviderOutcome::Warming) => panic!("the provider answered with a health envelope"),
            Err(error) => panic!("the provider was not read: {error:?}"),
        }
    }
}
