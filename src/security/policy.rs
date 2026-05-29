use crate::config::schema::{SecurityConfig, SecurityMode};
use crate::security::command::{CommandClassification, CommandRisk};
use crate::security::finding::{SecurityCategory, SecurityFinding};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityAction {
    Observe,
    Ask,
    Deny,
}

#[derive(Debug, Clone)]
pub struct SecurityDecisionHint {
    pub action: SecurityAction,
    pub reason: String,
    pub finding: Option<SecurityFinding>,
}

fn is_enabled(config: &SecurityConfig) -> bool {
    config.enabled && config.mode != SecurityMode::Off
}

pub fn action_for_command(
    classification: &CommandClassification,
    config: &SecurityConfig,
) -> SecurityDecisionHint {
    if !is_enabled(config) {
        return SecurityDecisionHint {
            action: SecurityAction::Observe,
            reason: "security is disabled or off".into(),
            finding: None,
        };
    }

    // Check explicit deny list
    if let Some(finding) = &classification.finding {
        let evidence = finding.evidence.clone();
        if config
            .denied_commands
            .iter()
            .any(|d| evidence.contains(d.as_str()))
        {
            return SecurityDecisionHint {
                action: SecurityAction::Deny,
                reason: format!("command matches denied pattern: {}", evidence),
                finding: classification.finding.clone(),
            };
        }
    }

    // Review mode: no auto-deny beyond critical; produce findings for reviewer
    if config.mode == SecurityMode::Review {
        if classification.risk >= CommandRisk::Critical && config.gates.ask_on_high_risk_command {
            let reason = classification
                .reasons
                .first()
                .cloned()
                .unwrap_or_else(|| "critical risk command".into());
            return SecurityDecisionHint {
                action: SecurityAction::Ask,
                reason: format!("[review] critical command: {}", reason),
                finding: classification.finding.clone(),
            };
        }
        if classification.risk >= CommandRisk::High && config.gates.ask_on_high_risk_command {
            let reason = classification
                .reasons
                .first()
                .cloned()
                .unwrap_or_else(|| "high-risk command".into());
            return SecurityDecisionHint {
                action: SecurityAction::Ask,
                reason: format!("[review] high-risk command: {}", reason),
                finding: classification.finding.clone(),
            };
        }
        return SecurityDecisionHint {
            action: SecurityAction::Observe,
            reason: "review mode: observation only".into(),
            finding: classification.finding.clone(),
        };
    }

    // Critical + deny_critical_commands => Deny
    if classification.risk == CommandRisk::Critical && config.gates.deny_critical_commands {
        let reason = classification
            .reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "critical risk command".into());
        return SecurityDecisionHint {
            action: SecurityAction::Deny,
            reason: format!("critical command: {}", reason),
            finding: classification.finding.clone(),
        };
    }

    // Critical + ask_on_high_risk_command (when deny is disabled) => Ask
    if classification.risk == CommandRisk::Critical && config.gates.ask_on_high_risk_command {
        let reason = classification
            .reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "critical risk command".into());
        return SecurityDecisionHint {
            action: SecurityAction::Ask,
            reason: format!("critical command (ask mode): {}", reason),
            finding: classification.finding.clone(),
        };
    }

    // Network exfiltration category check
    if classification
        .categories
        .contains(&SecurityCategory::NetworkExfiltration)
        && config.gates.ask_on_network_exfiltration
    {
        let reason = classification
            .reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "network exfiltration".into());
        return SecurityDecisionHint {
            action: SecurityAction::Ask,
            reason: format!("network exfiltration: {}", reason),
            finding: classification.finding.clone(),
        };
    }

    // Secret exposure category check
    if classification
        .categories
        .contains(&SecurityCategory::SecretExposure)
        && config.gates.ask_on_secret_exposure
    {
        let reason = classification
            .reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "secret exposure".into());
        return SecurityDecisionHint {
            action: SecurityAction::Ask,
            reason: format!("secret exposure: {}", reason),
            finding: classification.finding.clone(),
        };
    }

    // High risk + ask_on_high_risk_command => Ask
    if classification.risk == CommandRisk::High && config.gates.ask_on_high_risk_command {
        let reason = classification
            .reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "high-risk command".into());
        return SecurityDecisionHint {
            action: SecurityAction::Ask,
            reason: format!("high-risk command: {}", reason),
            finding: classification.finding.clone(),
        };
    }

    // Mode-specific behavior for medium risk
    match config.mode {
        SecurityMode::Strict if classification.risk == CommandRisk::Medium => {
            let reason = classification
                .reasons
                .first()
                .cloned()
                .unwrap_or_else(|| "medium-risk command in strict mode".into());
            return SecurityDecisionHint {
                action: SecurityAction::Ask,
                reason: format!("strict mode: {}", reason),
                finding: classification.finding.clone(),
            };
        }
        SecurityMode::Ambient => {
            // Medium risk observes unless specifically configured
        }
        _ => {}
    }

    SecurityDecisionHint {
        action: SecurityAction::Observe,
        reason: "no policy escalation triggered".into(),
        finding: classification.finding.clone(),
    }
}

pub fn action_for_findings(
    findings: &[SecurityFinding],
    config: &SecurityConfig,
) -> SecurityDecisionHint {
    if !is_enabled(config) {
        return SecurityDecisionHint {
            action: SecurityAction::Observe,
            reason: "security is disabled or off".into(),
            finding: None,
        };
    }

    if findings.is_empty() {
        return SecurityDecisionHint {
            action: SecurityAction::Observe,
            reason: "no findings".into(),
            finding: None,
        };
    }

    // Find the highest-severity finding
    let mut worst = &findings[0];
    for f in &findings[1..] {
        if f.severity > worst.severity {
            worst = f;
        }
    }

    // Review mode: observe only, report findings
    if config.mode == SecurityMode::Review {
        return SecurityDecisionHint {
            action: SecurityAction::Observe,
            reason: format!("[review] {}", worst.compact_summary()),
            finding: Some(worst.clone()),
        };
    }

    match worst.severity {
        crate::security::finding::Severity::Critical if config.gates.deny_critical_commands => {
            SecurityDecisionHint {
                action: SecurityAction::Deny,
                reason: format!("critical finding: {}", worst.compact_summary()),
                finding: Some(worst.clone()),
            }
        }
        crate::security::finding::Severity::Critical | crate::security::finding::Severity::High => {
            if config.gates.ask_on_high_risk_command {
                SecurityDecisionHint {
                    action: SecurityAction::Ask,
                    reason: format!("high/critical finding: {}", worst.compact_summary()),
                    finding: Some(worst.clone()),
                }
            } else {
                SecurityDecisionHint {
                    action: SecurityAction::Observe,
                    reason: format!("finding noted: {}", worst.compact_summary()),
                    finding: Some(worst.clone()),
                }
            }
        }
        crate::security::finding::Severity::Medium => {
            if config.mode == SecurityMode::Strict {
                SecurityDecisionHint {
                    action: SecurityAction::Ask,
                    reason: format!("strict mode finding: {}", worst.compact_summary()),
                    finding: Some(worst.clone()),
                }
            } else {
                SecurityDecisionHint {
                    action: SecurityAction::Observe,
                    reason: format!("finding noted: {}", worst.compact_summary()),
                    finding: Some(worst.clone()),
                }
            }
        }
        _ => SecurityDecisionHint {
            action: SecurityAction::Observe,
            reason: format!("low-info finding: {}", worst.compact_summary()),
            finding: Some(worst.clone()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{SecurityConfig, SecurityGateConfig, SecurityMode};
    use crate::security::command::{CommandClassification, CommandRisk};
    use crate::security::finding::{
        Confidence, FindingMode, FindingSource, SecurityCategory, SecurityFinding, Severity,
    };

    fn default_config() -> SecurityConfig {
        SecurityConfig::default()
    }

    fn disabled_config() -> SecurityConfig {
        SecurityConfig {
            enabled: false,
            ..Default::default()
        }
    }

    fn off_config() -> SecurityConfig {
        SecurityConfig {
            mode: SecurityMode::Off,
            ..Default::default()
        }
    }

    fn strict_config() -> SecurityConfig {
        SecurityConfig {
            mode: SecurityMode::Strict,
            ..Default::default()
        }
    }

    fn review_config() -> SecurityConfig {
        SecurityConfig {
            mode: SecurityMode::Review,
            ..Default::default()
        }
    }

    fn no_deny_config() -> SecurityConfig {
        SecurityConfig {
            gates: SecurityGateConfig {
                deny_critical_commands: false,
                ask_on_high_risk_command: false,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn make_classification(
        risk: CommandRisk,
        categories: Vec<SecurityCategory>,
    ) -> CommandClassification {
        CommandClassification {
            risk,
            categories,
            reasons: vec!["test reason".into()],
            finding: None,
        }
    }

    fn make_classification_with_finding(
        risk: CommandRisk,
        categories: Vec<SecurityCategory>,
    ) -> CommandClassification {
        CommandClassification {
            risk,
            categories: categories.clone(),
            reasons: vec!["test reason".into()],
            finding: Some(SecurityFinding {
                id: "test-finding".into(),
                severity: match risk {
                    CommandRisk::Critical => Severity::Critical,
                    CommandRisk::High => Severity::High,
                    CommandRisk::Medium => Severity::Medium,
                    CommandRisk::Low => Severity::Low,
                },
                confidence: Confidence::High,
                category: categories
                    .first()
                    .cloned()
                    .unwrap_or(SecurityCategory::Unknown),
                source: FindingSource::CommandClassifier,
                mode: FindingMode::Deterministic,
                file: None,
                line_range: None,
                evidence: "test evidence".into(),
                recommendation: "test recommendation".into(),
            }),
        }
    }

    fn make_finding(severity: Severity, category: SecurityCategory) -> SecurityFinding {
        SecurityFinding {
            id: format!("test-{}", category.label()),
            severity,
            confidence: Confidence::High,
            category,
            source: FindingSource::CommandClassifier,
            mode: FindingMode::Deterministic,
            file: None,
            line_range: None,
            evidence: "test evidence".into(),
            recommendation: "test".into(),
        }
    }

    #[test]
    fn test_security_disabled_returns_observe() {
        let c = make_classification(
            CommandRisk::Critical,
            vec![SecurityCategory::DangerousCommand],
        );
        let hint = action_for_command(&c, &disabled_config());
        assert_eq!(hint.action, SecurityAction::Observe);
    }

    #[test]
    fn test_security_off_returns_observe() {
        let c = make_classification(
            CommandRisk::Critical,
            vec![SecurityCategory::DangerousCommand],
        );
        let hint = action_for_command(&c, &off_config());
        assert_eq!(hint.action, SecurityAction::Observe);
    }

    #[test]
    fn test_critical_deny() {
        let c = make_classification_with_finding(
            CommandRisk::Critical,
            vec![SecurityCategory::DestructiveFilesystem],
        );
        let hint = action_for_command(&c, &default_config());
        assert_eq!(hint.action, SecurityAction::Deny);
        assert!(hint.reason.contains("critical"));
    }

    #[test]
    fn test_critical_no_deny_when_disabled() {
        let c = make_classification_with_finding(
            CommandRisk::Critical,
            vec![SecurityCategory::DestructiveFilesystem],
        );
        let hint = action_for_command(&c, &no_deny_config());
        assert_ne!(hint.action, SecurityAction::Deny);
    }

    #[test]
    fn test_high_risk_asks() {
        let c = make_classification(CommandRisk::High, vec![SecurityCategory::DangerousCommand]);
        let hint = action_for_command(&c, &default_config());
        assert_eq!(hint.action, SecurityAction::Ask);
    }

    #[test]
    fn test_network_exfiltration_asks() {
        let c = make_classification(
            CommandRisk::High,
            vec![SecurityCategory::NetworkExfiltration],
        );
        let hint = action_for_command(&c, &default_config());
        assert_eq!(hint.action, SecurityAction::Ask);
        assert!(hint.reason.contains("network exfiltration"));
    }

    #[test]
    fn test_secret_exposure_asks() {
        let c = make_classification(CommandRisk::High, vec![SecurityCategory::SecretExposure]);
        let hint = action_for_command(&c, &default_config());
        assert_eq!(hint.action, SecurityAction::Ask);
        assert!(hint.reason.contains("secret exposure"));
    }

    #[test]
    fn test_strict_mode_medium_asks() {
        let c = make_classification(
            CommandRisk::Medium,
            vec![SecurityCategory::DangerousCommand],
        );
        let hint = action_for_command(&c, &strict_config());
        assert_eq!(hint.action, SecurityAction::Ask);
        assert!(hint.reason.contains("strict mode"));
    }

    #[test]
    fn test_ambient_mode_medium_observes() {
        let c = make_classification(
            CommandRisk::Medium,
            vec![SecurityCategory::DangerousCommand],
        );
        let hint = action_for_command(&c, &default_config());
        assert_eq!(hint.action, SecurityAction::Observe);
    }

    #[test]
    fn test_review_mode_critical_asks_not_denies() {
        let c = make_classification(
            CommandRisk::Critical,
            vec![SecurityCategory::DangerousCommand],
        );
        let hint = action_for_command(&c, &review_config());
        assert_eq!(hint.action, SecurityAction::Ask);
        assert!(hint.reason.contains("[review]"));
    }

    #[test]
    fn test_review_mode_medium_observes() {
        let c = make_classification(
            CommandRisk::Medium,
            vec![SecurityCategory::DangerousCommand],
        );
        let hint = action_for_command(&c, &review_config());
        assert_eq!(hint.action, SecurityAction::Observe);
    }

    #[test]
    fn test_low_risk_always_observes() {
        let c = make_classification(CommandRisk::Low, vec![]);
        let hint = action_for_command(&c, &default_config());
        assert_eq!(hint.action, SecurityAction::Observe);
    }

    #[test]
    fn test_findings_disabled_returns_observe() {
        let findings = vec![make_finding(
            Severity::Critical,
            SecurityCategory::RemoteCodeExecution,
        )];
        let hint = action_for_findings(&findings, &disabled_config());
        assert_eq!(hint.action, SecurityAction::Observe);
    }

    #[test]
    fn test_findings_empty_returns_observe() {
        let hint = action_for_findings(&[], &default_config());
        assert_eq!(hint.action, SecurityAction::Observe);
    }

    #[test]
    fn test_findings_critical_denies() {
        let findings = vec![make_finding(
            Severity::Critical,
            SecurityCategory::RemoteCodeExecution,
        )];
        let hint = action_for_findings(&findings, &default_config());
        assert_eq!(hint.action, SecurityAction::Deny);
    }

    #[test]
    fn test_findings_high_asks() {
        let findings = vec![make_finding(
            Severity::High,
            SecurityCategory::DangerousCommand,
        )];
        let hint = action_for_findings(&findings, &default_config());
        assert_eq!(hint.action, SecurityAction::Ask);
    }

    #[test]
    fn test_findings_medium_strict_asks() {
        let findings = vec![make_finding(
            Severity::Medium,
            SecurityCategory::DangerousCommand,
        )];
        let hint = action_for_findings(&findings, &strict_config());
        assert_eq!(hint.action, SecurityAction::Ask);
    }

    #[test]
    fn test_findings_medium_ambient_observes() {
        let findings = vec![make_finding(
            Severity::Medium,
            SecurityCategory::DangerousCommand,
        )];
        let hint = action_for_findings(&findings, &default_config());
        assert_eq!(hint.action, SecurityAction::Observe);
    }

    #[test]
    fn test_findings_review_observes() {
        let findings = vec![make_finding(
            Severity::Critical,
            SecurityCategory::RemoteCodeExecution,
        )];
        let hint = action_for_findings(&findings, &review_config());
        assert_eq!(hint.action, SecurityAction::Observe);
        assert!(hint.reason.contains("[review]"));
    }

    #[test]
    fn test_findings_low_observes() {
        let findings = vec![make_finding(Severity::Low, SecurityCategory::ConfigRisk)];
        let hint = action_for_findings(&findings, &default_config());
        assert_eq!(hint.action, SecurityAction::Observe);
    }

    #[test]
    fn test_denied_commands_config() {
        let config = SecurityConfig {
            denied_commands: vec!["dangerous_tool".into()],
            ..default_config()
        };
        let c = CommandClassification {
            risk: CommandRisk::Low,
            categories: vec![],
            reasons: vec!["test".into()],
            finding: Some(SecurityFinding {
                id: "test".into(),
                severity: Severity::Low,
                confidence: Confidence::High,
                category: SecurityCategory::DangerousCommand,
                source: FindingSource::CommandClassifier,
                mode: FindingMode::Deterministic,
                file: None,
                line_range: None,
                evidence: "dangerous_tool --arg".into(),
                recommendation: "no".into(),
            }),
        };
        let hint = action_for_command(&c, &config);
        assert_eq!(hint.action, SecurityAction::Deny);
    }

    #[test]
    fn test_critical_no_deny_asks() {
        let config = SecurityConfig {
            gates: SecurityGateConfig {
                deny_critical_commands: false,
                ask_on_high_risk_command: true,
                ..SecurityGateConfig::default()
            },
            ..default_config()
        };
        let c = make_classification(
            CommandRisk::Critical,
            vec![SecurityCategory::DangerousCommand],
        );
        let hint = action_for_command(&c, &config);
        assert_eq!(hint.action, SecurityAction::Ask);
    }

    #[test]
    fn test_decision_hint_serializable_fields() {
        let c = make_classification(CommandRisk::High, vec![SecurityCategory::DangerousCommand]);
        let hint = action_for_command(&c, &default_config());
        assert_eq!(hint.action, SecurityAction::Ask);
        assert!(!hint.reason.is_empty());
    }

    #[test]
    fn test_network_exfil_in_review_mode_still_asks() {
        let c = make_classification(
            CommandRisk::High,
            vec![SecurityCategory::NetworkExfiltration],
        );
        let hint = action_for_command(&c, &review_config());
        // Review mode asks for high risk but doesn't deny
        assert_eq!(hint.action, SecurityAction::Ask);
    }

    #[test]
    fn test_findings_worst_severity_used() {
        let findings = vec![
            make_finding(Severity::Low, SecurityCategory::ConfigRisk),
            make_finding(Severity::High, SecurityCategory::DangerousCommand),
            make_finding(Severity::Medium, SecurityCategory::UnsafeCode),
        ];
        let hint = action_for_findings(&findings, &default_config());
        assert_eq!(hint.action, SecurityAction::Ask);
        assert!(hint.finding.is_some());
        assert_eq!(hint.finding.unwrap().severity, Severity::High);
    }
}
