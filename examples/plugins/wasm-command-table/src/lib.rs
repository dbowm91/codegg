use codegg_plugin_sdk::builders::*;
use codegg_plugin_sdk::codegg_plugin;
use codegg_protocol::plugin::{PluginInvocation, PluginResponse};

fn handle(_inv: PluginInvocation) -> PluginResponse {
    let columns = vec!["Plugin".into(), "Version".into(), "Status".into()];
    let rows = vec![
        vec!["wasm-command-table".into(), "0.1.0".into(), "active".into()],
        vec!["process-quota-text".into(), "0.1.0".into(), "active".into()],
        vec!["process-quota-json".into(), "0.1.0".into(), "active".into()],
    ];

    response_dialog(
        "wasm-table-results",
        "Loaded Plugins",
        table_node(columns, rows),
        true,
    )
}

codegg_plugin!(handle);
