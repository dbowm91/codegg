use crate::error::ToolError;
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Deserialize)]
struct QuestionInput {
    questions: Vec<QuestionSpec>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct QuestionSpec {
    question: String,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_options")]
    options: Option<Vec<String>>,
    #[serde(default)]
    initial: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum QuestionInputCompat {
    Questions {
        questions: Vec<QuestionSpec>,
    },
    SingleQuestion {
        question: String,
        #[serde(default, deserialize_with = "deserialize_options")]
        options: Option<Vec<String>>,
        #[serde(default)]
        initial: Option<String>,
    },
    QuestionList(Vec<QuestionSpec>),
}

fn deserialize_options<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    enum OptionCompat {
        Text(String),
        Object {
            label: Option<String>,
            value: Option<String>,
            description: Option<String>,
        },
    }

    let raw = Option::<Vec<OptionCompat>>::deserialize(deserializer)?;
    Ok(raw.map(|items| {
        items
            .into_iter()
            .map(|item| match item {
                OptionCompat::Text(text) => text,
                OptionCompat::Object {
                    label,
                    value,
                    description,
                } => label.or(value).or(description).unwrap_or_default(),
            })
            .filter(|s| !s.is_empty())
            .collect()
    }))
}

fn parse_question_input(input: serde_json::Value) -> Result<QuestionInput, ToolError> {
    let parsed: QuestionInputCompat = serde_json::from_value(input)
        .map_err(|e| ToolError::Execution(format!("invalid question input: {e}")))?;
    let questions = match parsed {
        QuestionInputCompat::Questions { questions } => questions,
        QuestionInputCompat::QuestionList(questions) => questions,
        QuestionInputCompat::SingleQuestion {
            question,
            options,
            initial,
        } => vec![QuestionSpec {
            question,
            options,
            initial,
        }],
    };
    Ok(QuestionInput { questions })
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

    fn category(&self) -> ToolCategory {
        ToolCategory::SafeMutating
    }

    /// Execute the question tool.
    ///
    /// NOTE: In normal AgentLoop flow, this method is never called because
    /// `check_tool_permission` intercepts the "question" tool before execution
    /// and routes it through `QuestionRegistry`. This fallback is only reached
    /// if the tool bypasses permission checks (e.g., exec mode with Allow default).
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed = parse_question_input(input)?;

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
    let parsed = parse_question_input(input)?;
    Ok(parsed.questions)
}

pub fn format_question_answers(answers: &str) -> String {
    answers.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_single_question_shape() {
        let questions = parse_question_questions(json!({
            "question": "Proceed?",
            "options": [{"label": "Yes"}, {"value": "No"}],
        }))
        .expect("single question should parse");

        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].question, "Proceed?");
        assert_eq!(
            questions[0].options.as_ref().expect("options"),
            &vec!["Yes".to_string(), "No".to_string()]
        );
    }

    #[test]
    fn parses_raw_question_array_shape() {
        let questions = parse_question_questions(json!([
            {"question": "First?"},
            {"question": "Second?", "initial": "default"},
        ]))
        .expect("raw array should parse");

        assert_eq!(questions.len(), 2);
        assert_eq!(questions[1].initial.as_deref(), Some("default"));
    }
}
