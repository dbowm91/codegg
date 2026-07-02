use crate::tui::app::state::diagnostics::TuiDiagnostics;
use codegg_protocol::ui::{
    ContainerNode, KeyValueEntry, KeyValueNode, TableNode, TextNode, UiNode,
};

pub struct TaskSummaryView {
    pub active: usize,
    pub completed: u64,
    pub cancelled: u64,
    pub panicked: u64,
    pub by_kind: Vec<(String, usize)>,
    pub oldest: Option<String>,
}

pub fn stats_node(
    diagnostics: &TuiDiagnostics,
    task_summary: &TaskSummaryView,
    shell_handles_count: usize,
) -> UiNode {
    let children = vec![
        app_state_section(diagnostics),
        recent_events_section(diagnostics),
        tasks_section(task_summary),
        background_activity_section(shell_handles_count, &task_summary.oldest),
    ];

    UiNode::Container(ContainerNode {
        title: Some("TUI Stats".into()),
        children,
    })
}

fn app_state_section(d: &TuiDiagnostics) -> UiNode {
    UiNode::Container(ContainerNode {
        title: Some("App State".into()),
        children: vec![UiNode::KeyValue(KeyValueNode {
            entries: vec![
                KeyValueEntry {
                    key: "Slow loops".into(),
                    value: d.slow_loop_count.to_string(),
                },
                KeyValueEntry {
                    key: "Slow renders".into(),
                    value: d.slow_render_count.to_string(),
                },
                KeyValueEntry {
                    key: "Slow commands".into(),
                    value: d.slow_command_count.to_string(),
                },
                KeyValueEntry {
                    key: "Dropped events".into(),
                    value: d.dropped_bus_events.to_string(),
                },
                KeyValueEntry {
                    key: "Render panics".into(),
                    value: d.render_panic_count.to_string(),
                },
                KeyValueEntry {
                    key: "Component panics".into(),
                    value: d.component_render_panic_count.to_string(),
                },
            ],
        })],
    })
}

fn recent_events_section(d: &TuiDiagnostics) -> UiNode {
    let mut children: Vec<UiNode> = Vec::new();

    if let Some(ref err) = d.last_render_error {
        children.push(UiNode::KeyValue(KeyValueNode {
            entries: vec![KeyValueEntry {
                key: "Last render error".into(),
                value: err.clone(),
            }],
        }));
    }
    if let Some(ref rec) = d.last_slow_loop {
        children.push(UiNode::KeyValue(KeyValueNode {
            entries: vec![KeyValueEntry {
                key: "Last slow loop".into(),
                value: format!("{}ms ago", rec.timestamp.elapsed().as_millis()),
            }],
        }));
    }
    if let Some(cmd) = d.recent_slow_commands.back() {
        children.push(UiNode::KeyValue(KeyValueNode {
            entries: vec![KeyValueEntry {
                key: "Last slow command".into(),
                value: format!("'{}' took {}ms", cmd.name, cmd.elapsed.as_millis()),
            }],
        }));
    }
    if let Some(panic_rec) = d.recent_component_render_panics.back() {
        children.push(UiNode::KeyValue(KeyValueNode {
            entries: vec![KeyValueEntry {
                key: "Last component panic".into(),
                value: format!(
                    "'{}' {}ms ago",
                    panic_rec.component,
                    panic_rec.timestamp.elapsed().as_millis()
                ),
            }],
        }));
    }

    if children.is_empty() {
        children.push(UiNode::Text(TextNode {
            text: "(no recent events)".into(),
        }));
    }

    UiNode::Container(ContainerNode {
        title: Some("Recent Events".into()),
        children,
    })
}

fn tasks_section(ts: &TaskSummaryView) -> UiNode {
    let mut children: Vec<UiNode> = Vec::new();

    children.push(UiNode::KeyValue(KeyValueNode {
        entries: vec![
            KeyValueEntry {
                key: "Active".into(),
                value: ts.active.to_string(),
            },
            KeyValueEntry {
                key: "Completed".into(),
                value: ts.completed.to_string(),
            },
            KeyValueEntry {
                key: "Cancelled".into(),
                value: ts.cancelled.to_string(),
            },
            KeyValueEntry {
                key: "Panicked".into(),
                value: ts.panicked.to_string(),
            },
        ],
    }));

    if !ts.by_kind.is_empty() {
        let mut rows: Vec<Vec<String>> = ts
            .by_kind
            .iter()
            .map(|(kind, count)| vec![kind.clone(), count.to_string()])
            .collect();
        rows.sort_by(|a, b| b[1].cmp(&a[1]));
        children.push(UiNode::Table(TableNode {
            columns: vec!["Kind".into(), "Count".into()],
            rows,
        }));
    }

    UiNode::Container(ContainerNode {
        title: Some("Tasks".into()),
        children,
    })
}

fn background_activity_section(shell_handles_count: usize, oldest: &Option<String>) -> UiNode {
    let mut entries = vec![KeyValueEntry {
        key: "Shell handles".into(),
        value: shell_handles_count.to_string(),
    }];
    if let Some(ref name) = oldest {
        entries.push(KeyValueEntry {
            key: "Oldest task".into(),
            value: name.clone(),
        });
    }

    UiNode::Container(ContainerNode {
        title: Some("Background Activity".into()),
        children: vec![UiNode::KeyValue(KeyValueNode { entries })],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::components::ui_node_renderer::UiNodeRenderer;

    fn default_diag() -> TuiDiagnostics {
        TuiDiagnostics::default()
    }

    fn default_tasks() -> TaskSummaryView {
        TaskSummaryView {
            active: 0,
            completed: 0,
            cancelled: 0,
            panicked: 0,
            by_kind: Vec::new(),
            oldest: None,
        }
    }

    #[test]
    fn test_stats_node_default_includes_all_sections() {
        let node = stats_node(&default_diag(), &default_tasks(), 0);
        let lines = UiNodeRenderer::node_to_lines(&node);
        let text = lines.join("\n");
        assert!(text.contains("TUI Stats"), "missing title: {text}");
        assert!(text.contains("App State"), "missing app state: {text}");
        assert!(
            text.contains("Recent Events"),
            "missing recent events: {text}"
        );
        assert!(text.contains("Tasks"), "missing tasks: {text}");
        assert!(
            text.contains("Background Activity"),
            "missing background activity: {text}"
        );
    }

    #[test]
    fn test_stats_node_with_data_includes_recent_events() {
        let mut d = default_diag();
        d.slow_loop_count = 3;
        d.render_panic_count = 1;
        d.last_render_error = Some("crash".into());
        d.record_slow_command("test_cmd", std::time::Duration::from_millis(500));

        let node = stats_node(&d, &default_tasks(), 0);
        let lines = UiNodeRenderer::node_to_lines(&node);
        let text = lines.join("\n");
        assert!(text.contains("crash"), "should show render error: {text}");
        assert!(
            text.contains("test_cmd"),
            "should show slow command: {text}"
        );
    }

    #[test]
    fn test_stats_node_includes_task_summary() {
        let ts = TaskSummaryView {
            active: 2,
            completed: 10,
            cancelled: 1,
            panicked: 0,
            by_kind: vec![("Command".into(), 1), ("Research".into(), 1)],
            oldest: Some("cmd1".into()),
        };
        let node = stats_node(&default_diag(), &ts, 0);
        let lines = UiNodeRenderer::node_to_lines(&node);
        let text = lines.join("\n");
        assert!(text.contains("Active"), "missing active: {text}");
        assert!(text.contains("Completed"), "missing completed: {text}");
        assert!(text.contains("Cancelled"), "missing cancelled: {text}");
        assert!(text.contains(": 2"), "missing active value: {text}");
        assert!(text.contains(": 10"), "missing completed value: {text}");
        assert!(text.contains("Command"), "missing by-kind: {text}");
        assert!(text.contains("cmd1"), "missing oldest: {text}");
    }

    #[test]
    fn test_stats_node_empty_by_kind_omits_table() {
        let ts = TaskSummaryView {
            active: 1,
            completed: 5,
            cancelled: 0,
            panicked: 0,
            by_kind: Vec::new(),
            oldest: None,
        };
        let node = stats_node(&default_diag(), &ts, 0);
        let lines = UiNodeRenderer::node_to_lines(&node);
        let text = lines.join("\n");
        assert!(!text.contains("Kind"), "should omit table header: {text}");
        assert!(!text.contains("Count"), "should omit count column: {text}");
    }

    #[test]
    fn test_stats_node_zero_shell_handles_shows_zero() {
        let node = stats_node(&default_diag(), &default_tasks(), 0);
        let lines = UiNodeRenderer::node_to_lines(&node);
        let text = lines.join("\n");
        assert!(
            text.contains("Shell handles: 0"),
            "should show zero handles: {text}"
        );
    }

    #[test]
    fn test_stats_node_renders_to_lines_without_panic() {
        let mut d = default_diag();
        d.slow_loop_count = 5;
        d.slow_render_count = 3;
        d.slow_command_count = 7;
        d.dropped_bus_events = 2;
        d.render_panic_count = 1;
        d.component_render_panic_count = 1;
        d.last_render_error = Some("test error".into());

        let ts = TaskSummaryView {
            active: 3,
            completed: 42,
            cancelled: 5,
            panicked: 0,
            by_kind: vec![("Command".into(), 2), ("Research".into(), 1)],
            oldest: Some("oldest_task".into()),
        };
        let node = stats_node(&d, &ts, 4);
        let lines = UiNodeRenderer::node_to_lines(&node);
        assert!(!lines.is_empty(), "should produce output lines");
    }
}
