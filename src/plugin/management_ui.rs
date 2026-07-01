use crate::protocol::ui::{
    ContainerNode, KeyValueEntry, KeyValueNode, TableNode, TextNode, UiNode,
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
}
