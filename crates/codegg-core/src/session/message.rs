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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartInfo {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(flatten)]
    pub data: PartData,
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
        };
        let json = serde_json::to_string(&data).unwrap();
        let deserialized: MessageData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "msg_1");
        assert_eq!(deserialized.parts.len(), 1);
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
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, msg.id);
        assert_eq!(deserialized.time_created, msg.time_created);
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
