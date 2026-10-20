//! Read-only host telemetry for named Eggwork nodes (Eggwork M002).
//!
//! # What this is
//!
//! An operator-facing view of what a remote node's *host* is doing. The
//! node owns a Gregg provider; the node polls it on its own schedule and
//! serves the resulting observation over its authenticated telemetry route.
//! CodeGG never talks to Gregg, never polls a provider, and never treats
//! the observation as anything other than a description of the host.
//!
//! # What this is not
//!
//! * **Not a measurement of a CodeGG execution.** Per-attempt CPU, RSS and
//!   elapsed time stay the executor's own numbers (plan §3.E). A host
//!   observation says nothing about which execution consumed what.
//! * **Not an admission input.** A node's free memory never reserves
//!   capacity. M003's operator ceiling and the node's advertised
//!   capabilities are what gate admission.
//! * **Not a second scheduler.** A telemetry read never creates, mutates,
//!   retries or releases a job.
//!
//! # Ownership
//!
//! One service per daemon, holding one cache and one probe lock per named
//! node. Sessions, agents and UI instances share it; none of them owns a
//! probe. Concurrent readers of the same node coalesce onto a single
//! upstream read instead of amplifying the node's poll of its provider.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eggwork_core::{
    NodeId, NodeTelemetry, TelemetryErrorCategory, TelemetryMetric, TelemetryProviderState,
    TelemetrySource,
};

use crate::scheduler::eggwork::{EggworkClientFactory, ResolvedEggworkNode};

/// Bound on one telemetry read.
///
/// The upstream client already caps a unary call at 120 s. An operator
/// diagnostic must not hold a request open that long, so CodeGG bounds it
/// again, well below the upstream ceiling.
const TELEMETRY_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a successful observation stays fresh.
///
/// A repeated read inside this window is answered from the cache without a
/// network call, so a refresh loop cannot amplify the node's provider poll.
const TELEMETRY_FRESH_TTL: Duration = Duration::from_secs(30);

/// How long a failed probe is remembered.
///
/// Failures are cached too, but briefly: an operator who fixes the node
/// must not wait out a long negative cache to see the recovery.
const TELEMETRY_FAILURE_TTL: Duration = Duration::from_secs(3);

/// Where a node's host telemetry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeTelemetrySource {
    /// Readings come from the node's locally installed Gregg provider.
    Gregg,
}

/// The provider's own view of whether its observation is usable.
///
/// This is the node's statement, not CodeGG's. `Ready` means the node
/// validated a sample inside its freshness bound; `Stale` means it holds a
/// sample that is now older than that bound; the remaining variants mean no
/// usable observation exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeTelemetryState {
    /// No provider is configured on the node. Nothing is polled.
    Disabled,
    /// A provider is configured but has not produced an observation yet.
    Warming,
    /// A validated observation is available and within its freshness bound.
    Ready,
    /// An observation exists but is older than the freshness bound.
    Stale,
    /// The provider could not be reached, or answered with something unusable.
    Unavailable,
    /// The provider answered with a payload that failed validation.
    Invalid,
}

impl NodeTelemetryState {
    /// Whether the observation behind this state may be read as current.
    ///
    /// Only the node's own `Ready` state qualifies. A `Stale` sample is
    /// still reported — with its age — but it is never presented as
    /// current, and it is never substituted for a missing measurement.
    pub fn is_fresh(self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Whether one metric dimension carries a measurement.
///
/// `Value` means the node reported the dimension, `Unknown` means the node
/// supports it but did not report it this time, and `Unsupported` means the
/// host platform cannot produce it at all. The last two are never rendered
/// as a zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeMetricState {
    /// The node reported a measurement.
    Value,
    /// The dimension is supported but was not reported in this sample.
    Unknown,
    /// The host platform cannot produce this dimension.
    Unsupported,
}

/// CPU utilization of the remote host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeCpu {
    /// Whether the node reported the dimension at all.
    pub state: NodeMetricState,
    /// Logical CPU cores visible to the kernel.
    pub logical_cores: Option<u32>,
    /// Aggregate CPU busy percentage.
    pub usage_pct: Option<f64>,
    /// Aggregate CPU I/O-wait percentage, when the host reports it.
    pub iowait_pct: Option<f64>,
}

/// Physical memory of the remote host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeMemory {
    /// Whether the node reported the dimension at all.
    pub state: NodeMetricState,
    /// Bytes available without swapping.
    pub available_bytes: Option<u64>,
    /// Total physical memory in bytes.
    pub total_bytes: Option<u64>,
    /// Bytes currently resident in physical memory.
    pub used_bytes: Option<u64>,
}

/// Load averages of the remote host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeLoad {
    /// Whether the node reported the dimension at all.
    pub state: NodeMetricState,
    /// One-minute load average.
    pub one: Option<f64>,
    /// Five-minute load average.
    pub five: Option<f64>,
    /// Fifteen-minute load average.
    pub fifteen: Option<f64>,
}

/// Swap utilization of the remote host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeSwap {
    /// Whether the node reported the dimension at all.
    pub state: NodeMetricState,
    /// Bytes currently in swap.
    pub used_bytes: Option<u64>,
    /// Total swap in bytes.
    pub total_bytes: Option<u64>,
}

/// Storage capacity of the remote host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeDisk {
    /// Whether the node reported the dimension at all.
    pub state: NodeMetricState,
    /// Free bytes across the reported volumes.
    pub available_bytes: Option<u64>,
    /// Total bytes across the reported volumes.
    pub total_bytes: Option<u64>,
    /// How many volumes contributed to the aggregate.
    pub volume_count: Option<u32>,
}

/// Aggregate disk throughput of the remote host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeDiskIo {
    /// Whether the node reported the dimension at all.
    pub state: NodeMetricState,
    /// Aggregate read throughput in bytes per second.
    pub read_bytes_per_sec: Option<u64>,
    /// Aggregate write throughput in bytes per second.
    pub write_bytes_per_sec: Option<u64>,
}

/// Aggregate network throughput of the remote host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeNetwork {
    /// Whether the node reported the dimension at all.
    pub state: NodeMetricState,
    /// Aggregate receive throughput in bytes per second.
    pub rx_bytes_per_sec: Option<u64>,
    /// Aggregate transmit throughput in bytes per second.
    pub tx_bytes_per_sec: Option<u64>,
}

/// One node's sanitized host telemetry.
///
/// The snapshot deliberately carries no interface identity, hostname, mount
/// path, process id, or provider error text: only the provider state, the
/// freshness of the observation, the metric dimensions the node actually
/// reported, and a bounded diagnostic when no usable observation exists.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeTelemetrySnapshot {
    /// The `[eggwork.nodes]` table key of the node this snapshot describes.
    ///
    /// This is the same identifier space as
    /// [`codegg_protocol::dto::ExecutionTargetDto::EggworkNode`], so an
    /// operator can copy it straight into a job target.
    pub node_id: String,
    /// The provider that produced the observation.
    pub source: NodeTelemetrySource,
    /// The provider's own view of the observation.
    pub state: NodeTelemetryState,
    /// Age of the observation in milliseconds, as reported by the node.
    pub age_ms: Option<u64>,
    /// The provider's bounded error *category*, when it reported one.
    ///
    /// The category is a closed vocabulary upstream; the provider's own
    /// message is never carried.
    pub last_error: Option<String>,
    /// Wall-clock time of the observation, in unix milliseconds.
    pub observed_at_unix_ms: Option<u64>,
    /// The interval the node sampled at, in milliseconds.
    pub sample_interval_ms: Option<u64>,
    /// CPU utilization.
    pub cpu: Option<NodeCpu>,
    /// Physical memory utilization.
    pub memory: Option<NodeMemory>,
    /// Load averages.
    pub load: Option<NodeLoad>,
    /// Swap utilization.
    pub swap: Option<NodeSwap>,
    /// Storage capacity.
    pub disk: Option<NodeDisk>,
    /// Aggregate disk throughput.
    pub disk_io: Option<NodeDiskIo>,
    /// Aggregate network throughput.
    pub network: Option<NodeNetwork>,
    /// A bounded, endpoint-free explanation of why this snapshot carries no
    /// usable observation.
    pub diagnostic: Option<String>,
}

impl NodeTelemetrySnapshot {
    /// A snapshot that carries no observation and explains why.
    fn unavailable(node_id: &str, diagnostic: impl Into<String>) -> Self {
        Self {
            node_id: node_id.to_string(),
            source: NodeTelemetrySource::Gregg,
            state: NodeTelemetryState::Unavailable,
            age_ms: None,
            last_error: None,
            observed_at_unix_ms: None,
            sample_interval_ms: None,
            cpu: None,
            memory: None,
            load: None,
            swap: None,
            disk: None,
            disk_io: None,
            network: None,
            diagnostic: Some(diagnostic.into()),
        }
    }
}

/// Why a telemetry read could not produce a snapshot at all.
///
/// A *node-level* failure is not an error here: the snapshot records the
/// failure as a diagnostic and the state `Unavailable`. This error is for
/// the reads that never reached a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeTelemetryError {
    /// The daemon has no Eggwork nodes configured, so there is nothing to
    /// observe. This is an explicit status, not a silent empty list.
    NoNodesConfigured,
    /// The requested node is not in the `[eggwork.nodes]` table.
    UnknownNode {
        /// The requested table key.
        node_id: String,
    },
}

/// One cached observation for one node.
struct CachedTelemetry {
    snapshot: NodeTelemetrySnapshot,
    fetched_at: Instant,
    /// A failed probe is remembered for a shorter time than a success.
    failure: bool,
}

impl CachedTelemetry {
    fn is_fresh(&self, now: Instant) -> bool {
        let ttl = if self.failure {
            TELEMETRY_FAILURE_TTL
        } else {
            TELEMETRY_FRESH_TTL
        };
        now.duration_since(self.fetched_at) < ttl
    }
}

#[derive(Default)]
struct TelemetryState {
    cache: HashMap<String, CachedTelemetry>,
    /// One probe lock per node: concurrent readers of the same node queue
    /// behind it and then find the cache already fresh.
    probe_locks: HashMap<String, Arc<tokio::sync::Mutex<()>>>,
}

/// Daemon-owned coordinator for node host telemetry.
///
/// Constructed once per daemon from the resolved `[eggwork.nodes]` table.
/// It is independent of job dispatch: the scheduler may be disabled and the
/// diagnostic still works, and a diagnostic read never touches the queue.
pub struct NodeTelemetryService {
    nodes: HashMap<String, ResolvedEggworkNode>,
    factory: Arc<dyn EggworkClientFactory>,
    state: Mutex<TelemetryState>,
}

impl NodeTelemetryService {
    /// Build the service from the daemon's resolved node table.
    pub fn new(
        nodes: HashMap<String, ResolvedEggworkNode>,
        factory: Arc<dyn EggworkClientFactory>,
    ) -> Self {
        Self {
            nodes,
            factory,
            state: Mutex::new(TelemetryState::default()),
        }
    }

    /// The configured node table keys, in a stable order.
    pub fn node_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.nodes.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Read the host telemetry of one node, or of every configured node.
    ///
    /// A node that cannot be read yields a snapshot carrying a bounded
    /// diagnostic — never an error, never a fabricated measurement. The
    /// error cases are only the ones where no node could be addressed at
    /// all.
    pub async fn snapshots(
        &self,
        node_id: Option<&str>,
    ) -> Result<Vec<NodeTelemetrySnapshot>, NodeTelemetryError> {
        if self.nodes.is_empty() {
            return Err(NodeTelemetryError::NoNodesConfigured);
        }
        let selected: Vec<(String, ResolvedEggworkNode)> = match node_id {
            Some(requested) => {
                let node =
                    self.nodes
                        .get(requested)
                        .ok_or_else(|| NodeTelemetryError::UnknownNode {
                            node_id: requested.to_string(),
                        })?;
                vec![(requested.to_string(), node.clone())]
            }
            None => {
                let mut all: Vec<(String, ResolvedEggworkNode)> = self
                    .nodes
                    .iter()
                    .map(|(key, node)| (key.clone(), node.clone()))
                    .collect();
                all.sort_by(|a, b| a.0.cmp(&b.0));
                all
            }
        };

        let mut snapshots = Vec::with_capacity(selected.len());
        for (key, node) in selected {
            snapshots.push(self.snapshot_for(&key, &node).await);
        }
        Ok(snapshots)
    }

    /// Read one node's telemetry, coalescing concurrent readers.
    async fn snapshot_for(&self, key: &str, node: &ResolvedEggworkNode) -> NodeTelemetrySnapshot {
        let probe_lock = {
            let mut state = self.lock_state();
            state
                .probe_locks
                .entry(key.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        // The lock is held across the read on purpose: a second reader waits
        // here instead of issuing a second upstream read, and finds the
        // cache warm when it wakes.
        let _guard = probe_lock.lock().await;

        if let Some(cached) = self.cached(key) {
            return cached;
        }
        let snapshot = self.probe(key, node).await;
        self.store(key, &snapshot);
        snapshot
    }

    fn cached(&self, key: &str) -> Option<NodeTelemetrySnapshot> {
        let now = Instant::now();
        let state = self.lock_state();
        state
            .cache
            .get(key)
            .filter(|cached| cached.is_fresh(now))
            .map(|cached| cached.snapshot.clone())
    }

    fn store(&self, key: &str, snapshot: &NodeTelemetrySnapshot) {
        let mut state = self.lock_state();
        state.cache.insert(
            key.to_string(),
            CachedTelemetry {
                snapshot: snapshot.clone(),
                fetched_at: Instant::now(),
                failure: snapshot.diagnostic.is_some(),
            },
        );
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, TelemetryState> {
        // The cache map is only ever held across short, non-awaiting
        // critical sections, so a poisoned lock can only mean a panic in
        // another thread's bookkeeping. Recovering the guard keeps a
        // diagnostic surface from taking the daemon down.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Perform one upstream read and project it.
    async fn probe(&self, key: &str, node: &ResolvedEggworkNode) -> NodeTelemetrySnapshot {
        let client = match self.factory.client_for(node) {
            Ok(client) => client,
            Err(reason) => {
                return NodeTelemetrySnapshot::unavailable(
                    key,
                    format!("client unavailable: {reason}"),
                )
            }
        };
        let expected = match NodeId::new(&node.node_id) {
            Ok(id) => id,
            Err(_) => {
                return NodeTelemetrySnapshot::unavailable(
                    key,
                    "the configured node id is not a valid Eggwork node identity",
                )
            }
        };

        let read = tokio::time::timeout(TELEMETRY_PROBE_TIMEOUT, client.telemetry(&expected)).await;
        let telemetry = match read {
            Err(_elapsed) => {
                return NodeTelemetrySnapshot::unavailable(key, "telemetry read timed out")
            }
            Ok(Err(error)) => {
                return NodeTelemetrySnapshot::unavailable(key, telemetry_diagnostic(&error))
            }
            Ok(Ok(telemetry)) => telemetry,
        };

        project(key, &telemetry)
    }
}

/// Map an upstream client error onto a bounded, endpoint-free diagnostic.
///
/// The distinction the operator needs is *why* there is no observation, and
/// the upstream error taxonomy already draws it: an old node that has no
/// telemetry route, a schema this client does not understand, a response
/// that failed validation, and a transport failure are four different
/// problems with four different fixes.
fn telemetry_diagnostic(error: &eggwork_client::ClientError) -> String {
    use eggwork_client::ClientError;
    match error {
        ClientError::TelemetryUnsupported => {
            "this node does not implement the host telemetry contract".to_string()
        }
        ClientError::ProtocolIncompatible => {
            "this node reports a host telemetry schema this client does not understand".to_string()
        }
        ClientError::InvalidResponse => {
            "this node's host telemetry response was rejected".to_string()
        }
        ClientError::Transport(_) => "host telemetry transport failed".to_string(),
        ClientError::Api { status, .. } => {
            format!("this node refused the host telemetry request (HTTP {status})")
        }
        ClientError::InvalidEndpoint => "the node endpoint is not usable".to_string(),
        ClientError::InvalidRoute => "the node route is not usable".to_string(),
        ClientError::UnsupportedCapability => "this node does not offer host telemetry".to_string(),
    }
}

/// Project an upstream telemetry document onto CodeGG's sanitized view.
///
/// The upstream document is already bounded and already carries the
/// provider's own state; this function's job is to keep CodeGG's own rules:
/// no identity beyond the table key, no provider message, and no value that
/// the node did not report.
fn project(node_key: &str, telemetry: &NodeTelemetry) -> NodeTelemetrySnapshot {
    let state = match telemetry.state {
        TelemetryProviderState::Disabled => NodeTelemetryState::Disabled,
        TelemetryProviderState::Warming => NodeTelemetryState::Warming,
        TelemetryProviderState::Ready => NodeTelemetryState::Ready,
        TelemetryProviderState::Stale => NodeTelemetryState::Stale,
        TelemetryProviderState::Unavailable => NodeTelemetryState::Unavailable,
        TelemetryProviderState::Invalid => NodeTelemetryState::Invalid,
    };
    let source = match telemetry.source {
        TelemetrySource::Gregg => NodeTelemetrySource::Gregg,
    };

    let diagnostic = match (state, telemetry.observation.as_ref()) {
        (NodeTelemetryState::Unavailable, None) => {
            Some("the node's telemetry provider is unavailable".to_string())
        }
        (NodeTelemetryState::Invalid, None) => {
            Some("the node's telemetry provider returned an invalid observation".to_string())
        }
        (NodeTelemetryState::Warming, None) => {
            Some("the node's telemetry provider has not produced an observation yet".to_string())
        }
        (NodeTelemetryState::Disabled, None) => {
            Some("the node has no telemetry provider configured".to_string())
        }
        _ => None,
    };

    let observation = telemetry.observation.as_ref();
    let (observed_at_unix_ms, sample_interval_ms) = match observation {
        Some(observation) => (
            Some(observation.observed_at_unix_ms),
            Some(observation.sample_interval_ms),
        ),
        None => (None, None),
    };

    NodeTelemetrySnapshot {
        node_id: node_key.to_string(),
        source,
        state,
        age_ms: telemetry.age_ms,
        last_error: telemetry.last_error.map(|category| {
            let category = match category {
                TelemetryErrorCategory::Connection => "connection",
                TelemetryErrorCategory::Timeout => "timeout",
                TelemetryErrorCategory::HttpStatus => "http_status",
                TelemetryErrorCategory::Malformed => "malformed",
                TelemetryErrorCategory::SchemaVersion => "schema_version",
                TelemetryErrorCategory::Invalid => "invalid",
                TelemetryErrorCategory::TooLarge => "too_large",
                TelemetryErrorCategory::ClockSkew => "clock_skew",
            };
            category.to_string()
        }),
        observed_at_unix_ms,
        sample_interval_ms,
        cpu: observation.map(|observation| project_cpu(&observation.cpu)),
        memory: observation.map(|observation| project_memory(&observation.memory)),
        load: observation.map(|observation| project_load(&observation.load)),
        swap: observation.map(|observation| project_swap(&observation.swap)),
        disk: observation.map(|observation| project_disk(&observation.disk)),
        disk_io: observation.map(|observation| project_disk_io(&observation.disk_io)),
        network: observation.map(|observation| project_network(&observation.network)),
        diagnostic,
    }
}

fn metric_state<T>(metric: &TelemetryMetric<T>) -> NodeMetricState {
    match metric {
        TelemetryMetric::Value(_) => NodeMetricState::Value,
        TelemetryMetric::Unknown => NodeMetricState::Unknown,
        TelemetryMetric::Unsupported => NodeMetricState::Unsupported,
    }
}

fn project_cpu(metric: &TelemetryMetric<eggwork_core::TelemetryCpu>) -> NodeCpu {
    let state = metric_state(metric);
    let value = metric.value();
    NodeCpu {
        state,
        logical_cores: value.map(|cpu| cpu.logical_cores),
        usage_pct: value.map(|cpu| f64::from(cpu.usage_pct)),
        iowait_pct: value.and_then(|cpu| cpu.iowait_pct).map(f64::from),
    }
}

fn project_memory(metric: &TelemetryMetric<eggwork_core::TelemetryMemory>) -> NodeMemory {
    let state = metric_state(metric);
    let value = metric.value();
    NodeMemory {
        state,
        available_bytes: value.map(|memory| memory.available_bytes),
        total_bytes: value.map(|memory| memory.total_bytes),
        used_bytes: value.map(|memory| memory.used_bytes),
    }
}

fn project_load(metric: &TelemetryMetric<eggwork_core::TelemetryLoad>) -> NodeLoad {
    let state = metric_state(metric);
    let value = metric.value();
    NodeLoad {
        state,
        one: value.map(|load| f64::from(load.one)),
        five: value.map(|load| f64::from(load.five)),
        fifteen: value.map(|load| f64::from(load.fifteen)),
    }
}

fn project_swap(metric: &TelemetryMetric<eggwork_core::TelemetrySwap>) -> NodeSwap {
    let state = metric_state(metric);
    let value = metric.value();
    NodeSwap {
        state,
        used_bytes: value.map(|swap| swap.used_bytes),
        total_bytes: value.map(|swap| swap.total_bytes),
    }
}

fn project_disk(metric: &TelemetryMetric<eggwork_core::TelemetryDisk>) -> NodeDisk {
    let state = metric_state(metric);
    let value = metric.value();
    NodeDisk {
        state,
        available_bytes: value.map(|disk| disk.available_bytes),
        total_bytes: value.map(|disk| disk.total_bytes),
        volume_count: value.map(|disk| disk.volume_count),
    }
}

fn project_disk_io(metric: &TelemetryMetric<eggwork_core::TelemetryDiskIo>) -> NodeDiskIo {
    let state = metric_state(metric);
    let value = metric.value();
    NodeDiskIo {
        state,
        read_bytes_per_sec: value.map(|io| io.read_bytes_per_sec),
        write_bytes_per_sec: value.map(|io| io.write_bytes_per_sec),
    }
}

fn project_network(metric: &TelemetryMetric<eggwork_core::TelemetryNetwork>) -> NodeNetwork {
    let state = metric_state(metric);
    let value = metric.value();
    NodeNetwork {
        state,
        rx_bytes_per_sec: value.map(|network| network.rx_bytes_per_sec),
        tx_bytes_per_sec: value.map(|network| network.tx_bytes_per_sec),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggwork_core::{
        HostTelemetry, TelemetryCpu, TelemetryDisk, TelemetryDiskIo, TelemetryMemory,
        TelemetryNetwork,
    };

    fn observation() -> HostTelemetry {
        HostTelemetry {
            observed_at_unix_ms: 1_700_000_000_000,
            sample_interval_ms: 2_000,
            cpu: TelemetryMetric::Value(TelemetryCpu {
                logical_cores: 8,
                usage_pct: 12.5,
                iowait_pct: None,
            }),
            memory: TelemetryMetric::Value(TelemetryMemory {
                available_bytes: 1_000,
                total_bytes: 2_000,
                used_bytes: 1_000,
            }),
            load: TelemetryMetric::Unsupported,
            swap: TelemetryMetric::Unknown,
            disk: TelemetryMetric::Value(TelemetryDisk {
                available_bytes: 10,
                total_bytes: 20,
                volume_count: 2,
            }),
            disk_io: TelemetryMetric::Value(TelemetryDiskIo {
                read_bytes_per_sec: 3,
                write_bytes_per_sec: 4,
            }),
            network: TelemetryMetric::Value(TelemetryNetwork {
                rx_bytes_per_sec: 5,
                tx_bytes_per_sec: 6,
            }),
        }
    }

    fn telemetry(state: TelemetryProviderState, obs: Option<HostTelemetry>) -> NodeTelemetry {
        NodeTelemetry {
            schema_version: 1,
            node_id: NodeId::new("node-a").unwrap(),
            source: TelemetrySource::Gregg,
            state,
            age_ms: Some(1_500),
            last_error: None,
            observation: obs,
        }
    }

    #[test]
    fn a_ready_observation_projects_every_reported_dimension() {
        let snapshot = project(
            "node-a",
            &telemetry(TelemetryProviderState::Ready, Some(observation())),
        );

        assert_eq!(snapshot.node_id, "node-a");
        assert_eq!(snapshot.state, NodeTelemetryState::Ready);
        assert!(snapshot.state.is_fresh());
        assert_eq!(snapshot.age_ms, Some(1_500));
        assert_eq!(snapshot.observed_at_unix_ms, Some(1_700_000_000_000));
        assert_eq!(snapshot.sample_interval_ms, Some(2_000));

        let cpu = snapshot.cpu.expect("cpu");
        assert_eq!(cpu.state, NodeMetricState::Value);
        assert_eq!(cpu.logical_cores, Some(8));
        assert_eq!(cpu.usage_pct, Some(12.5));
        // The host does not report iowait; it is absent, never zero.
        assert_eq!(cpu.iowait_pct, None);
    }

    #[test]
    fn an_unsupported_dimension_is_never_rendered_as_zero() {
        let snapshot = project(
            "node-a",
            &telemetry(TelemetryProviderState::Ready, Some(observation())),
        );

        let load = snapshot.load.expect("load");
        assert_eq!(load.state, NodeMetricState::Unsupported);
        assert_eq!(load.one, None);
        assert_eq!(load.five, None);
        assert_eq!(load.fifteen, None);

        let swap = snapshot.swap.expect("swap");
        assert_eq!(swap.state, NodeMetricState::Unknown);
        assert_eq!(swap.used_bytes, None);
        assert_eq!(swap.total_bytes, None);
    }

    #[test]
    fn a_disabled_provider_fabricates_nothing() {
        let snapshot = project("node-a", &telemetry(TelemetryProviderState::Disabled, None));

        assert_eq!(snapshot.state, NodeTelemetryState::Disabled);
        assert!(!snapshot.state.is_fresh());
        assert!(snapshot.cpu.is_none());
        assert!(snapshot.memory.is_none());
        assert_eq!(snapshot.observed_at_unix_ms, None);
        assert_eq!(
            snapshot.diagnostic.as_deref(),
            Some("the node has no telemetry provider configured")
        );
    }

    #[test]
    fn a_stale_observation_is_reported_but_not_called_fresh() {
        let snapshot = project(
            "node-a",
            &telemetry(TelemetryProviderState::Stale, Some(observation())),
        );

        assert_eq!(snapshot.state, NodeTelemetryState::Stale);
        assert!(!snapshot.state.is_fresh());
        // The numbers the node did report are still carried, with the age.
        assert!(snapshot.cpu.is_some());
        assert_eq!(snapshot.age_ms, Some(1_500));
        assert_eq!(snapshot.diagnostic, None);
    }

    #[test]
    fn the_provider_error_category_is_carried_but_never_its_message() {
        let mut document = telemetry(TelemetryProviderState::Unavailable, None);
        document.last_error = Some(TelemetryErrorCategory::Timeout);
        let snapshot = project("node-a", &document);

        assert_eq!(snapshot.last_error.as_deref(), Some("timeout"));
    }

    #[test]
    fn a_failed_probe_reports_a_bounded_diagnostic_and_no_metrics() {
        let snapshot = NodeTelemetrySnapshot::unavailable("node-a", "telemetry read timed out");

        assert_eq!(snapshot.state, NodeTelemetryState::Unavailable);
        assert_eq!(
            snapshot.diagnostic.as_deref(),
            Some("telemetry read timed out")
        );
        assert!(snapshot.cpu.is_none());
        assert!(snapshot.memory.is_none());
        assert_eq!(snapshot.observed_at_unix_ms, None);
    }
}
