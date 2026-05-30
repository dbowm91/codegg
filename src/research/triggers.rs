use crate::research::types::ResearchMode;

/// Configuration for trigger heuristics.
#[derive(Debug, Clone)]
pub struct TriggerConfig {
    pub enabled: bool,
    pub min_confidence: f64,
    pub keywords_comparison: Vec<String>,
    pub keywords_unknown_api: Vec<String>,
    pub keywords_security: Vec<String>,
    pub keywords_architecture: Vec<String>,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_confidence: 0.5,
            keywords_comparison: vec![
                "compare".into(),
                "versus".into(),
                "vs".into(),
                "which is better".into(),
                "pros and cons".into(),
                "recommend".into(),
            ],
            keywords_unknown_api: vec![
                "api".into(),
                "protocol".into(),
                "endpoint".into(),
                "integration".into(),
                "third-party".into(),
                "external".into(),
            ],
            keywords_security: vec![
                "security".into(),
                "vulnerability".into(),
                "cve".into(),
                "advisory".into(),
                "sanitize".into(),
                "validate input".into(),
            ],
            keywords_architecture: vec![
                "architecture".into(),
                "design".into(),
                "structure".into(),
                "refactor".into(),
                "decouple".into(),
            ],
        }
    }
}

/// The result of analyzing a task for research trigger conditions.
#[derive(Debug, Clone)]
pub struct TriggerAnalysis {
    pub should_invoke: bool,
    pub confidence: f64,
    pub suggested_mode: ResearchMode,
    pub reason: String,
}

fn contains_any_keyword(text: &str, keywords: &[String]) -> bool {
    let lower = text.to_lowercase();
    keywords.iter().any(|kw| lower.contains(&kw.to_lowercase()))
}

/// Conceptual error patterns that suggest previous failure was due to uncertainty.
const CONCEPTUAL_ERROR_PATTERNS: &[&str] = &[
    "conceptual",
    "unclear requirement",
    "wrong assumption",
    "misunderstood",
    "did not understand",
    "unexpected behavior",
    "not what i expected",
    "design flaw",
    "approach was wrong",
    "needs rethinking",
];

fn has_conceptual_failures(previous_failures: &[String]) -> bool {
    previous_failures.iter().any(|f| {
        let lower = f.to_lowercase();
        CONCEPTUAL_ERROR_PATTERNS
            .iter()
            .any(|pat| lower.contains(pat))
    })
}

/// Analyze a task to determine if research should be auto-invoked.
pub fn analyze_trigger(
    task_description: &str,
    file_paths: &[String],
    previous_failures: &[String],
    config: &TriggerConfig,
) -> TriggerAnalysis {
    if !config.enabled {
        return TriggerAnalysis {
            should_invoke: false,
            confidence: 0.0,
            suggested_mode: ResearchMode::NarrowAnswer,
            reason: "Trigger system disabled".into(),
        };
    }

    let task_lower = task_description.to_lowercase();

    // Rule 1: Comparison keywords → LibraryEvaluation
    if contains_any_keyword(task_description, &config.keywords_comparison) {
        let mut confidence: f64 = 0.8;
        let reason = "Task contains comparison/recommendation keywords".into();

        if has_conceptual_failures(previous_failures) {
            confidence = (confidence + 0.2).min(1.0);
        }

        return TriggerAnalysis {
            should_invoke: confidence >= config.min_confidence,
            confidence,
            suggested_mode: ResearchMode::LibraryEvaluation,
            reason,
        };
    }

    // Rule 2: Unknown API keywords → ApiInvestigation
    if contains_any_keyword(task_description, &config.keywords_unknown_api) {
        let has_new_files = file_paths.len() > 5
            || file_paths
                .iter()
                .any(|p| p.contains("new") || p.contains("add") || p.contains("create"));
        let mut confidence: f64 = if has_new_files { 0.7 } else { 0.6 };

        if has_conceptual_failures(previous_failures) {
            confidence = (confidence + 0.2).min(1.0);
        }

        if file_paths.len() > 10 {
            confidence = (confidence + 0.1).min(1.0);
        }

        return TriggerAnalysis {
            should_invoke: confidence >= config.min_confidence,
            confidence,
            suggested_mode: ResearchMode::ApiInvestigation,
            reason: "Task involves external API/protocol/integration".into(),
        };
    }

    // Rule 3: Security keywords → SecurityReview
    if contains_any_keyword(task_description, &config.keywords_security) {
        let mut confidence: f64 = 0.7;

        if has_conceptual_failures(previous_failures) {
            confidence = (confidence + 0.2).min(1.0);
        }

        return TriggerAnalysis {
            should_invoke: confidence >= config.min_confidence,
            confidence,
            suggested_mode: ResearchMode::SecurityReview,
            reason: "Task involves security concerns".into(),
        };
    }

    // Rule 4: Architecture keywords → ArchitectureDecision
    if contains_any_keyword(task_description, &config.keywords_architecture) {
        let mut confidence: f64 = 0.5;

        if has_conceptual_failures(previous_failures) {
            confidence = (confidence + 0.2).min(1.0);
        }

        if file_paths.len() > 10 {
            confidence = (confidence + 0.1).min(1.0);
        }

        return TriggerAnalysis {
            should_invoke: confidence >= config.min_confidence,
            confidence,
            suggested_mode: ResearchMode::ArchitectureDecision,
            reason: "Task involves architectural decisions".into(),
        };
    }

    // Rule 5: Large unknown surface with external dependencies
    if file_paths.len() > 10
        && (task_lower.contains("external") || task_lower.contains("dependency"))
    {
        let confidence: f64 = 0.6;
        return TriggerAnalysis {
            should_invoke: confidence >= config.min_confidence,
            confidence,
            suggested_mode: ResearchMode::ApiInvestigation,
            reason: "Large task surface with external dependencies".into(),
        };
    }

    // No trigger matched
    TriggerAnalysis {
        should_invoke: false,
        confidence: 0.0,
        suggested_mode: ResearchMode::NarrowAnswer,
        reason: "No trigger heuristic matched".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> TriggerConfig {
        TriggerConfig::default()
    }

    #[test]
    fn comparison_keywords_trigger_library_evaluation() {
        let analysis = analyze_trigger(
            "Compare React and Vue for the frontend",
            &[],
            &[],
            &default_config(),
        );
        assert!(analysis.should_invoke);
        assert!(analysis.confidence >= 0.8);
        assert_eq!(analysis.suggested_mode, ResearchMode::LibraryEvaluation);
    }

    #[test]
    fn vs_keyword_triggers() {
        let analysis = analyze_trigger(
            "Which is better for async: tokio vs async-std?",
            &[],
            &[],
            &default_config(),
        );
        assert!(analysis.should_invoke);
        assert_eq!(analysis.suggested_mode, ResearchMode::LibraryEvaluation);
    }

    #[test]
    fn api_keyword_triggers_api_investigation() {
        let paths = vec!["src/new_api.rs".into()];
        let analysis = analyze_trigger(
            "Integrate with the Stripe payment API",
            &paths,
            &[],
            &default_config(),
        );
        assert!(analysis.should_invoke);
        assert_eq!(analysis.suggested_mode, ResearchMode::ApiInvestigation);
        assert!(analysis.confidence >= 0.6);
    }

    #[test]
    fn security_keyword_triggers_security_review() {
        let analysis = analyze_trigger(
            "Check for vulnerability in the auth module",
            &[],
            &[],
            &default_config(),
        );
        assert!(analysis.should_invoke);
        assert_eq!(analysis.suggested_mode, ResearchMode::SecurityReview);
    }

    #[test]
    fn architecture_keyword_triggers() {
        let analysis = analyze_trigger(
            "Refactor the architecture to decouple services",
            &[],
            &[],
            &default_config(),
        );
        assert!(analysis.should_invoke);
        assert_eq!(analysis.suggested_mode, ResearchMode::ArchitectureDecision);
    }

    #[test]
    fn conceptual_failure_boosts_confidence() {
        let failures = vec!["Previous approach was wrong, needs rethinking".into()];
        let analysis = analyze_trigger(
            "Compare logging libraries",
            &[],
            &failures,
            &default_config(),
        );
        assert!(analysis.should_invoke);
        assert!(analysis.confidence > 0.8);
    }

    #[test]
    fn many_files_boosts_confidence() {
        let paths: Vec<String> = (0..15).map(|i| format!("src/file_{}.rs", i)).collect();
        let analysis = analyze_trigger("Refactor the architecture", &paths, &[], &default_config());
        assert!(analysis.should_invoke);
        assert!(analysis.confidence >= 0.6);
    }

    #[test]
    fn mechanical_task_no_trigger() {
        let analysis = analyze_trigger("Fix the typo in comments", &[], &[], &default_config());
        assert!(!analysis.should_invoke);
    }

    #[test]
    fn disabled_config_no_trigger() {
        let config = TriggerConfig {
            enabled: false,
            ..default_config()
        };
        let analysis = analyze_trigger("Compare React and Vue", &[], &[], &config);
        assert!(!analysis.should_invoke);
    }

    #[test]
    fn high_min_confidence_blocks_low_confidence() {
        let config = TriggerConfig {
            min_confidence: 0.9,
            ..default_config()
        };
        let analysis = analyze_trigger("Refactor the architecture", &[], &[], &config);
        assert!(!analysis.should_invoke);
        assert!(analysis.confidence < 0.9);
    }
}
