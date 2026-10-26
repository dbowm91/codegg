use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use crate::eggsact::adapter::{to_structured_result, EggsactRuntime};
use crate::error::ToolError;
use crate::tool::backend::{StructuredToolResult, ToolExecutionContext};
use crate::tool::{Tool, ToolCategory};

/// Generic wrapper around any eggsact tool, exposed to the model as a
/// Codegg-native deterministic tool.
///
/// Each instance maps a Codegg tool name to an eggsact tool name with a
/// fixed JSON Schema, description, and visibility flag. The wrapper
/// delegates both `execute()` and `execute_structured()` to the shared
/// `EggsactRuntime` and attaches eggsact provenance.
pub struct EggsactTool {
    codegg_name: &'static str,
    eggsact_name: &'static str,
    description: &'static str,
    parameters: serde_json::Value,
    category: ToolCategory,
    defer: bool,
    expose: bool,
    runtime: Arc<EggsactRuntime>,
}

impl EggsactTool {
    pub fn new(
        codegg_name: &'static str,
        eggsact_name: &'static str,
        description: &'static str,
        parameters: serde_json::Value,
        category: ToolCategory,
        defer: bool,
        expose: bool,
        runtime: Arc<EggsactRuntime>,
    ) -> Self {
        Self {
            codegg_name,
            eggsact_name,
            description,
            parameters,
            category,
            defer,
            expose,
            runtime,
        }
    }

    /// All 15 validators are pure functions over their inputs (no
    /// filesystem reads, no external state); the envelope carries the
    /// `ok` flag plus optional machine-code/result/findings fields.
    /// No property is required so the schema stays permissive for
    /// future envelope extensions.
    /// `ok` is transport (tool ran); validity is result.valid/verdict.
    fn contract_output_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "description": "Deterministic eggsact result envelope (ok plus optional machine_code, result, findings). `ok` is transport (tool ran); validity is result.valid/verdict.",
            "properties": {
                "ok": { "type": "boolean" },
                "machine_code": { "type": "string" },
                "result": {},
                "findings": { "type": "array" }
            }
        })
    }
}

#[async_trait]
impl Tool for EggsactTool {
    fn name(&self) -> &str {
        self.codegg_name
    }

    fn description(&self) -> &str {
        self.description
    }

    fn parameters(&self) -> serde_json::Value {
        self.parameters.clone()
    }

    fn category(&self) -> ToolCategory {
        self.category
    }

    fn defer_loading(&self) -> bool {
        self.defer
    }

    fn expose_in_definitions(&self) -> bool {
        self.expose
    }

    /// Program-callable contract for the pure validator class.
    ///
    /// All 15 eggsact validators are pure functions over their inputs
    /// (no filesystem or external reads), so every instance shares the
    /// same contract: `DirectOrProgrammatic` (keeps direct model calls
    /// working), `ReadOnly` (severity 0, the only class admissible under
    /// both the `read_only` broker ceiling and the `ReadOnly|ReadValidate`
    /// manifest gate; `SafeRepeat` would pass the context gate but fail
    /// the manifest gate), `Idempotent`, and a permissive envelope
    /// output schema.
    fn contract(
        &self,
        tool_name: &str,
        input_schema: serde_json::Value,
    ) -> crate::tool::contract::ToolContract {
        let contract = crate::tool::contract::ToolContract {
            name: tool_name.to_string(),
            caller_policy: crate::tool::contract::ToolCallerPolicy::DirectOrProgrammatic,
            effect_class: crate::tool::contract::ToolEffectClass::ReadOnly,
            idempotency: crate::tool::contract::IdempotencyClass::Idempotent,
            output_schema: Some(Self::contract_output_schema()),
            ..crate::tool::contract::ToolContract::legacy(tool_name, input_schema)
        };
        debug_assert!(contract.validate().is_ok());
        contract
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let mut result = self.runtime.call_json(self.eggsact_name, input)?;
        if self.codegg_name == "text_security_inspect" || self.codegg_name == "identifier_inspect" {
            apply_text_security_noise_filter(&mut result);
        }
        Ok(result.output)
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let mut result = self.runtime.call_json(self.eggsact_name, input)?;
        if self.codegg_name == "text_security_inspect" || self.codegg_name == "identifier_inspect" {
            apply_text_security_noise_filter(&mut result);
        }
        Ok(to_structured_result(self.eggsact_name, result))
    }
}

/// F5: enforce the tool's own caveat — single-character ASCII confusables on
/// plain English are informational noise. When findings carry no
/// mixed-script/invisible/secret/INSTRUCTION_PHRASE/non-confusable signal
/// (same rule as the preflight filter), demote single-char ASCII confusables
/// to info/informational so the verdict falls to allow. Applies to both
/// `text_security_inspect` and `identifier_inspect`. Findings are deduped by
/// (code/kind, normalized message, span) before demotion, and
/// IDENT_COLLISION findings require word-boundary/token equality — a mere
/// substring (e.g. `rm` inside `immediately`) must not fire.
pub(crate) fn apply_text_security_noise_filter(
    result: &mut crate::eggsact::adapter::EggsactCallResult,
) {
    dedup_deterministic_findings(result);
    if has_actionable_text_signal_for_deterministic(result) {
        return;
    }
    let has_confusable = result.output.to_lowercase().contains("confusable")
        || result
            .findings
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter().any(|item| {
                    item.get("type")
                        .or_else(|| item.get("kind"))
                        .or_else(|| item.get("code"))
                        .and_then(|v| v.as_str())
                        .map(|s| {
                            let l = s.to_lowercase();
                            l.contains("confusable")
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
        || result
            .result
            .as_ref()
            .and_then(|r| r.get("findings"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter().any(|item| {
                    item.get("code")
                        .or_else(|| item.get("type"))
                        .or_else(|| item.get("kind"))
                        .and_then(|v| v.as_str())
                        .map(|s| {
                            let l = s.to_lowercase();
                            l.contains("confusable")
                        })
                        .unwrap_or(false)
                        || item
                            .get("message")
                            .and_then(|v| v.as_str())
                            .map(|m| m.to_lowercase().contains("confusable"))
                            .unwrap_or(false)
                })
            })
            .unwrap_or(false);
    let has_collision = result
        .findings
        .as_ref()
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .any(|item| finding_kind(item).contains("collision"))
        })
        .unwrap_or(false)
        || result
            .result
            .as_ref()
            .and_then(|r| r.get("findings"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .any(|item| finding_kind(item).contains("collision"))
            })
            .unwrap_or(false);
    if !has_confusable && !has_collision {
        return;
    }
    // Demote single-char ASCII confusables and spurious IDENT_COLLISIONs in
    // both structured slots.
    let mut demoted_any = false;
    if let Some(findings) = result.findings.as_mut().and_then(|v| v.as_array_mut()) {
        for item in findings.iter_mut() {
            if is_single_char_ascii_confusable_finding(item) || is_spurious_ident_collision(item) {
                demote_to_info(item);
                demoted_any = true;
            }
        }
    }
    if let Some(res) = result.result.as_mut() {
        if let Some(arr) = res.get_mut("findings").and_then(|v| v.as_array_mut()) {
            for item in arr.iter_mut() {
                if is_single_char_ascii_confusable_finding(item)
                    || is_spurious_ident_collision(item)
                {
                    demote_to_info(item);
                    demoted_any = true;
                }
            }
        }
        // Verdict falls to allow once only informational noise remains.
        // Verdict drivers are INSTRUCTION_PHRASE/secret/mixed_script/invisible
        // alone; confusable-only review falls to allow.
        if demoted_any && !has_remaining_actionable_finding(res) {
            res["verdict"] = serde_json::Value::String("allow".to_string());
            // Keep the human-readable envelope consistent with the verdict.
            result.output = result
                .output
                .replace("verdict: review", "verdict: allow")
                .replace("verdict:review", "verdict:allow")
                .replace("VERDICT_REVIEW", "VERDICT_ALLOW")
                .replace("\"verdict\": \"review\"", "\"verdict\": \"allow\"")
                .replace("\"verdict\":\"review\"", "\"verdict\":\"allow\"");
        }
    }
}

/// Normalized message for dedup: lowercase, trimmed, inner whitespace collapsed.
fn normalize_message_for_dedup(s: &str) -> String {
    s.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn finding_kind(item: &serde_json::Value) -> String {
    item.get("type")
        .or_else(|| item.get("kind"))
        .or_else(|| item.get("code"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase()
}

fn finding_span_key(item: &serde_json::Value) -> String {
    for key in [
        "span", "location", "range", "line", "column", "offset", "path",
    ] {
        if let Some(v) = item.get(key) {
            if let Some(s) = v.as_str() {
                return s.to_string();
            }
            return v.to_string();
        }
    }
    String::new()
}

fn dedup_array(arr: &mut Vec<serde_json::Value>) {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    arr.retain(|item| {
        let kind = finding_kind(item);
        let msg = item.get("message").and_then(|v| v.as_str()).unwrap_or("");
        let key = format!(
            "{}|{}|{}",
            kind,
            normalize_message_for_dedup(msg),
            finding_span_key(item)
        );
        seen.insert(key)
    });
}

/// Dedup findings by (code/kind, normalized message, span) before demotion.
fn dedup_deterministic_findings(result: &mut crate::eggsact::adapter::EggsactCallResult) {
    if let Some(findings) = result.findings.as_mut().and_then(|v| v.as_array_mut()) {
        let vec = std::mem::take(findings);
        let mut vec = vec;
        dedup_array(&mut vec);
        *findings = vec;
    }
    if let Some(res) = result.result.as_mut() {
        if let Some(arr) = res.get_mut("findings").and_then(|v| v.as_array_mut()) {
            let vec = std::mem::take(arr);
            let mut vec = vec;
            dedup_array(&mut vec);
            *arr = vec;
        }
    }
}

fn quoted_tokens(msg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = msg;
    while let Some(start) = rest.find('\'') {
        let after = &rest[start + 1..];
        if let Some(end) = after.find('\'') {
            out.push(after[..end].to_string());
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    if out.is_empty() {
        let mut rest = msg;
        while let Some(start) = rest.find('"') {
            let after = &rest[start + 1..];
            if let Some(end) = after.find('"') {
                out.push(after[..end].to_string());
                rest = &after[end + 1..];
            } else {
                break;
            }
        }
    }
    out
}

/// IDENT_COLLISION guard: a mere substring must not fire. Require
/// word-boundary/token equality — tokens that are equal (casefolded) are a
/// real collision; when one token merely contains the other as a substring
/// (e.g. `rm` inside `immediately`) the finding is spurious.
fn is_spurious_ident_collision(item: &serde_json::Value) -> bool {
    let kind = finding_kind(item);
    if !kind.contains("collision") && !kind.contains("ident_collision") {
        return false;
    }
    let msg = item.get("message").and_then(|v| v.as_str()).unwrap_or("");
    /// F5: leading-char guard — a collision between tokens that share no
    /// leading character (e.g. `immediately` vs `rm`) is spurious regardless
    /// of substring relations. Real confusable collisions share a prefix.
    fn leading_chars_differ(a: &str, b: &str) -> bool {
        let fa = a.chars().find(|c| c.is_alphanumeric());
        let fb = b.chars().find(|c| c.is_alphanumeric());
        match (fa, fb) {
            (Some(x), Some(y)) => x.to_lowercase().next() != y.to_lowercase().next(),
            _ => false,
        }
    }
    if let Some(ids) = item.get("identifiers").and_then(|v| v.as_array()) {
        let tokens: Vec<String> = ids
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
            .collect();
        if tokens.len() >= 2 {
            for i in 0..tokens.len() {
                for j in (i + 1)..tokens.len() {
                    if tokens[i] == tokens[j] {
                        return false;
                    }
                }
            }
            for i in 0..tokens.len() {
                for j in (i + 1)..tokens.len() {
                    if leading_chars_differ(&tokens[i], &tokens[j]) {
                        return true;
                    }
                    if tokens[i].contains(&tokens[j]) || tokens[j].contains(&tokens[i]) {
                        return true;
                    }
                }
            }
            // Tokens that are neither equal nor substring-related and share
            // no other similarity signal are still spurious when they share
            // no leading character path; otherwise leave to upstream.
            return false;
        }
    }
    let tokens = quoted_tokens(msg);
    if tokens.len() < 2 {
        return false;
    }
    let lowered: Vec<String> = tokens.iter().map(|t| t.to_lowercase()).collect();
    for i in 0..lowered.len() {
        for j in (i + 1)..lowered.len() {
            if lowered[i] == lowered[j] {
                return false;
            }
        }
    }
    for i in 0..lowered.len() {
        for j in (i + 1)..lowered.len() {
            if leading_chars_differ(&lowered[i], &lowered[j]) {
                return true;
            }
            let (short, long) = if lowered[i].len() <= lowered[j].len() {
                (&lowered[i], &lowered[j])
            } else {
                (&lowered[j], &lowered[i])
            };
            if short.is_empty() {
                continue;
            }
            if long.contains(short.as_str()) {
                return true;
            }
        }
    }
    false
}

fn is_single_char_ascii_confusable_finding(item: &serde_json::Value) -> bool {
    let kind = item
        .get("type")
        .or_else(|| item.get("kind"))
        .or_else(|| item.get("code"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    if !(kind.contains("confusable")) {
        return false;
    }
    // Message shape: "Text contains confusable character 'm' (looks like 'rn')".
    // Single-char ASCII noise is `'X'` where X is one ASCII char. Longer or
    // non-ASCII confusables stay as-is.
    if let Some(msg) = item.get("message").and_then(|v| v.as_str()) {
        if let Some(start) = msg.find('\'') {
            let rest = &msg[start + 1..];
            if let Some(end) = rest.find('\'') {
                let inner = &rest[..end];
                let mut chars = inner.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    if c.is_ascii() {
                        return true;
                    }
                }
            }
        }
        // Fallback: a bare "Confusable characters: 1" style count without a
        // quoted char is treated as single-char noise when no actionable
        // signal exists (the caller already established that).
        if msg.to_lowercase().contains("confusable") {
            return true;
        }
    } else {
        // No message to disprove single-char noise; demote when the kind is
        // confusable and the caller proved no actionable signal.
        return true;
    }
    false
}

fn demote_to_info(item: &mut serde_json::Value) {
    if let Some(obj) = item.as_object_mut() {
        obj.insert(
            "severity".to_string(),
            serde_json::Value::String("info".to_string()),
        );
        obj.insert(
            "disposition".to_string(),
            serde_json::Value::String("informational".to_string()),
        );
    }
}

fn has_remaining_actionable_finding(res: &serde_json::Value) -> bool {
    if let Some(arr) = res.get("findings").and_then(|v| v.as_array()) {
        for item in arr {
            if is_spurious_ident_collision(item) {
                continue;
            }
            let sev = item
                .get("severity")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            if sev != "info" && sev != "informational" {
                return true;
            }
            let kind = item
                .get("code")
                .or_else(|| item.get("type"))
                .or_else(|| item.get("kind"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            if !kind.contains("confusable") && !kind.is_empty() {
                return true;
            }
        }
    }
    false
}

/// Same rule as the preflight filter: actionable means mixed-script,
/// invisible, secret, INSTRUCTION_PHRASE, or a non-confusable finding type.
/// Verdict drivers are INSTRUCTION_PHRASE/secret/mixed_script/invisible
/// alone; confusable-only review falls to allow.
fn has_actionable_text_signal_for_deterministic(
    result: &crate::eggsact::adapter::EggsactCallResult,
) -> bool {
    let output = result.output.to_lowercase();
    for token in [
        "instruction_phrase",
        "prompt_injection",
        "secret",
        "credential",
        "api_key",
        "private_key",
        "hidden_char",
        "invisible",
        "zero_width",
        "mixed_script",
        "mixed-script",
        "homoglyph_attack",
        "high ",
        "verdict: block",
        "verdict:block",
    ] {
        if output.contains(token) {
            return true;
        }
    }
    let check_array = |arr: &[serde_json::Value]| -> bool {
        arr.iter().any(|item| {
            if is_spurious_ident_collision(item) {
                return false;
            }
            if let Some(kind) = item
                .get("type")
                .or_else(|| item.get("kind"))
                .or_else(|| item.get("code"))
                .and_then(|v| v.as_str())
            {
                let kind = kind.to_lowercase();
                if kind != "confusable"
                    && kind != "confusables"
                    && kind != "text_inspect_warning"
                    && !kind.is_empty()
                {
                    return true;
                }
            }
            if let Some(msg) = item.get("message").and_then(|v| v.as_str()) {
                let lower = msg.to_lowercase();
                for token in [
                    "instruction_phrase",
                    "secret",
                    "mixed_script",
                    "mixed-script",
                    "invisible",
                    "hidden_char",
                    "zero_width",
                ] {
                    if lower.contains(token) {
                        return true;
                    }
                }
            }
            false
        })
    };
    if let Some(arr) = result.findings.as_ref().and_then(|v| v.as_array()) {
        if check_array(arr) {
            return true;
        }
    }
    if let Some(arr) = result
        .result
        .as_ref()
        .and_then(|r| r.get("findings"))
        .and_then(|v| v.as_array())
    {
        if check_array(arr) {
            return true;
        }
    }
    false
}

/// Helper to build a simple `{ "type": "object", "properties": ..., "required": ... }` schema.
fn simple_schema(props: serde_json::Value, required: &[&str]) -> serde_json::Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
    })
}

/// Build the full set of eggsact-backed deterministic tools.
///
/// Returns `(always_visible, deferred)` where each entry is an `EggsactTool`.
/// Caller decides registration order in `ToolRegistry::with_options`.
pub fn build_eggsact_tools(runtime: Arc<EggsactRuntime>) -> (Vec<EggsactTool>, Vec<EggsactTool>) {
    let mut always_visible: Vec<EggsactTool> = Vec::new();
    let mut deferred: Vec<EggsactTool> = Vec::new();

    // ── Always-visible (expose_in_definitions = true) ────────────────

    always_visible.push(EggsactTool::new(
        "text_equal",
        "text_equal",
        "Compare two strings for equality under various modes (raw, normalized, casefolded, trimmed). Use when verifying text identity; prefer over regex for equality checks. `length_only` means the strings differ in length after exact, normalization, and casefold equality all failed, so it can mask an accompanying case or whitespace difference.",
        simple_schema(json!({
            "a": { "type": "string", "description": "First text string" },
            "b": { "type": "string", "description": "Second text string" },
        }), &["a", "b"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "text_diff_explain",
        "text_diff_explain",
        "Explain why two strings differ with Unicode-aware span analysis, codepoints, confusables, and normalization equivalence. Use when diagnosing subtle text mismatches.",
        simple_schema(json!({
            "a": { "type": "string", "description": "First text string" },
            "b": { "type": "string", "description": "Second text string" },
        }), &["a", "b"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "text_replace_check",
        "text_replace_check",
        "Check whether a text replacement would apply cleanly before editing. Reports match count, positions, ambiguity, and before/after preview. Use before calling edit/apply_patch.",
        simple_schema(json!({
            "text": { "type": "string", "description": "Full source text" },
            "old": { "type": "string", "description": "Text to find" },
            "new": { "type": "string", "description": "Replacement text" },
        }), &["text", "old", "new"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "validate_json",
        "validate_json",
        "Validate JSON syntax and report precise parse errors or structure info. Use before writing or editing JSON files. Pass real newlines, not literal backslash-n sequences. `ok` is transport (tool ran); validity is result.valid/verdict.",
        simple_schema(json!({
            "text": { "type": "string", "description": "JSON text to validate" },
        }), &["text"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "validate_toml",
        "validate_toml",
        "Validate TOML files (Cargo.toml, pyproject.toml, etc.) and report parse errors with line/column. Use before writing or editing TOML. Pass real newlines, not literal backslash-n sequences.",
        simple_schema(json!({
            "text": { "type": "string", "description": "TOML text to validate" },
        }), &["text"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "command_preflight",
        "command_preflight",
        "Gating a command you are about to run -> preflight. Analyze a shell command before execution: parse argv, detect features (network, filesystem, env, process), find risk patterns, and return an ADVISORY verdict; never blocks except typed contract failures under block_on_definite. Use before running bash commands. Canonical pair: preflight gates shell risk before bash; for secrets/supply-chain/CVE use the security tool (classify_command shares the shell-risk signal with a different envelope).",
        simple_schema(json!({
            "command": { "type": "string", "description": "Shell command to analyze" },
        }), &["command"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "path_normalize",
        "path_normalize",
        "Normalize a filesystem path: collapse dot segments, resolve components. Use when comparing or validating file paths.",
        simple_schema(json!({
            "path": { "type": "string", "description": "Path to normalize" },
        }), &["path"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    always_visible.push(EggsactTool::new(
        "text_security_inspect",
        "text_security_inspect",
        "Security-oriented text hygiene pass: detect hidden chars, confusables, mixed scripts, normalization issues, and prompt injection patterns. Returns allow/review/block verdict. Use when handling untrusted text. Single-character ASCII confusable matches on plain English are informational noise; prioritize INSTRUCTION_PHRASE and secret-exposure findings over them.",
        simple_schema(json!({
            "text": { "type": "string", "description": "Text to inspect for security issues" },
        }), &["text"]),
        ToolCategory::ReadOnly,
        false,
        true,
        runtime.clone(),
    ));

    // ── Deferred / contextual (discoverable via tool_search only) ─────

    deferred.push(EggsactTool::new(
        "text_inspect",
        "text_inspect",
        "Inspect a string for hidden characters, Unicode confusables, mixed scripts, normalization state, and display-safe representation.",
        simple_schema(json!({
            "text": { "type": "string", "description": "Text to inspect" },
        }), &["text"]),
        ToolCategory::ReadOnly,
        true,
        true,
        runtime.clone(),
    ));

    deferred.push(EggsactTool::new(
        "config_preflight",
        "config_preflight",
        "Validate generated config text. Auto-detects format (JSON, TOML, dotenv/INI, cargo_toml) and runs the appropriate validator. YAML is not supported; YAML input is reported as unsupported rather than JSON. Returns valid/invalid with parse error details. `ok` is transport (tool ran); validity is result.valid/verdict.",
        simple_schema(json!({
            "text": { "type": "string", "description": "Config text to validate" },
        }), &["text"]),
        ToolCategory::ReadOnly,
        true,
        true,
        runtime.clone(),
    ));

    deferred.push(EggsactTool::new(
        "identifier_inspect",
        "identifier_inspect",
        "Inspect identifiers for validity and collisions. Detects confusables, mixed scripts, normalization issues, and casefold collisions across a list of identifiers.",
        simple_schema(json!({
            "identifiers": { "type": "array", "items": { "type": "string" }, "description": "List of identifiers to inspect" },
        }), &["identifiers"]),
        ToolCategory::ReadOnly,
        true,
        true,
        runtime.clone(),
    ));

    deferred.push(EggsactTool::new(
        "structured_data_compare",
        "structured_data_compare",
        "Compare structured config/data output (JSON). Calls canonicalization and shape comparison. Returns equal/not-equal verdict with structured diffs.",
        simple_schema(json!({
            "a": { "type": "string", "description": "First data string (JSON)" },
            "b": { "type": "string", "description": "Second data string (JSON)" },
        }), &["a", "b"]),
        ToolCategory::ReadOnly,
        true,
        true,
        runtime.clone(),
    ));

    deferred.push(EggsactTool::new(
        "text_fingerprint",
        "text_fingerprint",
        "Compute a deterministic SHA-256 fingerprint of text with canonicalization options. Use for deduplication or change detection.",
        simple_schema(json!({
            "text": { "type": "string", "description": "Text to fingerprint" },
        }), &["text"]),
        ToolCategory::ReadOnly,
        true,
        true,
        runtime.clone(),
    ));

    // ── Deferred math (R19): upstream MATH_TOOLS lacks the `codegg_core`
    // profile, so the default runtime cannot see them. Bind these two to a
    // math-capable (`full`) runtime so agents stop doing arithmetic by LLM
    // guess or by spawning Python for `2+2`. Descriptions carry
    // math/calculator/arithmetic/calc/units keywords for `tool_search`.
    let math_runtime = {
        let base = runtime.as_ref().config().clone();
        let math_config = crate::eggsact::adapter::EggsactConfig {
            profile: "full".to_string(),
            audience: base.audience.clone(),
            max_output_chars: base.max_output_chars,
        };
        match crate::eggsact::adapter::EggsactRuntime::new(math_config) {
            Ok(rt) => Arc::new(rt),
            Err(_) => runtime.clone(),
        }
    };

    deferred.push(EggsactTool::new(
        "math_eval",
        "math_eval",
        "Evaluate arithmetic, calculator, and scientific expressions deterministically (math calc arithmetic). Handles units and constants in the expression; state-mutating and random functions are disabled. Use for math and unit tasks instead of asking the model to calculate or spawning python_script for 2+2.",
        simple_schema(json!({
            "expression": { "type": "string", "description": "Math expression to evaluate (e.g. '2+2*2', 'sqrt(144)', '30m + 100ft')" },
        }), &["expression"]),
        ToolCategory::ReadOnly,
        true,
        true,
        math_runtime.clone(),
    ));

    deferred.push(EggsactTool::new(
        "unit_convert",
        "unit_convert",
        "Convert a numeric value between units deterministically (units conversion calc). Uses pre-defined conversion factors; use for km/m/ft/in/kg/lb and temperature conversions instead of guessing.",
        simple_schema(json!({
            "value": { "type": "number", "description": "Numeric value to convert (must be finite)" },
            "from_unit": { "type": "string", "description": "Source unit (e.g. 'km', 'ft', 'kg')" },
            "to_unit": { "type": "string", "description": "Target unit (e.g. 'm', 'in', 'lb')" },
        }), &["value", "from_unit", "to_unit"]),
        ToolCategory::ReadOnly,
        true,
        true,
        math_runtime,
    ));

    (always_visible, deferred)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptions() -> Vec<(String, String)> {
        let runtime = Arc::new(
            crate::eggsact::adapter::EggsactRuntime::new(
                crate::eggsact::adapter::EggsactConfig::default(),
            )
            .expect("eggsact runtime"),
        );
        let (visible, deferred) = build_eggsact_tools(runtime);
        visible
            .into_iter()
            .chain(deferred)
            .map(|tool| (tool.name().to_string(), tool.description().to_string()))
            .collect()
    }

    #[test]
    fn text_equal_description_documents_length_only_masking() {
        let map: std::collections::HashMap<_, _> = descriptions().into_iter().collect();
        assert!(map["text_equal"].contains("length_only"));
        assert!(map["text_equal"].contains("case or whitespace"));
    }

    #[test]
    fn text_security_inspect_description_flags_ascii_confusable_noise() {
        let map: std::collections::HashMap<_, _> = descriptions().into_iter().collect();
        assert!(map["text_security_inspect"].contains("Single-character ASCII"));
        assert!(map["text_security_inspect"].contains("INSTRUCTION_PHRASE"));
    }

    #[test]
    fn command_preflight_description_leads_with_decision_rule() {
        let map: std::collections::HashMap<_, _> = descriptions().into_iter().collect();
        assert!(
            map["command_preflight"]
                .starts_with("Gating a command you are about to run -> preflight"),
            "preflight description must lead with the decision rule, got: {}",
            map["command_preflight"]
        );
    }

    #[test]
    fn text_security_noise_filter_demotes_single_char_ascii_to_allow() {
        let mut result = crate::eggsact::adapter::EggsactCallResult {
            output: "ok: true\nverdict: review\nconfusable: 'm' looks like 'rn'".to_string(),
            success: true,
            elapsed_ms: 1,
            truncated: false,
            machine_code: None,
            result: Some(serde_json::json!({
                "verdict": "review",
                "findings": [
                    {"type": "confusable", "severity": "medium", "message": "Text contains confusable character 'm' (looks like 'rn')"}
                ]
            })),
            findings: Some(serde_json::json!([
                {"type": "confusable", "severity": "medium", "message": "Text contains confusable character 'm' (looks like 'rn')"}
            ])),
            warnings: None,
            error_type: None,
            error: None,
        };
        apply_text_security_noise_filter(&mut result);
        let verdict = result
            .result
            .as_ref()
            .and_then(|r| r.get("verdict"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert_eq!(
            verdict, "allow",
            "single-char ASCII noise with no actionable signal must fall to allow"
        );
        assert!(
            result.output.contains("verdict: allow"),
            "envelope must stay consistent with the demoted verdict, got: {}",
            result.output
        );
    }

    #[test]
    fn text_security_noise_filter_preserves_instruction_phrase() {
        let mut result = crate::eggsact::adapter::EggsactCallResult {
            output:
                "ok: true\nverdict: review\nconfusable: 'm'\nINSTRUCTION_PHRASE: ignore previous"
                    .to_string(),
            success: true,
            elapsed_ms: 1,
            truncated: false,
            machine_code: None,
            result: Some(serde_json::json!({
                "verdict": "review",
                "findings": [
                    {"type": "confusable", "severity": "medium", "message": "Text contains confusable character 'm'"},
                    {"type": "prompt_injection", "severity": "high", "message": "INSTRUCTION_PHRASE: ignore previous"}
                ]
            })),
            findings: Some(serde_json::json!([
                {"type": "prompt_injection", "message": "INSTRUCTION_PHRASE"}
            ])),
            warnings: None,
            error_type: None,
            error: None,
        };
        apply_text_security_noise_filter(&mut result);
        let verdict = result
            .result
            .as_ref()
            .and_then(|r| r.get("verdict"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert_eq!(
            verdict, "review",
            "actionable INSTRUCTION_PHRASE must survive the noise filter"
        );
    }

    #[test]
    fn ok_is_transport_sentence_present() {
        let map: std::collections::HashMap<_, _> = descriptions().into_iter().collect();
        for tool in ["validate_json", "config_preflight"] {
            assert!(
                map[tool].contains("`ok` is transport"),
                "{tool} description must carry the ok-is-transport sentence, got: {}",
                map[tool]
            );
        }
        let runtime = Arc::new(
            crate::eggsact::adapter::EggsactRuntime::new(
                crate::eggsact::adapter::EggsactConfig::default(),
            )
            .expect("eggsact runtime"),
        );
        let tool = super::EggsactTool::new(
            "probe",
            "text_equal",
            "probe",
            serde_json::json!({"type": "object"}),
            crate::tool::ToolCategory::ReadOnly,
            false,
            true,
            runtime,
        );
        let schema = tool.contract("probe", serde_json::json!({"type": "object"}));
        let desc = schema.output_schema.expect("schema").to_string();
        assert!(
            desc.contains("`ok` is transport"),
            "contract output schema must carry the ok-is-transport sentence, got: {desc}"
        );
    }

    #[test]
    fn noise_filter_dedups_identical_findings_before_demotion() {
        let finding = serde_json::json!({
            "type": "confusable",
            "severity": "medium",
            "message": "Text contains confusable character 'm' (looks like 'rn')"
        });
        let mut result = crate::eggsact::adapter::EggsactCallResult {
            output: "ok: true\nverdict: review\nconfusable: 'm'".to_string(),
            success: true,
            elapsed_ms: 1,
            truncated: false,
            machine_code: None,
            result: Some(serde_json::json!({
                "verdict": "review",
                "findings": [finding.clone(), finding.clone()]
            })),
            findings: Some(serde_json::json!([finding.clone(), finding])),
            warnings: None,
            error_type: None,
            error: None,
        };
        apply_text_security_noise_filter(&mut result);
        let count = result
            .result
            .as_ref()
            .and_then(|r| r.get("findings"))
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(99);
        assert_eq!(
            count, 1,
            "identical (kind, message, span) findings must dedup, got {count}"
        );
        let verdict = result
            .result
            .as_ref()
            .and_then(|r| r.get("verdict"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert_eq!(verdict, "allow");
    }

    #[test]
    fn ident_collision_substring_does_not_fire() {
        let mut result = crate::eggsact::adapter::EggsactCallResult {
            output: "ok: true\nverdict: review\nIDENT_COLLISION 'immediately' vs 'rm'".to_string(),
            success: true,
            elapsed_ms: 1,
            truncated: false,
            machine_code: None,
            result: Some(serde_json::json!({
                "verdict": "review",
                "findings": [
                    {"code": "IDENT_COLLISION", "severity": "medium", "message": "Identifiers 'immediately' and 'rm' collide"}
                ]
            })),
            findings: Some(serde_json::json!([
                {"code": "IDENT_COLLISION", "severity": "medium", "message": "Identifiers 'immediately' and 'rm' collide"}
            ])),
            warnings: None,
            error_type: None,
            error: None,
        };
        assert!(
            is_spurious_ident_collision(&result.result.as_ref().unwrap()["findings"][0]),
            "substring rm inside immediately must be spurious"
        );
        apply_text_security_noise_filter(&mut result);
        let verdict = result
            .result
            .as_ref()
            .and_then(|r| r.get("verdict"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let sev = result
            .result
            .as_ref()
            .and_then(|r| r.get("findings"))
            .and_then(|v| v.as_array())
            .map(|a| {
                a[0].get("severity")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string()
            })
            .unwrap_or_default();
        assert_eq!(sev, "info");
        assert_eq!(
            verdict, "allow",
            "spurious-only collision must not drive review"
        );
    }

    #[test]
    fn ident_collision_token_equality_is_not_spurious() {
        let item = serde_json::json!({
            "code": "IDENT_COLLISION",
            "severity": "medium",
            "message": "Identifiers 'Admin' and 'admin' collide"
        });
        assert!(
            !is_spurious_ident_collision(&item),
            "casefold-equal tokens are a real collision"
        );
    }
}
