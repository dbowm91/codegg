//! Chat body, reference, action validation, and audit-safe redaction.

use std::collections::BTreeMap;

use super::{
    ChatAction, ChatMessage, ChatReference, CollaborationConfig, CollaborationError,
    REDACTED_MARKER,
};
use crate::identity::PrincipalId;

// ── Input validation and redaction ─────────────────────────────────────

/// Lowercase substrings that mark surrounding text as credential-like.
///
/// Bodies are stored with values redacted, never rejected, so a client
/// that accidentally pastes a secret does not lose the message — but the
/// secret never reaches durable storage in cleartext.
const SECRET_KEY_SUBSTRINGS: [&str; 13] = [
    "password",
    "passwd",
    "secret",
    "api_key",
    "apikey",
    "access_token",
    "refresh_token",
    "private_key",
    "client_secret",
    "auth_token",
    "session_token",
    "bearer",
    "token",
];

/// High-confidence token prefixes redacted even without a key name.
const SECRET_TOKEN_PREFIXES: [&str; 7] = ["AKIA", "ghp_", "gho_", "ghu_", "ghs_", "sk-", "xoxb-"];

fn is_token_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

/// Replace credential-like values in `body` with [`REDACTED_MARKER`].
///
/// Returns the (possibly rewritten) body and whether any redaction
/// occurred. `key=value` / `key: value` values run to the next
/// whitespace; bare high-confidence token prefixes redact the attached
/// token run. The scan is deliberately conservative: it over-redacts
/// adjacent text rather than risk persisting a secret.
pub fn redact_secrets_in_body(body: &str) -> (String, bool) {
    let mut output = body.to_owned();
    let mut redacted = false;

    // Redact key=value / key: value pairs case-insensitively. Each pass
    // rescans the current output so offsets stay valid across mutations
    // and several distinct keys in one body are all covered. Bare key
    // mentions without an assignment operator (prose like "the api_key
    // field") are left alone.
    for key in SECRET_KEY_SUBSTRINGS {
        let mut search_from = 0;
        loop {
            let lowered = output.to_lowercase();
            let base = search_from.min(lowered.len());
            let Some(found) = lowered[base..].find(key) else {
                break;
            };
            let key_start = base + found;
            let after_key = &output[key_start + key.len()..];
            // Allow separators between the key and the value.
            let mut sep_len = 0;
            for ch in after_key.chars() {
                if ch == ' ' || ch == '\t' || ch == '"' || ch == '\'' {
                    sep_len += ch.len_utf8();
                } else if ch == '=' || ch == ':' {
                    sep_len += ch.len_utf8();
                    break;
                } else {
                    break;
                }
            }
            let had_operator = after_key[..sep_len].contains(['=', ':']);
            if !had_operator {
                search_from = key_start + key.len();
                continue;
            }
            let mut rest = &after_key[sep_len..];
            while rest.starts_with([' ', '\t', '"', '\'']) {
                rest = &rest[1..];
            }
            if rest.is_empty() {
                search_from = key_start + key.len();
                continue;
            }
            let value_len = rest
                .find([' ', '\t', '\n', '\r', '"', '\'', ',', ';'])
                .unwrap_or(rest.len());
            if value_len == 0 {
                search_from = key_start + key.len();
                continue;
            }
            let value_start = output.len() - rest.len();
            let value_end = value_start + value_len;
            output.replace_range(value_start..value_end, REDACTED_MARKER);
            redacted = true;
            search_from = value_start + REDACTED_MARKER.len();
        }
    }
    redact_token_prefixes(&output, redacted)
}

fn redact_token_prefixes(body: &str, mut redacted: bool) -> (String, bool) {
    let mut output = body.to_owned();
    for prefix in SECRET_TOKEN_PREFIXES {
        let mut search_from = 0;
        while let Some(found) = output[search_from..].find(prefix) {
            let token_start = search_from + found;
            let bytes = output.as_bytes();
            let mut token_end = token_start + prefix.len();
            while token_end < bytes.len() && is_token_char(bytes[token_end]) {
                token_end += 1;
            }
            // Require a minimum token tail so ordinary words sharing a
            // two-letter prefix are not clobbered.
            if token_end - token_start < prefix.len() + 4 {
                search_from = token_start + prefix.len();
                continue;
            }
            output.replace_range(token_start..token_end, REDACTED_MARKER);
            redacted = true;
            search_from = token_start + REDACTED_MARKER.len();
        }
    }
    (output, redacted)
}

pub(super) fn validate_body(
    config: &CollaborationConfig,
    body: &str,
) -> Result<String, CollaborationError> {
    if body.is_empty() {
        return Err(CollaborationError::invalid(
            "body",
            "message body must not be empty",
        ));
    }
    if body.len() > config.max_body_bytes {
        return Err(CollaborationError::BodyTooLarge {
            bytes: body.len(),
            max: config.max_body_bytes,
        });
    }
    if body.bytes().any(|b| b == 0) {
        return Err(CollaborationError::invalid(
            "body",
            "message body must not contain NUL",
        ));
    }
    if body
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(CollaborationError::invalid(
            "body",
            "message body contains an unsupported control character",
        ));
    }
    let (redacted, _) = redact_secrets_in_body(body);
    Ok(redacted)
}

pub(super) fn validate_channel_name(
    config: &CollaborationConfig,
    name: &str,
) -> Result<String, CollaborationError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CollaborationError::invalid(
            "channel_name",
            "channel name must not be empty",
        ));
    }
    if trimmed.len() > config.max_channel_name_len {
        return Err(CollaborationError::invalid(
            "channel_name",
            format!("channel name exceeds {} bytes", config.max_channel_name_len),
        ));
    }
    if trimmed.bytes().any(|b| b == 0) || trimmed.chars().any(char::is_control) {
        return Err(CollaborationError::invalid(
            "channel_name",
            "channel name contains an unsupported character",
        ));
    }
    Ok(trimmed.to_owned())
}

pub fn validate_idempotency_key(
    config: &CollaborationConfig,
    key: &str,
) -> Result<String, CollaborationError> {
    if key.is_empty() || key.len() > config.max_idempotency_key_len {
        return Err(CollaborationError::invalid(
            "idempotency_key",
            format!(
                "idempotency key must be 1..={} bytes",
                config.max_idempotency_key_len
            ),
        ));
    }
    crate::identity::validate_identity("idempotency_key", key).map_err(|_| {
        CollaborationError::invalid(
            "idempotency_key",
            "idempotency key contains an unsupported character",
        )
    })?;
    Ok(key.to_owned())
}

/// Validate an action idempotency key against the default bounds.
/// Daemon-side fast path for malformed retries before touching durable
/// state; full validation (with the live config) happens in
/// [`CollaborationService::insert_action`].
pub fn validate_idempotency_key_for_action(key: &str) -> Result<String, CollaborationError> {
    validate_idempotency_key(&CollaborationConfig::default(), key)
}

pub(super) fn validate_author_agent(
    config: &CollaborationConfig,
    agent: &str,
) -> Result<String, CollaborationError> {
    if agent.is_empty() || agent.len() > config.max_author_agent_len {
        return Err(CollaborationError::invalid(
            "author_agent",
            format!(
                "author agent label must be 1..={} bytes",
                config.max_author_agent_len
            ),
        ));
    }
    if agent.bytes().any(|b| b == 0) || agent.chars().any(char::is_control) {
        return Err(CollaborationError::invalid(
            "author_agent",
            "author agent label contains an unsupported character",
        ));
    }
    Ok(agent.to_owned())
}

pub(super) fn validate_mentions(
    config: &CollaborationConfig,
    mentions: &[String],
) -> Result<Vec<PrincipalId>, CollaborationError> {
    if mentions.len() > config.max_mentions {
        return Err(CollaborationError::invalid(
            "mentions",
            format!("message carries more than {} mentions", config.max_mentions),
        ));
    }
    let mut parsed = Vec::with_capacity(mentions.len());
    for raw in mentions {
        match PrincipalId::parse(raw) {
            Ok(id) => {
                if !parsed.contains(&id) {
                    parsed.push(id);
                }
            }
            Err(error) => {
                return Err(CollaborationError::invalid(
                    "mentions",
                    format!("invalid mention identity: {error}"),
                ));
            }
        }
    }
    Ok(parsed)
}

fn sanitize_display_hint(hint: Option<String>) -> Option<String> {
    let hint = hint.filter(|h| !h.trim().is_empty())?;
    let truncated: String = hint.chars().take(200).collect();
    let (redacted, _) = redact_secrets_in_body(&truncated);
    let cleaned: String = redacted
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
        .collect();
    let trimmed = cleaned.trim().to_owned();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

pub(super) fn validate_references(
    config: &CollaborationConfig,
    references: &[codegg_protocol::core::ChatReferenceDto],
) -> Result<Vec<ChatReference>, CollaborationError> {
    if references.len() > config.max_references {
        return Err(CollaborationError::invalid(
            "references",
            format!(
                "message carries more than {} references",
                config.max_references
            ),
        ));
    }
    let mut parsed = Vec::with_capacity(references.len());
    for reference in references {
        if reference.target_id.is_empty() || reference.target_id.len() > 128 {
            return Err(CollaborationError::invalid(
                "references",
                "reference target must be 1..=128 bytes",
            ));
        }
        crate::identity::validate_identity("chat_reference_target", &reference.target_id).map_err(
            |_| {
                CollaborationError::invalid(
                    "references",
                    "reference target contains an unsupported character",
                )
            },
        )?;
        // Unknown wire kinds are rejected at the serde layer before this
        // runs; hints are truncated and secret-scanned here.
        let mut parsed_reference = ChatReference::from_dto(reference);
        parsed_reference.display_hint = sanitize_display_hint(parsed_reference.display_hint);
        parsed.push(parsed_reference);
    }
    Ok(parsed)
}

/// Structural audit metadata for one message.
///
/// Bodies, hints, and mentions never enter audit metadata: only channel,
/// message, project, sequence, and revision locators plus the redaction
/// flag. Chat storage remains separate from the audit store; this map is
/// the structural hook future audit linkage consumes.
pub fn audit_metadata_for_message(message: &ChatMessage) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "chat.channel_id".to_owned(),
        message.channel_id.as_str().to_owned(),
    );
    metadata.insert("chat.message_id".to_owned(), message.id.as_str().to_owned());
    metadata.insert(
        "chat.project_id".to_owned(),
        message.project_id.as_str().to_owned(),
    );
    metadata.insert("chat.seq".to_owned(), message.seq.to_string());
    metadata.insert("chat.revision".to_owned(), message.revision.to_string());
    metadata.insert("chat.redacted".to_owned(), message.redacted.to_string());
    metadata
}

// ── M003: action input validation ────────────────────────────────────

/// Validate an optional action title: bounded, no NUL/control, no
/// credential-like material (fail closed, no side effect). Returns the
/// trimmed title or `None` when absent/blank.
pub fn validate_action_title(
    config: &CollaborationConfig,
    title: Option<&str>,
) -> Result<Option<String>, CollaborationError> {
    let Some(raw) = title else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > config.max_action_title_bytes {
        return Err(CollaborationError::invalid(
            "action_title",
            format!(
                "action title exceeds {} bytes",
                config.max_action_title_bytes
            ),
        ));
    }
    if trimmed.bytes().any(|b| b == 0)
        || trimmed
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(CollaborationError::invalid(
            "action_title",
            "action title contains an unsupported character",
        ));
    }
    let (_, secret) = redact_secrets_in_body(trimmed);
    if secret {
        return Err(CollaborationError::invalid(
            "action_title",
            "action title must not carry credential-like material",
        ));
    }
    Ok(Some(trimmed.to_owned()))
}

/// Validate an agent-task prompt: bounded inert text, no secrets.
/// Prompts are task descriptions, not chat bodies: secrets are
/// rejected (not redacted) so a redacted prompt never silently
/// changes the work that will run.
pub fn validate_action_prompt(
    config: &CollaborationConfig,
    prompt: &str,
) -> Result<String, CollaborationError> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return Err(CollaborationError::invalid(
            "action_prompt",
            "action prompt must not be empty",
        ));
    }
    if trimmed.len() > config.max_action_prompt_bytes {
        return Err(CollaborationError::invalid(
            "action_prompt",
            format!(
                "action prompt exceeds {} bytes",
                config.max_action_prompt_bytes
            ),
        ));
    }
    if trimmed.bytes().any(|b| b == 0)
        || trimmed
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(CollaborationError::invalid(
            "action_prompt",
            "action prompt contains an unsupported character",
        ));
    }
    let (_, secret) = redact_secrets_in_body(trimmed);
    if secret {
        return Err(CollaborationError::invalid(
            "action_prompt",
            "action prompt must not carry credential-like material",
        ));
    }
    Ok(trimmed.to_owned())
}

/// Validate an agent name for an action: bounded identity, no secrets.
pub fn validate_action_agent(
    config: &CollaborationConfig,
    agent: &str,
) -> Result<String, CollaborationError> {
    let trimmed = agent.trim();
    if trimmed.is_empty() || trimmed.len() > config.max_action_agent_len {
        return Err(CollaborationError::invalid(
            "action_agent",
            format!(
                "action agent must be 1..={} bytes",
                config.max_action_agent_len
            ),
        ));
    }
    crate::identity::validate_identity("action_agent", trimmed).map_err(|_| {
        CollaborationError::invalid(
            "action_agent",
            "action agent contains an unsupported character",
        )
    })?;
    let (_, secret) = redact_secrets_in_body(trimmed);
    if secret {
        return Err(CollaborationError::invalid(
            "action_agent",
            "action agent must not carry credential-like material",
        ));
    }
    Ok(trimmed.to_owned())
}

/// Validate a workspace locator for an action: bounded identity, no secrets.
pub fn validate_action_workspace(workspace_id: &str) -> Result<String, CollaborationError> {
    let trimmed = workspace_id.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return Err(CollaborationError::invalid(
            "action_workspace",
            "action workspace must be 1..=128 bytes",
        ));
    }
    crate::identity::validate_identity("action_workspace", trimmed).map_err(|_| {
        CollaborationError::invalid(
            "action_workspace",
            "action workspace contains an unsupported character",
        )
    })?;
    Ok(trimmed.to_owned())
}

/// Validate a job locator for a reference action.
pub fn validate_action_job_id(job_id: &str) -> Result<String, CollaborationError> {
    let trimmed = job_id.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return Err(CollaborationError::invalid(
            "action_job",
            "action job id must be 1..=128 bytes",
        ));
    }
    crate::identity::validate_identity("action_job", trimmed).map_err(|_| {
        CollaborationError::invalid(
            "action_job",
            "action job id contains an unsupported character",
        )
    })?;
    Ok(trimmed.to_owned())
}

/// Structural audit metadata for one chat action.
///
/// Titles/prompts never enter audit metadata: only channel, message,
/// action, project, kind, job, actor, and status locators plus the
/// decision outcome. Bodies stay in the canonical job store; chat
/// storage remains separate from the audit store.
pub fn audit_metadata_for_action(action: &ChatAction) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "chat.channel".to_owned(),
        action.channel_id.as_str().to_owned(),
    );
    metadata.insert(
        "chat.message".to_owned(),
        action.message_id.as_str().to_owned(),
    );
    metadata.insert("chat.action".to_owned(), action.id.clone());
    metadata.insert(
        "chat.action_kind".to_owned(),
        action.kind.as_str().to_owned(),
    );
    metadata.insert(
        "chat.project".to_owned(),
        action.project_id.as_str().to_owned(),
    );
    if let Some(job) = action.job_id.as_deref() {
        metadata.insert("job.id".to_owned(), job.to_owned());
    }
    metadata.insert("chat.status".to_owned(), action.status.clone());
    metadata.insert("decision.outcome".to_owned(), "allow".to_owned());
    metadata
}
