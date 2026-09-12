//! Shared MCP protocol negotiation and bounded metadata helpers.

use serde_json::{json, Value};

use crate::error::McpError;
use crate::mcp::McpTool;
use crate::provider::ToolDefinition;

pub const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
pub const LEGACY_PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_DISCOVER_METHOD: &str = "server/discover";
pub const CLIENT_NAME: &str = "codegg";
pub const CLIENT_VERSION: &str = "0.1.0";

pub const MAX_MCP_DESCRIPTION_BYTES: usize = 16 * 1024;
pub const MAX_MCP_SCHEMA_BYTES: usize = 256 * 1024;
pub const MAX_MCP_METADATA_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegotiatedProtocol {
    Modern(String),
    Legacy(String),
}

impl NegotiatedProtocol {
    pub fn modern() -> Self {
        Self::Modern(MODERN_PROTOCOL_VERSION.to_string())
    }

    pub fn legacy() -> Self {
        Self::Legacy(LEGACY_PROTOCOL_VERSION.to_string())
    }

    pub fn version(&self) -> &str {
        match self {
            Self::Modern(version) | Self::Legacy(version) => version,
        }
    }

    pub fn is_modern(&self) -> bool {
        matches!(self, Self::Modern(_))
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveryInfo {
    pub protocol: Option<NegotiatedProtocol>,
    pub server_version: Option<String>,
    /// Bounded, untrusted integration metadata for diagnostics and identity.
    pub metadata: Option<Value>,
}

pub fn modern_discovery_params() -> Value {
    json!({
        "_meta": {
            "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION,
            "io.modelcontextprotocol/clientInfo": {
                "name": CLIENT_NAME,
                "version": CLIENT_VERSION
            },
            "io.modelcontextprotocol/clientCapabilities": {}
        }
    })
}

pub fn legacy_initialize_params() -> Value {
    json!({
        "protocolVersion": LEGACY_PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": {
            "name": CLIENT_NAME,
            "version": CLIENT_VERSION
        }
    })
}

pub fn modern_params(params: Value) -> Value {
    let mut params = match params {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    params.insert(
        "_meta".to_string(),
        json!({
            "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION,
            "io.modelcontextprotocol/clientInfo": {
                "name": CLIENT_NAME,
                "version": CLIENT_VERSION
            },
            "io.modelcontextprotocol/clientCapabilities": {}
        }),
    );
    Value::Object(params)
}

pub fn parse_discovery(result: &Value) -> DiscoveryInfo {
    let versions = result
        .get("supportedVersions")
        .and_then(Value::as_array)
        .map(|versions| {
            versions
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let protocol = if versions.iter().any(|v| v == MODERN_PROTOCOL_VERSION) {
        Some(NegotiatedProtocol::modern())
    } else if versions.iter().any(|v| v == LEGACY_PROTOCOL_VERSION) {
        Some(NegotiatedProtocol::legacy())
    } else {
        None
    };

    let server_version = result
        .pointer("/_meta/io.modelcontextprotocol~1serverInfo/version")
        .and_then(Value::as_str)
        .or_else(|| {
            result
                .pointer("/serverInfo/version")
                .and_then(Value::as_str)
        })
        .map(str::to_owned);

    DiscoveryInfo {
        protocol,
        server_version,
        metadata: bounded_value(result, MAX_MCP_METADATA_BYTES),
    }
}

pub fn legacy_protocol_from_initialize(result: &Value) -> NegotiatedProtocol {
    let version = result
        .get("protocolVersion")
        .and_then(Value::as_str)
        .filter(|version| !version.is_empty())
        .unwrap_or(LEGACY_PROTOCOL_VERSION);
    NegotiatedProtocol::Legacy(version.to_string())
}

/// A modern probe may be rejected by a legacy server with implementation-defined
/// method/parameter errors. Only those explicit protocol-shape errors qualify for
/// the legacy retry; transport, auth, timeout, malformed-response, and server
/// failures retain their original classification.
pub fn is_legacy_probe_error(error: &McpError) -> bool {
    let McpError::Server(message) = error else {
        return false;
    };
    let message = message.to_ascii_lowercase();
    if message.contains("http 401")
        || message.contains("http 403")
        || message.contains("http 408")
        || message.contains("http 429")
        || message.contains("http 500")
        || message.contains("http 502")
        || message.contains("http 503")
        || message.contains("http 504")
    {
        return false;
    }
    message.contains("-32601")
        || message.contains("-32602")
        || message.contains("method not found")
        || message.contains("unknown method")
        || message.contains("unsupported method")
        || message.contains("must initialize")
        || message.contains("not initialized")
        || message.contains("initialize first")
}

pub fn bounded_string(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

pub fn bounded_value(value: &Value, limit: usize) -> Option<Value> {
    let encoded = serde_json::to_vec(value).ok()?;
    (encoded.len() <= limit).then(|| value.clone())
}

pub fn bounded_schema(value: Value) -> Value {
    if bounded_value(&value, MAX_MCP_SCHEMA_BYTES).is_some() {
        value
    } else {
        json!({"type": "object", "properties": {}})
    }
}

pub fn bounded_optional_value(value: Option<&Value>, limit: usize) -> Option<Value> {
    value.and_then(|value| bounded_value(value, limit))
}

pub fn parse_tools(result: &Value, server: &str) -> Result<Vec<McpTool>, McpError> {
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| McpError::Server("invalid tools response".into()))?;

    Ok(tools
        .iter()
        .filter_map(|tool| {
            let name = tool.get("name")?.as_str()?.to_string();
            let description = tool
                .get("description")
                .and_then(Value::as_str)
                .map(|value| bounded_string(value, MAX_MCP_DESCRIPTION_BYTES))
                .unwrap_or_default();
            let input_schema = bounded_schema(
                tool.get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
            );
            Some(McpTool {
                name,
                description,
                input_schema,
                server: server.to_string(),
                output_schema: bounded_optional_value(
                    tool.get("outputSchema"),
                    MAX_MCP_SCHEMA_BYTES,
                ),
                annotations: bounded_optional_value(
                    tool.get("annotations"),
                    MAX_MCP_METADATA_BYTES,
                ),
                metadata: bounded_optional_value(tool.get("_meta"), MAX_MCP_METADATA_BYTES),
            })
        })
        .collect())
}

pub fn to_tool_definition(tool: &McpTool, server: &str) -> ToolDefinition {
    ToolDefinition {
        name: format!("mcp__{server}__{}", tool.name),
        description: tool.description.clone(),
        parameters: tool.input_schema.clone(),
        defer_loading: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_selects_modern_and_bounds_metadata() {
        let result = json!({
            "supportedVersions": [LEGACY_PROTOCOL_VERSION, MODERN_PROTOCOL_VERSION],
            "_meta": {
                "io.modelcontextprotocol/serverInfo": {
                    "name": "fixture",
                    "version": "1.2.3"
                }
            },
            "instructions": "use the tools"
        });
        let info = parse_discovery(&result);
        assert_eq!(info.protocol, Some(NegotiatedProtocol::modern()));
        assert_eq!(info.server_version.as_deref(), Some("1.2.3"));
        assert!(info.metadata.is_some());
    }

    #[test]
    fn legacy_probe_classifier_does_not_mask_transport_or_auth_errors() {
        assert!(is_legacy_probe_error(&McpError::Server(
            "method not found (code -32601)".into()
        )));
        assert!(!is_legacy_probe_error(&McpError::Connection(
            "closed".into()
        )));
        assert!(!is_legacy_probe_error(&McpError::Timeout("slow".into())));
        assert!(!is_legacy_probe_error(&McpError::Server(
            "HTTP 401: denied".into()
        )));
    }

    #[test]
    fn tool_metadata_is_preserved_and_oversized_metadata_is_dropped() {
        let result = json!({
            "tools": [{
                "name": "search",
                "description": "search",
                "inputSchema": {"type": "object"},
                "outputSchema": {"type": "object", "properties": {"answer": {"type": "string"}}},
                "annotations": {"readOnlyHint": true},
                "_meta": {"origin": "fixture"}
            }]
        });
        let tools = parse_tools(&result, "fixture").expect("valid tool list");
        assert_eq!(tools[0].server, "fixture");
        assert!(tools[0].output_schema.is_some());
        assert_eq!(tools[0].annotations, Some(json!({"readOnlyHint": true})));
        assert_eq!(tools[0].metadata, Some(json!({"origin": "fixture"})));

        let oversized = json!({"x": "x".repeat(MAX_MCP_METADATA_BYTES)});
        let result = json!({"tools": [{"name": "large", "_meta": oversized}]});
        let tools = parse_tools(&result, "fixture").expect("valid tool list");
        assert!(tools[0].metadata.is_none());
    }
}
