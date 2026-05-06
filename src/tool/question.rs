use crate::error::ToolError;
use crate::tool::Tool;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct QuestionInput {
    questions: Vec<QuestionSpec>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct QuestionSpec {
    question: String,
    #[serde(default)]
    options: Option<Vec<String>>,
    #[serde(default)]
    initial: Option<String>,
}

#[derive(Debug, Serialize)]
struct QuestionPending {
    __pending__: bool,
    questions: Vec<QuestionSpec>,
}

pub struct QuestionTool;

#[async_trait]
impl Tool for QuestionTool {
    fn name(&self) -> &str {
        "question"
    }

    fn description(&self) -> &str {
        "Ask the user one or more clarifying questions. Returns answers to continue the agent loop."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": {
                                "type": "string",
                                "description": "The question to ask"
                            },
                            "options": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Optional list of answer choices"
                            },
                            "initial": {
                                "type": "string",
                                "description": "Optional initial/default value"
                            }
                        },
                        "required": ["question"]
                    },
                    "description": "List of questions to ask the user"
                }
            },
            "required": ["questions"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed: QuestionInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid question input: {e}")))?;

        if parsed.questions.is_empty() {
            return Err(ToolError::Execution("no questions provided".to_string()));
        }

        let pending = QuestionPending {
            __pending__: true,
            questions: parsed.questions,
        };

        serde_json::to_string_pretty(&pending)
            .map_err(|e| ToolError::Execution(format!("failed to serialize: {e}")))
    }
}

pub fn parse_question_questions(input: serde_json::Value) -> Result<Vec<QuestionSpec>, ToolError> {
    let parsed: QuestionInput = serde_json::from_value(input)
        .map_err(|e| ToolError::Execution(format!("invalid question input: {e}")))?;
    Ok(parsed.questions)
}

pub fn format_question_answers(answers: &str) -> String {
    answers.to_string()
}
