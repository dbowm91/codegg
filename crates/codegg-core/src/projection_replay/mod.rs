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

pub mod artifact_registry;
pub mod artifacts;
pub mod context;
pub mod handle;
pub mod metrics;
pub mod policy;
pub mod publication;
pub mod redactor;
pub mod retention;
pub mod safe_publication;
pub mod seam;
pub mod service;
pub mod store;
pub mod subscription;

pub use artifact_registry::{
    ArtifactRegistryError, HandleEntry, HandleId, ProjectionArtifactRegistry,
    RunStoreProjectionArtifactRegistry,
};
pub use artifacts::{
    ArtifactAccessDecision, ArtifactContentType, ArtifactKind, ArtifactReadOutcome,
    ArtifactReadRequest, ArtifactReadResponse, HandleLifecycle, HandleRegistrar, HandleRegistry,
    ProjectionArtifactHandle, ReadLifecycle,
};
pub use context::{
    AllowAllProjectResolver, BoundedProjectResolver, ProjectionAccessContext, ProjectionCapability,
    ProjectionCapabilitySet, ProjectionClientId, ProjectionPrincipalId, ProjectionProjectResolver,
    ProjectionTransportClass,
};
pub use metrics::{ProjectionReplayMetrics, ProjectionReplayMetricsSnapshot};
pub use policy::{
    ArtifactReadKind, DefaultAccessPolicy, DisclosureDecision, DisclosureReason,
    PolicyRegistry, ProjectionAccessPolicy,
};
pub use redactor::{ProjectionFieldRedactor, RedactionResult, RedactionSummary};
pub use retention::RetentionPolicy;
pub use seam::{
    ProjectionBindingContext, ProjectionDisclosureContext, ProjectionPublicationContext,
    ProjectionPublicationSeam,
};
pub use service::{ProjectionReplayService, PublishOutcome, ResumeOutcome};
pub use store::ProjectionReplayStore;
pub use subscription::SubscriptionRegistry;
