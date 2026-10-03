//! Desktop session-control interaction host (M004 WP E).
//!
//! Pending permission/question items render from the canonical
//! projection; this module answers them through existing daemon
//! authority (ADR-0007 turn-scoped controller leases). Rules:
//!
//! - the renderer sends only an opaque pending id (+ an allowed choice
//!   for permissions, bounded answers for questions). Principal,
//!   client, controller, turn, and session identity are never accepted
//!   from the renderer; the daemon parses the scoped id and
//!   revalidates owning session/turn/controller lease;
//! - the host additionally verifies the id's embedded session matches
//!   the route-selected session before sending (fail-closed early,
//!   no cross-session leakage even at the request layer);
//! - one response per pending id is in flight at a time
//!   (`RouteState.responding`); duplicates coalesce;
//! - every completion is fenced on the route/connection generation;
//!   on daemon success the host refreshes the controller summary so
//!   the renderer converges on authoritative state rather than
//!   guessing;
//! - observer/non-controller/stale-controller denials surface as
//!   daemon errors; the host never upgrades them.
//!
//! No generic `CoreRequest` bridge, no filesystem paths.

use codegg_client::DriverSnapshotView;
use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};

use super::bridge::ControllerSummaryView;
use super::bridge::SessionPresentationView;
use super::present::present_driver;
use super::HostState;

/// Bound on renderer-supplied question answers (JSON-encoded). The
/// daemon accepts any JSON value; this keeps a runaway draft from
/// becoming an unbounded request.
pub(crate) const MAX_QUESTION_ANSWERS_JSON: usize = 65_536;

impl HostState {
    /// Overlay the last daemon-fetched controller summary onto a
    /// projection-derived view. The presenter stays pure over the
    /// canonical snapshot; control state arrives via `SessionControlGet`
    /// and merges here at the host layer.
    pub(crate) async fn present_with_controller(
        &self,
        view: &DriverSnapshotView,
    ) -> SessionPresentationView {
        let controller = self.route.lock().await.controller.clone();
        let mut presented = present_driver(view);
        presented.controller = controller;
        presented
    }

    /// Snapshot the route-selected session for a control-plane call,
    /// requiring a live connection and a current route generation.
    async fn control_selection(&self, expected_generation: u64) -> Result<(String, u64), String> {
        let route = self.route.lock().await;
        if route.connection_generation != self.current_generation() || route.project_id.is_none() {
            return Err("no current route; select a project first".into());
        }
        if route.route_generation != expected_generation {
            return Err("stale route; refresh before responding".into());
        }
        let Some(session_id) = route.session_id.clone() else {
            return Err("no session selected for the current route".into());
        };
        Ok((session_id, route.connection_generation))
    }

    /// Parse `perm:<session>:<turn>:<id>` or
    /// `question:<session>:<turn>:<id>` and require the embedded
    /// session to equal the route-selected one. The daemon re-parses
    /// authoritatively; this early check keeps a cross-session id from
    /// ever leaving the host.
    fn scoped_id_session(kind: &str, pending_id: &str) -> Result<String, String> {
        let mut parts = pending_id.splitn(4, ':');
        let prefix = parts.next().unwrap_or_default();
        let session_id = parts.next().unwrap_or_default();
        let turn_id = parts.next().unwrap_or_default();
        let local_id = parts.next().unwrap_or_default();
        if prefix != kind || session_id.is_empty() || turn_id.is_empty() || local_id.is_empty() {
            return Err(format!("malformed {kind} id"));
        }
        Ok(session_id.to_owned())
    }

    /// Fetch and install the controller summary for the route-selected
    /// session. `None` (idle: no exclusive controller) is a valid
    /// outcome and clears the stored summary.
    pub(crate) async fn route_control_refresh(
        &self,
        expected_generation: u64,
    ) -> Result<Option<ControllerSummaryView>, String> {
        let (session_id, connection_generation) =
            self.control_selection(expected_generation).await?;
        let (client, _) = self.route_request().await?;
        let response = client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: CoreRequest::SessionControlGet {
                    session_id: session_id.clone(),
                },
            })
            .await
            .map_err(|error| error.to_string())?;
        let CoreResponse::SessionControl { controller, .. } = response else {
            return Err(crate::route::unexpected_daemon_response(&response));
        };
        let summary = controller.map(|controller| ControllerSummaryView {
            turn_id: controller.turn_id,
            controller_principal: controller.controller_principal,
            revision: controller.revision,
        });
        let mut route = self.route.lock().await;
        if self.current_generation() != connection_generation
            || route.connection_generation != connection_generation
            || route.route_generation != expected_generation
        {
            return Err("stale route; control refresh discarded".into());
        }
        route.controller = summary.clone();
        Ok(summary)
    }

    /// Answer one pending permission with an allowed choice. Returns
    /// the refreshed controller summary on daemon success.
    pub(crate) async fn route_permission_respond(
        &self,
        permission_id: String,
        choice: String,
        expected_generation: u64,
    ) -> Result<Option<ControllerSummaryView>, String> {
        if !matches!(
            choice.as_str(),
            "allow" | "always_allow" | "deny" | "always_deny"
        ) {
            return Err("invalid permission choice".into());
        }
        let request = CoreRequest::PermissionRespond {
            id: permission_id.clone(),
            choice,
        };
        self.route_control_respond("perm", permission_id, request, expected_generation)
            .await
    }

    /// Answer one pending question with bounded answers. Returns the
    /// refreshed controller summary on daemon success.
    pub(crate) async fn route_question_respond(
        &self,
        question_id: String,
        answers: serde_json::Value,
        expected_generation: u64,
    ) -> Result<Option<ControllerSummaryView>, String> {
        let encoded = serde_json::to_string(&answers).map_err(|error| error.to_string())?;
        if encoded.len() > MAX_QUESTION_ANSWERS_JSON {
            return Err("question answers exceed the bounded size".into());
        }
        let request = CoreRequest::QuestionRespond {
            id: question_id.clone(),
            answers,
        };
        self.route_control_respond("question", question_id, request, expected_generation)
            .await
    }

    /// Shared one-shot respond flow: cross-session fail-closed check,
    /// in-flight coalescing, fenced send, authoritative refresh.
    async fn route_control_respond(
        &self,
        kind: &str,
        pending_id: String,
        request: CoreRequest,
        expected_generation: u64,
    ) -> Result<Option<ControllerSummaryView>, String> {
        if pending_id.trim().is_empty() {
            return Err(format!("empty {kind} id"));
        }
        let (session_id, connection_generation) =
            self.control_selection(expected_generation).await?;
        let embedded = Self::scoped_id_session(kind, &pending_id)?;
        if embedded != session_id {
            return Err(format!("{kind} item belongs to another session"));
        }
        {
            let mut route = self.route.lock().await;
            if !route.responding.insert(pending_id.clone()) {
                return Err(format!("{kind} response already in flight"));
            }
        }
        let result = self
            .send_control_respond(request, connection_generation, expected_generation)
            .await;
        {
            let mut route = self.route.lock().await;
            route.responding.remove(&pending_id);
        }
        result
    }

    async fn send_control_respond(
        &self,
        request: CoreRequest,
        connection_generation: u64,
        expected_generation: u64,
    ) -> Result<Option<ControllerSummaryView>, String> {
        let (client, _) = self.route_request().await?;
        let response = client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: request,
            })
            .await
            .map_err(|error| error.to_string())?;
        match response {
            CoreResponse::Ack => {}
            CoreResponse::Error { code, message } => {
                return Err(format!("{code}: {message}"));
            }
            unexpected => {
                return Err(crate::route::unexpected_daemon_response(&unexpected));
            }
        }
        {
            let route = self.route.lock().await;
            if self.current_generation() != connection_generation
                || route.connection_generation != connection_generation
                || route.route_generation != expected_generation
            {
                return Err("stale route; control response discarded".into());
            }
        }
        // Converge on authoritative state: the daemon accepted the
        // response, so refresh the controller summary rather than
        // guessing at the new lease state.
        self.route_control_refresh(expected_generation).await
    }
}
