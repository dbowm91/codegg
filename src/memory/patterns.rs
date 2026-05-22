//! Rule-based pattern detection and importance scoring for memory consolidation.
//!
//! This module provides deterministic pattern matching to extract meaningful
//! information from conversation history without requiring LLM inference.

use crate::memory::Memory;
use crate::session::message::{Message, PartData};

#[derive(Debug, Clone)]
pub struct PatternMatch {
    pub pattern_type: PatternType,
    pub matched_text: String,
    pub score: f64,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternType {
    UserPreference,
    CodingConvention,
    Deprecation,
    NamingPattern,
    Architecture,
    ToolPreference,
}

pub struct PatternDetector {
    preference_patterns: Vec<PreferencePattern>,
    convention_patterns: Vec<ConventionPattern>,
}

struct PreferencePattern {
    regex: regex::Regex,
    base_score: f64,
    negation_modifier: f64,
}

struct ConventionPattern {
    regex: regex::Regex,
    pattern_type: PatternType,
    base_score: f64,
}

impl PatternDetector {
    pub fn new() -> Self {
        Self {
            preference_patterns: vec![
                PreferencePattern {
                    regex: regex::Regex::new(r"(?i)I prefer ([^.]+)").unwrap(),
                    base_score: 10.0,
                    negation_modifier: -3.0,
                },
                PreferencePattern {
                    regex: regex::Regex::new(r"(?i)I always ([^.]+)").unwrap(),
                    base_score: 12.0,
                    negation_modifier: -3.0,
                },
                PreferencePattern {
                    regex: regex::Regex::new(r"(?i)don't use ([^.]+)").unwrap(),
                    base_score: 8.0,
                    negation_modifier: -3.0,
                },
                PreferencePattern {
                    regex: regex::Regex::new(r"(?i)never use ([^.]+)").unwrap(),
                    base_score: 10.0,
                    negation_modifier: 0.0,
                },
                PreferencePattern {
                    regex: regex::Regex::new(r"(?i)use ([^.]+) instead").unwrap(),
                    base_score: 9.0,
                    negation_modifier: 0.0,
                },
                PreferencePattern {
                    regex: regex::Regex::new(r"(?i)([^ ]+) is deprecated").unwrap(),
                    base_score: 7.0,
                    negation_modifier: 0.0,
                },
                PreferencePattern {
                    regex: regex::Regex::new(r"(?i)we use ([^.]+)").unwrap(),
                    base_score: 8.0,
                    negation_modifier: 0.0,
                },
                PreferencePattern {
                    regex: regex::Regex::new(r"(?i)our ([^ ]+) follows ([^.]+)").unwrap(),
                    base_score: 9.0,
                    negation_modifier: 0.0,
                },
            ],
            convention_patterns: vec![
                ConventionPattern {
                    regex: regex::Regex::new(r"snake_case").unwrap(),
                    pattern_type: PatternType::NamingPattern,
                    base_score: 5.0,
                },
                ConventionPattern {
                    regex: regex::Regex::new(r"camelCase").unwrap(),
                    pattern_type: PatternType::NamingPattern,
                    base_score: 5.0,
                },
                ConventionPattern {
                    regex: regex::Regex::new(r"PascalCase").unwrap(),
                    pattern_type: PatternType::NamingPattern,
                    base_score: 5.0,
                },
                ConventionPattern {
                    regex: regex::Regex::new(r"kebab-case").unwrap(),
                    pattern_type: PatternType::NamingPattern,
                    base_score: 5.0,
                },
                ConventionPattern {
                    regex: regex::Regex::new(r"(?i)barrel file").unwrap(),
                    pattern_type: PatternType::Architecture,
                    base_score: 6.0,
                },
                ConventionPattern {
                    regex: regex::Regex::new(r"(?i)index\.([^ ]+)").unwrap(),
                    pattern_type: PatternType::Architecture,
                    base_score: 4.0,
                },
                ConventionPattern {
                    regex: regex::Regex::new(r"(?i)test in ([^.]+)").unwrap(),
                    pattern_type: PatternType::CodingConvention,
                    base_score: 5.0,
                },
                ConventionPattern {
                    regex: regex::Regex::new(r"mock\(").unwrap(),
                    pattern_type: PatternType::ToolPreference,
                    base_score: 4.0,
                },
                ConventionPattern {
                    regex: regex::Regex::new(r"(?i)linter|ESLint|clippy|ruff").unwrap(),
                    pattern_type: PatternType::ToolPreference,
                    base_score: 5.0,
                },
            ],
        }
    }

    pub fn detect_from_messages(&self, messages: &[Message]) -> Vec<PatternMatch> {
        let mut matches = Vec::new();

        for message in messages {
            let text_parts = self.extract_text_parts(message);
            for text in text_parts {
                let message_matches = self.detect_in_text(&text);
                matches.extend(message_matches);
            }
        }

        matches
    }

    fn extract_text_parts(&self, message: &Message) -> Vec<String> {
        let mut parts = Vec::new();
        for part in &message.data.parts {
            if let PartData::Text { text } = &part.data {
                parts.push(text.clone());
            }
        }
        parts
    }

    fn detect_in_text(&self, text: &str) -> Vec<PatternMatch> {
        let mut matches = Vec::new();

        for pref in &self.preference_patterns {
            for cap in pref.regex.captures_iter(text) {
                let full_match = cap.get(0).map(|m| m.as_str()).unwrap_or("");
                let detail = cap.get(1).map(|m| m.as_str()).unwrap_or("");

                let is_negation = full_match.to_lowercase().contains("don't")
                    || full_match.to_lowercase().contains("never")
                    || full_match.to_lowercase().contains("not");

                let base = if is_negation {
                    pref.base_score + pref.negation_modifier
                } else {
                    pref.base_score
                };

                matches.push(PatternMatch {
                    pattern_type: PatternType::UserPreference,
                    matched_text: detail.to_string(),
                    score: base,
                    context: full_match.to_string(),
                });
            }
        }

        for conv in &self.convention_patterns {
            for cap in conv.regex.captures_iter(text) {
                let full_match = cap.get(0).map(|m| m.as_str()).unwrap_or("").to_string();

                matches.push(PatternMatch {
                    pattern_type: conv.pattern_type.clone(),
                    matched_text: full_match.clone(),
                    score: conv.base_score,
                    context: full_match,
                });
            }
        }

        matches
    }

    pub fn aggregate_and_score(&self, matches: Vec<PatternMatch>) -> Vec<ScoredMemory> {
        let mut by_topic: HashMap<String, Vec<PatternMatch>> = HashMap::new();

        for m in matches {
            let key = m.matched_text.to_lowercase();
            by_topic.entry(key).or_default().push(m);
        }

        let mut scored: Vec<ScoredMemory> = by_topic
            .into_iter()
            .map(|(topic, topic_matches)| {
                let base_score = topic_matches.iter().map(|m| m.score).sum::<f64>()
                    / topic_matches.len() as f64;
                let frequency_bonus = (topic_matches.len() as f64 - 1.0) * 2.0;
                let context_sample = topic_matches.first().map(|m| m.context.clone()).unwrap_or_default();
                let pattern_type = topic_matches.first().map(|m| m.pattern_type.clone()).unwrap_or(PatternType::UserPreference);

                let final_score = base_score + frequency_bonus;

                ScoredMemory {
                    matched_text: topic,
                    score: final_score,
                    pattern_type,
                    context: context_sample,
                    frequency: topic_matches.len(),
                }
            })
            .collect();

        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }
}

impl Default for PatternDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct ScoredMemory {
    pub matched_text: String,
    pub score: f64,
    pub pattern_type: PatternType,
    pub context: String,
    pub frequency: usize,
}

impl ScoredMemory {
    pub fn to_memory(&self, namespace: &str) -> Memory {
        let title = self.generate_title();
        let content = self.generate_content();

        Memory {
            id: uuid::Uuid::new_v4().to_string(),
            namespace: namespace.to_string(),
            title: Some(title),
            content,
            uri: None,
            created_at: chrono::Utc::now().timestamp_millis(),
            updated_at: chrono::Utc::now().timestamp_millis(),
            access_count: 0,
            importance: (self.score / 20.0).min(1.0),
            superseded_by: None,
        }
    }

    fn generate_title(&self) -> String {
        match self.pattern_type {
            PatternType::UserPreference => format!("Preference: {}", self.matched_text),
            PatternType::CodingConvention => format!("Convention: {}", self.matched_text),
            PatternType::NamingPattern => format!("Naming: {}", self.matched_text),
            PatternType::Architecture => format!("Architecture: {}", self.matched_text),
            PatternType::Deprecation => format!("Deprecated: {}", self.matched_text),
            PatternType::ToolPreference => format!("Tool: {}", self.matched_text),
        }
    }

    fn generate_content(&self) -> String {
        let freq_note = if self.frequency > 1 {
            format!(" (mentioned {} times)", self.frequency)
        } else {
            String::new()
        };

        format!("{}{}", self.context, freq_note)
    }
}

use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preference_detection() {
        let detector = PatternDetector::new();
        let text = "I prefer concise code over verbose code";
        let matches = detector.detect_in_text(text);

        assert!(!matches.is_empty());
        assert_eq!(matches[0].pattern_type, PatternType::UserPreference);
        assert!(matches[0].score >= 8.0);
    }

    #[test]
    fn test_negation_detection() {
        let detector = PatternDetector::new();
        let text = "Don't use eval in JavaScript";
        let matches = detector.detect_in_text(text);

        assert!(!matches.is_empty());
        assert!(matches[0].score < 8.0);
    }

    #[test]
    fn test_naming_pattern_detection() {
        let detector = PatternDetector::new();
        let text = "We use snake_case for variable names";
        let matches = detector.detect_in_text(text);

        assert!(!matches.is_empty());
        assert!(matches.iter().any(|m| m.pattern_type == PatternType::NamingPattern));
    }

    #[test]
    fn test_scoring() {
        let detector = PatternDetector::new();
        let matches = vec![
            PatternMatch {
                pattern_type: PatternType::UserPreference,
                matched_text: "snake_case".to_string(),
                score: 10.0,
                context: "I prefer snake_case".to_string(),
            },
            PatternMatch {
                pattern_type: PatternType::UserPreference,
                matched_text: "snake_case".to_string(),
                score: 10.0,
                context: "Always use snake_case".to_string(),
            },
        ];

        let scored = detector.aggregate_and_score(matches);
        assert_eq!(scored.len(), 1);
        assert!(scored[0].frequency >= 1);
        assert!(scored[0].score > 10.0);
    }
}
