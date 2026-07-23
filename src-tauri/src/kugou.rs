use crate::source_runtime::{
    self, RemoteTrack, SourceAction, SourceCapability, SourceInfo, SourcePlaylist,
    SourcePlaylistDetail, SourceProvider, SourceQuality, SourceRecommendationsResponse,
    SourceRequest, SourceResponse, SourceRuntimeContext, SourceRuntimeError,
};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use qrcode::render::svg;
use qrcode::QrCode;
use reqwest::blocking::Client;
use reqwest::Method;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const KUGOU_PLUGIN_ID: &str = "fika.kugou";
pub const KUGOU_PROVIDER_ID: &str = "fika-kugou";
pub const KUGOU_HOST_BRIDGE_ID: &str = "kugou-music-api";
pub const KUGOU_API_BASIS_VERSION: &str = "1.5.1 (283f1e9)";

const CREDENTIAL_SERVICE: &str = "com.hvg.fika-music.kugou";
const ACCOUNT_REF_PREFIX: &str = "kugou-account:";
const GATEWAY_BASE_URL: &str = "https://gateway.kugou.com";
const LOGIN_BASE_URL: &str = "https://login-user.kugou.com";
const WEB_SIGNATURE_SALT: &str = "NVPh5oo715z5DIWAeQlhMDsWXXQV4hwt";
const ANDROID_SIGNATURE_SALT: &str = "OIlwieks28dk2k092lksi2UIkp";
const APP_ID: u32 = 1005;
const QR_APP_ID: u32 = 1001;
const SOURCE_APP_ID: u32 = 2919;
const CLIENT_VERSION: u32 = 20489;
const TRACK_URL_CLIENT_VERSION: u32 = 11430;
const TRACK_URL_PAGE_ID: u32 = 151369488;
const TRACK_URL_KEY_SALT: &str = "57ae12eb6890223e355ccfcb74edf70d";
const QR_SESSION_TTL_SECONDS: i64 = 300;
const MAX_PENDING_QR_SESSIONS: usize = 8;
const API_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_API_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const PLAYLIST_PAGE_SIZE: u64 = 200;
const MAX_PLAYLIST_TRACKS: usize = 20_000;
const MAX_PLAYLIST_PAGES: u64 = 100;
const USER_PLAYLIST_PAGE_SIZE: u64 = 100;
const MAX_USER_PLAYLIST_PAGES: u64 = 20;
const MAX_UPSTREAM_MESSAGE_CHARS: usize = 512;

type SharedConnection = Arc<Mutex<Connection>>;

// Request shapes and signatures are ported from KuGouMusicApi v1.5.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct KugouDevice {
    guid: String,
    mid: String,
    dfid: String,
}

impl KugouDevice {
    fn generate() -> Self {
        let guid = Uuid::new_v4().to_string();
        let digest = md5::compute(guid.as_bytes());
        let mid = u128::from_be_bytes(digest.0).to_string();
        Self {
            guid,
            mid,
            dfid: "-".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSession {
    token: String,
    user_id: String,
    device: KugouDevice,
}

#[derive(Debug, Clone)]
struct PendingQrSession {
    key: String,
    device: KugouDevice,
    expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct KugouAccount {
    pub account_ref: String,
    pub user_id: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub status: KugouAccountStatus,
    pub connected_at: i64,
    pub last_verified_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum KugouAccountStatus {
    Active,
    Expired,
}

impl KugouAccountStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Expired => "expired",
        }
    }

    fn parse(value: &str) -> Self {
        if value == "active" {
            Self::Active
        } else {
            Self::Expired
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct KugouQrLoginStart {
    pub session_id: String,
    pub qr_image_data_url: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum KugouQrLoginStatus {
    WaitingForScan,
    WaitingForConfirmation,
    Connected,
    Expired,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct KugouQrLoginPoll {
    pub status: KugouQrLoginStatus,
    pub account: Option<KugouAccount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KugouTrackUrl {
    pub url: String,
    pub is_preview: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KugouTrackIdentity {
    hash: String,
    album_id: u64,
    album_audio_id: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum KugouBridgeError {
    #[error("KuGou bridge is unavailable: {0}")]
    Bridge(String),
    #[error("KuGou account session expired; reconnect the account")]
    CredentialExpired,
    #[error("KuGou account was not found")]
    AccountNotFound,
    #[error("KuGou QR login session was not found or has expired")]
    QrSessionExpired,
    #[error("KuGou API rejected {operation} (code {code}): {message}")]
    Api {
        operation: &'static str,
        code: i64,
        message: String,
    },
    #[error("KuGou rate limit reached; wait before retrying")]
    RateLimited,
    #[error("KuGou Playlist id is invalid")]
    InvalidPlaylist,
    #[error("KuGou track information is invalid")]
    InvalidTrack,
    #[error("KuGou response for {operation} was invalid: {message}")]
    InvalidResponse {
        operation: &'static str,
        message: String,
    },
    #[error("KuGou persistence failed: {0}")]
    Persistence(String),
}

impl KugouBridgeError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Bridge(_) => "bridge-failure",
            Self::CredentialExpired => "credential-expired",
            Self::AccountNotFound => "account-not-found",
            Self::QrSessionExpired => "qr-session-expired",
            Self::Api { .. } => "api-failure",
            Self::RateLimited => "rate-limited",
            Self::InvalidPlaylist => "invalid-playlist",
            Self::InvalidTrack => "invalid-track",
            Self::InvalidResponse { .. } => "invalid-response",
            Self::Persistence(_) => "persistence-failure",
        }
    }
}

pub trait KugouProviderBridge: Send + Sync {
    fn music_url(
        &self,
        account_ref: &str,
        track_hash: &str,
        album_id: u64,
        album_audio_id: u64,
        quality: SourceQuality,
    ) -> Result<KugouTrackUrl, KugouBridgeError>;

    fn recommendations(
        &self,
        account_ref: &str,
        limit: u64,
    ) -> Result<SourceRecommendationsResponse, KugouBridgeError>;

    fn playlists(&self, account_ref: &str) -> Result<Vec<SourcePlaylist>, KugouBridgeError>;

    fn playlist(
        &self,
        account_ref: &str,
        playlist_id: &str,
    ) -> Result<SourcePlaylistDetail, KugouBridgeError>;
}

trait CredentialStore: Send + Sync {
    fn save(&self, account_ref: &str, secret: &str) -> Result<(), KugouBridgeError>;
    fn load(&self, account_ref: &str) -> Result<String, KugouBridgeError>;
    fn delete(&self, account_ref: &str) -> Result<(), KugouBridgeError>;
}

#[derive(Debug, Default)]
struct OsCredentialStore;

impl OsCredentialStore {
    fn entry(account_ref: &str) -> Result<keyring::Entry, KugouBridgeError> {
        keyring::Entry::new(CREDENTIAL_SERVICE, account_ref)
            .map_err(|error| KugouBridgeError::Persistence(error.to_string()))
    }
}

impl CredentialStore for OsCredentialStore {
    fn save(&self, account_ref: &str, secret: &str) -> Result<(), KugouBridgeError> {
        Self::entry(account_ref)?
            .set_password(secret)
            .map_err(|error| KugouBridgeError::Persistence(error.to_string()))
    }

    fn load(&self, account_ref: &str) -> Result<String, KugouBridgeError> {
        Self::entry(account_ref)?
            .get_password()
            .map_err(|error| match error {
                keyring::Error::NoEntry => KugouBridgeError::CredentialExpired,
                _ => KugouBridgeError::Persistence(error.to_string()),
            })
    }

    fn delete(&self, account_ref: &str) -> Result<(), KugouBridgeError> {
        match Self::entry(account_ref)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(KugouBridgeError::Persistence(error.to_string())),
        }
    }
}

trait KugouApi: Send + Sync {
    fn start_qr_login(&self, device: &KugouDevice) -> Result<JsonValue, KugouBridgeError>;
    fn poll_qr_login(&self, device: &KugouDevice, key: &str)
        -> Result<JsonValue, KugouBridgeError>;
    fn recommendations(&self, session: &StoredSession) -> Result<JsonValue, KugouBridgeError>;
    fn track_url(
        &self,
        session: &StoredSession,
        track: &KugouTrackIdentity,
        quality: SourceQuality,
        free_part: bool,
    ) -> Result<JsonValue, KugouBridgeError>;
    fn playlists(
        &self,
        session: &StoredSession,
        page: u64,
        page_size: u64,
    ) -> Result<JsonValue, KugouBridgeError>;
    fn playlist_page(
        &self,
        session: &StoredSession,
        playlist_id: &str,
        page: u64,
        page_size: u64,
    ) -> Result<JsonValue, KugouBridgeError>;
}

#[derive(Debug)]
struct KugouHttpApi {
    client: Client,
}

#[derive(Debug, Clone, Copy)]
enum SignatureKind {
    Web,
    Android,
}

impl KugouHttpApi {
    fn new() -> Result<Self, KugouBridgeError> {
        let client = Client::builder()
            .timeout(API_TIMEOUT)
            .build()
            .map_err(|error| KugouBridgeError::Bridge(error.to_string()))?;
        Ok(Self { client })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "request signing needs all upstream fields together"
    )]
    fn request_json(
        &self,
        operation: &'static str,
        base_url: &str,
        path: &str,
        method: Method,
        device: &KugouDevice,
        session: Option<&StoredSession>,
        query: BTreeMap<String, String>,
        body: Option<JsonValue>,
        signature_kind: SignatureKind,
        router: Option<&str>,
    ) -> Result<JsonValue, KugouBridgeError> {
        let mut params = default_params(device, session);
        params.extend(query);
        let body = body
            .map(|value| serde_json::to_string(&value))
            .transpose()
            .map_err(|error| KugouBridgeError::Bridge(error.to_string()))?;
        let signature = match signature_kind {
            SignatureKind::Web => web_signature(&params),
            SignatureKind::Android => android_signature(&params, body.as_deref().unwrap_or("")),
        };
        params.insert("signature".to_owned(), signature);

        let clienttime = params.get("clienttime").cloned().unwrap_or_default();
        let mut request = self
            .client
            .request(method, format!("{base_url}{path}"))
            .query(&params)
            .header(
                "User-Agent",
                "Android15-1070-11083-46-0-DiscoveryDRADProtocol-wifi",
            )
            .header("dfid", &device.dfid)
            .header("clienttime", clienttime)
            .header("mid", &device.mid)
            .header("kg-rc", "1")
            .header("kg-thash", "5d816a0")
            .header("kg-rec", "1")
            .header("kg-rf", "B9EDA08A64250DEFFBCADDEE00F8F25F");
        if let Some(router) = router {
            request = request.header("x-router", router);
        }
        if let Some(body) = body {
            request = request
                .header("Content-Type", "application/json")
                .body(body);
        }

        let response = request.send().map_err(|error| {
            let reason = if error.is_timeout() {
                "request timed out"
            } else if error.is_connect() {
                "connection failed"
            } else {
                "network request failed"
            };
            KugouBridgeError::Bridge(format!("{operation}: {reason}"))
        })?;
        read_api_response(response, operation)
    }
}

impl KugouApi for KugouHttpApi {
    fn start_qr_login(&self, device: &KugouDevice) -> Result<JsonValue, KugouBridgeError> {
        self.request_json(
            "start QR login",
            LOGIN_BASE_URL,
            "/v2/qrcode",
            Method::GET,
            device,
            None,
            BTreeMap::from([
                ("appid".to_owned(), QR_APP_ID.to_string()),
                ("type".to_owned(), "1".to_owned()),
                ("plat".to_owned(), "4".to_owned()),
                (
                    "qrcode_txt".to_owned(),
                    format!(
                        "https://h5.kugou.com/apps/loginQRCode/html/index.html?appid={APP_ID}&"
                    ),
                ),
                ("srcappid".to_owned(), SOURCE_APP_ID.to_string()),
            ]),
            None,
            SignatureKind::Web,
            None,
        )
    }

    fn poll_qr_login(
        &self,
        device: &KugouDevice,
        key: &str,
    ) -> Result<JsonValue, KugouBridgeError> {
        self.request_json(
            "poll QR login",
            LOGIN_BASE_URL,
            "/v2/get_userinfo_qrcode",
            Method::GET,
            device,
            None,
            BTreeMap::from([
                ("plat".to_owned(), "4".to_owned()),
                ("appid".to_owned(), APP_ID.to_string()),
                ("srcappid".to_owned(), SOURCE_APP_ID.to_string()),
                ("qrcode".to_owned(), key.to_owned()),
            ]),
            None,
            SignatureKind::Web,
            None,
        )
    }

    fn recommendations(&self, session: &StoredSession) -> Result<JsonValue, KugouBridgeError> {
        self.request_json(
            "fetch recommendations",
            GATEWAY_BASE_URL,
            "/everyday_song_recommend",
            Method::POST,
            &session.device,
            Some(session),
            BTreeMap::new(),
            Some(json!({
                "platform": "android",
                "userid": session.user_id,
            })),
            SignatureKind::Android,
            Some("everydayrec.service.kugou.com"),
        )
    }

    fn track_url(
        &self,
        session: &StoredSession,
        track: &KugouTrackIdentity,
        quality: SourceQuality,
        free_part: bool,
    ) -> Result<JsonValue, KugouBridgeError> {
        let hash = track.hash.to_ascii_lowercase();
        let key = track_url_key(&hash, &session.device.mid, &session.user_id);
        self.request_json(
            "resolve track URL",
            GATEWAY_BASE_URL,
            "/v5/url",
            Method::GET,
            &session.device,
            Some(session),
            BTreeMap::from([
                ("album_id".to_owned(), track.album_id.to_string()),
                ("area_code".to_owned(), "1".to_owned()),
                ("hash".to_owned(), hash),
                ("ssa_flag".to_owned(), "is_fromtrack".to_owned()),
                ("version".to_owned(), TRACK_URL_CLIENT_VERSION.to_string()),
                ("page_id".to_owned(), TRACK_URL_PAGE_ID.to_string()),
                ("quality".to_owned(), track_url_quality(quality).to_owned()),
                (
                    "album_audio_id".to_owned(),
                    track.album_audio_id.to_string(),
                ),
                ("behavior".to_owned(), "play".to_owned()),
                ("pid".to_owned(), "2".to_owned()),
                ("cmd".to_owned(), "26".to_owned()),
                ("pidversion".to_owned(), "3001".to_owned()),
                (
                    "IsFreePart".to_owned(),
                    if free_part { "1" } else { "0" }.to_owned(),
                ),
                (
                    "ppage_id".to_owned(),
                    "463467626,350369493,788954147".to_owned(),
                ),
                ("cdnBackup".to_owned(), "1".to_owned()),
                ("module".to_owned(), String::new()),
                ("clientver".to_owned(), TRACK_URL_CLIENT_VERSION.to_string()),
                ("key".to_owned(), key),
            ]),
            None,
            SignatureKind::Android,
            Some("trackercdn.kugou.com"),
        )
    }

    fn playlists(
        &self,
        session: &StoredSession,
        page: u64,
        page_size: u64,
    ) -> Result<JsonValue, KugouBridgeError> {
        self.request_json(
            "list playlists",
            GATEWAY_BASE_URL,
            "/v7/get_all_list",
            Method::POST,
            &session.device,
            Some(session),
            BTreeMap::from([
                ("plat".to_owned(), "1".to_owned()),
                ("userid".to_owned(), session.user_id.clone()),
                ("token".to_owned(), session.token.clone()),
            ]),
            Some(json!({
                "userid": session.user_id,
                "token": session.token,
                "total_ver": 979,
                "type": 2,
                "page": page,
                "pagesize": page_size,
            })),
            SignatureKind::Android,
            Some("cloudlist.service.kugou.com"),
        )
    }

    fn playlist_page(
        &self,
        session: &StoredSession,
        playlist_id: &str,
        page: u64,
        page_size: u64,
    ) -> Result<JsonValue, KugouBridgeError> {
        self.request_json(
            "read playlist",
            GATEWAY_BASE_URL,
            "/pubsongs/v2/get_other_list_file_nofilt",
            Method::GET,
            &session.device,
            Some(session),
            BTreeMap::from([
                ("area_code".to_owned(), "1".to_owned()),
                (
                    "begin_idx".to_owned(),
                    page.saturating_sub(1).saturating_mul(page_size).to_string(),
                ),
                ("plat".to_owned(), "1".to_owned()),
                ("type".to_owned(), "1".to_owned()),
                ("mode".to_owned(), "1".to_owned()),
                ("personal_switch".to_owned(), "1".to_owned()),
                (
                    "extend_fields".to_owned(),
                    "abtags,hot_cmt,popularization".to_owned(),
                ),
                ("pagesize".to_owned(), page_size.to_string()),
                ("global_collection_id".to_owned(), playlist_id.to_owned()),
            ]),
            None,
            SignatureKind::Android,
            None,
        )
    }
}

pub struct KugouServiceBridge {
    db: SharedConnection,
    credentials: Arc<dyn CredentialStore>,
    source_host: Arc<source_runtime::DefaultSourceHost>,
    api: Arc<dyn KugouApi>,
    qr_sessions: Mutex<BTreeMap<String, PendingQrSession>>,
}

impl fmt::Debug for KugouServiceBridge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pending_sessions = self
            .qr_sessions
            .lock()
            .map(|sessions| sessions.len())
            .unwrap_or_default();
        formatter
            .debug_struct("KugouServiceBridge")
            .field("api_basis_version", &KUGOU_API_BASIS_VERSION)
            .field("pending_qr_sessions", &pending_sessions)
            .finish_non_exhaustive()
    }
}

impl KugouServiceBridge {
    pub fn new(
        db: SharedConnection,
        source_host: Arc<source_runtime::DefaultSourceHost>,
    ) -> Result<Self, KugouBridgeError> {
        Self::with_dependencies(
            db,
            source_host,
            Arc::new(OsCredentialStore),
            Arc::new(KugouHttpApi::new()?),
        )
    }

    fn with_dependencies(
        db: SharedConnection,
        source_host: Arc<source_runtime::DefaultSourceHost>,
        credentials: Arc<dyn CredentialStore>,
        api: Arc<dyn KugouApi>,
    ) -> Result<Self, KugouBridgeError> {
        let bridge = Self {
            db,
            credentials,
            source_host,
            api,
            qr_sessions: Mutex::new(BTreeMap::new()),
        };
        bridge.restore_account_refs()?;
        Ok(bridge)
    }

    pub fn start_qr_login(&self) -> Result<KugouQrLoginStart, KugouBridgeError> {
        let device = KugouDevice::generate();
        let body = self.api.start_qr_login(&device)?;
        let key = json_string(body.pointer("/data/qrcode")).ok_or_else(|| {
            KugouBridgeError::InvalidResponse {
                operation: "start QR login",
                message: "response did not include a QR key".to_owned(),
            }
        })?;
        let qr_image_data_url = body
            .pointer("/data/qrcode_img")
            .and_then(JsonValue::as_str)
            .filter(|value| value.starts_with("data:image/"))
            .map(str::to_owned)
            .map_or_else(|| qr_data_url(&key), Ok)?;
        let session_id = Uuid::new_v4().to_string();
        let expires_at = now_timestamp() + QR_SESSION_TTL_SECONDS;
        let mut sessions = self
            .qr_sessions
            .lock()
            .map_err(|_| KugouBridgeError::Bridge("QR session lock was poisoned".to_owned()))?;
        sessions.retain(|_, session| session.expires_at > now_timestamp());
        if sessions.len() >= MAX_PENDING_QR_SESSIONS {
            if let Some(oldest_id) = sessions
                .iter()
                .min_by_key(|(_, session)| session.expires_at)
                .map(|(id, _)| id.clone())
            {
                sessions.remove(&oldest_id);
            }
        }
        sessions.insert(
            session_id.clone(),
            PendingQrSession {
                key,
                device,
                expires_at,
            },
        );
        Ok(KugouQrLoginStart {
            session_id,
            qr_image_data_url,
            expires_at,
        })
    }

    pub fn poll_qr_login(&self, session_id: &str) -> Result<KugouQrLoginPoll, KugouBridgeError> {
        let session = self
            .qr_sessions
            .lock()
            .map_err(|_| KugouBridgeError::Bridge("QR session lock was poisoned".to_owned()))?
            .get(session_id)
            .cloned()
            .ok_or(KugouBridgeError::QrSessionExpired)?;
        if session.expires_at <= now_timestamp() {
            self.remove_qr_session(session_id)?;
            return Ok(KugouQrLoginPoll {
                status: KugouQrLoginStatus::Expired,
                account: None,
            });
        }

        let body = self.api.poll_qr_login(&session.device, &session.key)?;
        match json_i64(body.pointer("/data/status")).unwrap_or_default() {
            0 => {
                self.remove_qr_session(session_id)?;
                Ok(KugouQrLoginPoll {
                    status: KugouQrLoginStatus::Expired,
                    account: None,
                })
            }
            1 => Ok(KugouQrLoginPoll {
                status: KugouQrLoginStatus::WaitingForScan,
                account: None,
            }),
            2 | 3 => Ok(KugouQrLoginPoll {
                status: KugouQrLoginStatus::WaitingForConfirmation,
                account: None,
            }),
            4 => {
                let data = body
                    .get("data")
                    .ok_or_else(|| KugouBridgeError::InvalidResponse {
                        operation: "poll QR login",
                        message: "connected response did not include account data".to_owned(),
                    })?;
                let token = json_string(data.get("token")).filter(|value| !value.is_empty());
                let user_id = json_string(data.get("userid")).filter(|value| value != "0");
                let (Some(token), Some(user_id)) = (token, user_id) else {
                    return Err(KugouBridgeError::InvalidResponse {
                        operation: "poll QR login",
                        message: "connected response did not include a token and user id"
                            .to_owned(),
                    });
                };
                let display_name =
                    first_json_string(data, &["nickname", "username", "user_name", "name"])
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| format!("KuGou {user_id}"));
                let avatar_url =
                    first_json_string(data, &["pic", "avatar", "user_pic", "headimgurl"])
                        .and_then(|url| normalize_image_url(&url));
                let stored = StoredSession {
                    token,
                    user_id: user_id.clone(),
                    device: session.device,
                };
                let secret = serde_json::to_string(&stored)
                    .map_err(|error| KugouBridgeError::Persistence(error.to_string()))?;
                let account = self.persist_account(user_id, display_name, avatar_url, &secret)?;
                self.remove_qr_session(session_id)?;
                Ok(KugouQrLoginPoll {
                    status: KugouQrLoginStatus::Connected,
                    account: Some(account),
                })
            }
            status => Err(KugouBridgeError::InvalidResponse {
                operation: "poll QR login",
                message: format!("unsupported QR status {status}"),
            }),
        }
    }

    pub fn cancel_qr_login(&self, session_id: &str) -> Result<(), KugouBridgeError> {
        if session_id.trim().is_empty() {
            return Err(KugouBridgeError::QrSessionExpired);
        }
        self.remove_qr_session(session_id)
    }

    pub fn accounts(&self) -> Result<Vec<KugouAccount>, KugouBridgeError> {
        let db = self
            .db
            .lock()
            .map_err(|_| KugouBridgeError::Persistence("database lock was poisoned".to_owned()))?;
        load_accounts(&db)
    }

    pub fn disconnect_account(&self, account_ref: &str) -> Result<(), KugouBridgeError> {
        validate_opaque_account_ref(account_ref)?;
        self.account(account_ref)?;
        let previous_secret = match self.credentials.load(account_ref) {
            Ok(secret) => Some(secret),
            Err(KugouBridgeError::CredentialExpired) => None,
            Err(error) => return Err(error),
        };
        self.credentials.delete(account_ref)?;
        let delete_result = self
            .db
            .lock()
            .map_err(|_| KugouBridgeError::Persistence("database lock was poisoned".to_owned()))?
            .execute(
                "DELETE FROM kugou_accounts WHERE account_ref = ?1",
                params![account_ref],
            )
            .map_err(|error| KugouBridgeError::Persistence(error.to_string()));
        if let Err(error) = delete_result {
            if let Some(secret) = previous_secret {
                let _ = self.credentials.save(account_ref, &secret);
            }
            return Err(error);
        }
        self.source_host
            .revoke_account_ref(KUGOU_PROVIDER_ID, account_ref)
            .map_err(|error| KugouBridgeError::Bridge(error.to_string()))?;
        Ok(())
    }

    fn account(&self, account_ref: &str) -> Result<KugouAccount, KugouBridgeError> {
        validate_opaque_account_ref(account_ref)?;
        let db = self
            .db
            .lock()
            .map_err(|_| KugouBridgeError::Persistence("database lock was poisoned".to_owned()))?;
        find_account(&db, account_ref)?.ok_or(KugouBridgeError::AccountNotFound)
    }

    fn session_for_account(&self, account_ref: &str) -> Result<StoredSession, KugouBridgeError> {
        validate_opaque_account_ref(account_ref)?;
        self.account(account_ref)?;
        let secret = match self.credentials.load(account_ref) {
            Ok(secret) => secret,
            Err(KugouBridgeError::CredentialExpired) => {
                self.mark_account_expired(account_ref);
                return Err(KugouBridgeError::CredentialExpired);
            }
            Err(error) => return Err(error),
        };
        let session = serde_json::from_str::<StoredSession>(&secret).map_err(|_| {
            self.mark_account_expired(account_ref);
            KugouBridgeError::CredentialExpired
        })?;
        if session.token.is_empty() || session.user_id == "0" || session.user_id.is_empty() {
            self.mark_account_expired(account_ref);
            return Err(KugouBridgeError::CredentialExpired);
        }
        Ok(session)
    }

    fn account_result<T>(
        &self,
        account_ref: &str,
        result: Result<T, KugouBridgeError>,
    ) -> Result<T, KugouBridgeError> {
        match result {
            Ok(value) => {
                if let Ok(db) = self.db.lock() {
                    let _ = db.execute(
                        "UPDATE kugou_accounts
                         SET status = 'active', last_verified_at = ?2
                         WHERE account_ref = ?1",
                        params![account_ref, now_timestamp()],
                    );
                }
                Ok(value)
            }
            Err(KugouBridgeError::CredentialExpired) => {
                self.mark_account_expired(account_ref);
                Err(KugouBridgeError::CredentialExpired)
            }
            Err(error) => Err(error),
        }
    }

    fn persist_account(
        &self,
        user_id: String,
        display_name: String,
        avatar_url: Option<String>,
        secret: &str,
    ) -> Result<KugouAccount, KugouBridgeError> {
        let now = now_timestamp();
        let existing = {
            let db = self.db.lock().map_err(|_| {
                KugouBridgeError::Persistence("database lock was poisoned".to_owned())
            })?;
            find_account_by_user_id(&db, &user_id)?
        };
        let account_ref = existing
            .as_ref()
            .map(|account| account.account_ref.clone())
            .unwrap_or_else(|| format!("{ACCOUNT_REF_PREFIX}{}", Uuid::new_v4()));
        let previous_secret = match self.credentials.load(&account_ref) {
            Ok(secret) => Some(secret),
            Err(KugouBridgeError::CredentialExpired) => None,
            Err(error) => return Err(error),
        };
        self.credentials.save(&account_ref, secret)?;
        if let Err(error) =
            self.source_host
                .register_account_ref(KUGOU_PROVIDER_ID, &account_ref, &account_ref)
        {
            restore_secret(
                self.credentials.as_ref(),
                &account_ref,
                previous_secret.as_deref(),
            );
            return Err(KugouBridgeError::Bridge(error.to_string()));
        }

        let account = KugouAccount {
            account_ref: account_ref.clone(),
            user_id,
            display_name,
            avatar_url,
            status: KugouAccountStatus::Active,
            connected_at: existing
                .as_ref()
                .map_or(now, |account| account.connected_at),
            last_verified_at: now,
        };
        let persisted = self
            .db
            .lock()
            .map_err(|_| KugouBridgeError::Persistence("database lock was poisoned".to_owned()))
            .and_then(|db| upsert_account(&db, &account));
        if let Err(error) = persisted {
            restore_secret(
                self.credentials.as_ref(),
                &account_ref,
                previous_secret.as_deref(),
            );
            if existing.is_none() {
                let _ = self
                    .source_host
                    .revoke_account_ref(KUGOU_PROVIDER_ID, &account_ref);
            }
            return Err(error);
        }
        Ok(account)
    }

    fn restore_account_refs(&self) -> Result<(), KugouBridgeError> {
        for account in self.accounts()? {
            self.source_host
                .register_account_ref(
                    KUGOU_PROVIDER_ID,
                    &account.account_ref,
                    &account.account_ref,
                )
                .map_err(|error| KugouBridgeError::Bridge(error.to_string()))?;
        }
        Ok(())
    }

    fn remove_qr_session(&self, session_id: &str) -> Result<(), KugouBridgeError> {
        self.qr_sessions
            .lock()
            .map_err(|_| KugouBridgeError::Bridge("QR session lock was poisoned".to_owned()))?
            .remove(session_id);
        Ok(())
    }

    fn mark_account_expired(&self, account_ref: &str) {
        if let Ok(db) = self.db.lock() {
            let _ = db.execute(
                "UPDATE kugou_accounts SET status = 'expired' WHERE account_ref = ?1",
                params![account_ref],
            );
        }
    }
}

impl KugouProviderBridge for KugouServiceBridge {
    fn music_url(
        &self,
        account_ref: &str,
        track_hash: &str,
        album_id: u64,
        album_audio_id: u64,
        quality: SourceQuality,
    ) -> Result<KugouTrackUrl, KugouBridgeError> {
        let track = KugouTrackIdentity {
            hash: validate_track_hash(track_hash)?,
            album_id,
            album_audio_id,
        };
        let session = self.session_for_account(account_ref)?;
        resolve_kugou_track_url(quality, |requested_quality, free_part| {
            let result = self
                .api
                .track_url(&session, &track, requested_quality, free_part);
            self.account_result(account_ref, result)
        })
    }

    fn recommendations(
        &self,
        account_ref: &str,
        limit: u64,
    ) -> Result<SourceRecommendationsResponse, KugouBridgeError> {
        let session = self.session_for_account(account_ref)?;
        let result = self.api.recommendations(&session);
        let body = self.account_result(account_ref, result)?;
        let songs = body
            .pointer("/data/song_list")
            .or_else(|| body.pointer("/data/songs"))
            .and_then(JsonValue::as_array)
            .ok_or_else(|| KugouBridgeError::InvalidResponse {
                operation: "fetch recommendations",
                message: "response did not include recommended tracks".to_owned(),
            })?;
        Ok(SourceRecommendationsResponse {
            list: songs
                .iter()
                .filter_map(remote_track_from_json)
                .take(limit.min(100) as usize)
                .collect(),
        })
    }

    fn playlists(&self, account_ref: &str) -> Result<Vec<SourcePlaylist>, KugouBridgeError> {
        let account = self.account(account_ref)?;
        let session = self.session_for_account(account_ref)?;
        collect_user_playlists(&account.display_name, |page| {
            let result = self.api.playlists(&session, page, USER_PLAYLIST_PAGE_SIZE);
            self.account_result(account_ref, result)
        })
    }

    fn playlist(
        &self,
        account_ref: &str,
        playlist_id: &str,
    ) -> Result<SourcePlaylistDetail, KugouBridgeError> {
        validate_playlist_id(playlist_id)?;
        let account = self.account(account_ref)?;
        let session = self.session_for_account(account_ref)?;
        let first = self.account_result(
            account_ref,
            self.api
                .playlist_page(&session, playlist_id, 1, PLAYLIST_PAGE_SIZE),
        )?;
        let data = first
            .get("data")
            .filter(|value| value.is_object())
            .ok_or_else(|| KugouBridgeError::InvalidResponse {
                operation: "read playlist",
                message: "response did not include Playlist data".to_owned(),
            })?;
        let metadata = data
            .get("list_info")
            .or_else(|| data.get("info"))
            .unwrap_or(data);
        let playlist =
            playlist_from_json(metadata, &account.display_name).unwrap_or_else(|| SourcePlaylist {
                id: playlist_id.to_owned(),
                name: "KuGou Playlist".to_owned(),
                description: None,
                cover_url: None,
                track_count: json_u64(data.get("count")).unwrap_or_default(),
                owner_name: account.display_name.clone(),
                can_mutate: false,
            });
        let total = json_u64(data.get("count"))
            .or_else(|| json_u64(metadata.get("count")))
            .unwrap_or(playlist.track_count);
        if usize::try_from(total).unwrap_or(usize::MAX) > MAX_PLAYLIST_TRACKS {
            return Err(KugouBridgeError::InvalidResponse {
                operation: "read playlist",
                message: format!("Playlist exceeds the {MAX_PLAYLIST_TRACKS} track limit"),
            });
        }
        let mut tracks = songs_from_playlist_page(data);
        let mut page = 2;
        while tracks.len() < total as usize && page <= MAX_PLAYLIST_PAGES {
            let body = self.account_result(
                account_ref,
                self.api
                    .playlist_page(&session, playlist_id, page, PLAYLIST_PAGE_SIZE),
            )?;
            let page_tracks = body
                .get("data")
                .map(songs_from_playlist_page)
                .unwrap_or_default();
            if page_tracks.is_empty() {
                break;
            }
            tracks.extend(page_tracks);
            page += 1;
        }
        tracks.truncate(total as usize);
        Ok(SourcePlaylistDetail { playlist, tracks })
    }
}

pub struct KugouSourceProvider {
    id: String,
    capabilities: BTreeSet<SourceCapability>,
    bridge: Arc<dyn KugouProviderBridge>,
}

impl fmt::Debug for KugouSourceProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KugouSourceProvider")
            .field("id", &self.id)
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

impl KugouSourceProvider {
    pub fn new(
        id: String,
        capabilities: BTreeSet<SourceCapability>,
        bridge: Arc<dyn KugouProviderBridge>,
    ) -> Self {
        Self {
            id,
            capabilities,
            bridge,
        }
    }

    fn prepare_bridge(
        context: &mut SourceRuntimeContext,
        operation: &str,
    ) -> Result<(), SourceRuntimeError> {
        context.require_capability(SourceCapability::BridgeKugouMusicApi, operation)?;
        context.ensure_not_cancelled(operation)
    }

    fn account_ref(
        context: &mut SourceRuntimeContext,
        requested_ref: &str,
        operation: &str,
    ) -> Result<String, SourceRuntimeError> {
        context
            .account_ref(requested_ref, operation)
            .map(|account_ref| account_ref.as_str().to_owned())
    }

    fn finish<T>(
        context: &mut SourceRuntimeContext,
        operation: &str,
        result: Result<T, KugouBridgeError>,
    ) -> Result<T, SourceRuntimeError> {
        context.ensure_not_cancelled(operation)?;
        result.map_err(|error| context.provider_error(error.to_string()))
    }
}

impl SourceProvider for KugouSourceProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn required_capabilities(&self) -> BTreeSet<SourceCapability> {
        self.capabilities.clone()
    }

    fn initialize(
        &self,
        context: &mut SourceRuntimeContext,
    ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
        context.info(format!(
            "initialized bundled KuGou Source Provider (KuGouMusicApi {KUGOU_API_BASIS_VERSION})"
        ));
        Ok(BTreeMap::from([(
            source_runtime::LX_SOURCE_KG.to_owned(),
            source_runtime::lx_music_source(
                source_runtime::LX_SOURCE_KG,
                "KuGou Music",
                vec![
                    SourceAction::MusicUrl,
                    SourceAction::MusicRecommendations,
                    SourceAction::PlaylistList,
                    SourceAction::PlaylistRead,
                ],
                source_runtime::standard_lx_qualities(),
            ),
        )]))
    }

    fn handle_request(
        &self,
        context: &mut SourceRuntimeContext,
        request: SourceRequest,
    ) -> Result<SourceResponse, SourceRuntimeError> {
        match request {
            SourceRequest::MusicUrl {
                music_info,
                quality,
                ..
            } => {
                let operation = "resolve KuGou track URL";
                Self::prepare_bridge(context, operation)?;
                let account_ref = music_info
                    .get("accountRef")
                    .and_then(JsonValue::as_str)
                    .ok_or_else(|| context.provider_error("Remote Track has no KuGou account"))?;
                let account_ref = Self::account_ref(context, account_ref, operation)?;
                let track = track_identity_from_music_info(&music_info).ok_or_else(|| {
                    context.provider_error("Remote Track has no valid KuGou hash")
                })?;
                let result = self.bridge.music_url(
                    &account_ref,
                    &track.hash,
                    track.album_id,
                    track.album_audio_id,
                    quality,
                );
                let resolved = Self::finish(context, operation, result)?;
                if resolved.is_preview {
                    context.warn(
                        "KuGou account does not include full-track access; playing the official preview",
                    );
                }
                Ok(SourceResponse::MusicUrl(resolved.url))
            }
            SourceRequest::MusicRecommendations {
                account_ref, limit, ..
            } => {
                let operation = "fetch KuGou recommendations";
                Self::prepare_bridge(context, operation)?;
                let account_ref = Self::account_ref(context, &account_ref, operation)?;
                let result = self.bridge.recommendations(&account_ref, limit);
                Self::finish(context, operation, result).map(SourceResponse::MusicRecommendations)
            }
            SourceRequest::PlaylistList { account_ref, .. } => {
                let operation = "list KuGou playlists";
                Self::prepare_bridge(context, operation)?;
                context.require_capability(SourceCapability::PlaylistRead, operation)?;
                let account_ref = Self::account_ref(context, &account_ref, operation)?;
                let result = self.bridge.playlists(&account_ref);
                Self::finish(context, operation, result).map(SourceResponse::PlaylistList)
            }
            SourceRequest::PlaylistRead {
                account_ref,
                playlist_id,
                ..
            } => {
                let operation = "read KuGou playlist";
                Self::prepare_bridge(context, operation)?;
                context.require_capability(SourceCapability::PlaylistRead, operation)?;
                let account_ref = Self::account_ref(context, &account_ref, operation)?;
                let result = self.bridge.playlist(&account_ref, &playlist_id);
                Self::finish(context, operation, result).map(SourceResponse::PlaylistRead)
            }
            request => Err(context.unsupported_action(request.source(), request.action())),
        }
    }
}

fn track_identity_from_music_info(music_info: &JsonValue) -> Option<KugouTrackIdentity> {
    let hash = first_json_string(music_info, &["hash", "id", "hash_128", "audio_hash"])
        .and_then(|hash| validate_track_hash(&hash).ok())?;
    let album_id = ["albumId", "album_id", "albumid"]
        .into_iter()
        .find_map(|key| json_u64(music_info.get(key)))
        .unwrap_or_default();
    let album_audio_id = ["mixSongId", "albumAudioId", "album_audio_id", "mixsongid"]
        .into_iter()
        .find_map(|key| json_u64(music_info.get(key)))
        .unwrap_or_default();
    Some(KugouTrackIdentity {
        hash,
        album_id,
        album_audio_id,
    })
}

fn validate_track_hash(track_hash: &str) -> Result<String, KugouBridgeError> {
    let track_hash = track_hash.trim();
    if track_hash.len() == 32 && track_hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(track_hash.to_ascii_uppercase())
    } else {
        Err(KugouBridgeError::InvalidTrack)
    }
}

fn track_url_quality(quality: SourceQuality) -> &'static str {
    match quality {
        SourceQuality::K128 => "128",
        SourceQuality::K320 => "320",
        SourceQuality::Flac => "flac",
        SourceQuality::Flac24Bit => "high",
    }
}

fn track_url_key(hash: &str, mid: &str, user_id: &str) -> String {
    format!(
        "{:x}",
        md5::compute(format!("{hash}{TRACK_URL_KEY_SALT}{APP_ID}{mid}{user_id}"))
    )
}

fn resolve_kugou_track_url<F>(
    quality: SourceQuality,
    mut fetch: F,
) -> Result<KugouTrackUrl, KugouBridgeError>
where
    F: FnMut(SourceQuality, bool) -> Result<JsonValue, KugouBridgeError>,
{
    let full = fetch(quality, false)?;
    if let Some(url) = track_url_from_json(&full) {
        return Ok(KugouTrackUrl {
            url,
            is_preview: false,
        });
    }

    let preview = fetch(SourceQuality::K128, true)?;
    track_url_from_json(&preview)
        .map(|url| KugouTrackUrl {
            url,
            is_preview: true,
        })
        .ok_or_else(|| KugouBridgeError::InvalidResponse {
            operation: "resolve track URL",
            message: "response did not include a playable URL or preview".to_owned(),
        })
}

fn track_url_from_json(body: &JsonValue) -> Option<String> {
    [
        body.get("url"),
        body.get("backupUrl"),
        body.pointer("/data/url"),
        body.pointer("/data/backupUrl"),
    ]
    .into_iter()
    .flatten()
    .find_map(playable_url_from_json)
}

fn playable_url_from_json(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(url) => normalize_track_url(url),
        JsonValue::Array(urls) => urls.iter().find_map(playable_url_from_json),
        _ => None,
    }
}

fn normalize_track_url(value: &str) -> Option<String> {
    let mut url = url::Url::parse(value.trim()).ok()?;
    match url.scheme() {
        "https" => Some(url.to_string()),
        "http" => {
            url.set_scheme("https").ok()?;
            Some(url.to_string())
        }
        _ => None,
    }
}

fn default_params(
    device: &KugouDevice,
    session: Option<&StoredSession>,
) -> BTreeMap<String, String> {
    let mut params = BTreeMap::from([
        ("dfid".to_owned(), device.dfid.clone()),
        ("mid".to_owned(), device.mid.clone()),
        ("uuid".to_owned(), "-".to_owned()),
        ("appid".to_owned(), APP_ID.to_string()),
        ("clientver".to_owned(), CLIENT_VERSION.to_string()),
        ("clienttime".to_owned(), now_timestamp().to_string()),
    ]);
    if let Some(session) = session {
        if !session.token.is_empty() {
            params.insert("token".to_owned(), session.token.clone());
        }
        if !session.user_id.is_empty() && session.user_id != "0" {
            params.insert("userid".to_owned(), session.user_id.clone());
        }
    }
    params
}

fn web_signature(params: &BTreeMap<String, String>) -> String {
    signature(params, "", WEB_SIGNATURE_SALT)
}

fn android_signature(params: &BTreeMap<String, String>, body: &str) -> String {
    signature(params, body, ANDROID_SIGNATURE_SALT)
}

fn signature(params: &BTreeMap<String, String>, body: &str, salt: &str) -> String {
    let params = params
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<String>();
    format!("{:x}", md5::compute(format!("{salt}{params}{body}{salt}")))
}

fn read_api_response(
    response: reqwest::blocking::Response,
    operation: &'static str,
) -> Result<JsonValue, KugouBridgeError> {
    let status = response.status();
    if status.as_u16() == 429 {
        return Err(KugouBridgeError::RateLimited);
    }
    let mut bytes = Vec::new();
    response
        .take((MAX_API_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| KugouBridgeError::Bridge(format!("{operation}: {error}")))?;
    if bytes.len() > MAX_API_RESPONSE_BYTES {
        return Err(KugouBridgeError::InvalidResponse {
            operation,
            message: format!("response exceeded the {MAX_API_RESPONSE_BYTES} byte limit"),
        });
    }
    let body = serde_json::from_slice::<JsonValue>(&bytes).map_err(|error| {
        KugouBridgeError::InvalidResponse {
            operation,
            message: error.to_string(),
        }
    })?;
    let error_code = json_i64(body.get("error_code")).unwrap_or_default();
    let api_status = json_i64(body.get("status"));
    if !status.is_success() || api_status == Some(0) || error_code != 0 {
        let message = upstream_message(&body);
        if status.as_u16() == 401
            || status.as_u16() == 403
            || message.to_ascii_lowercase().contains("token")
            || message.contains("登录")
        {
            return Err(KugouBridgeError::CredentialExpired);
        }
        let code = if error_code != 0 {
            error_code
        } else {
            json_i64(body.get("code")).unwrap_or_else(|| i64::from(status.as_u16()))
        };
        return Err(KugouBridgeError::Api {
            operation,
            code,
            message,
        });
    }
    Ok(body)
}

fn upstream_message(body: &JsonValue) -> String {
    ["errmsg", "error", "msg", "message"]
        .into_iter()
        .find_map(|key| json_string(body.get(key)))
        .unwrap_or_else(|| "request failed".to_owned())
        .chars()
        .take(MAX_UPSTREAM_MESSAGE_CHARS)
        .collect()
}

fn playlist_values(body: &JsonValue) -> Option<&Vec<JsonValue>> {
    [
        "/data/info",
        "/data/list",
        "/data/lists",
        "/data/playlist",
        "/info",
        "/list",
    ]
    .into_iter()
    .find_map(|pointer| body.pointer(pointer).and_then(JsonValue::as_array))
    .or_else(|| body.get("data").and_then(JsonValue::as_array))
    .or_else(|| {
        body.get("data")
            .and_then(JsonValue::as_object)
            .and_then(|data| {
                data.values().find_map(|value| {
                    value.as_array().filter(|items| {
                        items
                            .iter()
                            .any(|item| playlist_id_from_json(item).is_some())
                    })
                })
            })
    })
}

fn collect_user_playlists<F>(
    fallback_owner: &str,
    mut fetch_page: F,
) -> Result<Vec<SourcePlaylist>, KugouBridgeError>
where
    F: FnMut(u64) -> Result<JsonValue, KugouBridgeError>,
{
    let mut playlists = Vec::new();
    for page in 1..=MAX_USER_PLAYLIST_PAGES {
        let body = fetch_page(page)?;
        let values = playlist_values(&body).ok_or_else(|| KugouBridgeError::InvalidResponse {
            operation: "list playlists",
            message: "response did not include playlists".to_owned(),
        })?;
        let page_len = values.len();
        playlists.extend(
            values
                .iter()
                .filter_map(|value| playlist_from_json(value, fallback_owner)),
        );
        if page_len < USER_PLAYLIST_PAGE_SIZE as usize {
            break;
        }
    }
    Ok(playlists)
}

fn playlist_from_json(value: &JsonValue, fallback_owner: &str) -> Option<SourcePlaylist> {
    let id = playlist_id_from_json(value)?;
    let name = first_json_string(value, &["name", "listname", "specialname"])?;
    let description = first_json_string(value, &["intro", "description", "desc"])
        .filter(|description| !description.trim().is_empty());
    let cover_url = first_json_string(value, &["pic", "img", "sizable_cover", "cover"])
        .and_then(|url| normalize_image_url(&url));
    let track_count = ["count", "songcount", "track_count", "total"]
        .into_iter()
        .find_map(|key| json_u64(value.get(key)))
        .unwrap_or_default();
    let owner_name = first_json_string(
        value,
        &[
            "list_create_username",
            "username",
            "nickname",
            "author_name",
        ],
    )
    .filter(|owner| !owner.trim().is_empty())
    .unwrap_or_else(|| fallback_owner.to_owned());
    Some(SourcePlaylist {
        id,
        name,
        description,
        cover_url,
        track_count,
        owner_name,
        can_mutate: false,
    })
}

fn playlist_id_from_json(value: &JsonValue) -> Option<String> {
    [
        "global_collection_id",
        "list_create_gid",
        "parent_global_collection_id",
        "listid",
    ]
    .into_iter()
    .find_map(|key| json_string(value.get(key)).filter(|id| !id.is_empty()))
}

fn songs_from_playlist_page(data: &JsonValue) -> Vec<RemoteTrack> {
    data.get("songs")
        .or_else(|| data.get("list"))
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(remote_track_from_json)
        .collect()
}

fn remote_track_from_json(value: &JsonValue) -> Option<RemoteTrack> {
    let id = first_json_string(value, &["hash", "hash_128", "audio_hash"])?;
    let artist = first_json_string(value, &["author_name", "singername", "artist"])
        .or_else(|| singer_names(value))
        .unwrap_or_else(|| "Unknown artist".to_owned());
    let raw_title = first_json_string(value, &["songname", "audio_name", "name", "filename"])?;
    let title = raw_title
        .strip_prefix(&format!("{artist} - "))
        .unwrap_or(&raw_title)
        .to_owned();
    let album = first_json_string(value, &["album_name", "remark"])
        .or_else(|| value.pointer("/albuminfo/name").and_then(json_string_value))
        .filter(|album| !album.trim().is_empty());
    let duration_seconds = json_u64(value.get("time_length")).or_else(|| {
        json_u64(value.get("timelen"))
            .or_else(|| json_u64(value.get("duration")))
            .map(|milliseconds| milliseconds / 1_000)
    });
    let cover_url = first_json_string(value, &["sizable_cover", "cover", "img"])
        .or_else(|| {
            value
                .pointer("/trans_param/union_cover")
                .and_then(json_string_value)
        })
        .and_then(|url| normalize_image_url(&url));
    let album_id = first_json_string(value, &["album_id"])
        .or_else(|| value.pointer("/albuminfo/id").and_then(json_string_value));
    let mix_song_id = first_json_string(value, &["mixsongid", "album_audio_id"]);
    Some(RemoteTrack {
        id: id.clone(),
        source: source_runtime::LX_SOURCE_KG.to_owned(),
        title,
        artist,
        album,
        duration_seconds,
        cover_url,
        raw_info: json!({
            "id": id,
            "hash": id,
            "albumId": album_id,
            "mixSongId": mix_song_id,
        }),
    })
}

fn singer_names(value: &JsonValue) -> Option<String> {
    let names = value
        .get("singerinfo")
        .and_then(JsonValue::as_array)?
        .iter()
        .filter_map(|singer| json_string(singer.get("name")))
        .collect::<Vec<_>>();
    (!names.is_empty()).then(|| names.join(" / "))
}

fn normalize_image_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let value = value.replace("{size}", "400");
    if let Some(path) = value.strip_prefix("http://") {
        Some(format!("https://{path}"))
    } else if value.starts_with("https://") {
        Some(value)
    } else {
        None
    }
}

fn first_json_string(value: &JsonValue, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| json_string(value.get(*key)))
}

fn json_string(value: Option<&JsonValue>) -> Option<String> {
    value.and_then(json_string_value)
}

fn json_string_value(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(value) => Some(value.clone()),
        JsonValue::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn json_i64(value: Option<&JsonValue>) -> Option<i64> {
    value.and_then(|value| match value {
        JsonValue::Number(number) => number.as_i64(),
        JsonValue::String(value) => value.parse().ok(),
        _ => None,
    })
}

fn json_u64(value: Option<&JsonValue>) -> Option<u64> {
    value.and_then(|value| match value {
        JsonValue::Number(number) => number.as_u64(),
        JsonValue::String(value) => value.parse().ok(),
        _ => None,
    })
}

fn qr_data_url(key: &str) -> Result<String, KugouBridgeError> {
    let url = format!("https://h5.kugou.com/apps/loginQRCode/html/index.html?qrcode={key}");
    let code = QrCode::new(url.as_bytes()).map_err(|error| {
        KugouBridgeError::Bridge(format!("generate KuGou login QR code: {error}"))
    })?;
    let svg = code
        .render::<svg::Color>()
        .min_dimensions(320, 320)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(format!(
        "data:image/svg+xml;base64,{}",
        BASE64_STANDARD.encode(svg.as_bytes())
    ))
}

fn validate_opaque_account_ref(account_ref: &str) -> Result<(), KugouBridgeError> {
    account_ref
        .strip_prefix(ACCOUNT_REF_PREFIX)
        .and_then(|value| Uuid::parse_str(value).ok())
        .filter(|uuid| uuid.get_version_num() == 4)
        .map(|_| ())
        .ok_or(KugouBridgeError::AccountNotFound)
}

fn validate_playlist_id(playlist_id: &str) -> Result<(), KugouBridgeError> {
    let valid = !playlist_id.is_empty()
        && playlist_id.len() <= 256
        && playlist_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(KugouBridgeError::InvalidPlaylist)
    }
}

fn restore_secret(store: &dyn CredentialStore, account_ref: &str, previous: Option<&str>) {
    if let Some(previous) = previous {
        let _ = store.save(account_ref, previous);
    } else {
        let _ = store.delete(account_ref);
    }
}

fn upsert_account(connection: &Connection, account: &KugouAccount) -> Result<(), KugouBridgeError> {
    connection
        .execute(
            "INSERT INTO kugou_accounts
             (account_ref, provider_id, user_id, display_name, avatar_url, status,
              connected_at, last_verified_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(account_ref) DO UPDATE SET
               user_id = excluded.user_id,
               display_name = excluded.display_name,
               avatar_url = excluded.avatar_url,
               status = excluded.status,
               last_verified_at = excluded.last_verified_at",
            params![
                account.account_ref,
                KUGOU_PROVIDER_ID,
                account.user_id,
                account.display_name,
                account.avatar_url,
                account.status.as_str(),
                account.connected_at,
                account.last_verified_at,
            ],
        )
        .map(|_| ())
        .map_err(|error| KugouBridgeError::Persistence(error.to_string()))
}

fn find_account(
    connection: &Connection,
    account_ref: &str,
) -> Result<Option<KugouAccount>, KugouBridgeError> {
    connection
        .query_row(
            "SELECT account_ref, user_id, display_name, avatar_url, status,
                    connected_at, last_verified_at
             FROM kugou_accounts WHERE account_ref = ?1",
            params![account_ref],
            account_from_row,
        )
        .optional()
        .map_err(|error| KugouBridgeError::Persistence(error.to_string()))
}

fn find_account_by_user_id(
    connection: &Connection,
    user_id: &str,
) -> Result<Option<KugouAccount>, KugouBridgeError> {
    connection
        .query_row(
            "SELECT account_ref, user_id, display_name, avatar_url, status,
                    connected_at, last_verified_at
             FROM kugou_accounts WHERE user_id = ?1",
            params![user_id],
            account_from_row,
        )
        .optional()
        .map_err(|error| KugouBridgeError::Persistence(error.to_string()))
}

fn load_accounts(connection: &Connection) -> Result<Vec<KugouAccount>, KugouBridgeError> {
    let mut statement = connection
        .prepare(
            "SELECT account_ref, user_id, display_name, avatar_url, status,
                    connected_at, last_verified_at
             FROM kugou_accounts ORDER BY connected_at DESC, account_ref",
        )
        .map_err(|error| KugouBridgeError::Persistence(error.to_string()))?;
    let rows = statement
        .query_map([], account_from_row)
        .map_err(|error| KugouBridgeError::Persistence(error.to_string()))?
        .collect::<Result<Vec<_>, _>>();
    rows.map_err(|error| KugouBridgeError::Persistence(error.to_string()))
}

fn account_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<KugouAccount> {
    Ok(KugouAccount {
        account_ref: row.get(0)?,
        user_id: row.get(1)?,
        display_name: row.get(2)?,
        avatar_url: row.get(3)?,
        status: KugouAccountStatus::parse(&row.get::<_, String>(4)?),
        connected_at: row.get(5)?,
        last_verified_at: row.get(6)?,
    })
}

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_runtime::{DefaultSourceHost, SourceHost, SourceRuntime};

    const TEST_ACCOUNT_REF: &str = "kugou-account:00000000-0000-4000-8000-000000000001";

    #[derive(Debug, Default)]
    struct FakeProviderBridge;

    impl KugouProviderBridge for FakeProviderBridge {
        fn music_url(
            &self,
            _account_ref: &str,
            _track_hash: &str,
            _album_id: u64,
            _album_audio_id: u64,
            _quality: SourceQuality,
        ) -> Result<KugouTrackUrl, KugouBridgeError> {
            Ok(KugouTrackUrl {
                url: "https://fsandroid.kugou.com/track.mp3".to_owned(),
                is_preview: false,
            })
        }

        fn recommendations(
            &self,
            _account_ref: &str,
            _limit: u64,
        ) -> Result<SourceRecommendationsResponse, KugouBridgeError> {
            Ok(SourceRecommendationsResponse {
                list: vec![remote_track_from_json(&json!({
                    "hash": "4D766DEC7A90A011D730ED939D158131",
                    "songname": "Under My Skin",
                    "author_name": "Andrew Cui",
                    "time_length": 205
                }))
                .expect("track should normalize")],
            })
        }

        fn playlists(&self, _account_ref: &str) -> Result<Vec<SourcePlaylist>, KugouBridgeError> {
            Ok(Vec::new())
        }

        fn playlist(
            &self,
            _account_ref: &str,
            _playlist_id: &str,
        ) -> Result<SourcePlaylistDetail, KugouBridgeError> {
            Err(KugouBridgeError::InvalidPlaylist)
        }
    }

    fn provider_capabilities() -> BTreeSet<SourceCapability> {
        BTreeSet::from([
            SourceCapability::AccountRef,
            SourceCapability::PlaylistRead,
            SourceCapability::BridgeKugouMusicApi,
        ])
    }

    #[test]
    fn android_signature_should_match_upstream_fixture() {
        let params = BTreeMap::from([
            ("dfid".to_owned(), "-".to_owned()),
            ("mid".to_owned(), "123".to_owned()),
            ("uuid".to_owned(), "-".to_owned()),
            ("appid".to_owned(), "1005".to_owned()),
            ("clientver".to_owned(), "20489".to_owned()),
            ("clienttime".to_owned(), "1700000000".to_owned()),
            ("token".to_owned(), "token".to_owned()),
            ("userid".to_owned(), "42".to_owned()),
        ]);

        assert_eq!(
            android_signature(&params, r#"{"platform":"android","userid":"42"}"#),
            "de5015fcc81d0d1ef7b407ecbca736e4"
        );
    }

    #[test]
    fn track_url_key_should_match_upstream_fixture() {
        assert_eq!(
            track_url_key(
                "04de99837d367481c2cd07c107003e1d",
                "1234567890abcdef1234567890abcdef",
                "42"
            ),
            "66af81366b51b79e2eb241aa1a2f027d"
        );
    }

    #[test]
    fn web_signature_should_match_upstream_fixture() {
        let params = BTreeMap::from([
            ("dfid".to_owned(), "-".to_owned()),
            ("mid".to_owned(), "123".to_owned()),
            ("uuid".to_owned(), "-".to_owned()),
            ("appid".to_owned(), "1001".to_owned()),
            ("clientver".to_owned(), "20489".to_owned()),
            ("clienttime".to_owned(), "1700000000".to_owned()),
            ("type".to_owned(), "1".to_owned()),
            ("plat".to_owned(), "4".to_owned()),
            (
                "qrcode_txt".to_owned(),
                "https://h5.kugou.com/apps/loginQRCode/html/index.html?appid=1005&".to_owned(),
            ),
            ("srcappid".to_owned(), "2919".to_owned()),
        ]);

        assert_eq!(web_signature(&params), "b69b697d8ac16bd4fa523f6ac2f038c6");
    }

    #[test]
    fn recommendation_parser_should_normalize_kugou_song_shape() {
        let track = remote_track_from_json(&json!({
            "hash": "4D766DEC7A90A011D730ED939D158131",
            "songname": "Under My Skin",
            "author_name": "Andrew Cui",
            "album_name": "Under My Skin",
            "time_length": 205,
            "sizable_cover": "http://imge.kugou.com/stdmusic/{size}/cover.jpg"
        }))
        .expect("track should normalize");

        assert_eq!(
            (
                track.source.as_str(),
                track.duration_seconds,
                track.cover_url
            ),
            (
                source_runtime::LX_SOURCE_KG,
                Some(205),
                Some("https://imge.kugou.com/stdmusic/400/cover.jpg".to_owned())
            )
        );
    }

    #[test]
    fn track_identity_parser_should_keep_hash_and_kugou_album_ids() {
        let track = track_identity_from_music_info(&json!({
            "id": "04DE99837D367481C2CD07C107003E1D",
            "albumId": "123",
            "mixSongId": 456
        }))
        .expect("track identity should parse");

        assert_eq!(
            track,
            KugouTrackIdentity {
                hash: "04DE99837D367481C2CD07C107003E1D".to_owned(),
                album_id: 123,
                album_audio_id: 456,
            }
        );
    }

    #[test]
    fn track_url_resolver_should_retry_with_an_official_preview() {
        let mut requests = Vec::new();

        let resolved = resolve_kugou_track_url(SourceQuality::Flac, |quality, free_part| {
            requests.push((quality, free_part));
            Ok(if free_part {
                json!({
                    "status": 1,
                    "url": ["http://fsandroid.kugou.com/preview.mp3"]
                })
            } else {
                json!({ "status": 2, "url": [] })
            })
        })
        .expect("preview should resolve");

        assert_eq!(
            (resolved, requests),
            (
                KugouTrackUrl {
                    url: "https://fsandroid.kugou.com/preview.mp3".to_owned(),
                    is_preview: true,
                },
                vec![(SourceQuality::Flac, false), (SourceQuality::K128, true)]
            )
        );
    }

    #[test]
    #[ignore = "requires the live KuGou track URL service"]
    fn live_track_url_should_return_an_official_preview() {
        let api = KugouHttpApi::new().expect("HTTP client should initialize");
        let session = StoredSession {
            token: String::new(),
            user_id: "0".to_owned(),
            device: KugouDevice {
                guid: "00000000-0000-4000-8000-000000000001".to_owned(),
                mid: "1234567890abcdef1234567890abcdef".to_owned(),
                dfid: "-".to_owned(),
            },
        };
        let track = KugouTrackIdentity {
            hash: "04DE99837D367481C2CD07C107003E1D".to_owned(),
            album_id: 0,
            album_audio_id: 0,
        };

        let resolved = resolve_kugou_track_url(SourceQuality::K128, |quality, free_part| {
            api.track_url(&session, &track, quality, free_part)
        })
        .expect("live KuGou preview should resolve");

        assert!(resolved.is_preview && resolved.url.starts_with("https://"));
    }

    #[test]
    fn playlist_track_parser_should_convert_milliseconds_and_strip_artist_prefix() {
        let track = remote_track_from_json(&json!({
            "hash": "6B5DCE5832B0CC91F3CB90FECF2B5B02",
            "name": "Test Artist - Test Track",
            "timelen": 184344,
            "singerinfo": [{ "name": "Test Artist" }],
            "albuminfo": { "name": "Test Album", "id": 42 }
        }))
        .expect("track should normalize");

        assert_eq!(
            (track.title, track.duration_seconds),
            ("Test Track".to_owned(), Some(184))
        );
    }

    #[test]
    fn playlist_parser_should_use_global_collection_identity_and_be_read_only() {
        let playlist = playlist_from_json(
            &json!({
                "global_collection_id": "collection_3_1863870844_4_0",
                "name": "Daily",
                "count": 47,
                "list_create_username": "Fika"
            }),
            "Fallback",
        )
        .expect("playlist should normalize");

        assert_eq!(
            (
                playlist.id.as_str(),
                playlist.track_count,
                playlist.can_mutate
            ),
            ("collection_3_1863870844_4_0", 47, false)
        );
    }

    #[test]
    fn user_playlist_collection_should_fetch_until_a_partial_page() {
        let first_page = (0..USER_PLAYLIST_PAGE_SIZE)
            .map(|index| {
                json!({
                    "global_collection_id": format!("collection_{index}"),
                    "name": format!("Playlist {index}")
                })
            })
            .collect::<Vec<_>>();
        let mut requested_pages = Vec::new();

        let playlists = collect_user_playlists("Fika", |page| {
            requested_pages.push(page);
            Ok(if page == 1 {
                json!({ "data": { "info": first_page.clone() } })
            } else {
                json!({
                    "data": {
                        "info": [{
                            "global_collection_id": "collection_last",
                            "name": "Last Playlist"
                        }]
                    }
                })
            })
        })
        .expect("playlists should collect");

        assert_eq!((playlists.len(), requested_pages), (101, vec![1, 2]));
    }

    #[test]
    fn provider_should_expose_playback_and_read_only_kugou_actions() {
        let capabilities = provider_capabilities();
        let runtime = SourceRuntime::new();
        let provider = KugouSourceProvider::new(
            KUGOU_PROVIDER_ID.to_owned(),
            capabilities,
            Arc::new(FakeProviderBridge),
        );

        let report = runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        assert_eq!(
            report.sources[source_runtime::LX_SOURCE_KG].actions,
            vec![
                SourceAction::MusicUrl,
                SourceAction::MusicRecommendations,
                SourceAction::PlaylistList,
                SourceAction::PlaylistRead,
            ]
        );
    }

    #[test]
    fn provider_should_dispatch_recommendations_through_an_opaque_account_ref() {
        let host = Arc::new(DefaultSourceHost::new(Duration::from_secs(1), 1024));
        host.register_account_ref(KUGOU_PROVIDER_ID, "account", TEST_ACCOUNT_REF)
            .expect("account ref should register");
        let runtime_host: Arc<dyn SourceHost> = host;
        let capabilities = provider_capabilities();
        let runtime = SourceRuntime::with_host(runtime_host, capabilities.clone());
        let provider = KugouSourceProvider::new(
            KUGOU_PROVIDER_ID.to_owned(),
            capabilities,
            Arc::new(FakeProviderBridge),
        );
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicRecommendations {
                    source: source_runtime::LX_SOURCE_KG.to_owned(),
                    account_ref: "account".to_owned(),
                    limit: 10,
                },
            )
            .expect("recommendations should dispatch");

        assert!(matches!(
            outcome.response,
            SourceResponse::MusicRecommendations(SourceRecommendationsResponse { list })
                if list.len() == 1
        ));
    }

    #[test]
    fn provider_should_resolve_music_url_through_an_opaque_account_ref() {
        let host = Arc::new(DefaultSourceHost::new(Duration::from_secs(1), 1024));
        host.register_account_ref(KUGOU_PROVIDER_ID, "account", TEST_ACCOUNT_REF)
            .expect("account ref should register");
        let runtime_host: Arc<dyn SourceHost> = host;
        let capabilities = provider_capabilities();
        let runtime = SourceRuntime::with_host(runtime_host, capabilities.clone());
        let provider = KugouSourceProvider::new(
            KUGOU_PROVIDER_ID.to_owned(),
            capabilities,
            Arc::new(FakeProviderBridge),
        );
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicUrl {
                    source: source_runtime::LX_SOURCE_KG.to_owned(),
                    music_info: json!({
                        "id": "04DE99837D367481C2CD07C107003E1D",
                        "accountRef": "account"
                    }),
                    quality: SourceQuality::K320,
                },
            )
            .expect("music URL should dispatch");

        assert!(matches!(
            outcome.response,
            SourceResponse::MusicUrl(url)
                if url == "https://fsandroid.kugou.com/track.mp3"
        ));
    }

    #[test]
    fn malformed_account_refs_should_not_be_echoed() {
        let error = validate_opaque_account_ref("kugou-account:not-a-uuid")
            .expect_err("malformed account ref should fail");

        assert_eq!(error.to_string(), "KuGou account was not found");
    }
}
