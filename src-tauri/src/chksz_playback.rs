use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use url::Url;

use crate::audio_source_system::{AudioSourceManifest, BundledAudioSourceRegistration};
use crate::database::{AppCredentialStore, CredentialStoreError};
use crate::registry_support::sha256_hex;
use crate::source_runtime::{
    self, SourceAction, SourceCancellationToken, SourceCapability, SourceHost, SourceHostError,
    SourceHttpRequest, SourceInfo, SourceProvider, SourceQuality, SourceRequest, SourceResponse,
    SourceRuntimeApiVersion, SourceRuntimeContext, SourceRuntimeError, LX_SOURCE_WY,
};

pub const CHKSZ_AUDIO_SOURCE_ID: &str = "fika.chksz-netease-playback";
pub const CHKSZ_PROVIDER_ID: &str = "fika-chksz-netease-playback";
pub const CHKSZ_AUDIO_ADAPTER: &str = "builtin:chksz-netease-playback";
pub const CHKSZ_PROVIDER_API_VERSION: SourceRuntimeApiVersion = SourceRuntimeApiVersion::new(1, 4);

const CHKSZ_AUDIO_SOURCE_VERSION: &str = "0.1.0";
const CHKSZ_MUSIC_ENDPOINT: &str = "https://api.chksz.com/api/163_music";
const CHKSZ_API_KEY_REF: &str = "api-key";
const MAX_API_KEY_BYTES: usize = 512;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ChkszPlaybackError {
    #[error("ChKSz API key is not configured")]
    MissingApiKey,
    #[error("ChKSz API key is invalid")]
    InvalidApiKey,
    #[error("NetEase track ID is invalid")]
    InvalidTrack,
    #[error("ChKSz endpoint configuration is invalid")]
    EndpointConfiguration,
    #[error("ChKSz credential storage failed")]
    CredentialStore {
        #[source]
        source: CredentialStoreError,
    },
    #[error("ChKSz network request failed")]
    Host {
        #[source]
        source: SourceHostError,
    },
    #[error("ChKSz returned HTTP {0}")]
    HttpStatus(u16),
    #[error("ChKSz rejected the API key")]
    AuthenticationFailed,
    #[error("ChKSz rejected the playback request (code {0})")]
    ApiRejected(i64),
    #[error("ChKSz returned an invalid response")]
    InvalidResponse,
    #[error("ChKSz returned an invalid playback URL")]
    InvalidPlaybackUrl,
}

impl ChkszPlaybackError {
    fn code(&self) -> &'static str {
        match self {
            Self::MissingApiKey => "missing-api-key",
            Self::InvalidApiKey => "invalid-api-key",
            Self::InvalidTrack => "invalid-track",
            Self::EndpointConfiguration => "endpoint-configuration",
            Self::CredentialStore { .. } => "credential-store",
            Self::Host { .. } => "network",
            Self::HttpStatus(_) => "http-status",
            Self::AuthenticationFailed => "authentication-failed",
            Self::ApiRejected(_) => "api-rejected",
            Self::InvalidResponse => "invalid-response",
            Self::InvalidPlaybackUrl => "invalid-playback-url",
        }
    }
}

trait ChkszCredentialStore: Send + Sync {
    fn save(&self, key: &str) -> Result<(), ChkszPlaybackError>;
    fn load(&self) -> Result<Option<String>, ChkszPlaybackError>;
    fn clear(&self) -> Result<(), ChkszPlaybackError>;
}

impl ChkszCredentialStore for AppCredentialStore {
    fn save(&self, key: &str) -> Result<(), ChkszPlaybackError> {
        self.save_secret(CHKSZ_API_KEY_REF, key)
            .map_err(|source| ChkszPlaybackError::CredentialStore { source })
    }

    fn load(&self) -> Result<Option<String>, ChkszPlaybackError> {
        self.load_secret(CHKSZ_API_KEY_REF)
            .map_err(|source| ChkszPlaybackError::CredentialStore { source })
    }

    fn clear(&self) -> Result<(), ChkszPlaybackError> {
        self.delete_secret(CHKSZ_API_KEY_REF)
            .map_err(|source| ChkszPlaybackError::CredentialStore { source })
    }
}

trait ChkszPlaybackResolver: Send + Sync {
    fn resolve(
        &self,
        track_id: &str,
        quality: SourceQuality,
        cancellation: &SourceCancellationToken,
    ) -> Result<String, ChkszPlaybackError>;
}

pub(crate) struct ChkszPlaybackService {
    credentials: Arc<dyn ChkszCredentialStore>,
    source_host: Arc<dyn SourceHost>,
}

impl ChkszPlaybackService {
    pub(crate) fn new(
        connection: Arc<Mutex<Connection>>,
        source_host: Arc<dyn SourceHost>,
    ) -> Self {
        Self::with_dependencies(
            Arc::new(AppCredentialStore::new(connection, CHKSZ_PROVIDER_ID)),
            source_host,
        )
    }

    fn with_dependencies(
        credentials: Arc<dyn ChkszCredentialStore>,
        source_host: Arc<dyn SourceHost>,
    ) -> Self {
        Self {
            credentials,
            source_host,
        }
    }

    pub(crate) fn api_key_configured(&self) -> Result<bool, ChkszPlaybackError> {
        self.credentials
            .load()
            .map(|key| key.is_some_and(|value| !value.is_empty()))
    }

    pub(crate) fn set_api_key(&self, api_key: &str) -> Result<(), ChkszPlaybackError> {
        self.credentials.save(validate_api_key(api_key)?)
    }

    pub(crate) fn clear_api_key(&self) -> Result<(), ChkszPlaybackError> {
        self.credentials.clear()
    }
}

impl ChkszPlaybackResolver for ChkszPlaybackService {
    fn resolve(
        &self,
        track_id: &str,
        quality: SourceQuality,
        cancellation: &SourceCancellationToken,
    ) -> Result<String, ChkszPlaybackError> {
        if !valid_netease_track_id(track_id) {
            return Err(ChkszPlaybackError::InvalidTrack);
        }
        let api_key = self
            .credentials
            .load()?
            .filter(|value| !value.is_empty())
            .ok_or(ChkszPlaybackError::MissingApiKey)?;
        let mut endpoint = Url::parse(CHKSZ_MUSIC_ENDPOINT)
            .map_err(|_| ChkszPlaybackError::EndpointConfiguration)?;
        endpoint
            .query_pairs_mut()
            .append_pair("id", track_id)
            .append_pair("level", chksz_quality(quality))
            .append_pair("type", "json")
            .append_pair("apikey", &api_key);
        let response = self
            .source_host
            .http_request(
                CHKSZ_PROVIDER_ID,
                &SourceHttpRequest::get(endpoint.to_string()),
                cancellation,
            )
            .map_err(|source| ChkszPlaybackError::Host { source })?;
        if !response.is_success() {
            return Err(ChkszPlaybackError::HttpStatus(response.status));
        }
        let payload = serde_json::from_slice::<ChkszMusicResponse>(&response.body)
            .map_err(|_| ChkszPlaybackError::InvalidResponse)?;
        if payload.code == 401 {
            return Err(ChkszPlaybackError::AuthenticationFailed);
        }
        if payload.code != 200 {
            return Err(ChkszPlaybackError::ApiRejected(payload.code));
        }
        let playback_url = payload
            .data
            .map(|data| data.url)
            .filter(|url| !url.trim().is_empty())
            .ok_or(ChkszPlaybackError::InvalidResponse)?;
        validate_playback_url(&playback_url)?;
        Ok(playback_url)
    }
}

struct ChkszPlaybackProvider {
    id: String,
    capabilities: BTreeSet<SourceCapability>,
    resolver: Arc<dyn ChkszPlaybackResolver>,
}

impl ChkszPlaybackProvider {
    fn new(
        id: String,
        capabilities: BTreeSet<SourceCapability>,
        resolver: Arc<dyn ChkszPlaybackResolver>,
    ) -> Self {
        Self {
            id,
            capabilities,
            resolver,
        }
    }
}

impl SourceProvider for ChkszPlaybackProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn api_version(&self) -> SourceRuntimeApiVersion {
        CHKSZ_PROVIDER_API_VERSION
    }

    fn required_capabilities(&self) -> BTreeSet<SourceCapability> {
        self.capabilities.clone()
    }

    fn initialize(
        &self,
        context: &mut SourceRuntimeContext,
    ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
        context.info("initialized bundled ChKSz NetEase Audio Source");
        Ok(chksz_audio_source_catalog())
    }

    fn handle_request(
        &self,
        context: &mut SourceRuntimeContext,
        request: SourceRequest,
    ) -> Result<SourceResponse, SourceRuntimeError> {
        let SourceRequest::MusicUrl {
            music_info,
            quality,
            ..
        } = request
        else {
            return Err(context.unsupported_action(request.source(), request.action()));
        };
        let operation = "resolve ChKSz NetEase playback URL";
        context.require_capability(SourceCapability::NetworkAny, operation)?;
        context.ensure_not_cancelled(operation)?;
        let track_id = netease_track_id(&music_info).ok_or_else(|| {
            context.provider_error_with_code("invalid-track", "NetEase track ID is invalid")
        })?;
        let cancellation = context.cancellation_token();
        let playback_url = match self.resolver.resolve(&track_id, quality, &cancellation) {
            Ok(url) => url,
            Err(ChkszPlaybackError::Host {
                source: SourceHostError::Cancelled,
            }) => {
                context.ensure_not_cancelled(operation)?;
                return Err(context.provider_error_with_code("cancelled", "request was cancelled"));
            }
            Err(error) => {
                return Err(context.provider_error_with_code(error.code(), error.to_string()));
            }
        };
        context.ensure_not_cancelled(operation)?;
        context.info(format!(
            "resolved ChKSz musicUrl for NetEase track {track_id}"
        ));
        Ok(SourceResponse::MusicUrl(playback_url))
    }
}

pub(crate) fn bundled_audio_source_registration(
    service: Arc<ChkszPlaybackService>,
) -> BundledAudioSourceRegistration {
    let source_fingerprint = sha256_hex(
        format!(
            "{CHKSZ_AUDIO_ADAPTER}:{CHKSZ_PROVIDER_API_VERSION}:{CHKSZ_AUDIO_SOURCE_VERSION}:{CHKSZ_MUSIC_ENDPOINT}:standard:exhigh:lossless:hires"
        )
        .as_bytes(),
    );
    let manifest = AudioSourceManifest {
        manifest_version: crate::audio_source_system::AUDIO_SOURCE_MANIFEST_VERSION,
        id: CHKSZ_AUDIO_SOURCE_ID.to_owned(),
        name: "ChKSz NetEase Playback".to_owned(),
        version: CHKSZ_AUDIO_SOURCE_VERSION.to_owned(),
        description: Some(
            "Bundled NetEase playback URL resolver backed by the ChKSz 163_music API.".to_owned(),
        ),
        author: Some("Fika Music".to_owned()),
        homepage: Some("https://api.chksz.com/docs/163_music.html".to_owned()),
        provider_id: CHKSZ_PROVIDER_ID.to_owned(),
        adapter: CHKSZ_AUDIO_ADAPTER.to_owned(),
        source_fingerprint,
        capabilities: BTreeSet::from([SourceCapability::NetworkAny]),
        supported_api_version: CHKSZ_PROVIDER_API_VERSION,
        source_catalog: chksz_audio_source_catalog(),
    };
    BundledAudioSourceRegistration::new(manifest, move |context| {
        let resolver: Arc<dyn ChkszPlaybackResolver> = service.clone();
        let provider: Arc<dyn SourceProvider> = Arc::new(ChkszPlaybackProvider::new(
            context.provider_id,
            context.declared_capabilities,
            resolver,
        ));
        Ok(provider)
    })
}

fn chksz_audio_source_catalog() -> BTreeMap<String, SourceInfo> {
    BTreeMap::from([(
        LX_SOURCE_WY.to_owned(),
        source_runtime::lx_music_source(
            LX_SOURCE_WY,
            "NetEase",
            vec![SourceAction::MusicUrl],
            vec![
                SourceQuality::K128,
                SourceQuality::K320,
                SourceQuality::Flac,
                SourceQuality::Flac24Bit,
            ],
        ),
    )])
}

fn validate_api_key(api_key: &str) -> Result<&str, ChkszPlaybackError> {
    let api_key = api_key.trim();
    if api_key.is_empty()
        || api_key.len() > MAX_API_KEY_BYTES
        || !api_key.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(ChkszPlaybackError::InvalidApiKey);
    }
    Ok(api_key)
}

fn valid_netease_track_id(track_id: &str) -> bool {
    !track_id.is_empty()
        && track_id.len() <= 20
        && track_id.bytes().all(|byte| byte.is_ascii_digit())
}

fn netease_track_id(music_info: &JsonValue) -> Option<String> {
    ["id", "songId"].into_iter().find_map(|key| {
        let value = music_info.get(key)?;
        let track_id = value
            .as_str()
            .map(str::to_owned)
            .or_else(|| value.as_u64().map(|value| value.to_string()))?;
        valid_netease_track_id(&track_id).then_some(track_id)
    })
}

const fn chksz_quality(quality: SourceQuality) -> &'static str {
    match quality {
        SourceQuality::K128 => "standard",
        SourceQuality::K320 => "exhigh",
        SourceQuality::Flac => "lossless",
        SourceQuality::Flac24Bit => "hires",
    }
}

fn validate_playback_url(value: &str) -> Result<(), ChkszPlaybackError> {
    let url = Url::parse(value).map_err(|_| ChkszPlaybackError::InvalidPlaybackUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ChkszPlaybackError::InvalidPlaybackUrl);
    }
    Ok(())
}

#[derive(Deserialize)]
struct ChkszMusicResponse {
    code: i64,
    data: Option<ChkszMusicData>,
}

#[derive(Deserialize)]
struct ChkszMusicData {
    url: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_runtime::{SourceHttpResponse, SourceRuntime};
    use serde_json::json;

    #[derive(Default)]
    struct FakeCredentialStore {
        key: Mutex<Option<String>>,
    }

    impl ChkszCredentialStore for FakeCredentialStore {
        fn save(&self, key: &str) -> Result<(), ChkszPlaybackError> {
            *self
                .key
                .lock()
                .expect("credential lock should be available") = Some(key.to_owned());
            Ok(())
        }

        fn load(&self) -> Result<Option<String>, ChkszPlaybackError> {
            Ok(self
                .key
                .lock()
                .expect("credential lock should be available")
                .clone())
        }

        fn clear(&self) -> Result<(), ChkszPlaybackError> {
            *self
                .key
                .lock()
                .expect("credential lock should be available") = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingHost {
        requests: Mutex<Vec<SourceHttpRequest>>,
    }

    impl SourceHost for RecordingHost {
        fn http_request(
            &self,
            _source_id: &str,
            request: &SourceHttpRequest,
            _cancellation: &SourceCancellationToken,
        ) -> Result<SourceHttpResponse, SourceHostError> {
            self.requests
                .lock()
                .expect("request lock should be available")
                .push(request.clone());
            Ok(SourceHttpResponse {
                status: 200,
                final_url: CHKSZ_MUSIC_ENDPOINT.to_owned(),
                headers: BTreeMap::new(),
                content_type: Some("application/json".to_owned()),
                body: br#"{"code":200,"data":{"url":"https://m.example.test/song.flac"}}"#.to_vec(),
            })
        }
    }

    #[derive(Default)]
    struct FakeResolver {
        requests: Mutex<Vec<(String, SourceQuality)>>,
    }

    impl ChkszPlaybackResolver for FakeResolver {
        fn resolve(
            &self,
            track_id: &str,
            quality: SourceQuality,
            _cancellation: &SourceCancellationToken,
        ) -> Result<String, ChkszPlaybackError> {
            self.requests
                .lock()
                .expect("resolver lock should be available")
                .push((track_id.to_owned(), quality));
            Ok("https://m.example.test/song.flac".to_owned())
        }
    }

    struct CancellingResolver;

    impl ChkszPlaybackResolver for CancellingResolver {
        fn resolve(
            &self,
            _track_id: &str,
            _quality: SourceQuality,
            cancellation: &SourceCancellationToken,
        ) -> Result<String, ChkszPlaybackError> {
            cancellation.cancel();
            Err(ChkszPlaybackError::Host {
                source: SourceHostError::Cancelled,
            })
        }
    }

    #[test]
    fn service_should_send_api_key_and_map_flac24bit_to_hires() {
        let credentials = Arc::new(FakeCredentialStore::default());
        credentials
            .save("test-api-key")
            .expect("test key should save");
        let host = Arc::new(RecordingHost::default());
        let service = ChkszPlaybackService::with_dependencies(credentials, host.clone());

        let resolved = service
            .resolve(
                "1315196858",
                SourceQuality::Flac24Bit,
                &SourceCancellationToken::default(),
            )
            .expect("playback should resolve");
        let request = host
            .requests
            .lock()
            .expect("request lock should be available")[0]
            .clone();
        let query = Url::parse(&request.url)
            .expect("request URL should parse")
            .query_pairs()
            .into_owned()
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            (resolved, query),
            (
                "https://m.example.test/song.flac".to_owned(),
                BTreeMap::from([
                    ("apikey".to_owned(), "test-api-key".to_owned()),
                    ("id".to_owned(), "1315196858".to_owned()),
                    ("level".to_owned(), "hires".to_owned()),
                    ("type".to_owned(), "json".to_owned()),
                ]),
            )
        );
    }

    #[test]
    fn service_should_reject_resolution_without_an_api_key() {
        let service = ChkszPlaybackService::with_dependencies(
            Arc::new(FakeCredentialStore::default()),
            Arc::new(RecordingHost::default()),
        );

        let error = service
            .resolve(
                "1315196858",
                SourceQuality::Flac,
                &SourceCancellationToken::default(),
            )
            .expect_err("missing key should fail");

        assert!(matches!(error, ChkszPlaybackError::MissingApiKey));
    }

    #[test]
    fn provider_should_resolve_netease_music_url_through_the_service() {
        let resolver = Arc::new(FakeResolver::default());
        let provider = ChkszPlaybackProvider::new(
            CHKSZ_PROVIDER_ID.to_owned(),
            BTreeSet::from([SourceCapability::NetworkAny]),
            resolver.clone(),
        );
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicUrl {
                    source: LX_SOURCE_WY.to_owned(),
                    music_info: json!({ "id": "1315196858" }),
                    quality: SourceQuality::Flac,
                },
            )
            .expect("musicUrl should resolve");

        assert_eq!(
            (
                outcome.response,
                resolver
                    .requests
                    .lock()
                    .expect("resolver lock should be available")
                    .clone(),
            ),
            (
                SourceResponse::MusicUrl("https://m.example.test/song.flac".to_owned()),
                vec![("1315196858".to_owned(), SourceQuality::Flac)],
            )
        );
    }

    #[test]
    fn provider_should_preserve_host_request_cancellation() {
        let provider = ChkszPlaybackProvider::new(
            CHKSZ_PROVIDER_ID.to_owned(),
            BTreeSet::from([SourceCapability::NetworkAny]),
            Arc::new(CancellingResolver),
        );
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        let error = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicUrl {
                    source: LX_SOURCE_WY.to_owned(),
                    music_info: json!({ "id": "1315196858" }),
                    quality: SourceQuality::Flac,
                },
            )
            .expect_err("cancelled host request should remain cancelled");

        assert!(matches!(error, SourceRuntimeError::Cancelled { .. }));
    }
}
