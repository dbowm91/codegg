use crate::provider::ToolDefinition;

use super::artifact::stable_hash_hex;

pub fn tool_definitions_hash(defs: &[ToolDefinition]) -> String {
    let mut sorted: Vec<&ToolDefinition> = defs.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    // Build canonical string form then feed to stable sha256 (full 64-hex).
    // This replaces the DefaultHasher path entirely.
    let mut buf = String::new();
    for def in sorted {
        buf.push_str(&def.name);
        buf.push(':');
        buf.push_str(&def.description);
        buf.push(':');

        let canonical = canonicalize_json(&def.parameters);
        buf.push_str(&canonical);
        buf.push(':');

        if let Some(d) = def.defer_loading {
            buf.push_str(&d.to_string());
        }
        buf.push(';');
    }

    stable_hash_hex(buf)
}

fn canonicalize_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted: Vec<_> = map.iter().collect();
            sorted.sort_by_key(|(k, _)| (*k).clone());
            let mut parts = Vec::new();
            for (k, v) in sorted {
                parts.push(format!("{}:{}", k, canonicalize_json(v)));
            }
            format!("{{{}}}", parts.join(","))
        }
        serde_json::Value::Array(arr) => {
            let inner: Vec<_> = arr.iter().map(canonicalize_json).collect();
            format!("[{}]", inner.join(","))
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str, desc: &str, params: serde_json::Value) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: desc.to_string(),
            parameters: params,
            defer_loading: None,
        }
    }

    #[test]
    fn same_definitions_produce_same_hash() {
        let defs = vec![
            tool("bash", "Run bash", json!({})),
            tool("read", "Read file", json!({})),
        ];
        assert_eq!(tool_definitions_hash(&defs), tool_definitions_hash(&defs));
    }

    #[test]
    fn description_change_alters_hash() {
        let a = vec![tool("bash", "Run bash", json!({}))];
        let b = vec![tool("bash", "Execute bash", json!({}))];
        assert_ne!(tool_definitions_hash(&a), tool_definitions_hash(&b));
    }

    #[test]
    fn reordering_does_not_change_hash() {
        let a = vec![
            tool("bash", "Run bash", json!({})),
            tool("read", "Read file", json!({})),
        ];
        let b = vec![
            tool("read", "Read file", json!({})),
            tool("bash", "Run bash", json!({})),
        ];
        assert_eq!(tool_definitions_hash(&a), tool_definitions_hash(&b));
    }

    #[test]
    fn parameter_change_alters_hash() {
        let a = vec![tool("bash", "Run bash", json!({"type": "object"}))];
        let b = vec![tool("bash", "Run bash", json!({"type": "string"}))];
        assert_ne!(tool_definitions_hash(&a), tool_definitions_hash(&b));
    }

    #[test]
    fn defer_loading_change_alters_hash() {
        let mut a = tool("bash", "Run bash", json!({}));
        a.defer_loading = Some(false);
        let mut b = tool("bash", "Run bash", json!({}));
        b.defer_loading = Some(true);
        assert_ne!(tool_definitions_hash(&[a]), tool_definitions_hash(&[b]));
    }

    #[test]
    fn empty_definitions_produce_hash() {
        let hash = tool_definitions_hash(&[]);
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn json_key_order_insensitive() {
        let a = vec![tool("bash", "Run", json!({"a": 1, "b": 2}))];
        let b = vec![tool("bash", "Run", json!({"b": 2, "a": 1}))];
        assert_eq!(tool_definitions_hash(&a), tool_definitions_hash(&b));
    }
}
