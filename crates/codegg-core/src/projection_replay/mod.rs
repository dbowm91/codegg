//! Session Projection Replay subsystem (M2).
//!
//! Provides daemon-owned, bounded, session/project-scoped projection
//! subscriptions backed by durable SQLite replay storage. The service
//! maps canonical core events through M1 adapters, persists accepted
//! projection events transactionally before live delivery, and
//! manages per-stream sequence allocation, retention, checkpointing,
//! and subscriber lifecycle.
//!
//! ## Module structure
//!
//! * [`store`] — SQLite-backed stream/event/checkpoint storage.
//! * [`service`] — canonical publication seam, subscribe/resume/ack.
//! * [`subscription`] — in-memory subscription registry.
//! * [`publication`] — core event to projection event mapping.
//! * [`safe_publication`] — visibility classification gate.
//! * [`retention`] — count/age/byte retention policy and pruning.
//! * [`metrics`] — bounded observability counters.
//! * [`handle`] — daemon-level Arc-wrapped publication helper.

pub mod handle;
pub mod metrics;
pub mod publication;
pub mod retention;
pub mod safe_publication;
pub mod service;
pub mod store;
pub mod subscription;

pub use metrics::{ProjectionReplayMetrics, ProjectionReplayMetricsSnapshot};
pub use retention::RetentionPolicy;
pub use service::{ProjectionReplayService, PublishOutcome, ResumeOutcome};
pub use store::ProjectionReplayStore;
pub use subscription::SubscriptionRegistry;
