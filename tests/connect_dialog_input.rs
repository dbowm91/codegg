//! `/connect` dialog input behavior: vim keys on text fields, paste
//! sanitization at the secret boundary, and single-option credential
//! auto-advance.
//!
//! Regression cover for three reported `/connect` defects:
//!  - `j`/`k` were swallowed on every text-input step, so a display name or
//!    host containing them silently lost those characters.
//!  - A pasted credential carrying the source line's trailing newline was
//!    rejected by `SecretInput::new` ("API key is invalid") even though the
//!    key itself was correct.
//!  - A provider admitting a single credential kind must advance straight
//!    into secret entry instead of costing an extra Enter on a one-option
//!    choice step.

use codegg::protocol::provider::{ProviderSetupEntryDto, SecretInput};
use codegg::tui::components::component::Component;
use codegg::tui::components::dialogs::connect::{ConnectDialog, ConnectStep};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;

/// Synthetic credential with the same shape as a real gateway key. Never a
/// live secret.
const PASTED_KEY: &str = "sk-TESTpaste0Placeholder0Key0For0Regression0Only";

fn theme() -> Arc<codegg::tui::theme::Theme> {
    Arc::new(codegg::tui::theme::Theme::dark())
}

fn dto(id: &str, kinds: &[&str], policy: &str) -> ProviderSetupEntryDto {
    ProviderSetupEntryDto {
        id: id.to_string(),
        display_name: id.to_uppercase(),
        description: String::new(),
        connectable: true,
        credential_kinds: kinds.iter().map(|k| (*k).to_string()).collect(),
        endpoint_policy: policy.to_string(),
        default_endpoint: Some("https://example.test/v1".to_string()),
        requires_endpoint: false,
        env_var: None,
    }
}

/// A dialog already positioned on `step`, carrying one selectable provider.
fn dialog_at(
    step: ConnectStep,
    entries: &[ProviderSetupEntryDto],
    selected: usize,
) -> ConnectDialog {
    let mut dialog = ConnectDialog::new(Vec::new(), theme());
    dialog.set_setup_entries(entries);
    dialog.set_selected(selected);
    dialog.step = step;
    dialog
}

fn press(dialog: &mut ConnectDialog, code: KeyCode) {
    dialog.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn press_char(dialog: &mut ConnectDialog, c: char) {
    press(dialog, KeyCode::Char(c));
}

/// The buffer the step is currently writing into.
fn field(dialog: &ConnectDialog) -> String {
    if dialog.is_secret_input_step() {
        dialog.get_api_key()
    } else {
        dialog.form_input.clone()
    }
}

#[test]
fn vim_navigation_keys_insert_characters_on_every_text_step() {
    let entries = vec![dto("openai", &["api_key"], "fixed")];
    for step in [
        ConnectStep::EnterEndpoint,
        ConnectStep::EnterApiKey,
        ConnectStep::EnterDisplayName,
        ConnectStep::EnterHost,
        ConnectStep::EnterPort,
    ] {
        let mut dialog = dialog_at(step.clone(), &entries, 0);
        // A display name like "jack" or a host containing "k" must survive.
        for c in "jkgh".chars() {
            press_char(&mut dialog, c);
        }
        assert_eq!(
            field(&dialog),
            "jkgh",
            "{step:?} must accept j/k/g/h as ordinary characters"
        );
    }
}

#[test]
fn vim_navigation_keys_still_move_selections() {
    let entries = vec![
        dto("openai", &["api_key"], "fixed"),
        dto("mistral", &["api_key"], "fixed"),
    ];

    // Provider list: j/k still navigate.
    let mut dialog = dialog_at(ConnectStep::SelectProvider, &entries, 0);
    press_char(&mut dialog, 'j');
    assert_eq!(dialog.selected, 1, "j moves the provider selection down");
    press_char(&mut dialog, 'k');
    assert_eq!(dialog.selected, 0, "k moves the provider selection up");

    // Scope choice: arrows (and j/k) toggle the single binary option.
    let mut dialog = dialog_at(ConnectStep::SelectScope, &entries, 0);
    assert!(dialog.scope_personal);
    press(&mut dialog, KeyCode::Down);
    assert!(
        !dialog.scope_personal,
        "scope choice responds to navigation"
    );
    press(&mut dialog, KeyCode::Up);
    assert!(dialog.scope_personal);
}

#[test]
fn pasted_credential_with_trailing_newline_is_accepted() {
    // Reproduces the reported failure: a terminal paste carries the source
    // line's newline, which made `SecretInput::new` reject the whole key and
    // surface a misleading "API key is invalid".
    let entries = vec![dto("opencode_go", &["api_key", "bearer"], "fixed")];
    let mut dialog = dialog_at(ConnectStep::EnterApiKey, &entries, 0);

    dialog.handle_paste(format!("{PASTED_KEY}\n"));

    let stored = dialog.get_api_key();
    assert_eq!(
        stored, PASTED_KEY,
        "trailing newline must not enter the credential"
    );
    assert!(
        !stored.contains('\n') && !stored.chars().any(char::is_control),
        "credential must be control-character free"
    );
    assert!(
        SecretInput::new(stored).is_ok(),
        "a correctly pasted key must pass the secret envelope"
    );
}

#[test]
fn pasted_secret_survives_surrounding_whitespace_and_crlf() {
    let entries = vec![dto("opencode_go", &["api_key", "bearer"], "fixed")];
    let mut dialog = dialog_at(ConnectStep::EnterApiKey, &entries, 0);

    dialog.handle_paste(format!("  {PASTED_KEY}\r\n "));

    assert_eq!(dialog.get_api_key(), PASTED_KEY);
    assert!(SecretInput::new(dialog.get_api_key()).is_ok());
}

#[test]
fn pasted_form_text_is_stripped_of_control_characters() {
    let entries = vec![dto("custom", &["api_key"], "required")];
    for step in [
        ConnectStep::EnterEndpoint,
        ConnectStep::EnterDisplayName,
        ConnectStep::EnterHost,
        ConnectStep::EnterPort,
    ] {
        let mut dialog = dialog_at(step.clone(), &entries, 0);
        dialog.handle_paste("https://example.test/v1\r\n".to_string());
        assert_eq!(
            dialog.form_input, "https://example.test/v1",
            "{step:?} paste must drop the trailing line break"
        );
    }
}

#[test]
fn empty_paste_is_a_no_op() {
    let entries = vec![dto("opencode_go", &["api_key", "bearer"], "fixed")];
    let mut dialog = dialog_at(ConnectStep::EnterApiKey, &entries, 0);

    dialog.handle_paste("\n \r\n".to_string());

    assert!(
        dialog.get_api_key().is_empty(),
        "a paste of only control characters must not create content"
    );
}

#[test]
fn single_kind_provider_skips_the_credential_choice_step() {
    let entries = vec![dto("openai", &["api_key"], "fixed")];
    let mut dialog = dialog_at(ConnectStep::SelectProvider, &entries, 0);

    press(&mut dialog, KeyCode::Enter);

    assert_eq!(
        dialog.step,
        ConnectStep::EnterApiKey,
        "a one-option provider must not cost an extra Enter"
    );
}

#[test]
fn single_kind_provider_also_skips_the_choice_after_an_endpoint() {
    let entries = vec![dto("custom", &["api_key"], "required")];
    let mut dialog = dialog_at(ConnectStep::EnterEndpoint, &entries, 0);
    dialog.form_input = "https://example.test/v1".to_string();

    assert!(dialog.next_after_endpoint());

    assert_eq!(dialog.step, ConnectStep::EnterApiKey);
}

#[test]
fn dual_kind_provider_still_offers_the_credential_choice() {
    // OpenCode Go admits both kinds and its auth shape depends on which was
    // stored, so the choice must remain available.
    let entries = vec![dto("opencode_go", &["api_key", "bearer"], "fixed")];
    let mut dialog = dialog_at(ConnectStep::SelectProvider, &entries, 0);

    press(&mut dialog, KeyCode::Enter);
    assert_eq!(dialog.step, ConnectStep::SelectCredentialKind);

    press(&mut dialog, KeyCode::Enter);
    assert_eq!(dialog.step, ConnectStep::EnterApiKey);
}

#[test]
fn bearer_only_provider_pins_the_bearer_kind_without_a_choice() {
    let entries = vec![dto("bearer_only", &["bearer"], "fixed")];
    let mut dialog = dialog_at(ConnectStep::SelectProvider, &entries, 0);

    press(&mut dialog, KeyCode::Enter);

    assert_eq!(dialog.step, ConnectStep::EnterApiKey);
    assert_eq!(
        dialog.credential_kind,
        codegg::protocol::provider::ProviderCredentialKind::Bearer,
        "the single admitted kind must be pinned, never defaulted to api_key"
    );
}

#[test]
fn pasted_key_survives_the_whole_connect_flow() {
    // End-to-end shape of the reported session: select provider, paste a key
    // that carried a trailing newline, walk the remaining steps to Review,
    // and confirm nothing rejected the credential along the way.
    let entries = vec![dto("opencode_go", &["api_key", "bearer"], "fixed")];
    let mut dialog = dialog_at(ConnectStep::SelectProvider, &entries, 0);

    press(&mut dialog, KeyCode::Enter);
    assert_eq!(dialog.step, ConnectStep::SelectCredentialKind);
    press(&mut dialog, KeyCode::Enter);
    assert_eq!(dialog.step, ConnectStep::EnterApiKey);

    dialog.handle_paste(format!("{PASTED_KEY}\n"));
    press(&mut dialog, KeyCode::Enter);
    assert_eq!(
        dialog.step,
        ConnectStep::EnterDisplayName,
        "a correctly pasted key must advance the form"
    );

    // The display name legitimately contains j and k.
    for c in "jack".chars() {
        press_char(&mut dialog, c);
    }
    assert_eq!(dialog.form_input, "jack");
    press(&mut dialog, KeyCode::Enter);
    assert_eq!(dialog.step, ConnectStep::SelectScope);
    press(&mut dialog, KeyCode::Enter);
    assert_eq!(dialog.step, ConnectStep::Review);

    assert_eq!(dialog.error_message, None, "flow completed without error");
    assert!(SecretInput::new(dialog.get_api_key()).is_ok());
    assert_eq!(dialog.get_api_key(), PASTED_KEY);
}
