//! The embedding host mode: `codegg host --stdio`.
//!
//! One process, one workspace root, one session, driven by a versioned
//! newline-delimited JSON-RPC 2.0 protocol on stdio. The contract is
//! `docs/embedding.md`; the stability policy is ADR-0060.

pub mod protocol;
mod session;
#[cfg(test)]
mod tests;
mod turn;

use crate::headless::{HeadlessError, HeadlessRuntime};
use protocol::{
    code, ErrorClass, Incoming, InitializeResult, RequestBody, RequestFrame, RequestId,
    ResponseFrame, RpcError, SessionCloseParams, SessionCloseResult, SessionOpenParams,
    TurnCancelParams, TurnCancelResult, TurnSubmitParams, TurnSubmitResult, API_VERSION,
    CAPABILITIES, MAX_FRAME_BYTES, MAX_IDENTIFIER_CHARS,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

/// The exit code an unparseable inbound frame produces.
pub const EXIT_UNPARSEABLE_FRAME: i32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("host I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("host frame could not be encoded")]
    Encode,
    #[error("host inbound frame was refused")]
    UnparseableFrame,
}

impl HostError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::UnparseableFrame => EXIT_UNPARSEABLE_FRAME,
            Self::Io(_) | Self::Encode => 1,
        }
    }
}

pub(crate) struct FrameWriter<W> {
    inner: Arc<Mutex<W>>,
}

impl<W> Clone for FrameWriter<W> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    pub(crate) fn new(inner: W) -> Self {
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    pub(crate) async fn send<T: Serialize>(&self, frame: &T) -> Result<(), HostError> {
        let line = serde_json::to_string(frame).map_err(|_| HostError::Encode)?;
        let mut inner = self.inner.lock().await;
        inner.write_all(line.as_bytes()).await?;
        inner.write_all(b"\n").await?;
        inner.flush().await?;
        Ok(())
    }
}

pub async fn run_stdio<R, W>(input: R, output: W) -> Result<(), HostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(input);
    let mut host = Host::new(FrameWriter::new(output));
    host.serve(&mut reader).await
}

#[cfg(test)]
pub(crate) async fn run_stdio_with_seam<R, W>(
    input: R,
    output: W,
    provider: Option<Box<dyn crate::provider::Provider>>,
    config: Option<crate::config::schema::Config>,
) -> Result<(), HostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(input);
    let mut host = Host::new(FrameWriter::new(output));
    host.provider = provider;
    host.config = config;
    host.serve(&mut reader).await
}

struct Host<W> {
    writer: FrameWriter<W>,
    initialized: bool,
    session: Option<session::Session>,
    #[cfg(test)]
    provider: Option<Box<dyn crate::provider::Provider>>,
    #[cfg(test)]
    config: Option<crate::config::schema::Config>,
}

impl<W: AsyncWrite + Unpin + Send + 'static> Host<W> {
    fn new(writer: FrameWriter<W>) -> Self {
        Self {
            writer,
            initialized: false,
            session: None,
            #[cfg(test)]
            provider: None,
            #[cfg(test)]
            config: None,
        }
    }

    async fn serve<R: AsyncBufRead + Unpin>(&mut self, reader: &mut R) -> Result<(), HostError> {
        loop {
            match read_frame(reader).await? {
                FrameRead::Eof => {
                    self.shutdown().await;
                    return Ok(());
                }
                FrameRead::TooLarge => {
                    self.write_error_null(protocol::FrameError::TooLarge.rpc())
                        .await?;
                    self.shutdown().await;
                    return Err(HostError::UnparseableFrame);
                }
                FrameRead::Line(line) => match protocol::parse_frame(&line) {
                    Err(error) => {
                        self.write_error_null(error.rpc()).await?;
                        self.shutdown().await;
                        return Err(HostError::UnparseableFrame);
                    }
                    Ok(Incoming::Request(frame)) => {
                        if self.handle(frame).await? {
                            return Ok(());
                        }
                    }
                    Ok(Incoming::UnknownMethod { id, method }) => {
                        let error = if self.initialized {
                            RpcError::protocol(
                                code::METHOD_NOT_FOUND,
                                ErrorClass::InvalidRequest,
                                format!("unknown method: {method}"),
                            )
                        } else {
                            RpcError::new(
                                ErrorClass::NotInitialized,
                                "initialize has not succeeded",
                            )
                        };
                        self.write_error(id, error).await?;
                    }
                    Ok(Incoming::Notification { method }) => {
                        self.write_error_null(RpcError::protocol(
                            code::INVALID,
                            ErrorClass::InvalidRequest,
                            format!("a host may not send the notification {method}"),
                        ))
                        .await?;
                    }
                    Ok(Incoming::Unusable { id, detail }) => {
                        let error =
                            RpcError::protocol(code::INVALID, ErrorClass::InvalidRequest, detail);
                        match id {
                            Some(id) => self.write_error(id, error).await?,
                            None => self.write_error_null(error).await?,
                        }
                    }
                },
            }
        }
    }

    async fn shutdown(&mut self) {
        if let Some(session) = &self.session {
            session.cancel_active().await;
        }
    }

    async fn handle(&mut self, frame: RequestFrame) -> Result<bool, HostError> {
        let id = frame.id.clone();
        let method = frame.body.method();
        tracing::debug!(method, id = %id, "host request");
        if !matches!(frame.body, RequestBody::Initialize(_)) && !self.initialized {
            self.write_error(
                id,
                RpcError::new(ErrorClass::NotInitialized, "initialize has not succeeded"),
            )
            .await?;
            return Ok(false);
        }
        match frame.body {
            RequestBody::Initialize(params) => self.initialize(id, params).await?,
            RequestBody::SessionOpen(params) => self.session_open(id, params).await?,
            RequestBody::TurnSubmit(params) => self.turn_submit(id, params).await?,
            RequestBody::TurnCancel(params) => self.turn_cancel(id, params).await?,
            RequestBody::SessionClose(params) => {
                return self.session_close(id, params).await;
            }
        }
        Ok(false)
    }

    async fn initialize(
        &mut self,
        id: RequestId,
        params: protocol::InitializeParams,
    ) -> Result<(), HostError> {
        if let Err(error) = params.validate() {
            return self.write_error(id, error).await;
        }
        if let Err(error) = params.required_capabilities() {
            return self.write_error(id, error).await;
        }
        self.initialized = true;
        let result = InitializeResult {
            api_version: API_VERSION,
            codegg_version: env!("CARGO_PKG_VERSION").to_owned(),
            capabilities: CAPABILITIES
                .iter()
                .map(|capability| (*capability).to_owned())
                .collect(),
        };
        self.write_result(id, &result).await
    }

    async fn session_open(
        &mut self,
        id: RequestId,
        params: SessionOpenParams,
    ) -> Result<(), HostError> {
        if let Err(error) = params.validate() {
            return self.write_error(id, error).await;
        }
        if self.session.is_some() {
            return self
                .write_error(
                    id,
                    RpcError::new(ErrorClass::InvalidRequest, "a session is already open"),
                )
                .await;
        }
        let descriptor = match params.provider.clone() {
            Some(value) => match session::parse_descriptor(value) {
                Ok(descriptor) => Some(descriptor),
                Err(error) => return self.write_error(id, error).await,
            },
            None => None,
        };
        let workspace = match session::canonical_workspace(&params.workspace).await {
            Ok(workspace) => workspace,
            Err(error) => return self.write_error(id, error).await,
        };
        let approval = match params.approval.as_deref() {
            Some(raw) => match session::parse_approval(raw) {
                Ok(mode) => Some(mode),
                Err(error) => return self.write_error(id, error).await,
            },
            None => None,
        };
        let sandbox = match params.sandbox.as_deref() {
            Some(raw) => match session::parse_sandbox(raw) {
                Ok(profile) => Some(profile),
                Err(error) => return self.write_error(id, error).await,
            },
            None => None,
        };
        let mut options = crate::headless::HeadlessOptions::new(workspace);
        options.config = if descriptor.is_some() {
            crate::headless::HeadlessConfig::Default
        } else {
            crate::headless::HeadlessConfig::Load
        };
        options.model = Some(params.model.clone());
        options.agent = params.agent.clone();
        options.approval = approval;
        options.sandbox = sandbox;
        options.session_id = params.session_id.clone();
        options.descriptor = descriptor;
        #[cfg(test)]
        {
            options.provider_override = self.provider.as_ref().map(|provider| provider.clone_box());
            options.config_override = self.config.clone();
        }

        let runtime = match HeadlessRuntime::build(options).await {
            Ok(runtime) => runtime,
            Err(error) => return self.write_error(id, headless_error(error)).await,
        };
        let session = session::Session::new(runtime);
        let result = session.open_result();
        self.session = Some(session);
        self.write_result(id, &result).await
    }

    async fn turn_submit(
        &mut self,
        id: RequestId,
        params: TurnSubmitParams,
    ) -> Result<(), HostError> {
        if let Err(error) = params.validate() {
            return self.write_error(id, error).await;
        }
        let Some(session) = self.session.clone() else {
            return self
                .write_error(
                    id,
                    RpcError::new(ErrorClass::InvalidRequest, "no session is open"),
                )
                .await;
        };
        if params.session_id != session.id {
            return self.write_error(id, unknown_session()).await;
        }
        let turn_id = params
            .turn_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let admission = match session.begin_turn(&turn_id).await {
            Ok(admission) => admission,
            Err(session::AdmissionError::Busy) => {
                return self
                    .write_error(
                        id,
                        RpcError::new(ErrorClass::Busy, "a turn is already active"),
                    )
                    .await;
            }
            Err(session::AdmissionError::Unavailable) => {
                return self
                    .write_error(
                        id,
                        RpcError::new(ErrorClass::Internal, "the session loop is unavailable"),
                    )
                    .await;
            }
        };
        self.write_result(
            id,
            &TurnSubmitResult {
                turn_id: turn_id.clone(),
            },
        )
        .await?;
        turn::spawn(
            session,
            self.writer.clone(),
            admission,
            params.prompt,
            turn_id,
        );
        Ok(())
    }

    async fn turn_cancel(
        &mut self,
        id: RequestId,
        params: TurnCancelParams,
    ) -> Result<(), HostError> {
        if let Err(error) =
            protocol::check_len("sessionId", &params.session_id, MAX_IDENTIFIER_CHARS)
        {
            return self.write_error(id, error).await;
        }
        let Some(session) = self.session.clone() else {
            return self
                .write_error(
                    id,
                    RpcError::new(ErrorClass::InvalidRequest, "no session is open"),
                )
                .await;
        };
        if params.session_id != session.id {
            return self.write_error(id, unknown_session()).await;
        }
        let cancelled = session.cancel(&params.turn_id).await;
        self.write_result(id, &TurnCancelResult { cancelled }).await
    }

    async fn session_close(
        &mut self,
        id: RequestId,
        params: SessionCloseParams,
    ) -> Result<bool, HostError> {
        if let Err(error) =
            protocol::check_len("sessionId", &params.session_id, MAX_IDENTIFIER_CHARS)
        {
            self.write_error(id, error).await?;
            return Ok(false);
        }
        let Some(session) = self.session.as_ref() else {
            self.write_error(
                id,
                RpcError::new(ErrorClass::InvalidRequest, "no session is open"),
            )
            .await?;
            return Ok(false);
        };
        if params.session_id != session.id {
            self.write_error(id, unknown_session()).await?;
            return Ok(false);
        }
        session.cancel_active().await;
        self.write_result(id, &SessionCloseResult { closed: true })
            .await?;
        Ok(true)
    }

    async fn write_result<T: Serialize>(&self, id: RequestId, result: &T) -> Result<(), HostError> {
        let value = serde_json::to_value(result).map_err(|_| HostError::Encode)?;
        self.writer.send(&ResponseFrame::result(id, value)).await
    }

    async fn write_error(&self, id: RequestId, error: RpcError) -> Result<(), HostError> {
        self.writer.send(&ResponseFrame::error(id, error)).await
    }

    async fn write_error_null(&self, error: RpcError) -> Result<(), HostError> {
        self.writer
            .send(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": error,
            }))
            .await
    }
}

fn unknown_session() -> RpcError {
    RpcError::new(ErrorClass::InvalidRequest, "unknown sessionId")
}

fn headless_error(error: HeadlessError) -> RpcError {
    match error {
        HeadlessError::Config(source) => {
            tracing::warn!(error = %source, "host configuration could not be loaded");
            RpcError::new(ErrorClass::Internal, "configuration could not be loaded")
        }
        HeadlessError::AgentNotFound(agent) => RpcError::new(
            ErrorClass::InvalidRequest,
            format!("agent {agent:?} is not available"),
        ),
        HeadlessError::Agents(source) => {
            tracing::warn!(error = %source, "host agents could not be resolved");
            RpcError::new(ErrorClass::Internal, "agents could not be resolved")
        }
        HeadlessError::ProviderNotFound(provider) => RpcError::new(
            ErrorClass::InvalidRequest,
            format!("provider {provider:?} is not available"),
        ),
        HeadlessError::Descriptor(source) => {
            RpcError::new(ErrorClass::InvalidRequest, source.to_string())
        }
        HeadlessError::SessionId => RpcError::new(
            ErrorClass::InvalidRequest,
            "sessionId is not a valid session id",
        ),
    }
}

enum FrameRead {
    Eof,
    Line(Vec<u8>),
    TooLarge,
}

async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<FrameRead> {
    let mut buffer = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if buffer.is_empty() {
                return Ok(FrameRead::Eof);
            }
            if buffer.last() == Some(&b'\r') {
                buffer.pop();
            }
            return Ok(FrameRead::Line(buffer));
        }
        if let Some(position) = available.iter().position(|byte| *byte == b'\n') {
            if buffer.len().saturating_add(position) > MAX_FRAME_BYTES {
                reader.consume(position + 1);
                return Ok(FrameRead::TooLarge);
            }
            buffer.extend_from_slice(&available[..position]);
            reader.consume(position + 1);
            if buffer.last() == Some(&b'\r') {
                buffer.pop();
            }
            return Ok(FrameRead::Line(buffer));
        }
        if buffer.len().saturating_add(available.len()) > MAX_FRAME_BYTES {
            return Ok(FrameRead::TooLarge);
        }
        let length = available.len();
        buffer.extend_from_slice(available);
        reader.consume(length);
    }
}
