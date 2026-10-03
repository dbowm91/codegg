//! Desktop artifact excerpt host (M004 WP C / §12).
//!
//! Truncated projection items reference opaque artifact handles. This
//! module reads one bounded excerpt through the live projection
//! driver, which refreshes the authorized handle registry and builds
//! the read with consumer-side validation (opaque handle, project
//! binding, revision, 64 KiB window). The renderer supplies only the
//! handle plus a bounded range; handles never originated from renderer
//! paths, and stale/foreign handles fail closed before any read is
//! built. Daemon typed outcomes (`Denied`/`NotFound`/
//! `RevisionMismatch`/…) surface as bounded errors, never guessed
//! content. No generic `CoreRequest` bridge, no filesystem access.

use codegg_protocol::projection::replay::ProjectionArtifactReadOutcome;

use super::bridge::ArtifactExcerptView;
use super::HostState;

/// Default excerpt window when the renderer passes no length.
pub(crate) const DEFAULT_EXCERPT_BYTES: u64 = 32 * 1024;

/// Hard host-side window cap (mirrors the consumer 64 KiB maximum).
pub(crate) const MAX_EXCERPT_BYTES: u64 = 64 * 1024;

impl HostState {
    /// Read one bounded artifact excerpt for the route-selected
    /// session's live projection owner.
    ///
    /// Holds the route lock across the bounded excerpt round trip: the
    /// driver is owned by the route (no cloneable read lease exists),
    /// and the excerpt path never re-enters the route lock (the driver
    /// task only touches the daemon socket), so no lock cycle is
    /// possible. The window is small and local; navigation simply
    /// serializes behind an in-flight excerpt.
    #[allow(clippy::await_holding_lock)]
    pub(crate) async fn route_artifact_read(
        &self,
        handle: String,
        start: u64,
        length: Option<u64>,
        expected_generation: u64,
    ) -> Result<ArtifactExcerptView, String> {
        if handle.trim().is_empty() {
            return Err("artifact handle must not be empty".into());
        }
        let length = length.unwrap_or(DEFAULT_EXCERPT_BYTES);
        if length == 0 || length > MAX_EXCERPT_BYTES {
            return Err("artifact excerpt length out of bounds".into());
        }
        let route = self.route.lock().await;
        if route.connection_generation != self.current_generation() || route.project_id.is_none() {
            return Err("no current route; select a project first".into());
        }
        if route.route_generation != expected_generation {
            return Err("stale route; refresh before reading artifacts".into());
        }
        let Some(session_id) = route.session_id.clone() else {
            return Err("no session selected for the current route".into());
        };
        let project_id = route.project_id.clone().unwrap_or_default();
        let connection_generation = route.connection_generation;
        let Some(owner) = route.projection.as_ref() else {
            return Err("no session projection attached for the current route".into());
        };
        if owner.session_id != session_id
            || owner.connection_generation != connection_generation
            || owner.route_generation != expected_generation
        {
            return Err("session projection is stale; reattach".into());
        }
        let end = start.saturating_add(length);
        let outcome = owner
            .driver
            .artifact_excerpt(&project_id, &handle, start, Some(end))
            .await
            .map_err(|error| error.to_string())?;
        if self.current_generation() != connection_generation {
            return Err("stale route; artifact excerpt discarded".into());
        }
        match outcome {
            ProjectionArtifactReadOutcome::Ok(excerpt) => Ok(ArtifactExcerptView {
                handle: excerpt.handle_id,
                start: excerpt.start,
                end: excerpt.end,
                content_type: excerpt.content_type,
                content: excerpt.content,
                truncated: excerpt.truncated,
                redacted: excerpt.redacted,
            }),
            ProjectionArtifactReadOutcome::Denied { reason } => {
                Err(format!("artifact_denied: {reason}"))
            }
            ProjectionArtifactReadOutcome::NotFound => Err("artifact_not_found".into()),
            ProjectionArtifactReadOutcome::RevisionMismatch { current_revision } => Err(format!(
                "artifact_revision_mismatch: current revision {current_revision}"
            )),
            ProjectionArtifactReadOutcome::InvalidRequest { reason } => {
                Err(format!("artifact_invalid_request: {reason}"))
            }
            ProjectionArtifactReadOutcome::Oversized => Err("artifact_oversized".into()),
        }
    }
}
