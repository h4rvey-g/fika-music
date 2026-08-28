use crate::lx_js_importer::LxJsMetadata;
use crate::lx_js_runtime::{build_http_request, resolve_webview_media_url, LxHttpOptions};
use crate::registry_support::operation_nonce;
use crate::source_runtime::{
    lx_music_source, normalize_lx_music_info, SourceAction, SourceCapability, SourceHttpResponse,
    SourceInfo, SourceProvider, SourceQuality, SourceRequest, SourceResponse, SourceRuntimeContext,
    SourceRuntimeError, LX_SOURCE_KG, LX_SOURCE_KIND_MUSIC, LX_SOURCE_KW, LX_SOURCE_LOCAL,
    LX_SOURCE_MG, LX_SOURCE_TX, LX_SOURCE_WY,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) const DENO_RELEASE: &str = "2.9.5";

const RUNNER_SOURCE: &str = include_str!("lx_v8_runner.js");
const INSTALL_TIMEOUT: Duration = Duration::from_secs(3 * 60);
const EXECUTION_TIMEOUT: Duration = Duration::from_secs(30);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(50);
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXECUTABLE_BYTES: u64 = 192 * 1024 * 1024;
const MAX_PROTOCOL_BYTES: usize = 32 * 1024 * 1024;
const MAX_PROTOCOL_LINE_BYTES: usize = 32 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;
const MAX_HTTP_REQUESTS: usize = 16;
const MAX_LOG_BYTES: usize = 1_024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum LxV8SidecarError {
    #[error("the LX V8 sidecar is not published for this operating system and CPU architecture")]
    UnsupportedPlatform,
    #[error("could not initialize the LX V8 sidecar installer")]
    Client,
    #[error("LX V8 sidecar installation failed")]
    InstallNetwork,
    #[error("LX V8 sidecar release asset returned HTTP {0}")]
    InstallStatus(u16),
    #[error("LX V8 sidecar release asset failed integrity verification")]
    Integrity,
    #[error("configured LX V8 executable is unavailable")]
    ExecutableUnavailable,
    #[error("LX V8 execution was cancelled")]
    Cancelled,
    #[error("LX V8 {0} timed out")]
    Timeout(&'static str),
    #[error("LX V8 process could not start")]
    ProcessStart,
    #[error("LX V8 process failed: {0}")]
    ProcessFailed(String),
    #[error("LX V8 protocol failed: {0}")]
    Protocol(String),
    #[error("LX V8 returned an invalid source catalog")]
    InvalidCatalog,
    #[error("LX V8 file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("LX V8 archive failed: {0}")]
    Archive(#[from] zip::result::ZipError),
    #[error("LX V8 JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy)]
struct ReleaseAsset {
    archive_name: &'static str,
    archive_sha256: &'static str,
    archive_size: u64,
    executable_sha256: &'static str,
}

#[derive(Debug)]
enum InstallState {
    Missing,
    Installing,
    Ready(InstalledSidecar),
}

#[derive(Debug, Clone)]
struct InstalledSidecar {
    executable: PathBuf,
    runner: PathBuf,
}

#[derive(Debug)]
struct InstallCoordinator {
    state: Mutex<InstallState>,
    changed: Condvar,
}

pub(crate) struct LxV8Sidecar {
    root: PathBuf,
    client: Client,
    install: InstallCoordinator,
    executable_override: Option<PathBuf>,
}

impl fmt::Debug for LxV8Sidecar {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LxV8Sidecar")
            .field("root", &self.root)
            .field("deno_release", &DENO_RELEASE)
            .field(
                "has_executable_override",
                &self.executable_override.is_some(),
            )
            .finish_non_exhaustive()
    }
}

impl LxV8Sidecar {
    pub(crate) fn is_supported_platform() -> bool {
        current_release_asset().is_some()
    }

    pub(crate) fn new(root: impl Into<PathBuf>) -> Result<Self, LxV8SidecarError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(INSTALL_TIMEOUT)
            .user_agent("FikaMusic/0.2 lx-v8-installer")
            .build()
            .map_err(|_| LxV8SidecarError::Client)?;
        Ok(Self {
            root: root.into(),
            client,
            install: InstallCoordinator {
                state: Mutex::new(InstallState::Missing),
                changed: Condvar::new(),
            },
            executable_override: std::env::var_os("FIKA_LX_V8_PATH").map(PathBuf::from),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_executable(root: impl Into<PathBuf>, executable: PathBuf) -> Self {
        let mut sidecar = Self::new(root).expect("test V8 sidecar client should initialize");
        sidecar.executable_override = Some(executable);
        sidecar
    }

    fn execute(
        &self,
        context: &mut SourceRuntimeContext,
        source: &str,
        metadata: &LxJsMetadata,
        payload: Option<&JsonValue>,
    ) -> Result<V8Execution, LxV8SidecarError> {
        let cancellation = context.cancellation_token();
        let installed = self.ensure_installed(INSTALL_TIMEOUT, || cancellation.is_cancelled())?;
        let nonce = format!("{:032x}", operation_nonce());
        let command = SidecarCommand {
            nonce: &nonce,
            source,
            script_info: metadata,
            payload,
        };
        let mut child = spawn_sidecar(&installed)?;
        let mut stdin = child.stdin.take().ok_or(LxV8SidecarError::ProcessStart)?;
        let stdout = child.stdout.take().ok_or(LxV8SidecarError::ProcessStart)?;
        let stderr = child.stderr.take().ok_or(LxV8SidecarError::ProcessStart)?;
        if let Err(error) = write_protocol_message(&mut stdin, &command) {
            terminate_child(&mut child);
            return Err(error);
        }
        let output = spawn_stdout_reader(stdout);
        let stderr_output = spawn_stderr_reader(stderr);
        let deadline = Instant::now() + EXECUTION_TIMEOUT;
        let mut http_requests = 0_usize;
        let result = loop {
            if cancellation.is_cancelled() {
                terminate_child(&mut child);
                return Err(LxV8SidecarError::Cancelled);
            }
            if Instant::now() >= deadline {
                terminate_child(&mut child);
                return Err(LxV8SidecarError::Timeout("execution"));
            }
            match output.recv_timeout(PROCESS_POLL_INTERVAL) {
                Ok(OutputEvent::Line(line)) => {
                    let message = match serde_json::from_str::<SidecarMessage>(&line) {
                        Ok(message) if message.nonce() == nonce => message,
                        _ => continue,
                    };
                    match message {
                        SidecarMessage::HttpRequest {
                            id, url, options, ..
                        } => {
                            http_requests = http_requests.saturating_add(1);
                            let response = if http_requests > MAX_HTTP_REQUESTS {
                                HttpResponseMessage::error(
                                    &nonce,
                                    id,
                                    "LX V8 HTTP request limit exceeded",
                                )
                            } else {
                                handle_http_request(context, &nonce, id, url, options, deadline)
                            };
                            if let Err(error) = write_protocol_message(&mut stdin, &response) {
                                break Err(error);
                            }
                        }
                        SidecarMessage::Log { level, message, .. } => {
                            let message = sanitize_message(&message);
                            match level.as_str() {
                                "warn" => context.warn(format!("LX V8 script: {message}")),
                                "error" => context.error(format!("LX V8 script: {message}")),
                                _ => context.info(format!("LX V8 script: {message}")),
                            }
                        }
                        SidecarMessage::Complete {
                            ok,
                            catalog,
                            value,
                            error,
                            ..
                        } => {
                            if !ok {
                                break Err(LxV8SidecarError::ProcessFailed(sanitize_message(
                                    error.as_deref().unwrap_or("LX V8 request failed"),
                                )));
                            }
                            break parse_catalog(catalog.as_ref())
                                .map(|catalog| V8Execution { catalog, value });
                        }
                    }
                }
                Ok(OutputEvent::End) => {
                    let status = child.wait()?;
                    let stderr = stderr_output
                        .recv_timeout(Duration::from_secs(1))
                        .unwrap_or_default();
                    let detail = if status.success() {
                        "process exited before returning a result".to_owned()
                    } else {
                        sidecar_error_summary(&stderr)
                    };
                    break Err(LxV8SidecarError::ProcessFailed(detail));
                }
                Ok(OutputEvent::Error(message)) => {
                    break Err(LxV8SidecarError::Protocol(message));
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    break Err(LxV8SidecarError::Protocol(
                        "sidecar output reader stopped".to_owned(),
                    ));
                }
            }
        };
        terminate_child(&mut child);
        result
    }

    fn ensure_installed(
        &self,
        timeout: Duration,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<InstalledSidecar, LxV8SidecarError> {
        let deadline = Instant::now() + timeout;
        loop {
            if is_cancelled() {
                return Err(LxV8SidecarError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(LxV8SidecarError::Timeout("installation"));
            }
            let mut state = self
                .install
                .state
                .lock()
                .map_err(|_| LxV8SidecarError::ExecutableUnavailable)?;
            match &*state {
                InstallState::Ready(installed) if installed.executable.is_file() => {
                    return Ok(installed.clone());
                }
                InstallState::Ready(_) => *state = InstallState::Missing,
                InstallState::Installing => {
                    let wait = deadline
                        .saturating_duration_since(Instant::now())
                        .min(PROCESS_POLL_INTERVAL);
                    let _ = self
                        .install
                        .changed
                        .wait_timeout(state, wait)
                        .map_err(|_| LxV8SidecarError::ExecutableUnavailable)?;
                }
                InstallState::Missing => {
                    *state = InstallState::Installing;
                    drop(state);
                    let result = self.install(deadline, &is_cancelled);
                    let mut state = self
                        .install
                        .state
                        .lock()
                        .map_err(|_| LxV8SidecarError::ExecutableUnavailable)?;
                    *state = match &result {
                        Ok(installed) => InstallState::Ready(installed.clone()),
                        Err(_) => InstallState::Missing,
                    };
                    self.install.changed.notify_all();
                    return result;
                }
            }
        }
    }

    fn install(
        &self,
        deadline: Instant,
        is_cancelled: &impl Fn() -> bool,
    ) -> Result<InstalledSidecar, LxV8SidecarError> {
        let version_dir = self.root.join(DENO_RELEASE);
        fs::create_dir_all(&version_dir)?;
        restrict_directory(&version_dir)?;
        let runner = version_dir.join("lx-v8-runner.js");
        write_verified_runner(&runner)?;
        if let Some(executable) = &self.executable_override {
            return executable
                .is_file()
                .then(|| InstalledSidecar {
                    executable: executable.clone(),
                    runner,
                })
                .ok_or(LxV8SidecarError::ExecutableUnavailable);
        }
        let asset = current_release_asset().ok_or(LxV8SidecarError::UnsupportedPlatform)?;
        let executable = version_dir.join(executable_name());
        if verify_file_hash(&executable, asset.executable_sha256, is_cancelled)? {
            ensure_executable(&executable)?;
            return Ok(InstalledSidecar { executable, runner });
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(LxV8SidecarError::Timeout("installation"));
        }
        let url = format!(
            "https://github.com/denoland/deno/releases/download/v{DENO_RELEASE}/{}",
            asset.archive_name
        );
        let mut response = self
            .client
            .get(url)
            .timeout(remaining.min(INSTALL_TIMEOUT))
            .send()
            .map_err(|_| LxV8SidecarError::InstallNetwork)?;
        if !response.status().is_success() {
            return Err(LxV8SidecarError::InstallStatus(response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|length| length != asset.archive_size || length > MAX_ARCHIVE_BYTES)
        {
            return Err(LxV8SidecarError::Integrity);
        }
        let mut archive_file = tempfile::tempfile()?;
        let mut hasher = Sha256::new();
        let mut downloaded = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            if is_cancelled() {
                return Err(LxV8SidecarError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(LxV8SidecarError::Timeout("installation"));
            }
            let count = response
                .read(&mut buffer)
                .map_err(|_| LxV8SidecarError::InstallNetwork)?;
            if count == 0 {
                break;
            }
            downloaded = downloaded.saturating_add(count as u64);
            if downloaded > MAX_ARCHIVE_BYTES || downloaded > asset.archive_size {
                return Err(LxV8SidecarError::Integrity);
            }
            hasher.update(&buffer[..count]);
            archive_file.write_all(&buffer[..count])?;
        }
        if downloaded != asset.archive_size
            || format!("{:x}", hasher.finalize()) != asset.archive_sha256
        {
            return Err(LxV8SidecarError::Integrity);
        }
        archive_file.flush()?;
        archive_file.rewind()?;
        let mut archive = zip::ZipArchive::new(archive_file)?;
        let entry_name = if cfg!(windows) { "deno.exe" } else { "deno" };
        let mut entry = archive.by_name(entry_name)?;
        if entry.size() > MAX_EXECUTABLE_BYTES {
            return Err(LxV8SidecarError::Integrity);
        }
        let mut temporary = tempfile::Builder::new()
            .prefix(".deno-install-")
            .tempfile_in(&version_dir)?;
        std::io::copy(&mut entry, &mut temporary)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        ensure_executable(temporary.path())?;
        if !verify_file_hash(temporary.path(), asset.executable_sha256, is_cancelled)? {
            return Err(LxV8SidecarError::Integrity);
        }
        if executable.exists() {
            fs::remove_file(&executable)?;
        }
        temporary
            .persist(&executable)
            .map_err(|error| LxV8SidecarError::Io(error.error))?;
        Ok(InstalledSidecar { executable, runner })
    }
}

#[derive(Debug)]
struct V8Execution {
    catalog: BTreeMap<String, SourceInfo>,
    value: Option<JsonValue>,
}

#[derive(Debug, Clone)]
pub(crate) struct ImportedLxV8Provider {
    sidecar: std::sync::Arc<LxV8Sidecar>,
    provider_id: String,
    display_name: String,
    source: String,
    metadata: LxJsMetadata,
    source_catalog: BTreeMap<String, SourceInfo>,
}

impl ImportedLxV8Provider {
    pub(crate) fn new(
        sidecar: std::sync::Arc<LxV8Sidecar>,
        provider_id: impl Into<String>,
        display_name: impl Into<String>,
        source: impl Into<String>,
        metadata: LxJsMetadata,
        source_catalog: BTreeMap<String, SourceInfo>,
    ) -> Self {
        Self {
            sidecar,
            provider_id: provider_id.into(),
            display_name: display_name.into(),
            source: source.into(),
            metadata,
            source_catalog,
        }
    }
}

impl SourceProvider for ImportedLxV8Provider {
    fn id(&self) -> &str {
        &self.provider_id
    }

    fn required_capabilities(&self) -> BTreeSet<SourceCapability> {
        BTreeSet::from([SourceCapability::NetworkAny])
    }

    fn initialize(
        &self,
        context: &mut SourceRuntimeContext,
    ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
        context.require_capability(
            SourceCapability::NetworkAny,
            "initialize isolated LX V8 sidecar",
        )?;
        let execution = self
            .sidecar
            .execute(context, &self.source, &self.metadata, None)
            .map_err(|error| context.provider_error(error.to_string()))?;
        if execution.catalog != self.source_catalog {
            return Err(context
                .provider_error("LX V8 runtime catalog does not match the imported manifest"));
        }
        context.info(format!(
            "initialized {} in the isolated LX V8 sidecar",
            self.display_name
        ));
        Ok(execution.catalog)
    }

    fn handle_request(
        &self,
        context: &mut SourceRuntimeContext,
        request: SourceRequest,
    ) -> Result<SourceResponse, SourceRuntimeError> {
        let SourceRequest::MusicUrl {
            source,
            music_info,
            quality,
        } = request
        else {
            return Err(context.unsupported_action(request.source().to_owned(), request.action()));
        };
        context.require_capability(SourceCapability::NetworkAny, "execute LX V8 musicUrl")?;
        let music_info = normalize_lx_music_info(&source, music_info);
        let payload = json!({
            "source": source,
            "action": "musicUrl",
            "info": { "type": quality.as_str(), "musicInfo": music_info },
        });
        let execution = self
            .sidecar
            .execute(context, &self.source, &self.metadata, Some(&payload))
            .map_err(|error| context.provider_error(error.to_string()))?;
        let url = execution
            .value
            .as_ref()
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| context.provider_error("LX V8 musicUrl handler did not return a URL"))?;
        if !is_http_url(url) {
            return Err(context.provider_error("LX V8 musicUrl handler returned an invalid URL"));
        }
        Ok(SourceResponse::MusicUrl(resolve_webview_media_url(
            context, url,
        )))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SidecarCommand<'a> {
    nonce: &'a str,
    source: &'a str,
    script_info: &'a LxJsMetadata,
    payload: Option<&'a JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum SidecarMessage {
    #[serde(rename = "httpRequest")]
    HttpRequest {
        nonce: String,
        id: String,
        url: String,
        options: JsonValue,
    },
    #[serde(rename = "log")]
    Log {
        nonce: String,
        level: String,
        message: String,
    },
    #[serde(rename = "complete")]
    Complete {
        nonce: String,
        ok: bool,
        #[serde(default)]
        catalog: Option<BTreeMap<String, RawSourceInfo>>,
        #[serde(default)]
        value: Option<JsonValue>,
        #[serde(default)]
        error: Option<String>,
    },
}

impl SidecarMessage {
    fn nonce(&self) -> &str {
        match self {
            Self::HttpRequest { nonce, .. }
            | Self::Log { nonce, .. }
            | Self::Complete { nonce, .. } => nonce,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawSourceInfo {
    #[serde(rename = "name")]
    _name: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    actions: Vec<String>,
    qualitys: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HttpResponseMessage {
    nonce: String,
    #[serde(rename = "type")]
    kind: &'static str,
    id: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response: Option<HttpResponsePayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl HttpResponseMessage {
    fn error(nonce: &str, id: String, error: &str) -> Self {
        Self {
            nonce: nonce.to_owned(),
            kind: "httpResponse",
            id,
            ok: false,
            response: None,
            error: Some(error.to_owned()),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HttpResponsePayload {
    status_code: u16,
    headers: BTreeMap<String, String>,
    body: String,
}

fn handle_http_request(
    context: &mut SourceRuntimeContext,
    nonce: &str,
    id: String,
    url: String,
    options: JsonValue,
    deadline: Instant,
) -> HttpResponseMessage {
    let result = serde_json::from_value::<LxHttpOptions>(options)
        .map_err(|_| "LX V8 request options are invalid".to_owned())
        .and_then(|options| build_http_request(url, options, deadline))
        .and_then(|request| {
            context
                .http_request(request, "execute isolated LX V8 HTTP request")
                .map_err(|error| error.to_string())
        });
    match result {
        Ok(SourceHttpResponse {
            status,
            headers,
            body,
            ..
        }) => HttpResponseMessage {
            nonce: nonce.to_owned(),
            kind: "httpResponse",
            id,
            ok: true,
            response: Some(HttpResponsePayload {
                status_code: status,
                headers,
                body: String::from_utf8_lossy(&body).into_owned(),
            }),
            error: None,
        },
        Err(error) => HttpResponseMessage::error(nonce, id, &sanitize_message(&error)),
    }
}

fn parse_catalog(
    raw: Option<&BTreeMap<String, RawSourceInfo>>,
) -> Result<BTreeMap<String, SourceInfo>, LxV8SidecarError> {
    let mut catalog = BTreeMap::new();
    for (source_id, source) in raw.ok_or(LxV8SidecarError::InvalidCatalog)? {
        if !matches!(
            source_id.as_str(),
            LX_SOURCE_WY
                | LX_SOURCE_TX
                | LX_SOURCE_KW
                | LX_SOURCE_KG
                | LX_SOURCE_MG
                | LX_SOURCE_LOCAL
        ) || source.kind != LX_SOURCE_KIND_MUSIC
            || !source.actions.iter().any(|action| action == "musicUrl")
        {
            continue;
        }
        let mut qualities = source
            .qualitys
            .iter()
            .filter_map(|quality| SourceQuality::from_lx_str(quality))
            .collect::<Vec<_>>();
        qualities.sort();
        qualities.dedup();
        if qualities.is_empty() {
            qualities = crate::source_runtime::standard_lx_qualities();
        }
        catalog.insert(
            source_id.clone(),
            lx_music_source(
                source_id.clone(),
                canonical_source_name(source_id),
                vec![SourceAction::MusicUrl],
                qualities,
            ),
        );
    }
    if catalog.is_empty() {
        Err(LxV8SidecarError::InvalidCatalog)
    } else {
        Ok(catalog)
    }
}

fn canonical_source_name(source_id: &str) -> &str {
    match source_id {
        LX_SOURCE_WY => "NetEase",
        LX_SOURCE_TX => "QQ Music",
        LX_SOURCE_KW => "Kuwo",
        LX_SOURCE_KG => "Kugou",
        LX_SOURCE_MG => "Migu",
        LX_SOURCE_LOCAL => "Local Music",
        _ => source_id,
    }
}

enum OutputEvent {
    Line(String),
    End,
    Error(String),
}

fn spawn_stdout_reader(stdout: std::process::ChildStdout) -> Receiver<OutputEvent> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut total = 0_usize;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = sender.send(OutputEvent::End);
                    return;
                }
                Ok(count) => {
                    total = total.saturating_add(count);
                    if total > MAX_PROTOCOL_BYTES || count > MAX_PROTOCOL_LINE_BYTES {
                        let _ = sender.send(OutputEvent::Error(
                            "sidecar output exceeded the safety limit".to_owned(),
                        ));
                        return;
                    }
                    let line = line.trim_end_matches(['\r', '\n']).to_owned();
                    if sender.send(OutputEvent::Line(line)).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.send(OutputEvent::Error(error.to_string()));
                    return;
                }
            }
        }
    });
    receiver
}

fn spawn_stderr_reader(stderr: std::process::ChildStderr) -> Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr
            .take(MAX_STDERR_BYTES as u64 + 1)
            .read_to_end(&mut bytes);
        bytes.truncate(MAX_STDERR_BYTES);
        let _ = sender.send(bytes);
    });
    receiver
}

fn spawn_sidecar(installed: &InstalledSidecar) -> Result<Child, LxV8SidecarError> {
    Command::new(&installed.executable)
        .args([
            "run",
            "--quiet",
            "--no-config",
            "--no-lock",
            "--cached-only",
            "--no-remote",
            "--no-npm",
            "--no-prompt",
            "--v8-flags=--max-old-space-size=96,--stack-size=512",
        ])
        .arg(&installed.runner)
        .current_dir(
            installed
                .runner
                .parent()
                .ok_or(LxV8SidecarError::ExecutableUnavailable)?,
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .env("NO_COLOR", "1")
        .env("DENO_NO_UPDATE_CHECK", "1")
        .env("DENO_NO_PROMPT", "1")
        .spawn()
        .map_err(|_| LxV8SidecarError::ProcessStart)
}

fn write_protocol_message(
    stdin: &mut ChildStdin,
    message: &impl Serialize,
) -> Result<(), LxV8SidecarError> {
    let encoded = serde_json::to_vec(message)?;
    if encoded.len() > MAX_PROTOCOL_LINE_BYTES {
        return Err(LxV8SidecarError::Protocol(
            "sidecar input exceeded the safety limit".to_owned(),
        ));
    }
    stdin.write_all(&encoded)?;
    stdin.write_all(b"\n")?;
    stdin.flush()?;
    Ok(())
}

fn write_verified_runner(path: &Path) -> Result<(), LxV8SidecarError> {
    let is_regular_file =
        fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file());
    if is_regular_file && fs::read(path).is_ok_and(|bytes| bytes == RUNNER_SOURCE.as_bytes()) {
        restrict_runner(path)?;
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or(LxV8SidecarError::ExecutableUnavailable)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".lx-v8-runner-")
        .tempfile_in(parent)?;
    temporary.write_all(RUNNER_SOURCE.as_bytes())?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    restrict_runner(temporary.path())?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    temporary
        .persist(path)
        .map_err(|error| LxV8SidecarError::Io(error.error))?;
    Ok(())
}

fn verify_file_hash(
    path: &Path,
    expected_sha256: &str,
    is_cancelled: &impl Fn() -> bool,
) -> Result<bool, LxV8SidecarError> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(false);
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_EXECUTABLE_BYTES {
        return Ok(false);
    }
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if is_cancelled() {
            return Err(LxV8SidecarError::Cancelled);
        }
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(expected_sha256))
}

fn current_release_asset() -> Option<ReleaseAsset> {
    release_asset_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn release_asset_for(os: &str, arch: &str) -> Option<ReleaseAsset> {
    let asset = match (os, arch) {
        ("macos", "aarch64") => ReleaseAsset {
            archive_name: "deno-aarch64-apple-darwin.zip",
            archive_sha256: "b796aadd131f6930560c1ee040cf0d6f53933fbb987464e9ff46bd7ea4830615",
            archive_size: 38_511_993,
            executable_sha256: "b5bd08edab254d42d7b05aa5b6cb4c9b8d4dede4975aff76951ce2cce18866fa",
        },
        ("macos", "x86_64") => ReleaseAsset {
            archive_name: "deno-x86_64-apple-darwin.zip",
            archive_sha256: "c1b8b89a81e91b2a8b3f96def3195d08cfe3a105651da7908d53061f7140510d",
            archive_size: 42_346_648,
            executable_sha256: "befc4fee79127584c0f5c9f76ca6bb73c8e6ff523c01acd52e9c5db1968a09cb",
        },
        ("linux", "aarch64") => ReleaseAsset {
            archive_name: "deno-aarch64-unknown-linux-gnu.zip",
            archive_sha256: "6b7cae3a8fc4385a59dea3146fcb8bad7fea4230e0ad36a8c692afacbc254be0",
            archive_size: 39_902_077,
            executable_sha256: "e1a70c5eb03b0ebaf761077029ef86b9ba22d50e2b54ca45ce5437457f701b63",
        },
        ("linux", "x86_64") => ReleaseAsset {
            archive_name: "deno-x86_64-unknown-linux-gnu.zip",
            archive_sha256: "8b010a3b1a4a0188a67cdb8a7a27348b2a501af78aec7fc74f2ace167368d530",
            archive_size: 41_638_854,
            executable_sha256: "dc480c462c8c3582524f3e75c160613d0a975e1f66b5465995d58bae236da7d3",
        },
        ("windows", "aarch64") => ReleaseAsset {
            archive_name: "deno-aarch64-pc-windows-msvc.zip",
            archive_sha256: "73f20b3566a0a6e3f6912fd7bf5b3a7ccd04d68414baedea3b397437bdec6472",
            archive_size: 40_905_829,
            executable_sha256: "ec503fba3b205fd47777d0e90e84ac7ae74d45d94041b46d31b414894c52ad3b",
        },
        ("windows", "x86_64") => ReleaseAsset {
            archive_name: "deno-x86_64-pc-windows-msvc.zip",
            archive_sha256: "171efab55ac6b9881fd53ee4c20f8bf3bb1340ffc618483746909014db12216a",
            archive_size: 42_691_248,
            executable_sha256: "98f8c2a2d470e4ccb04c935c86ff8050817d877762aec5eaeeb9e409ccb3b9fd",
        },
        _ => return None,
    };
    Some(asset)
}

#[cfg(windows)]
fn executable_name() -> &'static str {
    "deno.exe"
}

#[cfg(not(windows))]
fn executable_name() -> &'static str {
    "deno"
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn ensure_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o500))
}

#[cfg(not(unix))]
fn ensure_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_runner(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o400))
}

#[cfg(not(unix))]
fn restrict_runner(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn sidecar_error_summary(stderr: &[u8]) -> String {
    let message = String::from_utf8_lossy(stderr);
    let line = message
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("error:"))
        .or_else(|| message.lines().map(str::trim).find(|line| !line.is_empty()))
        .unwrap_or("the V8 process exited unsuccessfully");
    let detail = line
        .rsplit_once(" Error: ")
        .map(|(_, detail)| detail)
        .or_else(|| line.strip_prefix("error: "))
        .unwrap_or(line);
    sanitize_message(detail)
}

fn sanitize_message(value: &str) -> String {
    let mut sanitized = value
        .split_whitespace()
        .map(|part| {
            let url_start = part.find("https://").or_else(|| part.find("http://"));
            url_start.map_or_else(
                || part.to_owned(),
                |index| format!("{}<url>", &part[..index]),
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    sanitized.retain(|character| !character.is_control());
    sanitized.truncate(MAX_LOG_BYTES);
    if sanitized.is_empty() {
        "LX V8 sidecar failed".to_owned()
    } else {
        sanitized
    }
}

fn is_http_url(value: &str) -> bool {
    url::Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lx_js_importer::analyze_lx_js_source;
    use crate::source_runtime::SourceRuntime;

    #[test]
    fn runtime_catalog_should_accept_music_url_sources() {
        let raw = BTreeMap::from([(
            LX_SOURCE_KG.to_owned(),
            RawSourceInfo {
                _name: Some("Kugou".to_owned()),
                kind: LX_SOURCE_KIND_MUSIC.to_owned(),
                actions: vec!["musicUrl".to_owned()],
                qualitys: vec!["128k".to_owned(), "flac".to_owned()],
            },
        )]);

        let catalog = parse_catalog(Some(&raw)).expect("catalog should parse");

        assert_eq!(
            catalog[LX_SOURCE_KG].qualities,
            [SourceQuality::K128, SourceQuality::Flac]
        );
    }

    #[test]
    fn process_error_summary_should_prefer_the_deno_error_over_stack_frames() {
        let stderr = br#"error: Uncaught (in promise) Error: get music url failed
    at Array.eval (<anonymous>:13:4700)
    at Generator.next (<anonymous>:13:7965)
"#;

        assert_eq!(sidecar_error_summary(stderr), "get music url failed");
    }

    #[test]
    fn release_assets_should_cover_supported_desktop_targets() {
        for (os, arch) in [
            ("macos", "aarch64"),
            ("macos", "x86_64"),
            ("linux", "aarch64"),
            ("linux", "x86_64"),
            ("windows", "aarch64"),
            ("windows", "x86_64"),
        ] {
            assert!(release_asset_for(os, arch).is_some(), "{os}/{arch}");
        }
    }

    #[test]
    fn executable_override_should_install_verified_runner() {
        let root = tempfile::tempdir().expect("test directory should exist");
        let executable = root.path().join(executable_name());
        fs::write(&executable, []).expect("fake executable should write");
        let sidecar = LxV8Sidecar::with_executable(root.path().join("runtime"), executable);

        let installed = sidecar
            .ensure_installed(Duration::from_secs(1), || false)
            .expect("override should install");

        assert_eq!(
            fs::read_to_string(installed.runner).expect("runner should read"),
            RUNNER_SOURCE
        );
    }

    #[test]
    #[ignore = "requires FIKA_LX_V8_LIVE_SOURCE and a live third-party endpoint"]
    fn live_opaque_source_should_initialize_through_v8_sidecar() {
        let source_path = std::env::var_os("FIKA_LX_V8_LIVE_SOURCE")
            .map(PathBuf::from)
            .expect("FIKA_LX_V8_LIVE_SOURCE should point to an LX source");
        let source = fs::read_to_string(&source_path).expect("live LX source should read");
        let report =
            analyze_lx_js_source(&source_path, &source).expect("live LX source should analyze");
        let root = tempfile::tempdir().expect("test directory should exist");
        let sidecar = std::sync::Arc::new(
            if let Some(executable) = std::env::var_os("FIKA_LX_V8_PATH").map(PathBuf::from) {
                LxV8Sidecar::with_executable(root.path(), executable)
            } else {
                LxV8Sidecar::new(root.path()).expect("V8 sidecar should initialize")
            },
        );
        let source_catalog = [
            LX_SOURCE_WY,
            LX_SOURCE_TX,
            LX_SOURCE_KW,
            LX_SOURCE_KG,
            LX_SOURCE_MG,
            LX_SOURCE_LOCAL,
        ]
        .into_iter()
        .map(|source_id| {
            (
                source_id.to_owned(),
                lx_music_source(
                    source_id,
                    canonical_source_name(source_id),
                    vec![SourceAction::MusicUrl],
                    crate::source_runtime::standard_lx_qualities(),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
        let provider = ImportedLxV8Provider::new(
            sidecar,
            "live-v8-provider",
            report.manifest.display_name,
            source,
            report.metadata,
            source_catalog,
        );
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        let source_id =
            std::env::var("FIKA_LX_V8_LIVE_SOURCE_ID").unwrap_or_else(|_| LX_SOURCE_KG.to_owned());
        let music_info = if source_id == LX_SOURCE_KG {
            json!({
                "id": "04DE99837D367481C2CD07C107003E1D",
                "hash": "04DE99837D367481C2CD07C107003E1D",
                "songmid": "04DE99837D367481C2CD07C107003E1D",
                "name": "无烟区",
                "singer": "陈粒",
                "interval": 322,
            })
        } else {
            let track_id = match source_id.as_str() {
                LX_SOURCE_TX => "001IKZC317ahOb",
                LX_SOURCE_KW => "321946135",
                LX_SOURCE_MG => "6005752DXKE",
                _ => "347230",
            };
            json!({
                "id": track_id,
                "title": "海阔天空",
                "name": "海阔天空",
                "artist": "Beyond",
                "singer": "Beyond",
            })
        };

        let initialized = runtime
            .initialize_provider(&provider)
            .expect("live opaque source should initialize through V8");
        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicUrl {
                    source: source_id,
                    music_info,
                    quality: SourceQuality::K128,
                },
            )
            .expect("live opaque source should resolve musicUrl through V8");
        let SourceResponse::MusicUrl(url) = outcome.response else {
            panic!("live opaque source should return musicUrl");
        };

        assert_eq!(initialized.sources.len(), 6);
        assert!(is_http_url(&url));
    }
}
