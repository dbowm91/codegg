use codegg_plugin_sdk::builders::*;
use codegg_plugin_sdk::codegg_plugin;
use codegg_protocol::plugin::{PluginCapabilityInvocation, PluginInvocation, PluginResponse};

fn handle(inv: PluginInvocation) -> PluginResponse {
    let is_event = matches!(&inv.capability, PluginCapabilityInvocation::Event { .. });
    if !is_event {
        return ok_response(vec![], serde_json::Value::Null);
    }

    let event_type = match &inv.capability {
        PluginCapabilityInvocation::Event { event_type } => event_type.clone(),
        _ => String::new(),
    };

    PluginResponse {
        ok: true,
        effects: vec![],
        data: serde_json::json!({ "observed_event": event_type }),
        diagnostics: vec![diagnostic(
            PluginDiagnosticLevel::Debug,
            format!("observed event: {}", event_type),
        )],
    }
}

codegg_plugin!(handle);
