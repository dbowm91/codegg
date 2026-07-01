use crate::protocol::ui::{
    CodeNode, ContainerNode, KeyValueEntry, KeyValueNode, MarkdownNode, ProgressNode, TableNode,
    TextNode, UiNode,
};

use super::management::{PluginDoctorReport, PluginManagementView};

/// Build a table node listing all plugins.
///
/// Columns: ID, Name, Version, Runtime, Trust, Enabled, Commands, Hooks,
/// Panels, Widgets, Events
pub fn plugins_table(plugins: &[PluginManagementView]) -> UiNode {
    let columns = vec![
        "ID".to_string(),
        "Name".to_string(),
        "Version".to_string(),
        "Runtime".to_string(),
        "Trust".to_string(),
        "Enabled".to_string(),
        "Commands".to_string(),
        "Hooks".to_string(),
        "Panels".to_string(),
        "Widgets".to_string(),
        "Events".to_string(),
    ];

    let rows = plugins
        .iter()
        .map(|p| {
            vec![
                p.id.clone(),
                p.name.clone(),
                p.version.clone(),
                p.runtime_kind.clone(),
                format!("{:?}", p.trust),
                if p.enabled { "yes" } else { "no" }.to_string(),
                p.command_count.to_string(),
                p.hook_count.to_string(),
                p.panel_count.to_string(),
                p.status_widget_count.to_string(),
                p.event_subscription_count.to_string(),
            ]
        })
        .collect();

    UiNode::Table(TableNode { columns, rows })
}

/// Build a detailed info node for a single plugin.
///
/// Returns a `UiNode::Container` with a title and a `UiNode::KeyValue`
/// holding all plugin fields grouped logically.
pub fn plugin_info_node(plugin: &PluginManagementView) -> UiNode {
    let mut entries = Vec::new();

    // Identity
    entries.push(KeyValueEntry {
        key: "ID".to_string(),
        value: plugin.id.clone(),
    });
    entries.push(KeyValueEntry {
        key: "Name".to_string(),
        value: plugin.name.clone(),
    });
    entries.push(KeyValueEntry {
        key: "Version".to_string(),
        value: plugin.version.clone(),
    });
    entries.push(KeyValueEntry {
        key: "API Version".to_string(),
        value: plugin.api_version.to_string(),
    });
    entries.push(KeyValueEntry {
        key: "Description".to_string(),
        value: plugin
            .description
            .clone()
            .unwrap_or_else(|| "(none)".to_string()),
    });

    // Runtime & trust
    entries.push(KeyValueEntry {
        key: "Runtime".to_string(),
        value: plugin.runtime_kind.clone(),
    });
    entries.push(KeyValueEntry {
        key: "Trust".to_string(),
        value: format!("{:?}", plugin.trust),
    });
    entries.push(KeyValueEntry {
        key: "Enabled".to_string(),
        value: if plugin.enabled { "yes" } else { "no" }.to_string(),
    });

    // Source
    entries.push(KeyValueEntry {
        key: "Source Path".to_string(),
        value: plugin
            .source_path
            .clone()
            .unwrap_or_else(|| "(unknown)".to_string()),
    });

    // Capabilities
    entries.push(KeyValueEntry {
        key: "Commands".to_string(),
        value: plugin.command_count.to_string(),
    });
    entries.push(KeyValueEntry {
        key: "Hooks".to_string(),
        value: plugin.hook_count.to_string(),
    });
    entries.push(KeyValueEntry {
        key: "Panels".to_string(),
        value: plugin.panel_count.to_string(),
    });
    entries.push(KeyValueEntry {
        key: "Widgets".to_string(),
        value: plugin.status_widget_count.to_string(),
    });
    entries.push(KeyValueEntry {
        key: "Events".to_string(),
        value: plugin.event_subscription_count.to_string(),
    });

    // Permissions & diagnostics
    entries.push(KeyValueEntry {
        key: "Permissions".to_string(),
        value: plugin.permissions_summary.clone(),
    });
    entries.push(KeyValueEntry {
        key: "Diagnostics".to_string(),
        value: plugin.diagnostic_count.to_string(),
    });

    UiNode::Container(ContainerNode {
        title: Some(format!("Plugin: {}", plugin.name)),
        children: vec![UiNode::KeyValue(KeyValueNode { entries })],
    })
}

/// Build a doctor report node.
///
/// Returns a `UiNode::Container` with the report title and check results
/// rendered as text lines.
pub fn doctor_report_node(report: &PluginDoctorReport) -> UiNode {
    let mut children = Vec::new();

    let summary = format!(
        "{} checks, {} passed, {} failed",
        report.checks.len(),
        report.checks.iter().filter(|c| c.passed).count(),
        report.checks.iter().filter(|c| !c.passed).count(),
    );
    children.push(UiNode::Text(TextNode { text: summary }));

    for check in &report.checks {
        let icon = if check.passed { "PASS" } else { "FAIL" };
        children.push(UiNode::Text(TextNode {
            text: format!("[{}] {}: {}", icon, check.name, check.message),
        }));
    }

    UiNode::Container(ContainerNode {
        title: Some(format!("Plugin Doctor: {}", report.plugin_name)),
        children,
    })
}

/// Convert a [`UiNode`] tree into a flat list of text lines for text-based
/// surfaces (info dialogs, log lines, etc.).
///
/// This is the textual fallback for environments that do not have a full
/// ratatui renderer. It is deterministic and stable: the same `UiNode`
/// always produces the same line list.
///
/// Supported variants: `Text`, `Markdown`, `Code`, `KeyValue`, `Table`,
/// `Progress`, `Container`, `Empty`. `Unsupported` falls back to a labeled
/// placeholder.
pub fn node_to_lines(node: &UiNode) -> Vec<String> {
    let mut out = Vec::new();
    render_node_lines(node, &mut out, 0);
    out
}

fn render_node_lines(node: &UiNode, out: &mut Vec<String>, depth: usize) {
    let indent = "  ".repeat(depth);
    match node {
        UiNode::Text(TextNode { text }) => {
            if text.is_empty() {
                out.push(String::new());
            } else {
                for line in text.lines() {
                    out.push(format!("{indent}{line}"));
                }
            }
        }
        UiNode::Markdown(MarkdownNode { markdown }) => {
            if markdown.is_empty() {
                out.push(String::new());
            } else {
                for line in markdown.lines() {
                    out.push(format!("{indent}{line}"));
                }
            }
        }
        UiNode::Code(CodeNode { language, code }) => {
            let lang = language.as_deref().unwrap_or("");
            out.push(format!("{indent}```{lang}"));
            for line in code.lines() {
                out.push(format!("{indent}{line}"));
            }
            out.push(format!("{indent}```"));
        }
        UiNode::KeyValue(KeyValueNode { entries }) => {
            for entry in entries {
                out.push(format!("{indent}{}: {}", entry.key, entry.value));
            }
        }
        UiNode::Table(TableNode { columns, rows }) => {
            if !columns.is_empty() {
                out.push(format!("{indent}{}", columns.join(" | ")));
                out.push(format!(
                    "{indent}{}",
                    columns
                        .iter()
                        .map(|c| "-".repeat(c.len().max(3)))
                        .collect::<Vec<_>>()
                        .join("-+-")
                ));
            }
            for row in rows {
                out.push(format!("{indent}{}", row.join(" | ")));
            }
        }
        UiNode::Progress(ProgressNode {
            label,
            current,
            total,
        }) => {
            let label_part = label.as_deref().unwrap_or("Progress");
            let total_part = total
                .map(|t| format!("/{}", t))
                .unwrap_or_default();
            out.push(format!("{indent}{label_part}: {current}{total_part}"));
        }
        UiNode::Container(ContainerNode { title, children }) => {
            if let Some(t) = title {
                out.push(format!("{indent}== {t} =="));
            }
            for child in children {
                render_node_lines(child, out, depth + 1);
            }
        }
        UiNode::Empty => {}
        UiNode::Unsupported { unknown_kind, .. } => {
            out.push(format!("{indent}[unsupported: {unknown_kind}]"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::management::PluginDoctorCheck;
    use crate::plugin::manifest::PluginTrustClass;

    fn sample_view(id: &str, name: &str) -> PluginManagementView {
        PluginManagementView {
            id: id.to_string(),
            name: name.to_string(),
            version: "1.0.0".to_string(),
            api_version: 1,
            enabled: true,
            runtime_kind: "builtin".to_string(),
            trust: PluginTrustClass::Builtin,
            source_path: None,
            command_count: 2,
            hook_count: 3,
            panel_count: 1,
            status_widget_count: 0,
            event_subscription_count: 1,
            permissions_summary: "none".to_string(),
            diagnostic_count: 0,
            description: Some("A test plugin".to_string()),
        }
    }

    fn sample_report(name: &str, checks: Vec<PluginDoctorCheck>) -> PluginDoctorReport {
        PluginDoctorCheck {
            name: "check1".to_string(),
            passed: true,
            message: "ok".to_string(),
        };
        PluginDoctorReport {
            plugin_id: "test:1".to_string(),
            plugin_name: name.to_string(),
            checks,
        }
    }

    #[test]
    fn plugins_table_includes_all_plugins() {
        let plugins = vec![
            sample_view("a:1", "alpha"),
            sample_view("b:1", "beta"),
            sample_view("c:1", "gamma"),
        ];
        let node = plugins_table(&plugins);
        match &node {
            UiNode::Table(t) => {
                assert_eq!(t.columns.len(), 11);
                assert_eq!(t.rows.len(), 3);
                assert_eq!(t.rows[0][0], "a:1");
                assert_eq!(t.rows[0][1], "alpha");
                assert_eq!(t.rows[1][0], "b:1");
                assert_eq!(t.rows[2][0], "c:1");
            }
            other => panic!("expected Table, got {:?}", other),
        }
    }

    #[test]
    fn plugins_table_empty() {
        let node = plugins_table(&[]);
        match &node {
            UiNode::Table(t) => {
                assert_eq!(t.columns.len(), 11);
                assert!(t.rows.is_empty());
            }
            other => panic!("expected Table, got {:?}", other),
        }
    }

    #[test]
    fn plugin_info_node_shows_all_fields() {
        let view = sample_view("test:1", "TestPlugin");
        let node = plugin_info_node(&view);
        match &node {
            UiNode::Container(c) => {
                assert_eq!(c.title.as_deref(), Some("Plugin: TestPlugin"));
                assert_eq!(c.children.len(), 1);
                match &c.children[0] {
                    UiNode::KeyValue(kv) => {
                        assert!(kv.entries.len() >= 15);
                        let keys: Vec<&str> = kv.entries.iter().map(|e| e.key.as_str()).collect();
                        assert!(keys.contains(&"ID"));
                        assert!(keys.contains(&"Name"));
                        assert!(keys.contains(&"Version"));
                        assert!(keys.contains(&"API Version"));
                        assert!(keys.contains(&"Description"));
                        assert!(keys.contains(&"Runtime"));
                        assert!(keys.contains(&"Trust"));
                        assert!(keys.contains(&"Enabled"));
                        assert!(keys.contains(&"Source Path"));
                        assert!(keys.contains(&"Commands"));
                        assert!(keys.contains(&"Hooks"));
                        assert!(keys.contains(&"Panels"));
                        assert!(keys.contains(&"Widgets"));
                        assert!(keys.contains(&"Events"));
                        assert!(keys.contains(&"Permissions"));
                        assert!(keys.contains(&"Diagnostics"));
                    }
                    other => panic!("expected KeyValue, got {:?}", other),
                }
            }
            other => panic!("expected Container, got {:?}", other),
        }
    }

    #[test]
    fn plugin_info_node_no_description_shows_none() {
        let mut view = sample_view("test:1", "TestPlugin");
        view.description = None;
        let node = plugin_info_node(&view);
        match &node {
            UiNode::Container(c) => match &c.children[0] {
                UiNode::KeyValue(kv) => {
                    let desc = kv.entries.iter().find(|e| e.key == "Description").unwrap();
                    assert_eq!(desc.value, "(none)");
                }
                other => panic!("expected KeyValue, got {:?}", other),
            },
            other => panic!("expected Container, got {:?}", other),
        }
    }

    #[test]
    fn doctor_report_node_shows_pass_fail() {
        let checks = vec![
            PluginDoctorCheck {
                name: "check_pass".to_string(),
                passed: true,
                message: "all good".to_string(),
            },
            PluginDoctorCheck {
                name: "check_fail".to_string(),
                passed: false,
                message: "something wrong".to_string(),
            },
            PluginDoctorCheck {
                name: "check_pass2".to_string(),
                passed: true,
                message: "fine".to_string(),
            },
        ];
        let report = sample_report("MyPlugin", checks);
        let node = doctor_report_node(&report);
        match &node {
            UiNode::Container(c) => {
                assert_eq!(c.title.as_deref(), Some("Plugin Doctor: MyPlugin"));
                // 1 summary line + 3 check lines = 4 children
                assert_eq!(c.children.len(), 4);
                match &c.children[0] {
                    UiNode::Text(t) => {
                        assert!(t.text.contains("3 checks"));
                        assert!(t.text.contains("2 passed"));
                        assert!(t.text.contains("1 failed"));
                    }
                    other => panic!("expected Text, got {:?}", other),
                }
                // Check pass/fail rendering
                match &c.children[1] {
                    UiNode::Text(t) => assert!(t.text.contains("[PASS]")),
                    other => panic!("expected Text, got {:?}", other),
                }
                match &c.children[2] {
                    UiNode::Text(t) => assert!(t.text.contains("[FAIL]")),
                    other => panic!("expected Text, got {:?}", other),
                }
            }
            other => panic!("expected Container, got {:?}", other),
        }
    }

    #[test]
    fn doctor_report_all_passing() {
        let checks = vec![
            PluginDoctorCheck {
                name: "a".to_string(),
                passed: true,
                message: "ok".to_string(),
            },
            PluginDoctorCheck {
                name: "b".to_string(),
                passed: true,
                message: "ok".to_string(),
            },
        ];
        let report = sample_report("GoodPlugin", checks);
        let node = doctor_report_node(&report);
        match &node {
            UiNode::Container(c) => match &c.children[0] {
                UiNode::Text(t) => {
                    assert!(t.text.contains("0 failed"));
                }
                other => panic!("expected Text, got {:?}", other),
            },
            other => panic!("expected Container, got {:?}", other),
        }
    }

    #[test]
    fn node_to_lines_text() {
        let node = UiNode::Text(TextNode {
            text: "hello\nworld".to_string(),
        });
        let lines = node_to_lines(&node);
        assert_eq!(lines, vec!["hello", "world"]);
    }

    #[test]
    fn node_to_lines_keyvalue() {
        let node = UiNode::KeyValue(KeyValueNode {
            entries: vec![
                KeyValueEntry {
                    key: "Name".into(),
                    value: "Test".into(),
                },
                KeyValueEntry {
                    key: "Version".into(),
                    value: "1.0.0".into(),
                },
            ],
        });
        let lines = node_to_lines(&node);
        assert_eq!(lines, vec!["Name: Test", "Version: 1.0.0"]);
    }

    #[test]
    fn node_to_lines_table() {
        let node = UiNode::Table(TableNode {
            columns: vec!["A".into(), "B".into()],
            rows: vec![vec!["1".into(), "2".into()], vec!["3".into(), "4".into()]],
        });
        let lines = node_to_lines(&node);
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("A | B"));
        assert!(lines[2].contains("1 | 2"));
    }

    #[test]
    fn node_to_lines_container_nested() {
        let node = UiNode::Container(ContainerNode {
            title: Some("Outer".to_string()),
            children: vec![UiNode::Text(TextNode {
                text: "inner".to_string(),
            })],
        });
        let lines = node_to_lines(&node);
        assert!(lines[0].contains("== Outer =="));
        assert!(lines[1].contains("inner"));
    }

    #[test]
    fn node_to_lines_empty() {
        let node = UiNode::Empty;
        let lines = node_to_lines(&node);
        assert!(lines.is_empty());
    }

    #[test]
    fn plugins_table_node_renders_to_lines() {
        let plugins = vec![sample_view("a:1", "alpha")];
        let node = plugins_table(&plugins);
        let lines = node_to_lines(&node);
        assert!(!lines.is_empty());
        assert!(lines.iter().any(|l| l.contains("a:1")));
        assert!(lines.iter().any(|l| l.contains("alpha")));
    }

    #[test]
    fn doctor_report_node_renders_to_lines() {
        let checks = vec![PluginDoctorCheck {
            name: "x".into(),
            passed: true,
            message: "ok".into(),
        }];
        let report = sample_report("Test", checks);
        let node = doctor_report_node(&report);
        let lines = node_to_lines(&node);
        assert!(lines.iter().any(|l| l.contains("== Plugin Doctor: Test ==")));
        assert!(lines.iter().any(|l| l.contains("[PASS]")));
    }

    #[test]
    fn plugin_info_node_renders_to_lines() {
        let view = sample_view("test:1", "TestPlugin");
        let node = plugin_info_node(&view);
        let lines = node_to_lines(&node);
        assert!(lines.iter().any(|l| l.contains("== Plugin: TestPlugin ==")));
        assert!(lines.iter().any(|l| l.contains("ID: test:1")));
    }
}
