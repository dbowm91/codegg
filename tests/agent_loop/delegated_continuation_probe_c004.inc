//! C004: the task tool advertises the continuation capability.
//!
//! A capability the model cannot see is a capability the model cannot use.
//! The `task` tool's action enum and its description are the only places the
//! continuation is offered to the model, so they are asserted directly.

use codegg::tool::task::TaskTool;
use codegg::tool::Tool;

fn task_tool() -> TaskTool {
    TaskTool::new(
        std::sync::Arc::new(tokio::sync::Mutex::new(codegg::tool::task::TaskStore::new())),
        None,
        Some("sess-c004-probe".to_string()),
        Vec::new(),
    )
}

#[tokio::test]
async fn the_task_tool_advertises_the_continuation_capability() {
    let tool = task_tool();
    let schema = tool.parameters();
    let actions = schema["properties"]["action"]["enum"]
        .as_array()
        .expect("the action enum is an array");
    assert!(
        actions
            .iter()
            .any(|action| action.as_str() == Some("continue")),
        "the action enum must offer the continuation: {actions:?}"
    );
}

#[tokio::test]
async fn the_task_tool_description_states_the_continuations_bound_and_warrant() {
    let tool = task_tool();
    let description = tool.description();
    assert!(
        description.contains("action=continue"),
        "the description must name the continuation action: {description}"
    );
    assert!(
        description.contains("one continuation per terminal failure"),
        "the description must state the bound: {description}"
    );
    assert!(
        description.contains("cannot widen"),
        "the description must state that the ceiling cannot be widened: {description}"
    );
}
