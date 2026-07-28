use crate::source_runtime::SourceCancellationToken;
use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) const YT_DLP_RELEASE: &str = "2026.07.04";

const AUDIO_FORMAT_SELECTOR: &str = "bestaudio[ext=m4a]/bestaudio";
const INSTALL_TIMEOUT: Duration = Duration::from_secs(3 * 60);
const METADATA_TIMEOUT: Duration = Duration::from_secs(90);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(50);
const MAX_CAPTURE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_SIDECAR_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedAudio {
    pub url: String,
    pub http_headers: BTreeMap<String, String>,
    pub total_bytes: Option<u64>,
    pub format_id: Option<String>,
    pub extension: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum YtDlpSidecarError {
    #[error("yt-dlp is not published for this operating system and CPU architecture")]
    UnsupportedPlatform,
    #[error("could not initialize the yt-dlp installer")]
    Client,
    #[error("yt-dlp installation failed")]
    InstallNetwork,
    #[error("yt-dlp release asset returned HTTP {0}")]
    InstallStatus(u16),
    #[error("yt-dlp release asset failed integrity verification")]
    Integrity,
    #[error("configured yt-dlp executable is unavailable")]
    ExecutableUnavailable,
    #[error("yt-dlp operation was cancelled")]
    Cancelled,
    #[error("yt-dlp {0} timed out")]
    Timeout(&'static str),
    #[error("yt-dlp could not start")]
    ProcessStart,
    #[error("yt-dlp {operation} failed: {detail}")]
    ProcessFailed {
        operation: &'static str,
        detail: String,
    },
    #[error("yt-dlp returned invalid audio metadata")]
    InvalidMetadata,
    #[error("yt-dlp returned no compatible audio-only stream")]
    NoAudioStream,
    #[error("yt-dlp output exceeded the safety limit")]
    OutputTooLarge,
    #[error("yt-dlp downloaded an empty file")]
    EmptyDownload,
    #[error("yt-dlp file operation failed: {0}")]
    Io(#[from] std::io::Error),
}

impl YtDlpSidecarError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedPlatform
            | Self::Client
            | Self::InstallNetwork
            | Self::InstallStatus(_)
            | Self::Integrity
            | Self::ExecutableUnavailable
            | Self::ProcessStart
            | Self::Io(_) => "playback-sidecar-unavailable",
            Self::Cancelled => "cancelled",
            Self::Timeout(_) => "playback-sidecar-timeout",
            Self::ProcessFailed { .. } | Self::InvalidMetadata => "playback-metadata-failure",
            Self::NoAudioStream => "playback-unavailable",
            Self::OutputTooLarge | Self::EmptyDownload => "download-failure",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReleaseAsset {
    name: &'static str,
    url: &'static str,
    sha256: &'static str,
    size: u64,
}

#[derive(Debug)]
enum InstallState {
    Missing,
    Installing,
    Ready(PathBuf),
}

#[derive(Debug)]
struct InstallCoordinator {
    state: Mutex<InstallState>,
    changed: Condvar,
}

pub(crate) struct YtDlpSidecar {
    root: PathBuf,
    client: Client,
    install: InstallCoordinator,
    prewarm_started: AtomicBool,
    executable_override: Option<PathBuf>,
}

impl fmt::Debug for YtDlpSidecar {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("YtDlpSidecar")
            .field("root", &self.root)
            .field("release", &YT_DLP_RELEASE)
            .field(
                "has_executable_override",
                &self.executable_override.is_some(),
            )
            .finish_non_exhaustive()
    }
}

impl YtDlpSidecar {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Result<Self, YtDlpSidecarError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(INSTALL_TIMEOUT)
            .user_agent("FikaMusic/0.1 yt-dlp-installer")
            .build()
            .map_err(|_| YtDlpSidecarError::Client)?;
        Ok(Self {
            root: root.into(),
            client,
            install: InstallCoordinator {
                state: Mutex::new(InstallState::Missing),
                changed: Condvar::new(),
            },
            prewarm_started: AtomicBool::new(false),
            executable_override: std::env::var_os("FIKA_YT_DLP_PATH").map(PathBuf::from),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_executable(root: impl Into<PathBuf>, executable: PathBuf) -> Self {
        let mut sidecar = Self::new(root).expect("test sidecar client should initialize");
        sidecar.executable_override = Some(executable);
        sidecar
    }

    pub(crate) fn prewarm(self: &Arc<Self>) {
        if self.prewarm_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let sidecar = Arc::clone(self);
        if thread::Builder::new()
            .name("fika-yt-dlp-install".to_owned())
            .spawn(move || {
                let cancellation = SourceCancellationToken::default();
                let _ = sidecar.ensure_installed(&cancellation, INSTALL_TIMEOUT);
            })
            .is_err()
        {
            self.prewarm_started.store(false, Ordering::Release);
        }
    }

    pub(crate) fn resolve_audio(
        &self,
        video_id: &str,
        cancellation: &SourceCancellationToken,
    ) -> Result<ResolvedAudio, YtDlpSidecarError> {
        if !is_canonical_video_id(video_id) {
            return Err(YtDlpSidecarError::InvalidMetadata);
        }
        let executable = self.ensure_installed(cancellation, INSTALL_TIMEOUT)?;
        let cache_dir = self.prepare_cache_dir()?;
        let args = metadata_args(video_id, &cache_dir);
        let output = run_captured_process(
            &executable,
            &args,
            None,
            "metadata extraction",
            METADATA_TIMEOUT,
            cancellation,
            || Ok(()),
        )?;
        ensure_process_success(&output, "metadata extraction")?;
        parse_audio_metadata(&output.stdout, video_id)
    }

    pub(crate) fn download_audio(
        &self,
        video_id: &str,
        destination: &Path,
        cancellation: &SourceCancellationToken,
        mut on_progress: impl FnMut(u64, Option<u64>),
    ) -> Result<u64, YtDlpSidecarError> {
        if !is_canonical_video_id(video_id) {
            return Err(YtDlpSidecarError::InvalidMetadata);
        }
        let executable = self.ensure_installed(cancellation, INSTALL_TIMEOUT)?;
        let cache_dir = self.prepare_cache_dir()?;
        let args = download_args(video_id, destination, &cache_dir);
        let mut last_size = 0;
        let output = run_captured_process(
            &executable,
            &args,
            destination.parent(),
            "audio download",
            DOWNLOAD_TIMEOUT,
            cancellation,
            || {
                let size = fs::metadata(destination).map_or(0, |metadata| metadata.len());
                if size > MAX_DOWNLOAD_BYTES {
                    return Err(YtDlpSidecarError::OutputTooLarge);
                }
                if size != last_size {
                    last_size = size;
                    on_progress(size, None);
                }
                Ok(())
            },
        )?;
        ensure_process_success(&output, "audio download")?;
        let size = fs::metadata(destination)?.len();
        if size == 0 {
            return Err(YtDlpSidecarError::EmptyDownload);
        }
        if size > MAX_DOWNLOAD_BYTES {
            return Err(YtDlpSidecarError::OutputTooLarge);
        }
        restrict_download_file(destination)?;
        on_progress(size, Some(size));
        Ok(size)
    }

    fn prepare_cache_dir(&self) -> Result<PathBuf, YtDlpSidecarError> {
        let cache_dir = self.root.join("cache");
        fs::create_dir_all(&cache_dir)?;
        restrict_directory(&cache_dir)?;
        Ok(cache_dir)
    }

    fn ensure_installed(
        &self,
        cancellation: &SourceCancellationToken,
        timeout: Duration,
    ) -> Result<PathBuf, YtDlpSidecarError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(Instant::now);
        loop {
            if cancellation.is_cancelled() {
                return Err(YtDlpSidecarError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(YtDlpSidecarError::Timeout("installation"));
            }
            let mut state = self
                .install
                .state
                .lock()
                .map_err(|_| YtDlpSidecarError::ExecutableUnavailable)?;
            match &*state {
                InstallState::Ready(path) if path.is_file() => return Ok(path.clone()),
                InstallState::Ready(_) => {
                    *state = InstallState::Missing;
                }
                InstallState::Installing => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    let wait = remaining.min(PROCESS_POLL_INTERVAL);
                    let _ = self
                        .install
                        .changed
                        .wait_timeout(state, wait)
                        .map_err(|_| YtDlpSidecarError::ExecutableUnavailable)?;
                }
                InstallState::Missing => {
                    *state = InstallState::Installing;
                    drop(state);
                    let result = self.install_binary(cancellation, deadline);
                    let mut state = self
                        .install
                        .state
                        .lock()
                        .map_err(|_| YtDlpSidecarError::ExecutableUnavailable)?;
                    *state = match &result {
                        Ok(path) => InstallState::Ready(path.clone()),
                        Err(_) => InstallState::Missing,
                    };
                    self.install.changed.notify_all();
                    return result;
                }
            }
        }
    }

    fn install_binary(
        &self,
        cancellation: &SourceCancellationToken,
        deadline: Instant,
    ) -> Result<PathBuf, YtDlpSidecarError> {
        if let Some(path) = &self.executable_override {
            return path
                .is_file()
                .then(|| path.clone())
                .ok_or(YtDlpSidecarError::ExecutableUnavailable);
        }
        let asset = current_release_asset().ok_or(YtDlpSidecarError::UnsupportedPlatform)?;
        let version_dir = self.root.join(YT_DLP_RELEASE);
        fs::create_dir_all(&version_dir)?;
        restrict_directory(&version_dir)?;
        let target = version_dir.join(executable_name());
        if verify_release_asset(&target, asset, cancellation)? {
            ensure_executable(&target)?;
            return Ok(target);
        }
        if cancellation.is_cancelled() {
            return Err(YtDlpSidecarError::Cancelled);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(YtDlpSidecarError::Timeout("installation"));
        }
        let mut response = self
            .client
            .get(asset.url)
            .timeout(remaining.min(INSTALL_TIMEOUT))
            .send()
            .map_err(|_| YtDlpSidecarError::InstallNetwork)?;
        if !response.status().is_success() {
            return Err(YtDlpSidecarError::InstallStatus(response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|length| length != asset.size || length > MAX_SIDECAR_BYTES)
        {
            return Err(YtDlpSidecarError::Integrity);
        }
        let mut temporary = tempfile::Builder::new()
            .prefix(".yt-dlp-install-")
            .tempfile_in(&version_dir)?;
        let mut hasher = Sha256::new();
        let mut downloaded = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            if cancellation.is_cancelled() {
                return Err(YtDlpSidecarError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(YtDlpSidecarError::Timeout("installation"));
            }
            let count = response
                .read(&mut buffer)
                .map_err(|_| YtDlpSidecarError::InstallNetwork)?;
            if count == 0 {
                break;
            }
            downloaded = downloaded.saturating_add(count as u64);
            if downloaded > MAX_SIDECAR_BYTES || downloaded > asset.size {
                return Err(YtDlpSidecarError::Integrity);
            }
            hasher.update(&buffer[..count]);
            temporary.write_all(&buffer[..count])?;
        }
        let actual_hash = format!("{:x}", hasher.finalize());
        if downloaded != asset.size || actual_hash != asset.sha256 {
            return Err(YtDlpSidecarError::Integrity);
        }
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        ensure_executable(temporary.path())?;

        if verify_release_asset(&target, asset, cancellation)? {
            return Ok(target);
        }
        if target.exists() {
            fs::remove_file(&target)?;
        }
        match temporary.persist_noclobber(&target) {
            Ok(_) => {
                ensure_executable(&target)?;
                Ok(target)
            }
            Err(_error) if verify_release_asset(&target, asset, cancellation)? => Ok(target),
            Err(error) => Err(YtDlpSidecarError::Io(error.error)),
        }
    }
}

fn metadata_args(video_id: &str, cache_dir: &Path) -> Vec<OsString> {
    let mut args = common_args(cache_dir);
    args.extend([
        OsString::from("--dump-single-json"),
        OsString::from("--skip-download"),
        OsString::from("--format"),
        OsString::from(AUDIO_FORMAT_SELECTOR),
        OsString::from("--"),
        OsString::from(watch_url(video_id)),
    ]);
    args
}

fn download_args(video_id: &str, destination: &Path, cache_dir: &Path) -> Vec<OsString> {
    let mut args = common_args(cache_dir);
    let output_name = destination.file_name().unwrap_or(destination.as_os_str());
    args.extend([
        OsString::from("--no-part"),
        OsString::from("--force-overwrites"),
        OsString::from("--no-mtime"),
        OsString::from("--format"),
        OsString::from(AUDIO_FORMAT_SELECTOR),
        OsString::from("--output"),
        output_name.to_owned(),
        OsString::from("--"),
        OsString::from(watch_url(video_id)),
    ]);
    args
}

fn common_args(cache_dir: &Path) -> Vec<OsString> {
    let mut args = [
        "--ignore-config",
        "--no-playlist",
        "--no-warnings",
        "--no-progress",
        "--quiet",
        "--socket-timeout",
        "10",
        "--retries",
        "2",
        "--extractor-retries",
        "2",
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<Vec<_>>();
    args.extend([
        OsString::from("--cache-dir"),
        cache_dir.as_os_str().to_owned(),
    ]);
    args
}

fn watch_url(video_id: &str) -> String {
    format!("https://www.youtube.com/watch?v={video_id}")
}

pub(crate) fn is_canonical_video_id(video_id: &str) -> bool {
    video_id.len() == 11
        && video_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[derive(Debug, Default, Deserialize)]
struct RawAudioFormat {
    url: Option<String>,
    format_id: Option<String>,
    ext: Option<String>,
    acodec: Option<String>,
    vcodec: Option<String>,
    filesize: Option<u64>,
    filesize_approx: Option<u64>,
    #[serde(default)]
    http_headers: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct RawMediaInfo {
    id: String,
    #[serde(flatten)]
    selected: RawAudioFormat,
    #[serde(default)]
    requested_downloads: Vec<RawAudioFormat>,
}

fn parse_audio_metadata(
    output: &[u8],
    expected_video_id: &str,
) -> Result<ResolvedAudio, YtDlpSidecarError> {
    let parsed: RawMediaInfo =
        serde_json::from_slice(output).map_err(|_| YtDlpSidecarError::InvalidMetadata)?;
    if parsed.id != expected_video_id {
        return Err(YtDlpSidecarError::InvalidMetadata);
    }
    let format = parsed
        .selected
        .url
        .as_ref()
        .map(|_| &parsed.selected)
        .or_else(|| {
            parsed
                .requested_downloads
                .iter()
                .find(|format| format.url.is_some())
        })
        .ok_or(YtDlpSidecarError::NoAudioStream)?;
    let audio_only = format.vcodec.as_deref() == Some("none")
        && format
            .acodec
            .as_deref()
            .is_some_and(|codec| codec != "none");
    let extension = format
        .ext
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !audio_only || !matches!(extension.as_str(), "m4a" | "mp4" | "webm" | "opus" | "ogg") {
        return Err(YtDlpSidecarError::NoAudioStream);
    }
    let url = format
        .url
        .as_deref()
        .filter(|url| crate::youtube_media_proxy::is_allowed_target(url))
        .ok_or(YtDlpSidecarError::InvalidMetadata)?;
    Ok(ResolvedAudio {
        url: url.to_owned(),
        http_headers: format.http_headers.clone(),
        total_bytes: format.filesize.or(format.filesize_approx),
        format_id: format.format_id.clone(),
        extension,
    })
}

struct CapturedOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_captured_process(
    executable: &Path,
    args: &[OsString],
    working_directory: Option<&Path>,
    operation: &'static str,
    timeout: Duration,
    cancellation: &SourceCancellationToken,
    mut on_tick: impl FnMut() -> Result<(), YtDlpSidecarError>,
) -> Result<CapturedOutput, YtDlpSidecarError> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut command = Command::new(executable);
    command.args(args);
    if let Some(working_directory) = working_directory {
        command.current_dir(working_directory);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?))
        .env("NO_COLOR", "1")
        .env_remove("PYTHONHOME")
        .env_remove("PYTHONPATH")
        .spawn()
        .map_err(|_| YtDlpSidecarError::ProcessStart)?;
    let deadline = Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(Instant::now);
    let status = loop {
        if cancellation.is_cancelled() {
            terminate_child(&mut child);
            return Err(YtDlpSidecarError::Cancelled);
        }
        if Instant::now() >= deadline {
            terminate_child(&mut child);
            return Err(YtDlpSidecarError::Timeout(operation));
        }
        on_tick().inspect_err(|_| terminate_child(&mut child))?;
        if stdout.metadata()?.len() > MAX_CAPTURE_BYTES
            || stderr.metadata()?.len() > MAX_CAPTURE_BYTES
        {
            terminate_child(&mut child);
            return Err(YtDlpSidecarError::OutputTooLarge);
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    };
    let stdout = read_capture(&mut stdout)?;
    let stderr = read_capture(&mut stderr)?;
    Ok(CapturedOutput {
        status,
        stdout,
        stderr,
    })
}

fn terminate_child(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_capture(file: &mut File) -> Result<Vec<u8>, YtDlpSidecarError> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(MAX_CAPTURE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CAPTURE_BYTES {
        return Err(YtDlpSidecarError::OutputTooLarge);
    }
    Ok(bytes)
}

fn ensure_process_success(
    output: &CapturedOutput,
    operation: &'static str,
) -> Result<(), YtDlpSidecarError> {
    if output.status.success() {
        return Ok(());
    }
    Err(YtDlpSidecarError::ProcessFailed {
        operation,
        detail: sidecar_error_summary(&output.stderr),
    })
}

fn sidecar_error_summary(stderr: &[u8]) -> String {
    let message = String::from_utf8_lossy(stderr);
    let line = message
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("the process exited unsuccessfully");
    let mut sanitized = line
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
    sanitized.truncate(512);
    if sanitized.is_empty() {
        "the process exited unsuccessfully".to_owned()
    } else {
        sanitized
    }
}

fn verify_release_asset(
    path: &Path,
    asset: ReleaseAsset,
    cancellation: &SourceCancellationToken,
) -> Result<bool, YtDlpSidecarError> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(false);
    };
    if !metadata.file_type().is_file() || metadata.len() != asset.size {
        return Ok(false);
    }
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if cancellation.is_cancelled() {
            return Err(YtDlpSidecarError::Cancelled);
        }
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()) == asset.sha256)
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
fn restrict_download_file(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_download_file(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn executable_name() -> &'static str {
    "yt-dlp.exe"
}

#[cfg(not(windows))]
fn executable_name() -> &'static str {
    "yt-dlp"
}

fn current_release_asset() -> Option<ReleaseAsset> {
    release_asset_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn release_asset_for(os: &str, arch: &str) -> Option<ReleaseAsset> {
    let (name, sha256, size) = match (os, arch) {
        ("macos", "x86_64" | "aarch64") => (
            "yt-dlp_macos",
            "498bd0dae17855c599d371d68ec5bafc439a9d8640e838be25c765a9792f261b",
            38_256_544,
        ),
        ("linux", "x86_64") => (
            "yt-dlp_linux",
            "6bbb3d314cde4febe36e5fa1d55462e29c974f63444e707871834f6d8cc210ae",
            39_924_536,
        ),
        ("linux", "aarch64") => (
            "yt-dlp_linux_aarch64",
            "b6ce97646773070d7a7ffd6bbbdcaecb47c48483909c54c915bf08a7a9b5e0b1",
            39_675_904,
        ),
        ("windows", "x86_64") => (
            "yt-dlp.exe",
            "52fe3c26dcf71fbdc85b528589020bb0b8e383155cfa81b64dd447bbe35e24b8",
            18_226_085,
        ),
        ("windows", "aarch64") => (
            "yt-dlp_arm64.exe",
            "1525690b037ecc0bb677e38e7147b0025179cbc9a8d0c57264e3100b18099280",
            22_250_288,
        ),
        ("windows", "x86") => (
            "yt-dlp_x86.exe",
            "cac3a9359367ea819289afe4c59f3e432865dafb6b08c938e2c22b4534898f12",
            14_300_315,
        ),
        _ => return None,
    };
    Some(ReleaseAsset {
        name,
        url: match name {
            "yt-dlp_macos" => {
                "https://github.com/yt-dlp/yt-dlp/releases/download/2026.07.04/yt-dlp_macos"
            }
            "yt-dlp_linux" => {
                "https://github.com/yt-dlp/yt-dlp/releases/download/2026.07.04/yt-dlp_linux"
            }
            "yt-dlp_linux_aarch64" => {
                "https://github.com/yt-dlp/yt-dlp/releases/download/2026.07.04/yt-dlp_linux_aarch64"
            }
            "yt-dlp.exe" => {
                "https://github.com/yt-dlp/yt-dlp/releases/download/2026.07.04/yt-dlp.exe"
            }
            "yt-dlp_arm64.exe" => {
                "https://github.com/yt-dlp/yt-dlp/releases/download/2026.07.04/yt-dlp_arm64.exe"
            }
            "yt-dlp_x86.exe" => {
                "https://github.com/yt-dlp/yt-dlp/releases/download/2026.07.04/yt-dlp_x86.exe"
            }
            _ => return None,
        },
        sha256,
        size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_parser_accepts_a_valid_audio_only_google_media_format() {
        let output = br#"{
            "id": "52YupZKmOi0",
            "url": "https://rr3---sn.example.googlevideo.com/videoplayback?id=track",
            "format_id": "140",
            "ext": "m4a",
            "acodec": "mp4a.40.2",
            "vcodec": "none",
            "filesize": 4076243,
            "http_headers": {
                "User-Agent": "YouTube test agent",
                "Accept-Language": "en-us,en;q=0.5"
            }
        }"#;

        let resolved = parse_audio_metadata(output, "52YupZKmOi0")
            .expect("valid yt-dlp audio metadata should parse");

        assert_eq!(resolved.total_bytes, Some(4_076_243));
        assert_eq!(resolved.http_headers["User-Agent"], "YouTube test agent");
        assert!(resolved.url.contains(".googlevideo.com/videoplayback"));
    }

    #[test]
    fn metadata_parser_rejects_non_audio_or_untrusted_media_urls() {
        let video = br#"{
            "id": "52YupZKmOi0",
            "url": "https://rr3---sn.example.googlevideo.com/videoplayback?id=track",
            "ext": "mp4",
            "acodec": "mp4a.40.2",
            "vcodec": "avc1"
        }"#;
        let host_attack = br#"{
            "id": "52YupZKmOi0",
            "url": "https://googlevideo.com.attacker.test/videoplayback?id=track",
            "ext": "m4a",
            "acodec": "mp4a.40.2",
            "vcodec": "none"
        }"#;

        assert!(matches!(
            parse_audio_metadata(video, "52YupZKmOi0"),
            Err(YtDlpSidecarError::NoAudioStream)
        ));
        assert!(matches!(
            parse_audio_metadata(host_attack, "52YupZKmOi0"),
            Err(YtDlpSidecarError::InvalidMetadata)
        ));
    }

    #[test]
    fn release_assets_are_pinned_with_platform_specific_hashes() {
        let macos = release_asset_for("macos", "aarch64").expect("macOS should be supported");
        let windows = release_asset_for("windows", "x86_64").expect("Windows should be supported");

        assert_eq!(macos.name, "yt-dlp_macos");
        assert_eq!(macos.sha256.len(), 64);
        assert_eq!(windows.name, "yt-dlp.exe");
        assert!(release_asset_for("linux", "riscv64").is_none());
    }

    #[test]
    fn process_arguments_ignore_user_config_and_terminate_options_before_the_url() {
        let args = metadata_args("52YupZKmOi0", Path::new("/tmp/fika-cache"));
        let values = args
            .iter()
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>();

        assert!(values.iter().any(|value| value == "--ignore-config"));
        assert_eq!(
            values[values.len() - 2..],
            ["--", "https://www.youtube.com/watch?v=52YupZKmOi0"]
        );
    }

    #[test]
    fn sidecar_errors_redact_signed_urls() {
        let message = sidecar_error_summary(
            b"ERROR: request url=https://rr.example.googlevideo.com/videoplayback?sig=secret failed",
        );

        assert_eq!(message, "ERROR: request url=<url> failed");
        assert!(!message.contains("secret"));
    }

    #[cfg(unix)]
    #[test]
    fn sidecar_download_uses_the_controlled_output_path() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temporary directory should open");
        let executable = root.path().join("fake-yt-dlp");
        fs::write(
            &executable,
            "#!/bin/sh\nwhile [ \"$1\" != \"--output\" ]; do shift; done\nshift\nprintf 'fika-audio' > \"$1\"\n",
        )
        .expect("fake sidecar should write");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
            .expect("fake sidecar should be executable");
        let sidecar = YtDlpSidecar::with_executable(root.path(), executable);
        let output_dir = root.path().join("%music%");
        fs::create_dir(&output_dir).expect("output directory should exist");
        let destination = output_dir.join("track.m4a");
        let cancellation = SourceCancellationToken::default();
        let mut observed = Vec::new();

        let bytes = sidecar
            .download_audio(
                "52YupZKmOi0",
                &destination,
                &cancellation,
                |downloaded, total| observed.push((downloaded, total)),
            )
            .expect("fake sidecar download should finish");

        assert_eq!(bytes, 10);
        assert_eq!(
            fs::read(destination).expect("download should read"),
            b"fika-audio"
        );
        assert_eq!(observed.last(), Some(&(10, Some(10))));
    }
}
