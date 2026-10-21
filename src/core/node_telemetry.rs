//! Daemon handling for the read-only node telemetry request (Eggwork M002).
//!
//! The handler is deliberately thin: it resolves the request against the
//! daemon-owned [`NodeTelemetryService`], converts the snapshots onto the
//! wire shape, and returns. It never touches the queue, never creates or
//! mutates a job, and never probes a node directly — the service owns the
//! cache and the probe lock, so two readers of the same node cannot
//! amplify the node's poll of its provider.

use codegg_protocol::core::CoreResponse;
use codegg_protocol::dto::{
    NodeCpuDto, NodeDiskDto, NodeDiskIoDto, NodeLoadDto, NodeMemoryDto, NodeMetricStateDto,
    NodeNetworkDto, NodeSwapDto, NodeTelemetryDto, NodeTelemetrySourceDto, NodeTelemetryStateDto,
};

use crate::scheduler::eggwork_telemetry::{
    NodeMetricState, NodeTelemetryError, NodeTelemetryService, NodeTelemetrySnapshot,
    NodeTelemetrySource, NodeTelemetryState,
};

/// Handle one operator telemetry read.
pub(crate) async fn handle_node_telemetry(
    service: Option<&NodeTelemetryService>,
    node_id: Option<String>,
) -> CoreResponse {
    let Some(service) = service else {
        return CoreResponse::Error {
            code: "node_telemetry_unavailable".to_string(),
            message: "no Eggwork nodes are configured".to_string(),
        };
    };
    let snapshots = match service.snapshots(node_id.as_deref()).await {
        Ok(snapshots) => snapshots,
        Err(NodeTelemetryError::NoNodesConfigured) => {
            return CoreResponse::Error {
                code: "node_telemetry_unavailable".to_string(),
                message: "no Eggwork nodes are configured".to_string(),
            }
        }
        Err(NodeTelemetryError::UnknownNode { node_id }) => {
            return CoreResponse::Error {
                code: "unknown_eggwork_node".to_string(),
                message: format!("no Eggwork node named '{node_id}' is configured"),
            }
        }
    };
    CoreResponse::NodeTelemetry {
        nodes: snapshots.iter().map(snapshot_to_dto).collect(),
    }
}

/// Project one snapshot onto the wire shape.
fn snapshot_to_dto(snapshot: &NodeTelemetrySnapshot) -> NodeTelemetryDto {
    NodeTelemetryDto {
        node_id: snapshot.node_id.clone(),
        source: match snapshot.source {
            NodeTelemetrySource::Gregg => NodeTelemetrySourceDto::Gregg,
        },
        state: match snapshot.state {
            NodeTelemetryState::Disabled => NodeTelemetryStateDto::Disabled,
            NodeTelemetryState::Warming => NodeTelemetryStateDto::Warming,
            NodeTelemetryState::Ready => NodeTelemetryStateDto::Ready,
            NodeTelemetryState::Stale => NodeTelemetryStateDto::Stale,
            NodeTelemetryState::Unavailable => NodeTelemetryStateDto::Unavailable,
            NodeTelemetryState::Invalid => NodeTelemetryStateDto::Invalid,
        },
        age_ms: snapshot.age_ms,
        last_error: snapshot.last_error.clone(),
        observed_at_unix_ms: snapshot.observed_at_unix_ms,
        sample_interval_ms: snapshot.sample_interval_ms,
        cpu: snapshot.cpu.map(|cpu| NodeCpuDto {
            state: metric_state(cpu.state),
            logical_cores: cpu.logical_cores,
            usage_pct: cpu.usage_pct,
            iowait_pct: cpu.iowait_pct,
        }),
        memory: snapshot.memory.map(|memory| NodeMemoryDto {
            state: metric_state(memory.state),
            available_bytes: memory.available_bytes,
            total_bytes: memory.total_bytes,
            used_bytes: memory.used_bytes,
        }),
        load: snapshot.load.map(|load| NodeLoadDto {
            state: metric_state(load.state),
            one: load.one,
            five: load.five,
            fifteen: load.fifteen,
        }),
        swap: snapshot.swap.map(|swap| NodeSwapDto {
            state: metric_state(swap.state),
            used_bytes: swap.used_bytes,
            total_bytes: swap.total_bytes,
        }),
        disk: snapshot.disk.map(|disk| NodeDiskDto {
            state: metric_state(disk.state),
            available_bytes: disk.available_bytes,
            total_bytes: disk.total_bytes,
            volume_count: disk.volume_count,
        }),
        disk_io: snapshot.disk_io.map(|io| NodeDiskIoDto {
            state: metric_state(io.state),
            read_bytes_per_sec: io.read_bytes_per_sec,
            write_bytes_per_sec: io.write_bytes_per_sec,
        }),
        network: snapshot.network.map(|network| NodeNetworkDto {
            state: metric_state(network.state),
            rx_bytes_per_sec: network.rx_bytes_per_sec,
            tx_bytes_per_sec: network.tx_bytes_per_sec,
        }),
        diagnostic: snapshot.diagnostic.clone(),
    }
}

fn metric_state(state: NodeMetricState) -> NodeMetricStateDto {
    match state {
        NodeMetricState::Value => NodeMetricStateDto::Value,
        NodeMetricState::Unknown => NodeMetricStateDto::Unknown,
        NodeMetricState::Unsupported => NodeMetricStateDto::Unsupported,
    }
}
