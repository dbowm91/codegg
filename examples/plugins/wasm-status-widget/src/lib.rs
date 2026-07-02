use codegg_plugin_sdk::builders::*;
use codegg_plugin_sdk::codegg_plugin;
use codegg_protocol::plugin::{PluginCapabilityInvocation, PluginInvocation, PluginResponse};
use codegg_protocol::ui::{PanelPlacement, StatusItemSpec, StatusPlacement, UiEffect, UiNode};

fn handle(inv: PluginInvocation) -> PluginResponse {
    match &inv.capability {
        PluginCapabilityInvocation::Panel { .. } => {
            let project = inv.context.project_dir.clone().unwrap_or_default();
            let model = inv.context.model.clone().unwrap_or_default();
            response_panel(
                "system-info",
                "System",
                PanelPlacement::Left,
                key_value_node(vec![("Project".into(), project), ("Model".into(), model)]),
            )
        }
        PluginCapabilityInvocation::StatusWidget { .. } => {
            let project = inv.context.project_dir.clone().unwrap_or_default();
            PluginResponse {
                ok: true,
                effects: vec![UiEffect::AddStatusItem {
                    item: StatusItemSpec {
                        id: "project-name".into(),
                        label: Some("proj".into()),
                        placement: StatusPlacement::Right,
                        body: UiNode::Text(TextNode { text: project }),
                    },
                }],
                data: serde_json::Value::Null,
                diagnostics: vec![],
            }
        }
        _ => ok_response(vec![], serde_json::Value::Null),
    }
}

codegg_plugin!(handle);
