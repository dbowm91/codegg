//! M007-G (ADR-0057): the daemon's plugin registry and its published
//! catalogue. The registry is the **single authority** for the catalogue: this
//! module owns the process-lifetime registry, the catalogue published on the
//! session projection, and the four mutating forms. Its files are
//! **user-scoped**, so no checkout writer permit mediates them.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::OnceCell;

use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse};
use crate::protocol::plugin_catalogue::{
    PluginManageActionDto, PluginManageRequestDto, PluginManageResultDto,
};

#[derive(Clone)]
pub struct DaemonPluginRegistry {
    service: Arc<crate::plugin::PluginService>,
    manager: Arc<crate::plugin::management::PluginManager>,
}

impl DaemonPluginRegistry {
    pub fn new(service: Arc<crate::plugin::PluginService>) -> Self {
        let manager = Arc::new(crate::plugin::management::PluginManager::new(
            service.clone(),
        ));
        Self { service, manager }
    }

    pub fn service(&self) -> &Arc<crate::plugin::PluginService> {
        &self.service
    }

    pub fn manager(&self) -> &Arc<crate::plugin::management::PluginManager> {
        &self.manager
    }
}

static REGISTRY_BUILDS: AtomicUsize = AtomicUsize::new(0);

static REGISTRY: OnceCell<Option<Arc<DaemonPluginRegistry>>> = OnceCell::const_new();

pub async fn process_registry() -> Option<Arc<DaemonPluginRegistry>> {
    REGISTRY
        .get_or_init(|| async {
            REGISTRY_BUILDS.fetch_add(1, Ordering::SeqCst);
            crate::plugin::create_default_plugin_service()
                .await
                .map(|service| Arc::new(DaemonPluginRegistry::new(service)))
        })
        .await
        .clone()
}

pub fn registry_build_count() -> usize {
    REGISTRY_BUILDS.load(Ordering::SeqCst)
}

/// The catalogue entries the daemon publishes for a workspace.
pub async fn catalogue_entries(
    registry: &DaemonPluginRegistry,
    workspace_id: Option<&str>,
) -> Vec<crate::protocol::projection::dto::PluginCatalogueEntryProjection> {
    let views = match workspace_id {
        Some(workspace) => registry.manager().list_for_workspace(workspace).await,
        None => registry.manager().list().await,
    };
    views
        .iter()
        .map(
            |view| crate::protocol::projection::dto::PluginCatalogueEntryProjection {
                plugin_id: view.id.clone(),
                name: view.name.clone(),
                version: view.version.clone(),
                enabled: view.enabled,
                trust: trust_name(view.trust),
                runtime_kind: view.runtime_kind.clone(),
                command_count: view.command_count,
                hook_count: view.hook_count,
                diagnostic_count: view.diagnostic_count,
                has_error: view.last_error.is_some(),
                source_path: view.source_path.clone(),
            },
        )
        .collect()
}

fn trust_name(trust: crate::plugin::PluginTrustClass) -> String {
    serde_json::to_value(trust)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

pub async fn manage(
    registry: &DaemonPluginRegistry,
    request: &PluginManageRequestDto,
) -> Result<PluginManageResultDto, (String, String)> {
    let target = request.target.trim();
    if target.is_empty() {
        return Err((
            "plugin_target_required".to_string(),
            "the plugin request named no plugin".to_string(),
        ));
    }
    match request.action {
        PluginManageActionDto::Enable => {
            let view = registry
                .manager()
                .enable(target)
                .await
                .map_err(management_error)?;
            Ok(PluginManageResultDto {
                action: request.action,
                plugin_id: view.id,
                enabled: Some(view.enabled),
                removed_paths: Vec::new(),
                install_path: None,
                message: "the plugin is enabled".to_string(),
            })
        }
        PluginManageActionDto::Disable => {
            let view = registry
                .manager()
                .disable(target)
                .await
                .map_err(management_error)?;
            Ok(PluginManageResultDto {
                action: request.action,
                plugin_id: view.id,
                enabled: Some(view.enabled),
                removed_paths: Vec::new(),
                install_path: None,
                message: "the plugin is disabled".to_string(),
            })
        }
        PluginManageActionDto::Remove => {
            let result = registry
                .manager()
                .uninstall(target)
                .await
                .map_err(management_error)?;
            let removed_paths = if result.removed_files {
                result
                    .install_path
                    .as_ref()
                    .map(|path| vec![path.display().to_string()])
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let message = match &result.warning {
                Some(warning) => format!("the plugin is unregistered; {warning}"),
                None if result.removed_files => "the plugin is removed".to_string(),
                None => "the plugin is unregistered".to_string(),
            };
            Ok(PluginManageResultDto {
                action: request.action,
                plugin_id: result.view.id,
                enabled: None,
                removed_paths,
                install_path: None,
                message,
            })
        }
        PluginManageActionDto::Install => {
            let path = std::path::Path::new(target);
            let view = registry
                .manager()
                .install_from_path(path)
                .await
                .map_err(management_error)?;
            let install_path = view.source_path.clone();
            Ok(PluginManageResultDto {
                action: request.action,
                plugin_id: view.id,
                enabled: Some(view.enabled),
                removed_paths: Vec::new(),
                install_path,
                message: "the plugin is installed".to_string(),
            })
        }
    }
}

fn management_error(error: crate::plugin::management::PluginManagementError) -> (String, String) {
    use crate::plugin::management::PluginManagementError;
    let code = match error {
        PluginManagementError::NotFound(_) | PluginManagementError::NoMatch(_) => {
            "plugin_not_found"
        }
        PluginManagementError::Ambiguous(_, _) => "plugin_selector_ambiguous",
        PluginManagementError::Install(_) => "plugin_install_failed",
        PluginManagementError::Activation(_) => "plugin_activation_failed",
        PluginManagementError::Registry(_) => "plugin_registry_failed",
        PluginManagementError::Service(_) => "plugin_service_failed",
    };
    (code.to_string(), error.to_string())
}

pub fn manage_response(result: PluginManageResultDto) -> CoreResponse {
    CoreResponse::PluginManageResult { result }
}

pub fn catalogue_event(
    session_id: String,
    entries: Vec<crate::protocol::projection::dto::PluginCatalogueEntryProjection>,
) -> CoreEvent {
    CoreEvent::PluginCatalogueUpdated {
        session_id,
        entries,
    }
}

/// Publish the daemon's plugin catalogue on the session projection.
pub async fn publish_catalogue(
    event_log: &crate::core::event_log::EventLog,
    registry: &DaemonPluginRegistry,
    session_id: &str,
    workspace_id: Option<&str>,
) -> Vec<crate::protocol::projection::dto::PluginCatalogueEntryProjection> {
    let entries = catalogue_entries(registry, workspace_id).await;
    event_log
        .publish(
            Some(session_id.to_string()),
            None,
            catalogue_event(session_id.to_string(), entries.clone()),
        )
        .await;
    entries
}

impl super::daemon::CoreDaemon {
    pub async fn plugin_registry(&self) -> Option<Arc<DaemonPluginRegistry>> {
        process_registry().await
    }

    pub(crate) async fn handle_plugin_request(
        &self,
        request: CoreRequest,
    ) -> Result<CoreResponse, crate::error::AppError> {
        let CoreRequest::PluginManage { request } = request else {
            return Ok(CoreResponse::Error {
                code: "unsupported_plugin_request".to_string(),
                message: "this request is not a plugin management request".to_string(),
            });
        };
        let Some(registry) = self.plugin_registry().await else {
            return Ok(CoreResponse::Error {
                code: "plugin_registry_unavailable".to_string(),
                message: "the plugin registry is unavailable; plugins are disabled".to_string(),
            });
        };
        let session_id = request.session_id.clone();
        match manage(&registry, &request).await {
            Ok(result) => {
                publish_catalogue(&self.event_log, &registry, &session_id, None).await;
                Ok(manage_response(result))
            }
            Err((code, message)) => Ok(CoreResponse::Error { code, message }),
        }
    }

    pub(crate) async fn publish_plugin_catalogue(
        &self,
        session_id: &str,
        workspace_id: Option<&str>,
    ) -> Vec<crate::protocol::projection::dto::PluginCatalogueEntryProjection> {
        let Some(registry) = self.plugin_registry().await else {
            return Vec::new();
        };
        publish_catalogue(&self.event_log, &registry, session_id, workspace_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::management::PluginManagementError;

    fn empty_registry() -> DaemonPluginRegistry {
        DaemonPluginRegistry::new(Arc::new(crate::plugin::PluginService::new(Arc::new(
            crate::plugin::registry::PluginRegistry::new(),
        ))))
    }

    #[test]
    fn the_registry_is_built_once_for_the_whole_process() {
        let before = registry_build_count();
        assert!(
            before <= 1,
            "the process must hold at most one plugin registry, saw {before}"
        );
    }

    #[tokio::test]
    async fn the_process_registry_is_one_instance() {
        let first = process_registry().await;
        let second = process_registry().await;
        match (first, second) {
            (Some(first), Some(second)) => {
                assert!(
                    Arc::ptr_eq(&first, &second),
                    "the process must hand out one registry, not a second"
                );
            }
            (None, None) => {}
            _ => panic!("the process registry must be the same answer twice"),
        }
        assert_eq!(
            registry_build_count(),
            1,
            "the registry must be built once, not once per caller"
        );
    }

    #[test]
    fn a_management_failure_carries_a_typed_code() {
        let not_found = management_error(PluginManagementError::NotFound("plugin:x".to_string()));
        assert_eq!(not_found.0, "plugin_not_found");
        assert!(not_found.1.contains("plugin:x"));

        let install = management_error(PluginManagementError::Install("bad".to_string()));
        assert_eq!(install.0, "plugin_install_failed");

        let ambiguous = management_error(PluginManagementError::Ambiguous(
            "p".to_string(),
            vec!["plugin:a".to_string()],
        ));
        assert_eq!(ambiguous.0, "plugin_selector_ambiguous");
    }

    #[tokio::test]
    async fn a_manage_request_without_a_target_is_refused() {
        let registry = empty_registry();
        let error = manage(
            &registry,
            &PluginManageRequestDto {
                session_id: "session".to_string(),
                action: PluginManageActionDto::Enable,
                target: "   ".to_string(),
            },
        )
        .await
        .expect_err("an empty target must be refused");
        assert_eq!(error.0, "plugin_target_required");
    }

    #[tokio::test]
    async fn the_catalogue_of_an_empty_registry_is_empty() {
        let registry = empty_registry();
        let entries = catalogue_entries(&registry, None).await;
        assert!(
            entries.is_empty(),
            "a registry with no plugins publishes no entries, saw {entries:?}"
        );
    }
}
