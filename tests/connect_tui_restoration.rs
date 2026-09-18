//! Provider /connect restoration M003 — app-level TUI harness.
//!
//! Covers the missing integration boundary from the implementation plan:
//! clean profile → `/connect` through slash routing → catalog with an
//! ordinary provider plus Eggpool → keyboard-driven ordinary setup with fake
//! validation → reopened Eggpool setup with endpoint/TLS → mouse row
//! selection → cancellation without orphans → secret-free snapshots.
//!
//! The fake daemon serves the real pre-credential catalog
//! (`codegg::provider::provider_setup_catalog`) through the secret-free
//! `ProviderSetupList` projection and validates generic creates with the
//! same policy shape as the daemon provisioner (fixed/optional/required/
//! proxy-preset, credential-kind admission). No live provider network is
//! used; ordinary validation is offline and Eggpool/custom validation is
//! structural (no probe network in the fake).

use async_trait::async_trait;
use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope,
};
use codegg::protocol::provider::{
    CreateProviderConnectionResult, ProviderConnectionSummaryDto, ProviderModelDto,
    ProviderSetupEntryDto, SecretInput,
};
use codegg::tui::app::{App, TuiCommand, TuiMsg};
use codegg::tui::components::component::Component;
use codegg::tui::components::dialogs::connect::{ConnectDialog, ConnectStep};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const ORDINARY_SECRET: &str = "sk-connect-m003-ordinary-secret-1";
const EGGPOOL_SECRET: &str = "sk-connect-m003-eggpool-secret-2";
const CANCEL_SECRET: &str = "sk-connect-m003-cancel-secret-3";

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct FakeDurableConnection {
    id: String,
    provider_id: String,
    endpoint: String,
    display_name: String,
}

#[derive(Default)]
struct FakeConnectDaemon {
    connections: Mutex<Vec<FakeDurableConnection>>,
    creates: Mutex<Vec<String>>,
    cancels: Mutex<Vec<String>>,
    request_debugs: Mutex<Vec<String>>,
}

fn catalog_dtos() -> Vec<ProviderSetupEntryDto> {
    use codegg::provider::{setup_catalog::SetupEndpointPolicy, CredentialCapability};
    codegg::provider::provider_setup_catalog()
        .iter()
        .map(|definition| {
            let credential_kinds = match definition.credential_capability {
                CredentialCapability::ApiKeyOnly => vec!["api_key".to_string()],
                CredentialCapability::ApiKeyOrBearer => {
                    vec!["api_key".to_string(), "bearer".to_string()]
                }
            };
            let (endpoint_policy, default_endpoint) = match definition.endpoint_policy {
                SetupEndpointPolicy::Fixed { base_url } => {
                    ("fixed".to_string(), Some(base_url.to_string()))
                }
                SetupEndpointPolicy::OptionalOverride { default_base_url } => (
                    "optional_override".to_string(),
                    default_base_url.map(str::to_string),
                ),
                SetupEndpointPolicy::RequiredEndpoint => ("required".to_string(), None),
                SetupEndpointPolicy::ProxyPreset { .. } => ("proxy_preset".to_string(), None),
            };
            ProviderSetupEntryDto {
                id: definition.id.to_string(),
                display_name: definition.display_name.to_string(),
                description: definition.description.to_string(),
                connectable: definition.connectable,
                credential_kinds,
                endpoint_policy,
                default_endpoint,
                requires_endpoint: definition.requires_endpoint(),
                env_var: definition.env_var.map(str::to_string),
            }
        })
        .collect()
}

fn fake_models(provider_id: &str) -> Vec<ProviderModelDto> {
    let (id, name) = match provider_id {
        "openai" => ("gpt-4.1-mini", "GPT-4.1 Mini"),
        "anthropic" => ("claude-test", "Claude Test"),
        "eggpool" => ("eggpool-model", "Eggpool Model"),
        _ => ("fake-model", "Fake Model"),
    };
    vec![ProviderModelDto {
        id: id.to_string(),
        name: name.to_string(),
        context_window: 128_000,
        max_output_tokens: Some(4096),
        supports_tools: true,
        supports_vision: false,
    }]
}

#[async_trait]
impl CoreClient for FakeConnectDaemon {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        match request.payload {
            CoreRequest::ProviderSetupList => Ok(CoreResponse::ProviderSetupList {
                providers: catalog_dtos(),
            }),
            CoreRequest::ProviderConnectionCreate { request } => {
                // Redacted-debug guard: the request Debug must never carry
                // the plaintext credential.
                let debug = format!("{request:?}");
                let secret = request.credential.expose().to_string();
                assert!(
                    !debug.contains(&secret),
                    "generic create request Debug leaked the secret"
                );
                self.request_debugs.lock().unwrap().push(debug);
                // Policy-shape validation mirroring the daemon provisioner.
                let dtos = catalog_dtos();
                let dto = dtos
                    .iter()
                    .find(|dto| dto.id == request.provider_id)
                    .cloned()
                    .expect("fake catalog covers the requested provider");
                if !dto.connectable {
                    return Ok(CoreResponse::Error {
                        code: "unsupported_provider".to_string(),
                        message: "provider is not connectable".to_string(),
                    });
                }
                let kind_label = match request.credential_kind {
                    codegg::protocol::provider::ProviderCredentialKind::ApiKey => "api_key",
                    codegg::protocol::provider::ProviderCredentialKind::Bearer => "bearer",
                };
                if !dto.credential_kinds.iter().any(|k| k == kind_label) {
                    return Ok(CoreResponse::Error {
                        code: "unsupported_credential_kind".to_string(),
                        message: "credential kind not admitted".to_string(),
                    });
                }
                if request.credential.expose().trim().is_empty() {
                    return Ok(CoreResponse::Error {
                        code: "invalid_credential".to_string(),
                        message: "credential is empty".to_string(),
                    });
                }
                let endpoint = match dto.endpoint_policy.as_str() {
                    "fixed" => {
                        if request
                            .endpoint
                            .as_deref()
                            .is_some_and(|v| !v.trim().is_empty())
                            || request.port.is_some()
                            || request.tls_policy.is_some()
                        {
                            return Ok(CoreResponse::Error {
                                code: "invalid_endpoint".to_string(),
                                message: "endpoint is managed by the provider".to_string(),
                            });
                        }
                        dto.default_endpoint.clone().unwrap_or_default()
                    }
                    "optional_override" => {
                        if request.port.is_some() {
                            return Ok(CoreResponse::Error {
                                code: "invalid_endpoint".to_string(),
                                message: "explicit ports are proxy-only".to_string(),
                            });
                        }
                        match request.endpoint.as_deref().filter(|v| !v.trim().is_empty()) {
                            Some(raw) => {
                                if !(raw.starts_with("http://") || raw.starts_with("https://")) {
                                    return Ok(CoreResponse::Error {
                                        code: "invalid_endpoint".to_string(),
                                        message: "endpoint must be an http(s) URL".to_string(),
                                    });
                                }
                                raw.to_string()
                            }
                            None => dto.default_endpoint.clone().unwrap_or_default(),
                        }
                    }
                    "required" => {
                        if request.port.is_some() {
                            return Ok(CoreResponse::Error {
                                code: "invalid_endpoint".to_string(),
                                message: "explicit ports are proxy-only".to_string(),
                            });
                        }
                        match request.endpoint.as_deref().filter(|v| !v.trim().is_empty()) {
                            Some(raw) => {
                                if !(raw.starts_with("http://") || raw.starts_with("https://")) {
                                    return Ok(CoreResponse::Error {
                                        code: "invalid_endpoint".to_string(),
                                        message: "endpoint must be an http(s) URL".to_string(),
                                    });
                                }
                                raw.to_string()
                            }
                            None => {
                                return Ok(CoreResponse::Error {
                                    code: "invalid_endpoint".to_string(),
                                    message: "endpoint is required".to_string(),
                                });
                            }
                        }
                    }
                    "proxy_preset" => {
                        let host = request.endpoint.clone().unwrap_or_default();
                        if host.trim().is_empty() {
                            return Ok(CoreResponse::Error {
                                code: "invalid_endpoint".to_string(),
                                message: "host is required".to_string(),
                            });
                        }
                        let port = request.port.unwrap_or(11300);
                        if port == 0 {
                            return Ok(CoreResponse::Error {
                                code: "invalid_endpoint".to_string(),
                                message: "port is invalid".to_string(),
                            });
                        }
                        format!("http://{host}:{port}/v1")
                    }
                    _ => {
                        return Ok(CoreResponse::Error {
                            code: "unsupported_provider".to_string(),
                            message: "unknown endpoint policy".to_string(),
                        });
                    }
                };
                // Duplicate detection on the durable identity.
                {
                    let connections = self.connections.lock().unwrap();
                    if connections
                        .iter()
                        .any(|c| c.provider_id == request.provider_id && c.endpoint == endpoint)
                    {
                        return Ok(CoreResponse::Error {
                            code: "connection_conflict".to_string(),
                            message: "equivalent connection exists".to_string(),
                        });
                    }
                }
                let n = self.connections.lock().unwrap().len() + 1;
                let connection_id = format!("conn-{}-{n}", request.provider_id);
                let display_name = request
                    .display_name
                    .clone()
                    .unwrap_or_else(|| dto.display_name.clone());
                self.creates
                    .lock()
                    .unwrap()
                    .push(request.provider_id.clone());
                self.connections
                    .lock()
                    .unwrap()
                    .push(FakeDurableConnection {
                        id: connection_id.clone(),
                        provider_id: request.provider_id.clone(),
                        endpoint: endpoint.clone(),
                        display_name,
                    });
                let operation_id = request
                    .operation_id
                    .clone()
                    .unwrap_or_else(|| "prov-fake".to_string());
                Ok(CoreResponse::ProviderConnectionCreated {
                    result: CreateProviderConnectionResult {
                        operation_id,
                        connection: ProviderConnectionSummaryDto {
                            id: connection_id,
                            provider_kind: request.provider_id.clone(),
                            display_name: dto.display_name.clone(),
                            endpoint,
                            tls_policy: "required".to_string(),
                            scope: "personal".to_string(),
                            state: "active".to_string(),
                            revision: 1,
                            model_count: fake_models(&request.provider_id).len(),
                            catalog_revision: Some("rev-1".to_string()),
                            health: None,
                        },
                        models: fake_models(&request.provider_id),
                        catalog_revision: "rev-1".to_string(),
                    },
                })
            }
            CoreRequest::EggpoolConnectionCancel { operation_id } => {
                self.cancels.lock().unwrap().push(operation_id.clone());
                Ok(CoreResponse::EggpoolConnectionCancelled { operation_id })
            }
            other => panic!("unexpected fake connect request: {other:?}"),
        }
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::channel(256);
        rx
    }
}

fn key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}

/// Drive one key through the live FocusManager component and feed any
/// resulting TuiMsg back through the synchronous App owner, mirroring the
/// runtime event loop.
fn press_key(app: &mut App, code: crossterm::event::KeyCode) {
    let msg = app.focus_manager.handle_key(key(code));
    if let Some(msg) = msg {
        // SubmitConnect flows into the generic provisioning send; CloseDialog
        // dismisses through the standard path.
        app.process_msg(msg);
    }
}

fn live_step(app: &App) -> ConnectStep {
    app.focus_manager
        .with_dialog::<ConnectDialog, _>(
            codegg::tui::components::component::DialogType::Connect,
            |live| live.step.clone(),
        )
        .expect("connect dialog is mounted")
}

fn live_selected_id(app: &App) -> String {
    app.focus_manager
        .with_dialog::<ConnectDialog, _>(
            codegg::tui::components::component::DialogType::Connect,
            |live| {
                live.providers
                    .get(live.selected)
                    .map(|entry| entry.id.clone())
                    .unwrap_or_default()
            },
        )
        .expect("connect dialog is mounted")
}

fn paste_secret(app: &mut App, secret: &str) {
    app.focus_manager
        .dialog_mut::<ConnectDialog>(codegg::tui::components::component::DialogType::Connect)
        .expect("connect dialog is mounted")
        .handle_paste(secret.to_string());
}

fn paste_form_text(app: &mut App, text: &str) {
    app.focus_manager
        .dialog_mut::<ConnectDialog>(codegg::tui::components::component::DialogType::Connect)
        .expect("connect dialog is mounted")
        .handle_paste(text.to_string());
}

fn dispatch_connect(app: &mut App) {
    app.prompt_state.prompt.set_text("/connect".to_string());
    app.prompt_state
        .prompt
        .set_cursor("/connect".chars().count());
    app.process_msg(TuiMsg::SubmitPrompt);
}

async fn recv_setup(rx: &mut mpsc::Receiver<TuiCommand>) -> Vec<ProviderSetupEntryDto> {
    let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("setup completion arrives")
        .expect("channel open");
    match cmd {
        TuiCommand::ConnectSetupLoaded { providers, error } => {
            assert!(error.is_none(), "catalog load failed: {error:?}");
            providers
        }
        other => panic!("expected ConnectSetupLoaded, got {other:?}"),
    }
}

async fn recv_finished(
    rx: &mut mpsc::Receiver<TuiCommand>,
) -> Result<CreateProviderConnectionResult, String> {
    let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("provision completion arrives")
        .expect("channel open");
    match cmd {
        TuiCommand::ProviderConnectionFinished { result, .. } => result,
        other => panic!("expected ProviderConnectionFinished, got {other:?}"),
    }
}

fn apply_setup_loaded(app: &mut App, providers: &[ProviderSetupEntryDto]) {
    if let Some(dialog) = app.dialog_state.connect_dialog.as_mut() {
        dialog.set_setup_entries(providers);
    }
    app.focus_manager.with_dialog_mut::<ConnectDialog, _>(
        codegg::tui::components::component::DialogType::Connect,
        |live| {
            live.set_setup_entries(providers);
        },
    );
}

fn index_of(app: &App, id: &str) -> usize {
    app.focus_manager
        .with_dialog::<ConnectDialog, _>(
            codegg::tui::components::component::DialogType::Connect,
            |live| live.providers.iter().position(|entry| entry.id == id),
        )
        .expect("connect dialog is mounted")
        .unwrap_or_else(|| panic!("catalog is missing '{id}'"))
}

fn move_selection_to(app: &mut App, target_idx: usize) {
    use crossterm::event::KeyCode;
    for _ in 0..64 {
        let current = app
            .focus_manager
            .with_dialog::<ConnectDialog, _>(
                codegg::tui::components::component::DialogType::Connect,
                |live| live.selected,
            )
            .expect("connect dialog is mounted");
        if current == target_idx {
            return;
        }
        if current < target_idx {
            press_key(app, KeyCode::Down);
        } else {
            press_key(app, KeyCode::Up);
        }
    }
    panic!("selection did not reach row {target_idx}");
}

fn render_without_secret(app: &App, secret: &str) {
    let clone = app
        .focus_manager
        .with_dialog::<ConnectDialog, _>(
            codegg::tui::components::component::DialogType::Connect,
            |live| live.clone(),
        )
        .expect("connect dialog is mounted");
    // Focus clones never carry the plaintext.
    assert!(
        clone.api_key_input.is_empty(),
        "focus clone must not carry the secret"
    );
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            use ratatui::widgets::Widget;
            (&clone).render(frame.area(), frame.buffer_mut());
        })
        .expect("draw connect dialog");
    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol().to_string())
        .collect();
    assert!(
        !content.contains(secret),
        "rendered dialog must not contain the secret"
    );
}

fn assert_history_and_prompt_clean(app: &App, secrets: &[&str]) {
    let prompt_text = app.prompt_state.prompt.get_text();
    for secret in secrets {
        assert!(
            !prompt_text.contains(secret),
            "prompt text must not contain a secret"
        );
    }
    for entry in app.session_state.history.iter() {
        for secret in secrets {
            assert!(
                !entry.text.contains(secret),
                "prompt history must not contain a secret"
            );
        }
    }
}

#[test]
fn connect_tui_has_no_eggpool_only_hardcode() {
    // Structural guard: the provider list must be built from the
    // secret-free setup catalog operation, never from a hard-coded
    // Eggpool-only vector.
    let manifest = env!("CARGO_MANIFEST_DIR");
    let path = format!("{manifest}/src/tui/app/mod.rs");
    let source = std::fs::read_to_string(&path).expect("read app/mod.rs");
    let start = source
        .find("fn open_connect_dialog")
        .expect("open_connect_dialog exists");
    let rest = &source[start..];
    // Bound the window to this function body so the adjacent rotation
    // editor (which legitimately names its Eggpool credential target) does
    // not trip the guard.
    let end = rest[1..]
        .find("\n    fn ")
        .map(|idx| idx + 1)
        .unwrap_or(rest.len());
    let window = &rest[..end];
    assert!(
        window.contains("ProviderSetupList"),
        "open_connect_dialog must fetch the daemon setup catalog"
    );
    assert!(
        window.contains("new_loading"),
        "open_connect_dialog must open in a loading state"
    );
    assert!(
        window.contains("ConnectSetupLoaded"),
        "open_connect_dialog must complete through ConnectSetupLoaded"
    );
    assert!(
        !window.contains("\"eggpool\""),
        "open_connect_dialog must not hard-code the eggpool provider"
    );
    // The loading constructor itself carries no providers.
    let theme = std::sync::Arc::new(codegg::tui::theme::Theme::dark());
    let dialog = ConnectDialog::new_loading(theme);
    assert!(dialog.is_loading());
    assert!(dialog.providers.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn connect_tui_restoration_keyboard_mouse_cancel_and_secrecy() {
    // Clean-profile precondition: no master-key env vars influence the
    // secret-free catalog or the fake validation below.
    for var in [
        "CODEGG_MASTER_KEY",
        "CODEGG_ENCRYPTION_KEY",
        "OPENCODE_ENCRYPTION_KEY",
    ] {
        std::env::remove_var(var);
    }
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let mut app = App::new_for_testing(project_dir.path().to_string_lossy().to_string());
    let daemon = Arc::new(FakeConnectDaemon::default());
    app.core_client = Some(daemon.clone());
    let (tx, mut rx) = mpsc::channel(64);
    app.tui_cmd_tx = Some(tx);

    // 1-2. `/connect` through normal slash-command routing opens a loading
    // dialog with no Eggpool-only fallback.
    dispatch_connect(&mut app);
    {
        let dialog = app
            .dialog_state
            .connect_dialog
            .as_ref()
            .expect("connect dialog opens");
        assert!(dialog.is_loading(), "dialog opens in a loading state");
        assert!(
            dialog.providers.is_empty(),
            "no Eggpool-only list before the catalog arrives"
        );
    }
    let providers = recv_setup(&mut rx).await;
    assert!(
        providers.iter().any(|entry| entry.id == "openai"),
        "catalog carries an ordinary direct provider"
    );
    assert!(
        providers.iter().any(|entry| entry.id == "eggpool"),
        "catalog carries Eggpool as one selectable upstream"
    );
    apply_setup_loaded(&mut app, &providers);

    // 3. Ordinary provider by keyboard: openai is an optional-override
    // form, so no Eggpool host/port/TLS steps appear.
    use crossterm::event::KeyCode;
    let openai_idx = index_of(&app, "openai");
    move_selection_to(&mut app, openai_idx);
    assert_eq!(live_selected_id(&app), "openai");
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(live_step(&app), ConnectStep::EnterEndpoint);
    // Blank endpoint uses the catalog default for optional overrides.
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(live_step(&app), ConnectStep::EnterApiKey);
    paste_secret(&mut app, ORDINARY_SECRET);
    render_without_secret(&app, ORDINARY_SECRET);
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(live_step(&app), ConnectStep::EnterDisplayName);
    paste_form_text(&mut app, "ci-ordinary");
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(live_step(&app), ConnectStep::SelectScope);
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(live_step(&app), ConnectStep::Review);
    press_key(&mut app, KeyCode::Enter);
    let ordinary = recv_finished(&mut rx)
        .await
        .expect("ordinary fake validation succeeds");
    assert_eq!(ordinary.connection.provider_kind, "openai");
    assert!(!ordinary.models.is_empty());
    assert_eq!(daemon.connections.lock().unwrap().len(), 1);
    // Success dismisses through the existing connections projection path:
    // drop the dialog without creating a second TUI-only provider list.
    app.dialog_state.connect_dialog = None;
    app.process_msg(TuiMsg::CloseDialog);

    // 4. Reopen `/connect` and complete Eggpool with endpoint/TLS fields.
    dispatch_connect(&mut app);
    let providers = recv_setup(&mut rx).await;
    apply_setup_loaded(&mut app, &providers);
    let eggpool_idx = index_of(&app, "eggpool");
    move_selection_to(&mut app, eggpool_idx);
    assert_eq!(live_selected_id(&app), "eggpool");
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(live_step(&app), ConnectStep::EnterHost);
    paste_form_text(&mut app, "127.0.0.1");
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(live_step(&app), ConnectStep::EnterPort);
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(live_step(&app), ConnectStep::SelectTls);
    // TLS controls cycle without touching the (still empty) secret.
    app.focus_manager
        .dialog_mut::<ConnectDialog>(codegg::tui::components::component::DialogType::Connect)
        .expect("connect dialog is mounted")
        .cycle_tls_policy(true);
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(live_step(&app), ConnectStep::SelectCredentialKind);
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(live_step(&app), ConnectStep::EnterApiKey);
    paste_secret(&mut app, EGGPOOL_SECRET);
    render_without_secret(&app, EGGPOOL_SECRET);
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(live_step(&app), ConnectStep::EnterDisplayName);
    paste_form_text(&mut app, "ci-eggpool");
    press_key(&mut app, KeyCode::Enter);
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(live_step(&app), ConnectStep::Review);
    press_key(&mut app, KeyCode::Enter);
    let eggpool = recv_finished(&mut rx)
        .await
        .expect("eggpool fake validation succeeds");
    assert_eq!(eggpool.connection.provider_kind, "eggpool");
    assert!(
        eggpool.connection.endpoint.contains("127.0.0.1"),
        "eggpool endpoint echoes the proxy host, got {}",
        eggpool.connection.endpoint
    );
    assert_eq!(daemon.connections.lock().unwrap().len(), 2);
    app.dialog_state.connect_dialog = None;
    app.process_msg(TuiMsg::CloseDialog);

    // 5. Provider-row selection through mouse hit testing.
    dispatch_connect(&mut app);
    let providers = recv_setup(&mut rx).await;
    apply_setup_loaded(&mut app, &providers);
    let anthropic_idx = index_of(&app, "anthropic");
    let area = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 30,
    };
    app.dialog_area = Some(area);
    // Three content rows per entry; target the middle row of the entry.
    let rel_y = (1 + anthropic_idx * 3 + 1) as u16;
    app.on_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 5,
        row: area.y + rel_y,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    assert_eq!(
        live_selected_id(&app),
        "anthropic",
        "mouse click selects the provider row"
    );
    assert_eq!(
        app.dialog_state
            .connect_dialog
            .as_ref()
            .expect("connect dialog opens")
            .selected,
        anthropic_idx,
        "mouse selection syncs to the stored dialog copy"
    );
    // Cancel from provider selection: no create, no secret.
    app.process_msg(TuiMsg::CloseDialog);
    assert_eq!(
        daemon.connections.lock().unwrap().len(),
        2,
        "mouse-path cancel creates nothing"
    );

    // 6. Cancel from secret entry leaves no credential/journal orphan.
    dispatch_connect(&mut app);
    let providers = recv_setup(&mut rx).await;
    apply_setup_loaded(&mut app, &providers);
    let openai_idx = index_of(&app, "openai");
    move_selection_to(&mut app, openai_idx);
    press_key(&mut app, KeyCode::Enter);
    press_key(&mut app, KeyCode::Enter);
    assert_eq!(live_step(&app), ConnectStep::EnterApiKey);
    paste_secret(&mut app, CANCEL_SECRET);
    // One Esc steps back and forgets the typed secret; subsequent Esc
    // presses walk back to selection and then dismiss.
    press_key(&mut app, KeyCode::Esc);
    let after_back = app
        .focus_manager
        .with_dialog::<ConnectDialog, _>(
            codegg::tui::components::component::DialogType::Connect,
            |live| (live.step.clone(), live.get_api_key()),
        )
        .expect("connect dialog is mounted");
    assert_eq!(after_back.0, ConnectStep::EnterEndpoint);
    assert!(
        after_back.1.is_empty(),
        "stepping back from secret entry forgets the secret"
    );
    press_key(&mut app, KeyCode::Esc);
    assert_eq!(live_step(&app), ConnectStep::SelectProvider);
    app.process_msg(TuiMsg::CloseDialog);
    // Drain any stray completions (none expected: nothing was submitted).
    assert_eq!(
        daemon.connections.lock().unwrap().len(),
        2,
        "secret-entry cancel creates no durable connection"
    );
    assert!(
        daemon.creates.lock().unwrap().len() == 2,
        "only the two submitted provisions reached the daemon"
    );

    // 7. Secret-free snapshots: redacted request debugs, clean render,
    // prompt, and history.
    for debug in daemon.request_debugs.lock().unwrap().iter() {
        for secret in [ORDINARY_SECRET, EGGPOOL_SECRET, CANCEL_SECRET] {
            assert!(
                !debug.contains(secret),
                "daemon request debug must stay redacted"
            );
        }
    }
    for connection in daemon.connections.lock().unwrap().iter() {
        let flat = format!("{connection:?}");
        for secret in [ORDINARY_SECRET, EGGPOOL_SECRET, CANCEL_SECRET] {
            assert!(
                !flat.contains(secret),
                "durable connection record must not contain a secret"
            );
        }
    }
    assert_history_and_prompt_clean(&app, &[ORDINARY_SECRET, EGGPOOL_SECRET, CANCEL_SECRET]);
    // SecretInput itself never round-trips through Debug in plaintext.
    let probe = SecretInput::new(ORDINARY_SECRET).expect("secret input");
    assert!(!format!("{probe:?}").contains(ORDINARY_SECRET));
}
