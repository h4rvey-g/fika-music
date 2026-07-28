use crate::audio_source_system::{
    AudioSourceManifest, BundledAudioSourceBuildContext, BundledAudioSourceRegistration,
};
use crate::registry_support::sha256_hex;
use crate::source_runtime::{
    self, SourceAction, SourceCapability, SourceInfo, SourceProvider, SourceQuality, SourceRequest,
    SourceResponse, SourceRuntimeApiVersion, SourceRuntimeContext, SourceRuntimeError,
};
use crate::youtube_music::YOUTUBE_MUSIC_SOURCE_ID;
use crate::yt_dlp_sidecar::{
    is_canonical_video_id, ResolvedAudio, YtDlpSidecar, YtDlpSidecarError, YT_DLP_RELEASE,
};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

pub const YOUTUBE_MUSIC_AUDIO_SOURCE_ID: &str = "fika.youtube-music-playback";
pub const YOUTUBE_MUSIC_AUDIO_PROVIDER_ID: &str = "fika-youtube-music-playback";
pub const YOUTUBE_MUSIC_AUDIO_ADAPTER: &str = "builtin:youtube-music-playback";
pub const YOUTUBE_MUSIC_AUDIO_PROVIDER_API_VERSION: SourceRuntimeApiVersion =
    SourceRuntimeApiVersion::new(1, 4);
const YOUTUBE_MUSIC_AUDIO_SOURCE_VERSION: &str = "0.2.0";

trait YoutubePlaybackResolver: Send + Sync {
    fn prewarm(&self) {}

    fn resolve(
        &self,
        video_id: &str,
        cancellation: &source_runtime::SourceCancellationToken,
    ) -> Result<ResolvedAudio, YtDlpSidecarError>;
}

#[derive(Clone)]
struct YtDlpPlaybackResolver {
    sidecar: Arc<YtDlpSidecar>,
}

impl fmt::Debug for YtDlpPlaybackResolver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("YtDlpPlaybackResolver")
            .finish_non_exhaustive()
    }
}

impl YtDlpPlaybackResolver {
    fn new(sidecar: Arc<YtDlpSidecar>) -> Self {
        Self { sidecar }
    }
}

impl YoutubePlaybackResolver for YtDlpPlaybackResolver {
    fn prewarm(&self) {
        self.sidecar.prewarm();
    }

    fn resolve(
        &self,
        video_id: &str,
        cancellation: &source_runtime::SourceCancellationToken,
    ) -> Result<ResolvedAudio, YtDlpSidecarError> {
        self.sidecar.resolve_audio(video_id, cancellation)
    }
}

pub struct YoutubeMusicPlaybackProvider {
    id: String,
    capabilities: BTreeSet<SourceCapability>,
    resolver: Arc<dyn YoutubePlaybackResolver>,
}

impl fmt::Debug for YoutubeMusicPlaybackProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("YoutubeMusicPlaybackProvider")
            .field("id", &self.id)
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

impl YoutubeMusicPlaybackProvider {
    fn new(
        id: String,
        capabilities: BTreeSet<SourceCapability>,
        resolver: Arc<dyn YoutubePlaybackResolver>,
    ) -> Self {
        Self {
            id,
            capabilities,
            resolver,
        }
    }

    fn from_build_context(
        context: BundledAudioSourceBuildContext,
        sidecar: Arc<YtDlpSidecar>,
    ) -> Result<Self, String> {
        Ok(Self::new(
            context.provider_id,
            context.declared_capabilities,
            Arc::new(YtDlpPlaybackResolver::new(sidecar)),
        ))
    }
}

impl SourceProvider for YoutubeMusicPlaybackProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn api_version(&self) -> SourceRuntimeApiVersion {
        YOUTUBE_MUSIC_AUDIO_PROVIDER_API_VERSION
    }

    fn required_capabilities(&self) -> BTreeSet<SourceCapability> {
        self.capabilities.clone()
    }

    fn initialize(
        &self,
        context: &mut SourceRuntimeContext,
    ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
        if context.has_capability(SourceCapability::NetworkAny) {
            self.resolver.prewarm();
        }
        context.info(format!(
            "initialized bundled YouTube Music Audio Source (yt-dlp sidecar {YT_DLP_RELEASE})"
        ));
        Ok(youtube_audio_source_catalog())
    }

    fn handle_request(
        &self,
        context: &mut SourceRuntimeContext,
        request: SourceRequest,
    ) -> Result<SourceResponse, SourceRuntimeError> {
        let SourceRequest::MusicUrl { music_info, .. } = request else {
            return Err(context.unsupported_action(request.source(), request.action()));
        };
        let operation = "resolve YouTube Music playback URL";
        context.require_capability(SourceCapability::NetworkAny, operation)?;
        context.ensure_not_cancelled(operation)?;
        let video_id = video_id_from_music_info(&music_info).ok_or_else(|| {
            context.provider_error_with_code(
                "invalid-track",
                "YouTube Music playback requires a valid videoId",
            )
        })?;
        let cancellation = context.cancellation_token();
        let resolved = self
            .resolver
            .resolve(&video_id, &cancellation)
            .map_err(|error| context.provider_error_with_code(error.code(), error.to_string()))?;
        context.ensure_not_cancelled(operation)?;
        if !crate::youtube_media_proxy::register_media_headers(
            &resolved.url,
            &resolved.http_headers,
        ) {
            return Err(context.provider_error_with_code(
                YtDlpSidecarError::InvalidMetadata.code(),
                YtDlpSidecarError::InvalidMetadata.to_string(),
            ));
        }
        context.info(format!(
            "yt-dlp selected audio format {} ({}, {} bytes)",
            resolved.format_id.as_deref().unwrap_or("unknown"),
            resolved.extension,
            resolved
                .total_bytes
                .map_or_else(|| "unknown".to_owned(), |bytes| bytes.to_string())
        ));
        Ok(SourceResponse::MusicUrl(resolved.url))
    }
}

pub(crate) fn bundled_audio_source_registration(
    sidecar: Arc<YtDlpSidecar>,
) -> BundledAudioSourceRegistration {
    let source_fingerprint = sha256_hex(
        format!(
            "{YOUTUBE_MUSIC_AUDIO_ADAPTER}:{YOUTUBE_MUSIC_AUDIO_PROVIDER_API_VERSION}:{YOUTUBE_MUSIC_AUDIO_SOURCE_VERSION}:{YT_DLP_RELEASE}"
        )
        .as_bytes(),
    );
    let manifest = AudioSourceManifest {
        manifest_version: crate::audio_source_system::AUDIO_SOURCE_MANIFEST_VERSION,
        id: YOUTUBE_MUSIC_AUDIO_SOURCE_ID.to_owned(),
        name: "YouTube Music Playback".to_owned(),
        version: YOUTUBE_MUSIC_AUDIO_SOURCE_VERSION.to_owned(),
        description: Some(
            "Bundled Rust Audio Source backed by a verified official yt-dlp sidecar. Catalog browsing is provided by the separate YouTube Music Plugin."
                .to_owned(),
        ),
        author: Some("Fika Music".to_owned()),
        homepage: Some("https://github.com/yt-dlp/yt-dlp".to_owned()),
        provider_id: YOUTUBE_MUSIC_AUDIO_PROVIDER_ID.to_owned(),
        adapter: YOUTUBE_MUSIC_AUDIO_ADAPTER.to_owned(),
        source_fingerprint,
        capabilities: BTreeSet::from([SourceCapability::NetworkAny]),
        supported_api_version: YOUTUBE_MUSIC_AUDIO_PROVIDER_API_VERSION,
        source_catalog: youtube_audio_source_catalog(),
    };
    BundledAudioSourceRegistration::new(manifest, move |context| {
        let provider: Arc<dyn SourceProvider> = Arc::new(
            YoutubeMusicPlaybackProvider::from_build_context(context, Arc::clone(&sidecar))?,
        );
        Ok(provider)
    })
}

fn youtube_audio_source_catalog() -> BTreeMap<String, SourceInfo> {
    BTreeMap::from([(
        YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
        source_runtime::lx_music_source(
            YOUTUBE_MUSIC_SOURCE_ID,
            "YouTube Music",
            vec![SourceAction::MusicUrl],
            vec![SourceQuality::K128],
        ),
    )])
}

pub(crate) fn video_id_from_music_info(music_info: &JsonValue) -> Option<String> {
    ["videoId", "id"]
        .into_iter()
        .find_map(|key| music_info.get(key).and_then(JsonValue::as_str))
        .map(str::to_owned)
        .filter(|id| is_canonical_video_id(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_runtime::SourceRuntime;
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tauri::http::header::{CONTENT_RANGE, CONTENT_TYPE, RANGE};
    use tauri::http::{Request, StatusCode};

    #[derive(Debug)]
    struct FakeResolver(Result<ResolvedAudio, &'static str>);

    impl YoutubePlaybackResolver for FakeResolver {
        fn resolve(
            &self,
            _video_id: &str,
            _cancellation: &source_runtime::SourceCancellationToken,
        ) -> Result<ResolvedAudio, YtDlpSidecarError> {
            self.0
                .clone()
                .map_err(|_| YtDlpSidecarError::InvalidMetadata)
        }
    }

    #[derive(Debug)]
    struct PrewarmResolver(Arc<AtomicUsize>);

    impl YoutubePlaybackResolver for PrewarmResolver {
        fn prewarm(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }

        fn resolve(
            &self,
            _video_id: &str,
            _cancellation: &source_runtime::SourceCancellationToken,
        ) -> Result<ResolvedAudio, YtDlpSidecarError> {
            Ok(fake_audio())
        }
    }

    fn fake_audio() -> ResolvedAudio {
        ResolvedAudio {
            url: "https://rr5---sn.example.googlevideo.com/videoplayback?id=fake".to_owned(),
            http_headers: BTreeMap::new(),
            total_bytes: Some(1_024),
            format_id: Some("140".to_owned()),
            extension: "m4a".to_owned(),
        }
    }

    #[test]
    fn playback_provider_exposes_only_music_url_for_youtube() {
        let provider = YoutubeMusicPlaybackProvider::new(
            YOUTUBE_MUSIC_AUDIO_PROVIDER_ID.to_owned(),
            BTreeSet::from([SourceCapability::NetworkAny]),
            Arc::new(FakeResolver(Ok(fake_audio()))),
        );
        let runtime = SourceRuntime::new();
        let report = runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        assert_eq!(
            report.sources[YOUTUBE_MUSIC_SOURCE_ID].actions,
            [SourceAction::MusicUrl]
        );
        assert_eq!(
            report.sources[YOUTUBE_MUSIC_SOURCE_ID].qualities,
            [SourceQuality::K128]
        );
    }

    #[test]
    fn playback_provider_prewarms_sidecar_only_after_network_is_granted() {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = YoutubeMusicPlaybackProvider::new(
            YOUTUBE_MUSIC_AUDIO_PROVIDER_ID.to_owned(),
            BTreeSet::from([SourceCapability::NetworkAny]),
            Arc::new(PrewarmResolver(Arc::clone(&calls))),
        );
        let runtime = SourceRuntime::new();

        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize without a grant");
        assert_eq!(calls.load(Ordering::Relaxed), 0);

        let granted_runtime = SourceRuntime::new();
        granted_runtime
            .replace_provider_granted_capabilities(
                YOUTUBE_MUSIC_AUDIO_PROVIDER_ID,
                [SourceCapability::NetworkAny],
            )
            .expect("network grant should install");
        granted_runtime
            .initialize_provider(&provider)
            .expect("provider should initialize with a grant");
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn playback_provider_requires_a_canonical_video_id() {
        let provider = YoutubeMusicPlaybackProvider::new(
            YOUTUBE_MUSIC_AUDIO_PROVIDER_ID.to_owned(),
            BTreeSet::from([SourceCapability::NetworkAny]),
            Arc::new(FakeResolver(Ok(fake_audio()))),
        );
        let runtime = SourceRuntime::new();
        runtime
            .replace_provider_granted_capabilities(
                YOUTUBE_MUSIC_AUDIO_PROVIDER_ID,
                [SourceCapability::NetworkAny],
            )
            .expect("capability grant should install");
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        let error = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicUrl {
                    source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
                    music_info: json!({ "videoId": "https://not-an-id.test" }),
                    quality: SourceQuality::K128,
                },
            )
            .expect_err("arbitrary URLs must not be accepted as track IDs");

        assert!(error.to_string().contains("valid videoId"));
    }

    #[test]
    fn playback_provider_returns_the_resolved_audio_url() {
        let provider = YoutubeMusicPlaybackProvider::new(
            YOUTUBE_MUSIC_AUDIO_PROVIDER_ID.to_owned(),
            BTreeSet::from([SourceCapability::NetworkAny]),
            Arc::new(FakeResolver(Ok(fake_audio()))),
        );
        let runtime = SourceRuntime::new();
        runtime
            .replace_provider_granted_capabilities(
                YOUTUBE_MUSIC_AUDIO_PROVIDER_ID,
                [SourceCapability::NetworkAny],
            )
            .expect("capability grant should install");
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicUrl {
                    source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
                    music_info: json!({ "videoId": "ZrOKjDZOtkA" }),
                    quality: SourceQuality::K128,
                },
            )
            .expect("playback should resolve");

        assert_eq!(outcome.response, SourceResponse::MusicUrl(fake_audio().url));
    }

    #[test]
    #[ignore = "live YouTube contract test"]
    fn live_sidecar_returns_playback_bytes_and_downloads_audio() {
        let root = tempfile::tempdir().expect("temporary directory should open");
        let sidecar = Arc::new(
            YtDlpSidecar::new(root.path().join("sidecar"))
                .expect("sidecar client should initialize"),
        );
        let resolver = YtDlpPlaybackResolver::new(Arc::clone(&sidecar));
        let cancellation = source_runtime::SourceCancellationToken::default();
        let resolved = resolver
            .resolve("52YupZKmOi0", &cancellation)
            .expect("public video should resolve");
        assert!(crate::youtube_media_proxy::register_media_headers(
            &resolved.url,
            &resolved.http_headers,
        ));
        let encoded = utf8_percent_encode(&resolved.url, NON_ALPHANUMERIC);
        let request = Request::builder()
            .uri(format!(
                "{}://localhost/{encoded}",
                crate::youtube_media_proxy::YOUTUBE_MEDIA_PROTOCOL
            ))
            .header(RANGE, "bytes=0-1023")
            .body(Vec::new())
            .expect("proxy request should build");
        let response = crate::youtube_media_proxy::protocol_response(request);
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert!(content_type.starts_with("audio/"), "{content_type}");
        assert!(response.headers().contains_key(CONTENT_RANGE));
        assert!(!response.body().is_empty());

        let download_path = root.path().join("download.m4a");
        let bytes = sidecar
            .download_audio("52YupZKmOi0", &download_path, &cancellation, |_, _| {})
            .expect("sidecar should download the public audio stream");
        assert!(bytes > 1_024);
        assert!(lofty::read_from_path(download_path).is_ok());
    }
}
