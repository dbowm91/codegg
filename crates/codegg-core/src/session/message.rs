use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: MessageData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageData {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(rename = "messageID")]
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub parts: Vec<PartInfo>,
    /// Who produced this message.
    ///
    /// Optional because rows written before this field existed — including
    /// every compatibility import — have no role recorded. Readers must fall
    /// back to inferring it from `parts` when this is `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<MessageRole>,
}

/// Author of a persisted transcript message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

/// The wire name of a tool-call id.
///
/// It must not be `id`: [`PartInfo`] and [`PartData`] are joined by
/// `#[serde(flatten)]`, so two fields that share a wire name serialize to a
/// duplicate key and no reader can decode the result.
const TOOL_CALL_ID_FIELD: &str = "toolCallID";

/// One part of a persisted message.
///
/// # Wire format
///
/// The part id is `id`; the **tool-call** id is `toolCallID`. They are
/// distinct because `PartInfo` and `PartData` are flattened together: a shared
/// name emits a duplicate `id` key, which `serde_json` rejects with
/// ``duplicate field `id` ``.
///
/// # Rows written before `toolCallID` existed
///
/// A tool-call part written before that rename carries exactly **one** `id`,
/// and that value is the tool-call id. The writer built the row with
/// `serde_json::to_value`, where a duplicate key collapses to the last value
/// written (`PartData::ToolCall::id`), so `PartInfo::id` was never stored and
/// cannot be recovered from any row. This deserializer therefore reads that
/// single `id` as the tool-call id and synthesizes the part id as
/// `{messageID}:{toolCallID}` — deterministic, unique within its message, and
/// distinguishable from the pre-fix `{messageID}_{index}` spelling because the
/// separator is `:`.
///
/// Reads recover in place; every write is canonical. No storage migration is
/// owed, so `STORAGE_LAYOUT_VERSION` is unchanged.
#[derive(Debug, Clone, Serialize)]
pub struct PartInfo {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(flatten)]
    pub data: PartData,
}

impl<'de> Deserialize<'de> for PartInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        /// The part's own fields plus every flattened key, undecoded.
        #[derive(Deserialize)]
        struct Flat {
            #[serde(default)]
            id: Option<String>,
            #[serde(rename = "sessionID")]
            session_id: String,
            #[serde(rename = "messageID")]
            message_id: String,
            #[serde(flatten)]
            data: serde_json::Map<String, serde_json::Value>,
        }

        let Flat {
            id,
            session_id,
            message_id,
            mut data,
        } = Flat::deserialize(deserializer)?;
        let id = id.ok_or_else(|| serde::de::Error::missing_field("id"))?;
        let id = if data.get("type").and_then(|kind| kind.as_str()) == Some("tool_call")
            && !data.contains_key(TOOL_CALL_ID_FIELD)
        {
            // Pre-rename row: the single `id` is the tool-call id, and the
            // part id was already lost at write time. Recover one and
            // synthesize the other rather than failing the whole row.
            data.insert(
                TOOL_CALL_ID_FIELD.to_string(),
                serde_json::Value::String(id.clone()),
            );
            format!("{message_id}:{id}")
        } else {
            id
        };
        Ok(PartInfo {
            id,
            session_id,
            message_id,
            data: serde_json::from_value(serde_json::Value::Object(data))
                .map_err(serde::de::Error::custom)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PartData {
    Text {
        text: String,
    },
    Reasoning {
        reasoning: String,
    },
    ToolCall {
        #[serde(rename = "toolCallID")]
        id: String,
        name: String,
        input: serde_json::Value,
        output: Option<String>,
        status: ToolStatus,
    },
    Image {
        url: String,
    },
    File {
        path: String,
        content: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Part {
    pub id: String,
    pub message_id: String,
    pub session_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_data_default_parts() {
        let data = MessageData {
            id: String::new(),
            session_id: String::new(),
            message_id: String::new(),
            parts: Vec::new(),
            role: None,
        };
        assert!(data.parts.is_empty());
    }

    #[test]
    fn test_part_data_text() {
        let part = PartData::Text {
            text: "hello".to_string(),
        };
        assert!(matches!(part, PartData::Text { .. }));
    }

    #[test]
    fn test_part_data_reasoning() {
        let part = PartData::Reasoning {
            reasoning: "thinking...".to_string(),
        };
        assert!(matches!(part, PartData::Reasoning { .. }));
    }

    #[test]
    fn test_part_data_tool_call() {
        let part = PartData::ToolCall {
            id: "call_1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
            output: Some("file.txt".to_string()),
            status: ToolStatus::Completed,
        };
        assert!(matches!(part, PartData::ToolCall { .. }));
    }

    #[test]
    fn test_part_data_image() {
        let part = PartData::Image {
            url: "http://example.com/img.png".to_string(),
        };
        assert!(matches!(part, PartData::Image { .. }));
    }

    #[test]
    fn test_part_data_file() {
        let part = PartData::File {
            path: "/tmp/test.txt".to_string(),
            content: "content".to_string(),
        };
        assert!(matches!(part, PartData::File { .. }));
    }

    #[test]
    fn test_tool_status_default() {
        let status = ToolStatus::default();
        assert!(matches!(status, ToolStatus::Pending));
    }

    #[test]
    fn test_message_data_serialization() {
        let data = MessageData {
            id: "msg_1".to_string(),
            session_id: "sess_1".to_string(),
            message_id: "msg_1".to_string(),
            parts: vec![PartInfo {
                id: "part_1".to_string(),
                session_id: "sess_1".to_string(),
                message_id: "msg_1".to_string(),
                data: PartData::Text {
                    text: "hello".to_string(),
                },
            }],
            role: Some(MessageRole::Assistant),
        };
        let json = serde_json::to_string(&data).unwrap();
        let deserialized: MessageData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "msg_1");
        assert_eq!(deserialized.parts.len(), 1);
        assert_eq!(deserialized.role, Some(MessageRole::Assistant));
    }

    #[test]
    fn test_part_info_serialization() {
        let info = PartInfo {
            id: "p1".to_string(),
            session_id: "s1".to_string(),
            message_id: "m1".to_string(),
            data: PartData::Text {
                text: "test".to_string(),
            },
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        let deserialized: PartInfo = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized.data, PartData::Text { .. }));
    }

    #[test]
    fn test_message_roundtrip() {
        let msg = Message {
            id: "msg_1".to_string(),
            session_id: "sess_1".to_string(),
            time_created: 1000,
            time_updated: 2000,
            data: MessageData {
                id: "msg_1".to_string(),
                session_id: "sess_1".to_string(),
                message_id: "msg_1".to_string(),
                parts: vec![],
                role: None,
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, msg.id);
        assert_eq!(deserialized.time_created, msg.time_created);
    }

    /// A tool-call part must survive a serde round trip with **both**
    /// identifiers intact and distinct.
    ///
    /// `PartInfo` and `PartData::ToolCall` are joined by `#[serde(flatten)]`,
    /// so two fields that share a wire name collapse into a duplicate key on
    /// serialization and fail to decode. `test_part_data_tool_call` above only
    /// asserts the variant tag, which is why this defect survived.
    #[test]
    fn tool_call_part_round_trips_with_distinct_part_and_call_ids() {
        let info = PartInfo {
            id: "m1_0".to_string(),
            session_id: "s".to_string(),
            message_id: "m1".to_string(),
            data: PartData::ToolCall {
                id: "call_1".to_string(),
                name: "read".to_string(),
                input: serde_json::json!({}),
                output: None,
                status: ToolStatus::Pending,
            },
        };

        let json = serde_json::to_string(&info).expect("serialize a tool-call part");
        let decoded: PartInfo = serde_json::from_str(&json).expect("round-trip a tool-call part");
        assert_eq!(decoded.id, "m1_0");
        assert_eq!(decoded.session_id, "s");
        assert_eq!(decoded.message_id, "m1");
        match decoded.data {
            PartData::ToolCall { id, name, .. } => {
                assert_eq!(id, "call_1", "the tool-call id must survive distinctly");
                assert_eq!(name, "read");
            }
            other => panic!("expected a tool-call part, got {other:?}"),
        }
    }

    /// A tool-call row written before the tool-call id had its own wire name
    /// carries a single `id` — the call id — and no part id. Reading it must
    /// recover the call id and synthesize the part id rather than fail.
    #[test]
    fn legacy_tool_call_part_recovers_call_id_and_synthesizes_part_id() {
        let json = r#"{"id":"call_1","sessionID":"s","messageID":"m1","type":"tool_call",
                       "name":"read","input":{},"output":null,"status":"pending"}"#;

        let part: PartInfo = serde_json::from_str(json).expect("decode a pre-rename row");
        assert_eq!(part.id, "m1:call_1");
        assert_eq!(part.session_id, "s");
        assert_eq!(part.message_id, "m1");
        match part.data {
            PartData::ToolCall { id, status, .. } => {
                assert_eq!(id, "call_1", "the stored `id` is the tool-call id");
                assert_eq!(status, ToolStatus::Pending);
            }
            other => panic!("expected a tool-call part, got {other:?}"),
        }
    }

    #[test]
    fn test_message_data_default_id() {
        let json = r#"{"parts":[]}"#;
        let data: MessageData = serde_json::from_str(json).unwrap();
        assert_eq!(data.id, "");
        assert_eq!(data.session_id, "");
        assert_eq!(data.message_id, "");
    }
}
