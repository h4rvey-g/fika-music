use crate::source_runtime::{
    self, JsonScalar, RemoteTrack, SourceAction, SourceAlbumSearchResponse,
    SourceAlbumSearchResult, SourceArtistSearchResponse, SourceArtistSearchResult,
    SourceCapability, SourceEntityRef, SourceInfo, SourcePlaylist, SourcePlaylistDetail,
    SourcePlaylistMutation, SourcePlaylistMutationKind, SourcePlaylistSearchResponse,
    SourcePlaylistSearchResult, SourceProvider, SourceQuality, SourceRecommendationsResponse,
    SourceRequest, SourceResponse, SourceRuntimeContext, SourceRuntimeError, SourceSearchResponse,
    SourceSuggestionsResponse, SourceTrackRef,
};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use netease_music::{
    ApiResponse, LoginQrCheckParams, NeteaseMusicClient, PlaylistDetailParams, SearchParams,
    SearchSuggestParams, SongDetailParams, SongQualityLevel, SongUrlV1Params, UserPlaylistParams,
};
use qrcode::render::svg;
use qrcode::QrCode;
use rayon::prelude::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const NETEASE_PLUGIN_ID: &str = "fika.netease";
pub const NETEASE_PROVIDER_ID: &str = "fika-netease";
pub const NETEASE_HOST_BRIDGE_ID: &str = "netease-api-enhanced";
pub const NETEASE_API_BASIS_VERSION: &str = "4.32.1";

const CREDENTIAL_SERVICE: &str = "com.hvg.fika-music.netease";
const ACCOUNT_REF_PREFIX: &str = "netease-account:";
const QR_SESSION_TTL_SECONDS: i64 = 300;
const MAX_PENDING_QR_SESSIONS: usize = 8;
const API_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_API_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
// Playlist detail responses can include every track id and embedded track metadata.
const MAX_PLAYLIST_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const PLAYLIST_TRACK_BATCH_SIZE: usize = 500;
const MAX_PLAYLIST_TRACK_CONCURRENCY: usize = 4;
const MAX_AUDIT_RECORDS: u32 = 200;
const MAX_AUDIT_MESSAGE_CHARS: usize = 512;
const MAX_UPSTREAM_MESSAGE_CHARS: usize = 512;

type SharedConnection = Arc<Mutex<Connection>>;

static PLAYLIST_TRACK_POOL: OnceLock<Result<rayon::ThreadPool, String>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct NeteaseAccount {
    pub account_ref: String,
    pub user_id: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub status: NeteaseAccountStatus,
    pub connected_at: i64,
    pub last_verified_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum NeteaseAccountStatus {
    Active,
    Expired,
}

impl NeteaseAccountStatus {
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
pub struct NeteaseQrLoginStart {
    pub session_id: String,
    pub qr_image_data_url: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum NeteaseQrLoginStatus {
    WaitingForScan,
    WaitingForConfirmation,
    Connected,
    Expired,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct NeteaseQrLoginPoll {
    pub status: NeteaseQrLoginStatus,
    pub account: Option<NeteaseAccount>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct NeteaseMutationAudit {
    pub id: i64,
    pub account_ref: String,
    pub operation: SourcePlaylistMutationKind,
    pub playlist_id: String,
    pub track_id: String,
    pub outcome: String,
    pub message: Option<String>,
    pub occurred_at: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum NeteaseBridgeError {
    #[error("NetEase bridge is unavailable: {0}")]
    Bridge(String),
    #[error("NetEase account session expired; reconnect the account")]
    CredentialExpired,
    #[error("NetEase account was not found")]
    AccountNotFound,
    #[error("NetEase QR login session was not found or has expired")]
    QrSessionExpired,
    #[error("NetEase API rejected {operation} (code {code}): {message}")]
    Api {
        operation: &'static str,
        code: i64,
        message: String,
    },
    #[error("NetEase rate limit reached; wait before retrying")]
    RateLimited,
    #[error("unsupported Remote Track: {0}")]
    UnsupportedTrack(String),
    #[error("NetEase Playlist id is invalid")]
    InvalidPlaylist,
    #[error("NetEase playlist is read-only for this account")]
    ReadOnlyPlaylist,
    #[error("NetEase response for {operation} was invalid: {message}")]
    InvalidResponse {
        operation: &'static str,
        message: String,
    },
    #[error("NetEase persistence failed: {0}")]
    Persistence(String),
}

impl NeteaseBridgeError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Bridge(_) => "bridge-failure",
            Self::CredentialExpired => "credential-expired",
            Self::AccountNotFound => "account-not-found",
            Self::QrSessionExpired => "qr-session-expired",
            Self::Api { .. } => "api-failure",
            Self::RateLimited => "rate-limited",
            Self::UnsupportedTrack(_) => "unsupported-track",
            Self::InvalidPlaylist => "invalid-playlist",
            Self::ReadOnlyPlaylist => "playlist-read-only",
            Self::InvalidResponse { .. } => "invalid-response",
            Self::Persistence(_) => "persistence-failure",
        }
    }
}

pub trait NeteaseProviderBridge: Send + Sync {
    fn music_search(
        &self,
        keyword: &str,
        page: u64,
        page_size: u64,
    ) -> Result<SourceSearchResponse, NeteaseBridgeError>;

    fn artist_search(
        &self,
        keyword: &str,
        page: u64,
        page_size: u64,
    ) -> Result<SourceArtistSearchResponse, NeteaseBridgeError>;

    fn album_search(
        &self,
        keyword: &str,
        page: u64,
        page_size: u64,
    ) -> Result<SourceAlbumSearchResponse, NeteaseBridgeError>;

    fn playlist_search(
        &self,
        keyword: &str,
        page: u64,
        page_size: u64,
    ) -> Result<SourcePlaylistSearchResponse, NeteaseBridgeError>;

    fn search_suggestions(
        &self,
        keyword: &str,
        limit: u64,
    ) -> Result<SourceSuggestionsResponse, NeteaseBridgeError>;

    fn artist_top_tracks(
        &self,
        artist: &SourceEntityRef,
        limit: u64,
    ) -> Result<SourceSearchResponse, NeteaseBridgeError>;

    fn album_tracks(
        &self,
        album: &SourceEntityRef,
        page: u64,
        page_size: u64,
    ) -> Result<SourceSearchResponse, NeteaseBridgeError>;

    fn public_playlist_tracks(
        &self,
        playlist: &SourceEntityRef,
        page: u64,
        page_size: u64,
    ) -> Result<SourceSearchResponse, NeteaseBridgeError>;

    fn recommendations(
        &self,
        account_ref: &str,
        limit: u64,
    ) -> Result<SourceRecommendationsResponse, NeteaseBridgeError>;

    fn playlists(&self, account_ref: &str) -> Result<Vec<SourcePlaylist>, NeteaseBridgeError>;

    fn playlist(
        &self,
        account_ref: &str,
        playlist_id: &str,
    ) -> Result<SourcePlaylistDetail, NeteaseBridgeError>;

    fn mutate_playlist(
        &self,
        account_ref: &str,
        playlist_id: &str,
        track: &SourceTrackRef,
        operation: SourcePlaylistMutationKind,
    ) -> Result<SourcePlaylistMutation, NeteaseBridgeError>;

    fn music_url(
        &self,
        account_ref: Option<&str>,
        track_id: &str,
        quality: SourceQuality,
    ) -> Result<String, NeteaseBridgeError>;
}

trait CredentialStore: Send + Sync {
    fn save(&self, account_ref: &str, secret: &str) -> Result<(), NeteaseBridgeError>;
    fn load(&self, account_ref: &str) -> Result<String, NeteaseBridgeError>;
    fn delete(&self, account_ref: &str) -> Result<(), NeteaseBridgeError>;
}

#[derive(Debug, Default)]
struct OsCredentialStore;

impl OsCredentialStore {
    fn entry(account_ref: &str) -> Result<keyring::Entry, NeteaseBridgeError> {
        keyring::Entry::new(CREDENTIAL_SERVICE, account_ref)
            .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))
    }
}

impl CredentialStore for OsCredentialStore {
    fn save(&self, account_ref: &str, secret: &str) -> Result<(), NeteaseBridgeError> {
        Self::entry(account_ref)?
            .set_password(secret)
            .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))
    }

    fn load(&self, account_ref: &str) -> Result<String, NeteaseBridgeError> {
        Self::entry(account_ref)?
            .get_password()
            .map_err(|error| match error {
                keyring::Error::NoEntry => NeteaseBridgeError::CredentialExpired,
                _ => NeteaseBridgeError::Persistence(error.to_string()),
            })
    }

    fn delete(&self, account_ref: &str) -> Result<(), NeteaseBridgeError> {
        match Self::entry(account_ref)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(NeteaseBridgeError::Persistence(error.to_string())),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredSession {
    cookies: BTreeMap<String, String>,
}

#[derive(Clone)]
struct PendingQrSession {
    key: String,
    client: NeteaseMusicClient,
    expires_at: i64,
}

pub struct NeteaseServiceBridge {
    db: SharedConnection,
    credentials: Arc<dyn CredentialStore>,
    source_host: Arc<source_runtime::DefaultSourceHost>,
    qr_sessions: Mutex<BTreeMap<String, PendingQrSession>>,
}

impl fmt::Debug for NeteaseServiceBridge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pending_sessions = self
            .qr_sessions
            .lock()
            .map(|sessions| sessions.len())
            .unwrap_or_default();
        formatter
            .debug_struct("NeteaseServiceBridge")
            .field("api_basis_version", &NETEASE_API_BASIS_VERSION)
            .field("pending_qr_sessions", &pending_sessions)
            .finish_non_exhaustive()
    }
}

impl NeteaseServiceBridge {
    pub fn new(
        db: SharedConnection,
        source_host: Arc<source_runtime::DefaultSourceHost>,
    ) -> Result<Self, NeteaseBridgeError> {
        Self::with_credentials(db, source_host, Arc::new(OsCredentialStore))
    }

    fn with_credentials(
        db: SharedConnection,
        source_host: Arc<source_runtime::DefaultSourceHost>,
        credentials: Arc<dyn CredentialStore>,
    ) -> Result<Self, NeteaseBridgeError> {
        let bridge = Self {
            db,
            credentials,
            source_host,
            qr_sessions: Mutex::new(BTreeMap::new()),
        };
        bridge.restore_account_refs()?;
        Ok(bridge)
    }

    pub fn start_qr_login(&self) -> Result<NeteaseQrLoginStart, NeteaseBridgeError> {
        let client = new_client()?;
        let (response, qr_url) = client
            .login_qr_key()
            .map_err(|error| bridge_failure("start QR login", error))?;
        let body = checked_body(response, "start QR login")?;
        let key = body
            .pointer("/data/unikey")
            .or_else(|| body.get("unikey"))
            .and_then(JsonValue::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| NeteaseBridgeError::InvalidResponse {
                operation: "start QR login",
                message: "response did not include a QR key".to_owned(),
            })?
            .to_owned();
        let qr_image_data_url = qr_data_url(&qr_url)?;
        let session_id = Uuid::new_v4().to_string();
        let expires_at = now_timestamp() + QR_SESSION_TTL_SECONDS;
        let mut sessions = self
            .qr_sessions
            .lock()
            .map_err(|_| NeteaseBridgeError::Bridge("QR session lock was poisoned".to_owned()))?;
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
                client,
                expires_at,
            },
        );
        Ok(NeteaseQrLoginStart {
            session_id,
            qr_image_data_url,
            expires_at,
        })
    }

    pub fn poll_qr_login(
        &self,
        session_id: &str,
    ) -> Result<NeteaseQrLoginPoll, NeteaseBridgeError> {
        let session = self
            .qr_sessions
            .lock()
            .map_err(|_| NeteaseBridgeError::Bridge("QR session lock was poisoned".to_owned()))?
            .get(session_id)
            .cloned()
            .ok_or(NeteaseBridgeError::QrSessionExpired)?;

        if session.expires_at <= now_timestamp() {
            self.remove_qr_session(session_id)?;
            return Ok(NeteaseQrLoginPoll {
                status: NeteaseQrLoginStatus::Expired,
                account: None,
            });
        }

        let response = session
            .client
            .login_qr_check(LoginQrCheckParams {
                unikey: session.key,
            })
            .map_err(|error| bridge_failure("poll QR login", error))?;
        validate_response_size(&response, "poll QR login", MAX_API_RESPONSE_BYTES)?;
        let code = response
            .body
            .get("code")
            .and_then(JsonValue::as_i64)
            .or(response.code)
            .unwrap_or_default();

        match code {
            800 => {
                self.remove_qr_session(session_id)?;
                Ok(NeteaseQrLoginPoll {
                    status: NeteaseQrLoginStatus::Expired,
                    account: None,
                })
            }
            801 => Ok(NeteaseQrLoginPoll {
                status: NeteaseQrLoginStatus::WaitingForScan,
                account: None,
            }),
            802 => Ok(NeteaseQrLoginPoll {
                status: NeteaseQrLoginStatus::WaitingForConfirmation,
                account: None,
            }),
            803 => {
                let account = self.finish_qr_login(&session.client)?;
                self.remove_qr_session(session_id)?;
                Ok(NeteaseQrLoginPoll {
                    status: NeteaseQrLoginStatus::Connected,
                    account: Some(account),
                })
            }
            _ => Err(api_error("poll QR login", code, &response.body)),
        }
    }

    pub fn accounts(&self) -> Result<Vec<NeteaseAccount>, NeteaseBridgeError> {
        let db = self.db.lock().map_err(|_| {
            NeteaseBridgeError::Persistence("database lock was poisoned".to_owned())
        })?;
        load_accounts(&db)
    }

    pub fn cancel_qr_login(&self, session_id: &str) -> Result<(), NeteaseBridgeError> {
        if session_id.trim().is_empty() {
            return Err(NeteaseBridgeError::QrSessionExpired);
        }
        self.remove_qr_session(session_id)
    }

    pub fn disconnect_account(&self, account_ref: &str) -> Result<(), NeteaseBridgeError> {
        validate_opaque_account_ref(account_ref)?;
        self.account(account_ref)?;
        let previous_secret = match self.credentials.load(account_ref) {
            Ok(secret) => Some(secret),
            Err(NeteaseBridgeError::CredentialExpired) => None,
            Err(error) => return Err(error),
        };
        self.credentials.delete(account_ref)?;
        let delete_result = self
            .db
            .lock()
            .map_err(|_| NeteaseBridgeError::Persistence("database lock was poisoned".to_owned()))?
            .execute(
                "DELETE FROM netease_accounts WHERE account_ref = ?1",
                params![account_ref],
            )
            .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()));
        if let Err(error) = delete_result {
            if let Some(secret) = previous_secret {
                let _ = self.credentials.save(account_ref, &secret);
            }
            return Err(error);
        }
        self.source_host
            .revoke_account_ref(NETEASE_PROVIDER_ID, account_ref)
            .map_err(|error| NeteaseBridgeError::Bridge(error.to_string()))?;
        Ok(())
    }

    pub fn mutation_audit(
        &self,
        account_ref: Option<&str>,
        limit: u32,
    ) -> Result<Vec<NeteaseMutationAudit>, NeteaseBridgeError> {
        if let Some(account_ref) = account_ref {
            validate_opaque_account_ref(account_ref)?;
        }
        let limit = limit.clamp(1, MAX_AUDIT_RECORDS);
        let db = self.db.lock().map_err(|_| {
            NeteaseBridgeError::Persistence("database lock was poisoned".to_owned())
        })?;
        load_mutation_audit(&db, account_ref, limit)
    }

    fn finish_qr_login(
        &self,
        client: &NeteaseMusicClient,
    ) -> Result<NeteaseAccount, NeteaseBridgeError> {
        let body = checked_body(
            client
                .account()
                .map_err(|error| bridge_failure("verify QR login", error))?,
            "verify QR login",
        )?;
        let profile = body
            .get("profile")
            .filter(|value| value.is_object())
            .ok_or(NeteaseBridgeError::CredentialExpired)?;
        let user_id =
            json_id(profile.get("userId")).ok_or_else(|| NeteaseBridgeError::InvalidResponse {
                operation: "verify QR login",
                message: "account profile did not include a user id".to_owned(),
            })?;
        let display_name = profile
            .get("nickname")
            .and_then(JsonValue::as_str)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("NetEase account")
            .to_owned();
        let avatar_url = json_string(profile.get("avatarUrl"));
        let session = StoredSession {
            cookies: client
                .cookies()
                .into_iter()
                .map(|cookie| (cookie.name, cookie.value))
                .collect(),
        };
        if !session.cookies.contains_key("MUSIC_U") {
            return Err(NeteaseBridgeError::CredentialExpired);
        }
        let secret = serde_json::to_string(&session)
            .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))?;
        self.persist_account(user_id, display_name, avatar_url, &secret)
    }

    fn persist_account(
        &self,
        user_id: String,
        display_name: String,
        avatar_url: Option<String>,
        secret: &str,
    ) -> Result<NeteaseAccount, NeteaseBridgeError> {
        let now = now_timestamp();
        let existing = {
            let db = self.db.lock().map_err(|_| {
                NeteaseBridgeError::Persistence("database lock was poisoned".to_owned())
            })?;
            find_account_by_user_id(&db, &user_id)?
        };
        let account_ref = existing
            .as_ref()
            .map(|account| account.account_ref.clone())
            .unwrap_or_else(|| format!("{ACCOUNT_REF_PREFIX}{}", Uuid::new_v4()));
        let previous_secret = match self.credentials.load(&account_ref) {
            Ok(secret) => Some(secret),
            Err(NeteaseBridgeError::CredentialExpired) => None,
            Err(error) => return Err(error),
        };
        self.credentials.save(&account_ref, secret)?;
        if let Err(error) =
            self.source_host
                .register_account_ref(NETEASE_PROVIDER_ID, &account_ref, &account_ref)
        {
            if let Some(previous_secret) = previous_secret.as_deref() {
                let _ = self.credentials.save(&account_ref, previous_secret);
            } else {
                let _ = self.credentials.delete(&account_ref);
            }
            return Err(NeteaseBridgeError::Bridge(error.to_string()));
        }

        let account = NeteaseAccount {
            account_ref: account_ref.clone(),
            user_id,
            display_name,
            avatar_url,
            status: NeteaseAccountStatus::Active,
            connected_at: existing
                .as_ref()
                .map_or(now, |account| account.connected_at),
            last_verified_at: now,
        };
        let persisted = self
            .db
            .lock()
            .map_err(|_| NeteaseBridgeError::Persistence("database lock was poisoned".to_owned()))
            .and_then(|db| upsert_account(&db, &account));
        if let Err(error) = persisted {
            if let Some(previous_secret) = previous_secret {
                let _ = self.credentials.save(&account_ref, &previous_secret);
            } else {
                let _ = self.credentials.delete(&account_ref);
            }
            if existing.is_none() {
                let _ = self
                    .source_host
                    .revoke_account_ref(NETEASE_PROVIDER_ID, &account_ref);
            }
            return Err(error);
        }
        Ok(account)
    }

    fn restore_account_refs(&self) -> Result<(), NeteaseBridgeError> {
        for account in self.accounts()? {
            self.source_host
                .register_account_ref(
                    NETEASE_PROVIDER_ID,
                    &account.account_ref,
                    &account.account_ref,
                )
                .map_err(|error| NeteaseBridgeError::Bridge(error.to_string()))?;
        }
        Ok(())
    }

    fn remove_qr_session(&self, session_id: &str) -> Result<(), NeteaseBridgeError> {
        self.qr_sessions
            .lock()
            .map_err(|_| NeteaseBridgeError::Bridge("QR session lock was poisoned".to_owned()))?
            .remove(session_id);
        Ok(())
    }

    fn client_for_account(
        &self,
        account_ref: &str,
    ) -> Result<NeteaseMusicClient, NeteaseBridgeError> {
        validate_opaque_account_ref(account_ref)?;
        let account_exists = {
            let db = self.db.lock().map_err(|_| {
                NeteaseBridgeError::Persistence("database lock was poisoned".to_owned())
            })?;
            find_account(&db, account_ref)?.is_some()
        };
        if !account_exists {
            return Err(NeteaseBridgeError::AccountNotFound);
        }
        let secret = match self.credentials.load(account_ref) {
            Ok(secret) => secret,
            Err(NeteaseBridgeError::CredentialExpired) => {
                self.mark_account_expired(account_ref);
                return Err(NeteaseBridgeError::CredentialExpired);
            }
            Err(error) => return Err(error),
        };
        let session = match serde_json::from_str::<StoredSession>(&secret) {
            Ok(session) => session,
            Err(_) => {
                self.mark_account_expired(account_ref);
                return Err(NeteaseBridgeError::CredentialExpired);
            }
        };
        let mut builder = NeteaseMusicClient::builder().timeout(API_TIMEOUT);
        for (name, value) in session.cookies {
            builder = builder.cookie(name, value);
        }
        builder
            .build()
            .map_err(|error| bridge_failure("create authenticated client", error))
    }

    fn mark_account_expired(&self, account_ref: &str) {
        if let Ok(db) = self.db.lock() {
            let _ = db.execute(
                "UPDATE netease_accounts SET status = 'expired' WHERE account_ref = ?1",
                params![account_ref],
            );
        }
    }

    fn record_mutation(
        &self,
        account_ref: &str,
        playlist_id: &str,
        track_id: &str,
        operation: SourcePlaylistMutationKind,
        outcome: &str,
        message: Option<&str>,
    ) -> Result<NeteaseMutationAudit, NeteaseBridgeError> {
        let occurred_at = now_timestamp();
        let message = message.map(|message| {
            message
                .chars()
                .take(MAX_AUDIT_MESSAGE_CHARS)
                .collect::<String>()
        });
        let db = self.db.lock().map_err(|_| {
            NeteaseBridgeError::Persistence("database lock was poisoned".to_owned())
        })?;
        db.execute(
            "INSERT INTO netease_mutation_audit
             (account_ref, operation, playlist_id, track_id, outcome, message, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                account_ref,
                mutation_kind_str(operation),
                playlist_id,
                track_id,
                outcome,
                message.as_deref(),
                occurred_at
            ],
        )
        .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))?;
        let id = db.last_insert_rowid();
        let _ = db.execute(
            "DELETE FROM netease_mutation_audit
             WHERE account_ref = ?1 AND id NOT IN (
                SELECT id FROM netease_mutation_audit
                WHERE account_ref = ?1
                ORDER BY occurred_at DESC, id DESC
                LIMIT ?2
             )",
            params![account_ref, MAX_AUDIT_RECORDS],
        );
        Ok(NeteaseMutationAudit {
            id,
            account_ref: account_ref.to_owned(),
            operation,
            playlist_id: playlist_id.to_owned(),
            track_id: track_id.to_owned(),
            outcome: outcome.to_owned(),
            message,
            occurred_at,
        })
    }
}

impl NeteaseProviderBridge for NeteaseServiceBridge {
    fn music_search(
        &self,
        keyword: &str,
        page: u64,
        page_size: u64,
    ) -> Result<SourceSearchResponse, NeteaseBridgeError> {
        let body = netease_search(keyword, "1", page, page_size)?;
        let result = body.get("result").unwrap_or(&body);
        let list = result
            .get("songs")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(remote_track_from_json)
            .collect::<Vec<_>>();
        Ok(SourceSearchResponse {
            is_end: search_is_end(result, page, page_size, list.len(), "songCount"),
            total: result.get("songCount").and_then(JsonValue::as_u64),
            list,
        })
    }

    fn artist_search(
        &self,
        keyword: &str,
        page: u64,
        page_size: u64,
    ) -> Result<SourceArtistSearchResponse, NeteaseBridgeError> {
        let body = netease_search(keyword, "100", page, page_size)?;
        let result = body.get("result").unwrap_or(&body);
        let list = result
            .get("artists")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(netease_artist_from_json)
            .collect::<Vec<_>>();
        Ok(SourceArtistSearchResponse {
            is_end: search_is_end(result, page, page_size, list.len(), "artistCount"),
            total: result.get("artistCount").and_then(JsonValue::as_u64),
            list,
        })
    }

    fn album_search(
        &self,
        keyword: &str,
        page: u64,
        page_size: u64,
    ) -> Result<SourceAlbumSearchResponse, NeteaseBridgeError> {
        let body = netease_search(keyword, "10", page, page_size)?;
        let result = body.get("result").unwrap_or(&body);
        let list = result
            .get("albums")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(netease_album_from_json)
            .collect::<Vec<_>>();
        Ok(SourceAlbumSearchResponse {
            is_end: search_is_end(result, page, page_size, list.len(), "albumCount"),
            total: result.get("albumCount").and_then(JsonValue::as_u64),
            list,
        })
    }

    fn playlist_search(
        &self,
        keyword: &str,
        page: u64,
        page_size: u64,
    ) -> Result<SourcePlaylistSearchResponse, NeteaseBridgeError> {
        let body = netease_search(keyword, "1000", page, page_size)?;
        let result = body.get("result").unwrap_or(&body);
        let list = result
            .get("playlists")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(netease_playlist_search_from_json)
            .collect::<Vec<_>>();
        Ok(SourcePlaylistSearchResponse {
            is_end: search_is_end(result, page, page_size, list.len(), "playlistCount"),
            total: result.get("playlistCount").and_then(JsonValue::as_u64),
            list,
        })
    }

    fn search_suggestions(
        &self,
        keyword: &str,
        limit: u64,
    ) -> Result<SourceSuggestionsResponse, NeteaseBridgeError> {
        let body = new_client()?
            .search_suggest(SearchSuggestParams {
                keywords: keyword.to_owned(),
                suggest_type: Some("mobile".to_owned()),
            })
            .map_err(|error| bridge_failure("fetch search suggestions", error))
            .and_then(|response| checked_body(response, "fetch search suggestions"))?;
        let list = body
            .get("result")
            .and_then(|result| result.get("allMatch").or_else(|| result.get("suggestions")))
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                json_string(item.get("keyword"))
                    .or_else(|| json_string(item.get("name")))
                    .or_else(|| item.as_str().map(str::to_owned))
            })
            .filter(|suggestion| !suggestion.trim().is_empty())
            .take(limit as usize)
            .collect();
        Ok(SourceSuggestionsResponse { list })
    }

    fn artist_top_tracks(
        &self,
        artist: &SourceEntityRef,
        limit: u64,
    ) -> Result<SourceSearchResponse, NeteaseBridgeError> {
        validate_netease_track_id(&artist.id)?;
        let body = new_client()?
            .raw_weapi(
                "https://music.163.com/api/artist/top/song",
                json!({ "id": artist.id }),
            )
            .map_err(|error| bridge_failure("read artist top tracks", error))
            .and_then(|response| checked_body(response, "read artist top tracks"))?;
        let list = body
            .get("songs")
            .or_else(|| body.pointer("/data/songs"))
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(remote_track_from_json)
            .take(limit as usize)
            .collect();
        Ok(SourceSearchResponse {
            is_end: true,
            total: None,
            list,
        })
    }

    fn album_tracks(
        &self,
        album: &SourceEntityRef,
        page: u64,
        page_size: u64,
    ) -> Result<SourceSearchResponse, NeteaseBridgeError> {
        validate_netease_playlist_id(&album.id)?;
        let body = new_client()?
            .raw_weapi(
                &format!("https://music.163.com/api/v1/album/{}", album.id),
                json!({}),
            )
            .map_err(|error| bridge_failure("read album", error))
            .and_then(|response| checked_playlist_body(response))?;
        paginate_tracks(
            body.get("songs").and_then(JsonValue::as_array),
            page,
            page_size,
        )
    }

    fn public_playlist_tracks(
        &self,
        playlist: &SourceEntityRef,
        page: u64,
        page_size: u64,
    ) -> Result<SourceSearchResponse, NeteaseBridgeError> {
        validate_netease_playlist_id(&playlist.id)?;
        let client = new_client()?;
        let body = checked_playlist_body(
            client
                .playlist_detail(PlaylistDetailParams {
                    id: playlist.id.clone(),
                    s: Some(8),
                })
                .map_err(|error| bridge_failure("read public playlist", error))?,
        )?;
        let playlist_json = body.get("playlist").unwrap_or(&body);
        let ids = playlist_track_ids(playlist_json);
        let total = ids.len();
        let start = usize::try_from(page.saturating_sub(1).saturating_mul(page_size))
            .unwrap_or(usize::MAX)
            .min(total);
        let end = start.saturating_add(page_size as usize).min(total);
        let list = if start == end {
            Vec::new()
        } else {
            fetch_playlist_tracks(&client, &ids[start..end])?
        };
        Ok(SourceSearchResponse {
            is_end: end >= total,
            total: Some(total as u64),
            list,
        })
    }

    fn recommendations(
        &self,
        account_ref: &str,
        limit: u64,
    ) -> Result<SourceRecommendationsResponse, NeteaseBridgeError> {
        let client = self.client_for_account(account_ref)?;
        let result = client
            .recommend_songs()
            .map_err(|error| bridge_failure("fetch recommendations", error))
            .and_then(|response| checked_body(response, "fetch recommendations"));
        let body = self.account_result(account_ref, result)?;
        let songs = body
            .pointer("/data/dailySongs")
            .or_else(|| body.get("recommend"))
            .and_then(JsonValue::as_array)
            .ok_or_else(|| NeteaseBridgeError::InvalidResponse {
                operation: "fetch recommendations",
                message: "response did not include recommended tracks".to_owned(),
            })?;
        let list = songs
            .iter()
            .filter_map(remote_track_from_json)
            .take(limit.min(100) as usize)
            .collect();
        Ok(SourceRecommendationsResponse { list })
    }

    fn playlists(&self, account_ref: &str) -> Result<Vec<SourcePlaylist>, NeteaseBridgeError> {
        let account = self.account(account_ref)?;
        let client = self.client_for_account(account_ref)?;
        let result = client
            .user_playlist(UserPlaylistParams {
                uid: account.user_id.clone(),
                limit: Some(100),
                offset: Some(0),
            })
            .map_err(|error| bridge_failure("list playlists", error))
            .and_then(|response| checked_body(response, "list playlists"));
        let body = self.account_result(account_ref, result)?;
        let playlists = body
            .get("playlist")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| NeteaseBridgeError::InvalidResponse {
                operation: "list playlists",
                message: "response did not include playlists".to_owned(),
            })?;
        Ok(playlists
            .iter()
            .filter_map(|playlist| playlist_from_json(playlist, &account.user_id))
            .collect())
    }

    fn playlist(
        &self,
        account_ref: &str,
        playlist_id: &str,
    ) -> Result<SourcePlaylistDetail, NeteaseBridgeError> {
        validate_netease_playlist_id(playlist_id)?;
        let account = self.account(account_ref)?;
        let client = self.client_for_account(account_ref)?;
        let body = self.playlist_body(account_ref, playlist_id, &client)?;
        let playlist_json =
            body.get("playlist")
                .ok_or_else(|| NeteaseBridgeError::InvalidResponse {
                    operation: "read playlist",
                    message: "response did not include playlist details".to_owned(),
                })?;
        let playlist = playlist_from_json(playlist_json, &account.user_id).ok_or_else(|| {
            NeteaseBridgeError::InvalidResponse {
                operation: "read playlist",
                message: "playlist metadata was incomplete".to_owned(),
            }
        })?;
        let track_ids = playlist_track_ids(playlist_json);
        let embedded_tracks = complete_embedded_playlist_tracks(playlist_json, &track_ids);
        drop(body);
        let tracks = match embedded_tracks {
            Some(tracks) => tracks,
            None => {
                let result = fetch_playlist_tracks(&client, &track_ids);
                self.account_result(account_ref, result)?
            }
        };
        Ok(SourcePlaylistDetail { playlist, tracks })
    }

    fn mutate_playlist(
        &self,
        account_ref: &str,
        playlist_id: &str,
        track: &SourceTrackRef,
        operation: SourcePlaylistMutationKind,
    ) -> Result<SourcePlaylistMutation, NeteaseBridgeError> {
        let result = self.mutate_playlist_inner(account_ref, playlist_id, track, operation);
        match result {
            Ok(()) => {
                let audit = self.record_mutation(
                    account_ref,
                    playlist_id,
                    &track.id,
                    operation,
                    "succeeded",
                    None,
                )?;
                Ok(SourcePlaylistMutation {
                    audit_id: audit.id,
                    operation,
                    playlist_id: playlist_id.to_owned(),
                    track_id: track.id.clone(),
                    occurred_at: audit.occurred_at,
                })
            }
            Err(error) => {
                let message = error.to_string();
                let _ = self.record_mutation(
                    account_ref,
                    playlist_id,
                    &track.id,
                    operation,
                    "failed",
                    Some(&message),
                );
                Err(error)
            }
        }
    }

    fn music_url(
        &self,
        account_ref: Option<&str>,
        track_id: &str,
        quality: SourceQuality,
    ) -> Result<String, NeteaseBridgeError> {
        validate_netease_track_id(track_id)?;
        let client = match account_ref {
            Some(account_ref) => self.client_for_account(account_ref)?,
            None => new_client()?,
        };
        let result = client
            .song_url_v1(SongUrlV1Params {
                id: track_id.to_owned(),
                level: Some(match quality {
                    SourceQuality::K128 => SongQualityLevel::Standard,
                    SourceQuality::K320 => SongQualityLevel::Exhigh,
                    SourceQuality::Flac => SongQualityLevel::Lossless,
                    SourceQuality::Flac24Bit => SongQualityLevel::Hires,
                }),
                encode_type: Some("flac".to_owned()),
            })
            .map_err(|error| bridge_failure("resolve track URL", error))
            .and_then(|response| checked_body(response, "resolve track URL"));
        let body = match account_ref {
            Some(account_ref) => self.account_result(account_ref, result)?,
            None => result?,
        };
        body.get("data")
            .and_then(JsonValue::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("url"))
            .and_then(JsonValue::as_str)
            .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            .map(str::to_owned)
            .ok_or_else(|| {
                NeteaseBridgeError::UnsupportedTrack(
                    "NetEase did not return a playable URL; the track may be unavailable for this account or region"
                        .to_owned(),
                )
            })
    }
}

impl NeteaseServiceBridge {
    fn account(&self, account_ref: &str) -> Result<NeteaseAccount, NeteaseBridgeError> {
        validate_opaque_account_ref(account_ref)?;
        let db = self.db.lock().map_err(|_| {
            NeteaseBridgeError::Persistence("database lock was poisoned".to_owned())
        })?;
        find_account(&db, account_ref)?.ok_or(NeteaseBridgeError::AccountNotFound)
    }

    fn account_result<T>(
        &self,
        account_ref: &str,
        result: Result<T, NeteaseBridgeError>,
    ) -> Result<T, NeteaseBridgeError> {
        match result {
            Ok(value) => {
                if let Ok(db) = self.db.lock() {
                    let _ = db.execute(
                        "UPDATE netease_accounts
                         SET status = 'active', last_verified_at = ?2
                         WHERE account_ref = ?1",
                        params![account_ref, now_timestamp()],
                    );
                }
                Ok(value)
            }
            Err(NeteaseBridgeError::CredentialExpired) => {
                self.mark_account_expired(account_ref);
                Err(NeteaseBridgeError::CredentialExpired)
            }
            Err(error) => Err(error),
        }
    }

    fn playlist_body(
        &self,
        account_ref: &str,
        playlist_id: &str,
        client: &NeteaseMusicClient,
    ) -> Result<JsonValue, NeteaseBridgeError> {
        let result = client
            .playlist_detail(PlaylistDetailParams {
                id: playlist_id.to_owned(),
                s: Some(8),
            })
            .map_err(|error| bridge_failure("read playlist", error))
            .and_then(checked_playlist_body);
        self.account_result(account_ref, result)
    }

    fn mutate_playlist_inner(
        &self,
        account_ref: &str,
        playlist_id: &str,
        track: &SourceTrackRef,
        operation: SourcePlaylistMutationKind,
    ) -> Result<(), NeteaseBridgeError> {
        if track.source != source_runtime::LX_SOURCE_WY {
            return Err(NeteaseBridgeError::UnsupportedTrack(
                "Only NetEase Remote Tracks can be added to a NetEase Playlist".to_owned(),
            ));
        }
        validate_netease_track_id(&track.id)?;
        validate_netease_playlist_id(playlist_id)?;
        let account = self.account(account_ref)?;
        let client = self.client_for_account(account_ref)?;
        let body = self.playlist_body(account_ref, playlist_id, &client)?;
        let playlist = body
            .get("playlist")
            .and_then(|value| playlist_from_json(value, &account.user_id))
            .ok_or_else(|| NeteaseBridgeError::InvalidResponse {
                operation: "read playlist",
                message: "playlist metadata was incomplete".to_owned(),
            })?;
        if !playlist.can_mutate {
            return Err(NeteaseBridgeError::ReadOnlyPlaylist);
        }
        drop(body);
        let op = mutation_kind_str(operation);
        let tracks = vec![track.id.clone()];
        let request = || {
            client
                .raw_weapi(
                    "https://music.163.com/api/playlist/manipulate/tracks",
                    json!({
                        "op": op,
                        "pid": playlist_id,
                        "trackIds": serde_json::to_string(&tracks).unwrap_or_default(),
                        "imme": "true",
                    }),
                )
                .map_err(|error| bridge_failure("mutate playlist", error))
        };
        let mut response = request()?;
        if response_code(&response) == 512 {
            let doubled = vec![track.id.clone(), track.id.clone()];
            response = client
                .raw_weapi(
                    "https://music.163.com/api/playlist/manipulate/tracks",
                    json!({
                        "op": op,
                        "pid": playlist_id,
                        "trackIds": serde_json::to_string(&doubled).unwrap_or_default(),
                        "imme": "true",
                    }),
                )
                .map_err(|error| bridge_failure("mutate playlist", error))?;
        }
        self.account_result(account_ref, checked_body(response, "mutate playlist"))?;
        Ok(())
    }
}

pub struct NeteaseSourceProvider {
    id: String,
    capabilities: BTreeSet<SourceCapability>,
    bridge: Arc<dyn NeteaseProviderBridge>,
}

impl fmt::Debug for NeteaseSourceProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NeteaseSourceProvider")
            .field("id", &self.id)
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

impl NeteaseSourceProvider {
    pub fn new(
        id: String,
        capabilities: BTreeSet<SourceCapability>,
        bridge: Arc<dyn NeteaseProviderBridge>,
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
        context.require_capability(SourceCapability::BridgeNeteaseApiEnhanced, operation)?;
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
        result: Result<T, NeteaseBridgeError>,
    ) -> Result<T, SourceRuntimeError> {
        context.ensure_not_cancelled(operation)?;
        result.map_err(|error| context.provider_error_with_code(error.code(), error.to_string()))
    }
}

fn netease_search(
    keyword: &str,
    search_type: &str,
    page: u64,
    page_size: u64,
) -> Result<JsonValue, NeteaseBridgeError> {
    let limit = u32::try_from(page_size).unwrap_or(100);
    let offset =
        u32::try_from(page.saturating_sub(1).saturating_mul(page_size)).unwrap_or(u32::MAX);
    new_client()?
        .search(SearchParams {
            keywords: keyword.to_owned(),
            search_type: Some(search_type.to_owned()),
            limit: Some(limit),
            offset: Some(offset),
        })
        .map_err(|error| bridge_failure("search NetEase", error))
        .and_then(|response| checked_body(response, "search NetEase"))
}

fn search_is_end(
    result: &JsonValue,
    page: u64,
    page_size: u64,
    returned: usize,
    count_key: &str,
) -> bool {
    result
        .get(count_key)
        .and_then(JsonValue::as_u64)
        .is_some_and(|total| page.saturating_mul(page_size) >= total)
        || returned < page_size as usize
}

fn platform_id(id: &str) -> BTreeMap<String, JsonScalar> {
    BTreeMap::from([("id".to_owned(), JsonScalar::String(id.to_owned()))])
}

fn netease_artist_from_json(value: &JsonValue) -> Option<SourceArtistSearchResult> {
    let id = json_id(value.get("id"))?;
    Some(SourceArtistSearchResult {
        id: id.clone(),
        source: source_runtime::LX_SOURCE_WY.to_owned(),
        name: json_string(value.get("name"))?,
        cover_url: json_string(value.get("picUrl")).or_else(|| json_string(value.get("img1v1Url"))),
        platform_ids: platform_id(&id),
        raw_info: json!({ "id": id }),
    })
}

fn netease_album_from_json(value: &JsonValue) -> Option<SourceAlbumSearchResult> {
    let id = json_id(value.get("id"))?;
    let artists = value
        .get("artists")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artist| json_string(artist.get("name")))
        .collect::<Vec<_>>();
    let publish_time = value.get("publishTime").and_then(JsonValue::as_u64);
    Some(SourceAlbumSearchResult {
        id: id.clone(),
        source: source_runtime::LX_SOURCE_WY.to_owned(),
        title: json_string(value.get("name"))?,
        artist: if artists.is_empty() {
            value
                .get("artist")
                .and_then(|artist| json_string(artist.get("name")))
                .unwrap_or_else(|| "Unknown artist".to_owned())
        } else {
            artists.join(" / ")
        },
        release_year: publish_time
            .and_then(|millis| UNIX_EPOCH.checked_add(Duration::from_millis(millis)))
            .and_then(system_time_year),
        cover_url: json_string(value.get("picUrl")),
        track_count: value.get("size").and_then(JsonValue::as_u64),
        platform_ids: platform_id(&id),
        raw_info: json!({ "id": id }),
    })
}

fn system_time_year(time: SystemTime) -> Option<u32> {
    let seconds = time.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let days = seconds / 86_400;
    let mut year = 1970_u32;
    let mut remaining = days;
    loop {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if remaining < days_in_year {
            return Some(year);
        }
        remaining -= days_in_year;
        year = year.saturating_add(1);
    }
}

fn netease_playlist_search_from_json(value: &JsonValue) -> Option<SourcePlaylistSearchResult> {
    let id = json_id(value.get("id"))?;
    Some(SourcePlaylistSearchResult {
        id: id.clone(),
        source: source_runtime::LX_SOURCE_WY.to_owned(),
        name: json_string(value.get("name"))?,
        description: json_string(value.get("description")),
        cover_url: json_string(value.get("coverImgUrl")),
        track_count: value.get("trackCount").and_then(JsonValue::as_u64),
        owner_name: value
            .get("creator")
            .and_then(|creator| json_string(creator.get("nickname"))),
        platform_ids: platform_id(&id),
        raw_info: json!({ "id": id }),
    })
}

fn paginate_tracks(
    songs: Option<&Vec<JsonValue>>,
    page: u64,
    page_size: u64,
) -> Result<SourceSearchResponse, NeteaseBridgeError> {
    let songs = songs.ok_or_else(|| NeteaseBridgeError::InvalidResponse {
        operation: "read collection",
        message: "response did not include tracks".to_owned(),
    })?;
    let total = songs.len();
    let start = usize::try_from(page.saturating_sub(1).saturating_mul(page_size))
        .unwrap_or(usize::MAX)
        .min(total);
    let end = start.saturating_add(page_size as usize).min(total);
    Ok(SourceSearchResponse {
        is_end: end >= total,
        total: Some(total as u64),
        list: songs[start..end]
            .iter()
            .filter_map(remote_track_from_json)
            .collect(),
    })
}

impl SourceProvider for NeteaseSourceProvider {
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
            "initialized bundled NetEase Source Provider (api-enhanced {NETEASE_API_BASIS_VERSION})"
        ));
        Ok(BTreeMap::from([(
            source_runtime::LX_SOURCE_WY.to_owned(),
            source_runtime::lx_music_source(
                source_runtime::LX_SOURCE_WY,
                "NetEase Cloud Music",
                vec![
                    SourceAction::MusicSearch,
                    SourceAction::ArtistSearch,
                    SourceAction::AlbumSearch,
                    SourceAction::PlaylistSearch,
                    SourceAction::SearchSuggestions,
                    SourceAction::ArtistTopTracks,
                    SourceAction::AlbumRead,
                    SourceAction::PlaylistReadPublic,
                    SourceAction::MusicUrl,
                    SourceAction::MusicRecommendations,
                    SourceAction::PlaylistList,
                    SourceAction::PlaylistRead,
                    SourceAction::PlaylistAddTrack,
                    SourceAction::PlaylistRemoveTrack,
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
            SourceRequest::MusicSearch {
                keyword,
                page,
                page_size,
                ..
            } => {
                let operation = "search NetEase tracks";
                Self::prepare_bridge(context, operation)?;
                Self::finish(
                    context,
                    operation,
                    self.bridge.music_search(&keyword, page, page_size),
                )
                .map(SourceResponse::MusicSearch)
            }
            SourceRequest::ArtistSearch {
                keyword,
                page,
                page_size,
                ..
            } => {
                let operation = "search NetEase artists";
                Self::prepare_bridge(context, operation)?;
                Self::finish(
                    context,
                    operation,
                    self.bridge.artist_search(&keyword, page, page_size),
                )
                .map(SourceResponse::ArtistSearch)
            }
            SourceRequest::AlbumSearch {
                keyword,
                page,
                page_size,
                ..
            } => {
                let operation = "search NetEase albums";
                Self::prepare_bridge(context, operation)?;
                Self::finish(
                    context,
                    operation,
                    self.bridge.album_search(&keyword, page, page_size),
                )
                .map(SourceResponse::AlbumSearch)
            }
            SourceRequest::PlaylistSearch {
                keyword,
                page,
                page_size,
                ..
            } => {
                let operation = "search NetEase playlists";
                Self::prepare_bridge(context, operation)?;
                Self::finish(
                    context,
                    operation,
                    self.bridge.playlist_search(&keyword, page, page_size),
                )
                .map(SourceResponse::PlaylistSearch)
            }
            SourceRequest::SearchSuggestions { keyword, limit, .. } => {
                let operation = "fetch NetEase search suggestions";
                Self::prepare_bridge(context, operation)?;
                Self::finish(
                    context,
                    operation,
                    self.bridge.search_suggestions(&keyword, limit),
                )
                .map(SourceResponse::SearchSuggestions)
            }
            SourceRequest::ArtistTopTracks { artist, limit, .. } => {
                let operation = "read NetEase artist top tracks";
                Self::prepare_bridge(context, operation)?;
                Self::finish(
                    context,
                    operation,
                    self.bridge.artist_top_tracks(&artist, limit),
                )
                .map(SourceResponse::ArtistTopTracks)
            }
            SourceRequest::AlbumRead {
                album,
                page,
                page_size,
                ..
            } => {
                let operation = "read NetEase album";
                Self::prepare_bridge(context, operation)?;
                Self::finish(
                    context,
                    operation,
                    self.bridge.album_tracks(&album, page, page_size),
                )
                .map(SourceResponse::AlbumRead)
            }
            SourceRequest::PlaylistReadPublic {
                playlist,
                page,
                page_size,
                ..
            } => {
                let operation = "read NetEase public playlist";
                Self::prepare_bridge(context, operation)?;
                Self::finish(
                    context,
                    operation,
                    self.bridge
                        .public_playlist_tracks(&playlist, page, page_size),
                )
                .map(SourceResponse::PlaylistReadPublic)
            }
            SourceRequest::MusicUrl {
                music_info,
                quality,
                ..
            } => {
                let operation = "resolve NetEase track URL";
                Self::prepare_bridge(context, operation)?;
                let track_id = track_id_from_music_info(&music_info)
                    .ok_or_else(|| context.provider_error("Remote Track has no NetEase id"))?;
                let account_ref = music_info
                    .get("accountRef")
                    .and_then(JsonValue::as_str)
                    .map(|requested_ref| Self::account_ref(context, requested_ref, operation))
                    .transpose()?;
                let result = self
                    .bridge
                    .music_url(account_ref.as_deref(), &track_id, quality);
                Self::finish(context, operation, result).map(SourceResponse::MusicUrl)
            }
            SourceRequest::MusicRecommendations {
                account_ref, limit, ..
            } => {
                let operation = "fetch NetEase recommendations";
                Self::prepare_bridge(context, operation)?;
                let account_ref = Self::account_ref(context, &account_ref, operation)?;
                let result = self.bridge.recommendations(&account_ref, limit);
                Self::finish(context, operation, result).map(SourceResponse::MusicRecommendations)
            }
            SourceRequest::PlaylistList { account_ref, .. } => {
                let operation = "list NetEase playlists";
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
                let operation = "read NetEase playlist";
                Self::prepare_bridge(context, operation)?;
                context.require_capability(SourceCapability::PlaylistRead, operation)?;
                let account_ref = Self::account_ref(context, &account_ref, operation)?;
                let result = self.bridge.playlist(&account_ref, &playlist_id);
                Self::finish(context, operation, result).map(SourceResponse::PlaylistRead)
            }
            SourceRequest::PlaylistAddTrack {
                account_ref,
                playlist_id,
                track,
                ..
            } => {
                let operation = "add NetEase playlist track";
                Self::prepare_bridge(context, operation)?;
                context.require_capability(SourceCapability::PlaylistWrite, operation)?;
                context.require_capability(SourceCapability::PlaylistRead, operation)?;
                let account_ref = Self::account_ref(context, &account_ref, operation)?;
                let result = self.bridge.mutate_playlist(
                    &account_ref,
                    &playlist_id,
                    &track,
                    SourcePlaylistMutationKind::Add,
                );
                Self::finish(context, operation, result).map(SourceResponse::PlaylistAddTrack)
            }
            SourceRequest::PlaylistRemoveTrack {
                account_ref,
                playlist_id,
                track,
                ..
            } => {
                let operation = "remove NetEase playlist track";
                Self::prepare_bridge(context, operation)?;
                context.require_capability(SourceCapability::PlaylistWrite, operation)?;
                context.require_capability(SourceCapability::PlaylistRead, operation)?;
                let account_ref = Self::account_ref(context, &account_ref, operation)?;
                let result = self.bridge.mutate_playlist(
                    &account_ref,
                    &playlist_id,
                    &track,
                    SourcePlaylistMutationKind::Remove,
                );
                Self::finish(context, operation, result).map(SourceResponse::PlaylistRemoveTrack)
            }
            request => Err(context.unsupported_action(request.source(), request.action())),
        }
    }
}

fn new_client() -> Result<NeteaseMusicClient, NeteaseBridgeError> {
    NeteaseMusicClient::builder()
        .timeout(API_TIMEOUT)
        .build()
        .map_err(|error| bridge_failure("create client", error))
}

fn checked_body(
    response: ApiResponse,
    operation: &'static str,
) -> Result<JsonValue, NeteaseBridgeError> {
    checked_body_with_limit(response, operation, MAX_API_RESPONSE_BYTES)
}

fn checked_playlist_body(response: ApiResponse) -> Result<JsonValue, NeteaseBridgeError> {
    checked_body_with_limit(response, "read playlist", MAX_PLAYLIST_RESPONSE_BYTES)
}

fn checked_body_with_limit(
    response: ApiResponse,
    operation: &'static str,
    maximum_bytes: usize,
) -> Result<JsonValue, NeteaseBridgeError> {
    validate_response_size(&response, operation, maximum_bytes)?;
    let code = response_code(&response);
    if code == 301 {
        return Err(NeteaseBridgeError::CredentialExpired);
    }
    if response.status == 429 {
        return Err(NeteaseBridgeError::RateLimited);
    }
    if !(200..300).contains(&response.status) || !matches!(code, 0 | 200 | 201) {
        return Err(api_error(operation, code, &response.body));
    }
    Ok(response.body)
}

fn validate_response_size(
    response: &ApiResponse,
    operation: &'static str,
    maximum_bytes: usize,
) -> Result<(), NeteaseBridgeError> {
    if response.raw.len() > maximum_bytes {
        Err(NeteaseBridgeError::InvalidResponse {
            operation,
            message: format!("response exceeded the {maximum_bytes} byte limit"),
        })
    } else {
        Ok(())
    }
}

fn response_code(response: &ApiResponse) -> i64 {
    response
        .body
        .get("code")
        .and_then(JsonValue::as_i64)
        .or(response.code)
        .unwrap_or_else(|| i64::from(response.status))
}

fn api_error(operation: &'static str, code: i64, body: &JsonValue) -> NeteaseBridgeError {
    if code == 301 {
        return NeteaseBridgeError::CredentialExpired;
    }
    if matches!(code, 405 | 406 | 429 | 509) {
        return NeteaseBridgeError::RateLimited;
    }
    let message = body
        .get("message")
        .or_else(|| body.get("msg"))
        .and_then(JsonValue::as_str)
        .filter(|message| !message.trim().is_empty())
        .unwrap_or("upstream request failed")
        .chars()
        .take(MAX_UPSTREAM_MESSAGE_CHARS)
        .collect();
    NeteaseBridgeError::Api {
        operation,
        code,
        message,
    }
}

fn bridge_failure(
    operation: &'static str,
    error: netease_music::NeteaseError,
) -> NeteaseBridgeError {
    NeteaseBridgeError::Bridge(format!("{operation}: {error}"))
}

fn qr_data_url(qr_url: &str) -> Result<String, NeteaseBridgeError> {
    let code = QrCode::new(qr_url.as_bytes())
        .map_err(|error| NeteaseBridgeError::Bridge(format!("create QR image: {error}")))?;
    let svg = code
        .render::<svg::Color>()
        .min_dimensions(256, 256)
        .dark_color(svg::Color("#111111"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(format!(
        "data:image/svg+xml;base64,{}",
        BASE64_STANDARD.encode(svg.as_bytes())
    ))
}

fn validate_opaque_account_ref(account_ref: &str) -> Result<(), NeteaseBridgeError> {
    let opaque_id = account_ref
        .strip_prefix(ACCOUNT_REF_PREFIX)
        .and_then(|value| Uuid::parse_str(value).ok());
    if opaque_id.is_some() {
        Ok(())
    } else {
        Err(NeteaseBridgeError::AccountNotFound)
    }
}

fn validate_netease_track_id(track_id: &str) -> Result<(), NeteaseBridgeError> {
    if !track_id.is_empty() && track_id.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(())
    } else {
        Err(NeteaseBridgeError::UnsupportedTrack(
            "NetEase track id is invalid".to_owned(),
        ))
    }
}

fn validate_netease_playlist_id(playlist_id: &str) -> Result<(), NeteaseBridgeError> {
    if !playlist_id.is_empty() && playlist_id.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(())
    } else {
        Err(NeteaseBridgeError::InvalidPlaylist)
    }
}

fn track_id_from_music_info(music_info: &JsonValue) -> Option<String> {
    json_id(music_info.get("id"))
}

fn json_id(value: Option<&JsonValue>) -> Option<String> {
    value.and_then(|value| {
        value
            .as_str()
            .map(str::to_owned)
            .or_else(|| value.as_u64().map(|id| id.to_string()))
            .or_else(|| value.as_i64().map(|id| id.to_string()))
    })
}

fn json_string(value: Option<&JsonValue>) -> Option<String> {
    value
        .and_then(JsonValue::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn playlist_track_ids(playlist: &JsonValue) -> Vec<String> {
    playlist
        .get("trackIds")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(|track| json_id(track.get("id")))
        .collect()
}

fn complete_embedded_playlist_tracks(
    playlist: &JsonValue,
    track_ids: &[String],
) -> Option<Vec<RemoteTrack>> {
    let values = playlist.get("tracks").and_then(JsonValue::as_array)?;
    if !track_ids.is_empty() && values.len() != track_ids.len() {
        return None;
    }
    let tracks = values
        .iter()
        .map(remote_track_from_json)
        .collect::<Option<Vec<_>>>()?;
    if track_ids.is_empty()
        || tracks
            .iter()
            .zip(track_ids)
            .all(|(track, expected_id)| track.id == expected_id.as_str())
    {
        Some(tracks)
    } else {
        None
    }
}

fn fetch_playlist_tracks(
    client: &NeteaseMusicClient,
    track_ids: &[String],
) -> Result<Vec<RemoteTrack>, NeteaseBridgeError> {
    collect_playlist_track_batches(track_ids, |batch| {
        let body = client
            .song_detail(SongDetailParams {
                ids: batch.to_vec(),
            })
            .map_err(|error| bridge_failure("read playlist tracks", error))
            .and_then(|response| checked_body(response, "read playlist tracks"))?;
        let songs = body
            .get("songs")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| NeteaseBridgeError::InvalidResponse {
                operation: "read playlist tracks",
                message: "track detail response did not include tracks".to_owned(),
            })?;
        Ok(songs.iter().filter_map(remote_track_from_json).collect())
    })
}

fn collect_playlist_track_batches<F>(
    track_ids: &[String],
    fetch_batch: F,
) -> Result<Vec<RemoteTrack>, NeteaseBridgeError>
where
    F: Fn(&[String]) -> Result<Vec<RemoteTrack>, NeteaseBridgeError> + Send + Sync,
{
    if track_ids.is_empty() {
        return Ok(Vec::new());
    }
    if track_ids.len() <= PLAYLIST_TRACK_BATCH_SIZE {
        return fetch_batch(track_ids);
    }
    let pool = playlist_track_pool()?;
    let batches = pool.install(|| {
        track_ids
            .par_chunks(PLAYLIST_TRACK_BATCH_SIZE)
            .map(fetch_batch)
            .collect::<Result<Vec<_>, _>>()
    })?;
    let mut tracks = Vec::with_capacity(track_ids.len());
    for batch in batches {
        tracks.extend(batch);
    }
    Ok(tracks)
}

fn playlist_track_pool() -> Result<&'static rayon::ThreadPool, NeteaseBridgeError> {
    PLAYLIST_TRACK_POOL
        .get_or_init(|| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(MAX_PLAYLIST_TRACK_CONCURRENCY)
                .thread_name(|index| format!("netease-playlist-{index}"))
                .build()
                .map_err(|error| format!("create playlist fetch pool: {error}"))
        })
        .as_ref()
        .map_err(|message| NeteaseBridgeError::Bridge(message.clone()))
}

fn remote_track_from_json(value: &JsonValue) -> Option<RemoteTrack> {
    let id = json_id(value.get("id"))?;
    let title = json_string(value.get("name"))?;
    let artists = value
        .get("ar")
        .or_else(|| value.get("artists"))
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artist| json_string(artist.get("name")))
        .collect::<Vec<_>>();
    let album = value.get("al").or_else(|| value.get("album"));
    let duration_millis = value
        .get("dt")
        .or_else(|| value.get("duration"))
        .and_then(JsonValue::as_u64);
    let raw_info = json!({ "id": id.clone() });
    let platform_ids = BTreeMap::from([(
        "id".to_owned(),
        source_runtime::JsonScalar::String(id.clone()),
    )]);
    Some(RemoteTrack {
        id,
        source: source_runtime::LX_SOURCE_WY.to_owned(),
        title,
        artist: if artists.is_empty() {
            "Unknown artist".to_owned()
        } else {
            artists.join(" / ")
        },
        album: album.and_then(|album| json_string(album.get("name"))),
        duration_seconds: duration_millis.map(|duration| duration / 1000),
        cover_url: album
            .and_then(|album| json_string(album.get("picUrl")))
            .or_else(|| json_string(value.get("picUrl"))),
        track_number: value
            .get("no")
            .and_then(JsonValue::as_u64)
            .and_then(|number| u32::try_from(number).ok()),
        disc_number: value
            .get("cd")
            .and_then(JsonValue::as_str)
            .and_then(|disc| disc.split('/').next())
            .and_then(|disc| disc.parse().ok()),
        platform_ids,
        raw_info,
    })
}

fn playlist_from_json(value: &JsonValue, account_user_id: &str) -> Option<SourcePlaylist> {
    let creator = value.get("creator");
    let owner_id = creator.and_then(|creator| json_id(creator.get("userId")));
    Some(SourcePlaylist {
        id: json_id(value.get("id"))?,
        name: json_string(value.get("name"))?,
        description: json_string(value.get("description")),
        cover_url: json_string(value.get("coverImgUrl")),
        track_count: value
            .get("trackCount")
            .and_then(JsonValue::as_u64)
            .unwrap_or_default(),
        owner_name: creator
            .and_then(|creator| json_string(creator.get("nickname")))
            .unwrap_or_else(|| "NetEase".to_owned()),
        can_mutate: owner_id.as_deref() == Some(account_user_id),
    })
}

fn mutation_kind_str(operation: SourcePlaylistMutationKind) -> &'static str {
    match operation {
        SourcePlaylistMutationKind::Add => "add",
        SourcePlaylistMutationKind::Remove => "remove",
    }
}

fn parse_mutation_kind(operation: &str) -> SourcePlaylistMutationKind {
    if operation == "remove" {
        SourcePlaylistMutationKind::Remove
    } else {
        SourcePlaylistMutationKind::Add
    }
}

fn upsert_account(
    connection: &Connection,
    account: &NeteaseAccount,
) -> Result<(), NeteaseBridgeError> {
    connection
        .execute(
            "INSERT INTO netease_accounts
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
                NETEASE_PROVIDER_ID,
                account.user_id,
                account.display_name,
                account.avatar_url,
                account.status.as_str(),
                account.connected_at,
                account.last_verified_at
            ],
        )
        .map(|_| ())
        .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))
}

fn account_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NeteaseAccount> {
    let status: String = row.get(4)?;
    Ok(NeteaseAccount {
        account_ref: row.get(0)?,
        user_id: row.get(1)?,
        display_name: row.get(2)?,
        avatar_url: row.get(3)?,
        status: NeteaseAccountStatus::parse(&status),
        connected_at: row.get(5)?,
        last_verified_at: row.get(6)?,
    })
}

fn load_accounts(connection: &Connection) -> Result<Vec<NeteaseAccount>, NeteaseBridgeError> {
    let mut statement = connection
        .prepare(
            "SELECT account_ref, user_id, display_name, avatar_url, status,
                    connected_at, last_verified_at
             FROM netease_accounts
             WHERE provider_id = ?1
             ORDER BY connected_at DESC",
        )
        .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))?;
    let accounts = statement
        .query_map(params![NETEASE_PROVIDER_ID], account_from_row)
        .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))?;
    Ok(accounts)
}

fn find_account(
    connection: &Connection,
    account_ref: &str,
) -> Result<Option<NeteaseAccount>, NeteaseBridgeError> {
    connection
        .query_row(
            "SELECT account_ref, user_id, display_name, avatar_url, status,
                    connected_at, last_verified_at
             FROM netease_accounts
             WHERE provider_id = ?1 AND account_ref = ?2",
            params![NETEASE_PROVIDER_ID, account_ref],
            account_from_row,
        )
        .optional()
        .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))
}

fn find_account_by_user_id(
    connection: &Connection,
    user_id: &str,
) -> Result<Option<NeteaseAccount>, NeteaseBridgeError> {
    connection
        .query_row(
            "SELECT account_ref, user_id, display_name, avatar_url, status,
                    connected_at, last_verified_at
             FROM netease_accounts
             WHERE provider_id = ?1 AND user_id = ?2",
            params![NETEASE_PROVIDER_ID, user_id],
            account_from_row,
        )
        .optional()
        .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))
}

fn load_mutation_audit(
    connection: &Connection,
    account_ref: Option<&str>,
    limit: u32,
) -> Result<Vec<NeteaseMutationAudit>, NeteaseBridgeError> {
    let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<NeteaseMutationAudit> {
        let operation: String = row.get(2)?;
        Ok(NeteaseMutationAudit {
            id: row.get(0)?,
            account_ref: row.get(1)?,
            operation: parse_mutation_kind(&operation),
            playlist_id: row.get(3)?,
            track_id: row.get(4)?,
            outcome: row.get(5)?,
            message: row.get(6)?,
            occurred_at: row.get(7)?,
        })
    };
    let rows = if let Some(account_ref) = account_ref {
        let mut statement = connection
            .prepare(
                "SELECT id, account_ref, operation, playlist_id, track_id, outcome, message,
                        occurred_at
                 FROM netease_mutation_audit
                 WHERE account_ref = ?1
                 ORDER BY occurred_at DESC, id DESC
                 LIMIT ?2",
            )
            .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))?;
        let records = statement
            .query_map(params![account_ref, limit], map_row)
            .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))?
            .collect::<Result<Vec<_>, _>>();
        records
    } else {
        let mut statement = connection
            .prepare(
                "SELECT id, account_ref, operation, playlist_id, track_id, outcome, message,
                        occurred_at
                 FROM netease_mutation_audit
                 ORDER BY occurred_at DESC, id DESC
                 LIMIT ?1",
            )
            .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))?;
        let records = statement
            .query_map(params![limit], map_row)
            .map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))?
            .collect::<Result<Vec<_>, _>>();
        records
    };
    rows.map_err(|error| NeteaseBridgeError::Persistence(error.to_string()))
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
    use crate::source_runtime::{DefaultSourceHost, SourceHost, SourceRuntime, SourceRuntimeError};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    const TEST_ACCOUNT_REF: &str = "netease-account:00000000-0000-4000-8000-000000000001";

    #[derive(Debug, Default)]
    struct MemoryCredentialStore {
        values: Mutex<BTreeMap<String, String>>,
    }

    impl CredentialStore for MemoryCredentialStore {
        fn save(&self, account_ref: &str, secret: &str) -> Result<(), NeteaseBridgeError> {
            self.values
                .lock()
                .map_err(|_| NeteaseBridgeError::Persistence("test store lock".to_owned()))?
                .insert(account_ref.to_owned(), secret.to_owned());
            Ok(())
        }

        fn load(&self, account_ref: &str) -> Result<String, NeteaseBridgeError> {
            self.values
                .lock()
                .map_err(|_| NeteaseBridgeError::Persistence("test store lock".to_owned()))?
                .get(account_ref)
                .cloned()
                .ok_or(NeteaseBridgeError::CredentialExpired)
        }

        fn delete(&self, account_ref: &str) -> Result<(), NeteaseBridgeError> {
            self.values
                .lock()
                .map_err(|_| NeteaseBridgeError::Persistence("test store lock".to_owned()))?
                .remove(account_ref);
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct FakeProviderBridge {
        mutation_called: AtomicBool,
    }

    impl NeteaseProviderBridge for FakeProviderBridge {
        fn music_search(
            &self,
            _keyword: &str,
            _page: u64,
            _page_size: u64,
        ) -> Result<SourceSearchResponse, NeteaseBridgeError> {
            Ok(SourceSearchResponse {
                is_end: true,
                total: Some(0),
                list: Vec::new(),
            })
        }

        fn artist_search(
            &self,
            _keyword: &str,
            _page: u64,
            _page_size: u64,
        ) -> Result<SourceArtistSearchResponse, NeteaseBridgeError> {
            Ok(SourceArtistSearchResponse {
                is_end: true,
                total: Some(0),
                list: Vec::new(),
            })
        }

        fn album_search(
            &self,
            _keyword: &str,
            _page: u64,
            _page_size: u64,
        ) -> Result<SourceAlbumSearchResponse, NeteaseBridgeError> {
            Ok(SourceAlbumSearchResponse {
                is_end: true,
                total: Some(0),
                list: Vec::new(),
            })
        }

        fn playlist_search(
            &self,
            _keyword: &str,
            _page: u64,
            _page_size: u64,
        ) -> Result<SourcePlaylistSearchResponse, NeteaseBridgeError> {
            Ok(SourcePlaylistSearchResponse {
                is_end: true,
                total: Some(0),
                list: Vec::new(),
            })
        }

        fn search_suggestions(
            &self,
            _keyword: &str,
            _limit: u64,
        ) -> Result<SourceSuggestionsResponse, NeteaseBridgeError> {
            Ok(SourceSuggestionsResponse { list: Vec::new() })
        }

        fn artist_top_tracks(
            &self,
            _artist: &SourceEntityRef,
            _limit: u64,
        ) -> Result<SourceSearchResponse, NeteaseBridgeError> {
            self.music_search("", 1, 1)
        }

        fn album_tracks(
            &self,
            _album: &SourceEntityRef,
            _page: u64,
            _page_size: u64,
        ) -> Result<SourceSearchResponse, NeteaseBridgeError> {
            self.music_search("", 1, 1)
        }

        fn public_playlist_tracks(
            &self,
            _playlist: &SourceEntityRef,
            _page: u64,
            _page_size: u64,
        ) -> Result<SourceSearchResponse, NeteaseBridgeError> {
            self.music_search("", 1, 1)
        }

        fn recommendations(
            &self,
            _account_ref: &str,
            _limit: u64,
        ) -> Result<SourceRecommendationsResponse, NeteaseBridgeError> {
            Ok(SourceRecommendationsResponse {
                list: vec![RemoteTrack {
                    id: "347230".to_owned(),
                    source: source_runtime::LX_SOURCE_WY.to_owned(),
                    title: "Test Track".to_owned(),
                    artist: "Test Artist".to_owned(),
                    album: Some("Test Album".to_owned()),
                    duration_seconds: Some(180),
                    cover_url: None,
                    track_number: None,
                    disc_number: None,
                    platform_ids: BTreeMap::new(),
                    raw_info: json!({ "id": 347230 }),
                }],
            })
        }

        fn playlists(&self, _account_ref: &str) -> Result<Vec<SourcePlaylist>, NeteaseBridgeError> {
            Ok(Vec::new())
        }

        fn playlist(
            &self,
            _account_ref: &str,
            _playlist_id: &str,
        ) -> Result<SourcePlaylistDetail, NeteaseBridgeError> {
            Err(NeteaseBridgeError::ReadOnlyPlaylist)
        }

        fn mutate_playlist(
            &self,
            _account_ref: &str,
            playlist_id: &str,
            track: &SourceTrackRef,
            operation: SourcePlaylistMutationKind,
        ) -> Result<SourcePlaylistMutation, NeteaseBridgeError> {
            self.mutation_called.store(true, Ordering::Release);
            Ok(SourcePlaylistMutation {
                audit_id: 1,
                operation,
                playlist_id: playlist_id.to_owned(),
                track_id: track.id.clone(),
                occurred_at: 1,
            })
        }

        fn music_url(
            &self,
            _account_ref: Option<&str>,
            _track_id: &str,
            _quality: SourceQuality,
        ) -> Result<String, NeteaseBridgeError> {
            Ok("https://example.invalid/track.mp3".to_owned())
        }
    }

    fn test_database() -> SharedConnection {
        let mut connection = Connection::open_in_memory().expect("test database should open");
        crate::database::initialize(&mut connection).expect("test schema should initialize");
        Arc::new(Mutex::new(connection))
    }

    fn provider_capabilities() -> BTreeSet<SourceCapability> {
        BTreeSet::from([
            SourceCapability::AccountRef,
            SourceCapability::PlaylistRead,
            SourceCapability::PlaylistWrite,
            SourceCapability::BridgeNeteaseApiEnhanced,
        ])
    }

    #[test]
    fn remote_track_parser_should_normalize_api_enhanced_song_shape() {
        let track = remote_track_from_json(&json!({
            "id": 347230,
            "name": "海阔天空",
            "ar": [{ "name": "Beyond" }],
            "al": { "name": "乐与怒", "picUrl": "https://example.test/cover.jpg" },
            "dt": 326000
        }))
        .expect("track should normalize");

        assert_eq!(track.duration_seconds, Some(326));
    }

    #[test]
    fn remote_track_parser_should_not_copy_unneeded_upstream_payload() {
        let track = remote_track_from_json(&json!({
            "id": 347230,
            "name": "Test Track",
            "ar": [{ "name": "Test Artist", "aliases": vec!["x".repeat(8_192)] }],
            "al": { "name": "Test Album", "picUrl": "https://example.test/cover.jpg" },
            "dt": 180_000,
            "privilege": { "payload": "x".repeat(8_192) }
        }))
        .expect("track should normalize");

        assert!(
            serde_json::to_vec(&track)
                .expect("track should serialize")
                .len()
                < 1_024,
            "normalized tracks should not retain the full upstream payload"
        );
    }

    #[test]
    fn playlist_track_batches_should_use_bounded_parallelism() {
        let ids = (0..(PLAYLIST_TRACK_BATCH_SIZE * MAX_PLAYLIST_TRACK_CONCURRENCY))
            .map(|id| id.to_string())
            .collect::<Vec<_>>();
        let active = AtomicUsize::new(0);
        let maximum_active = AtomicUsize::new(0);

        collect_playlist_track_batches(&ids, |_| {
            let active_now = active.fetch_add(1, Ordering::AcqRel) + 1;
            maximum_active.fetch_max(active_now, Ordering::AcqRel);
            std::thread::sleep(Duration::from_millis(20));
            active.fetch_sub(1, Ordering::AcqRel);
            Ok(Vec::new())
        })
        .expect("track batches should complete");

        assert!(
            (2..=MAX_PLAYLIST_TRACK_CONCURRENCY).contains(&maximum_active.load(Ordering::Acquire))
        );
    }

    #[test]
    fn playlist_track_batches_should_preserve_playlist_order() {
        let ids = (0..(PLAYLIST_TRACK_BATCH_SIZE + 1))
            .map(|id| id.to_string())
            .collect::<Vec<_>>();

        let tracks = collect_playlist_track_batches(&ids, |batch| {
            Ok(batch
                .iter()
                .map(|id| RemoteTrack {
                    id: id.clone(),
                    source: source_runtime::LX_SOURCE_WY.to_owned(),
                    title: id.clone(),
                    artist: "Artist".to_owned(),
                    album: None,
                    duration_seconds: None,
                    cover_url: None,
                    track_number: None,
                    disc_number: None,
                    platform_ids: BTreeMap::new(),
                    raw_info: json!({ "id": id }),
                })
                .collect())
        })
        .expect("track batches should complete");

        assert_eq!(
            tracks.into_iter().map(|track| track.id).collect::<Vec<_>>(),
            ids
        );
    }

    #[test]
    fn provider_should_expose_only_the_slice_four_source_actions() {
        let capabilities = provider_capabilities();
        let runtime = SourceRuntime::new();
        let provider = NeteaseSourceProvider::new(
            NETEASE_PROVIDER_ID.to_owned(),
            capabilities,
            Arc::new(FakeProviderBridge::default()),
        );

        let report = runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        assert_eq!(
            report.sources[source_runtime::LX_SOURCE_WY].actions,
            vec![
                SourceAction::MusicSearch,
                SourceAction::ArtistSearch,
                SourceAction::AlbumSearch,
                SourceAction::PlaylistSearch,
                SourceAction::SearchSuggestions,
                SourceAction::ArtistTopTracks,
                SourceAction::AlbumRead,
                SourceAction::PlaylistReadPublic,
                SourceAction::MusicUrl,
                SourceAction::MusicRecommendations,
                SourceAction::PlaylistList,
                SourceAction::PlaylistRead,
                SourceAction::PlaylistAddTrack,
                SourceAction::PlaylistRemoveTrack,
            ]
        );
    }

    #[test]
    fn playlist_parser_should_only_allow_owner_mutations() {
        let playlist = playlist_from_json(
            &json!({
                "id": 10,
                "name": "Daily",
                "trackCount": 2,
                "creator": { "userId": 42, "nickname": "Fika" }
            }),
            "42",
        )
        .expect("playlist should normalize");

        assert!(playlist.can_mutate);
    }

    #[test]
    fn checked_body_should_classify_code_301_as_credential_expiry() {
        let error = checked_body(
            ApiResponse {
                status: 200,
                code: Some(301),
                body: json!({ "code": 301 }),
                raw: Default::default(),
                cookies: Vec::new(),
            },
            "test",
        )
        .expect_err("301 should fail");

        assert!(matches!(error, NeteaseBridgeError::CredentialExpired));
    }

    #[test]
    fn checked_body_should_classify_rate_limit_without_retrying() {
        let error = checked_body(
            ApiResponse {
                status: 200,
                code: Some(509),
                body: json!({ "code": 509 }),
                raw: Default::default(),
                cookies: Vec::new(),
            },
            "test",
        )
        .expect_err("509 should fail");

        assert!(matches!(error, NeteaseBridgeError::RateLimited));
    }

    #[test]
    fn checked_body_should_classify_http_429_even_with_a_success_body_code() {
        let error = checked_body(
            ApiResponse {
                status: 429,
                code: Some(200),
                body: json!({ "code": 200 }),
                raw: Default::default(),
                cookies: Vec::new(),
            },
            "test",
        )
        .expect_err("HTTP 429 should fail");

        assert!(matches!(error, NeteaseBridgeError::RateLimited));
    }

    #[test]
    fn checked_body_should_reject_oversized_upstream_responses() {
        let error = checked_body(
            ApiResponse {
                status: 200,
                code: Some(200),
                body: json!({ "code": 200 }),
                raw: vec![0; MAX_API_RESPONSE_BYTES + 1].into(),
                cookies: Vec::new(),
            },
            "test",
        )
        .expect_err("oversized response should fail");

        assert!(matches!(error, NeteaseBridgeError::InvalidResponse { .. }));
    }

    #[test]
    fn checked_body_should_allow_large_playlist_responses() {
        let body = checked_playlist_body(ApiResponse {
            status: 200,
            code: Some(200),
            body: json!({ "code": 200 }),
            raw: vec![0; MAX_API_RESPONSE_BYTES + 1].into(),
            cookies: Vec::new(),
        })
        .expect("large playlist responses should be accepted");

        assert_eq!(body, json!({ "code": 200 }));
    }

    #[test]
    fn checked_playlist_body_should_reject_responses_above_playlist_limit() {
        let error = checked_playlist_body(ApiResponse {
            status: 200,
            code: Some(200),
            body: json!({ "code": 200 }),
            raw: vec![0; MAX_PLAYLIST_RESPONSE_BYTES + 1].into(),
            cookies: Vec::new(),
        })
        .expect_err("playlist responses above the aggregate limit should fail");

        assert!(matches!(error, NeteaseBridgeError::InvalidResponse { .. }));
    }

    #[test]
    fn missing_secure_session_should_mark_account_expired() {
        let db = test_database();
        let account = NeteaseAccount {
            account_ref: TEST_ACCOUNT_REF.to_owned(),
            user_id: "42".to_owned(),
            display_name: "Fika".to_owned(),
            avatar_url: None,
            status: NeteaseAccountStatus::Active,
            connected_at: 1,
            last_verified_at: 1,
        };
        upsert_account(
            &db.lock().expect("test database lock should remain healthy"),
            &account,
        )
        .expect("test account should persist");
        let bridge = NeteaseServiceBridge::with_credentials(
            Arc::clone(&db),
            Arc::new(DefaultSourceHost::new(Duration::from_secs(1), 1024)),
            Arc::new(MemoryCredentialStore::default()),
        )
        .expect("test bridge should initialize");

        let error = match bridge.client_for_account(&account.account_ref) {
            Ok(_) => panic!("missing credential should not create a client"),
            Err(error) => error,
        };

        assert!(matches!(error, NeteaseBridgeError::CredentialExpired));
        assert_eq!(
            bridge.accounts().expect("accounts should load")[0].status,
            NeteaseAccountStatus::Expired
        );
    }

    #[test]
    fn malformed_account_refs_should_fail_without_being_echoed() {
        let error = validate_opaque_account_ref("netease-account:not-a-uuid")
            .expect_err("malformed Account Ref should fail");

        assert!(matches!(error, NeteaseBridgeError::AccountNotFound));
        assert_eq!(error.to_string(), "NetEase account was not found");
    }

    #[test]
    fn provider_should_dispatch_recommendations_through_an_opaque_account_ref() {
        let host = Arc::new(DefaultSourceHost::new(Duration::from_secs(1), 1024));
        host.register_account_ref(NETEASE_PROVIDER_ID, "account", TEST_ACCOUNT_REF)
            .expect("account ref should register");
        let runtime_host: Arc<dyn SourceHost> = host;
        let capabilities = provider_capabilities();
        let runtime = SourceRuntime::with_host(runtime_host, capabilities.clone());
        let provider = NeteaseSourceProvider::new(
            NETEASE_PROVIDER_ID.to_owned(),
            capabilities,
            Arc::new(FakeProviderBridge::default()),
        );
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicRecommendations {
                    source: source_runtime::LX_SOURCE_WY.to_owned(),
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
    fn provider_should_deny_mutation_before_calling_bridge_without_playlist_write() {
        let host = Arc::new(DefaultSourceHost::new(Duration::from_secs(1), 1024));
        host.register_account_ref(NETEASE_PROVIDER_ID, "account", TEST_ACCOUNT_REF)
            .expect("account ref should register");
        let runtime_host: Arc<dyn SourceHost> = host;
        let declared = provider_capabilities();
        let granted = BTreeSet::from([
            SourceCapability::AccountRef,
            SourceCapability::PlaylistRead,
            SourceCapability::BridgeNeteaseApiEnhanced,
        ]);
        let runtime = SourceRuntime::with_host(runtime_host, granted);
        let bridge = Arc::new(FakeProviderBridge::default());
        let provider =
            NeteaseSourceProvider::new(NETEASE_PROVIDER_ID.to_owned(), declared, bridge.clone());
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        let error = runtime
            .dispatch_request(
                &provider,
                SourceRequest::PlaylistAddTrack {
                    source: source_runtime::LX_SOURCE_WY.to_owned(),
                    account_ref: "account".to_owned(),
                    playlist_id: "10".to_owned(),
                    track: SourceTrackRef {
                        id: "347230".to_owned(),
                        source: source_runtime::LX_SOURCE_WY.to_owned(),
                    },
                },
            )
            .expect_err("missing playlist write should deny mutation");

        assert!(
            matches!(error, SourceRuntimeError::CapabilityDenied { .. })
                && !bridge.mutation_called.load(Ordering::Acquire)
        );
    }

    #[test]
    fn provider_should_deny_mutation_without_playlist_read_for_ownership_check() {
        let host = Arc::new(DefaultSourceHost::new(Duration::from_secs(1), 1024));
        host.register_account_ref(NETEASE_PROVIDER_ID, "account", TEST_ACCOUNT_REF)
            .expect("account ref should register");
        let runtime_host: Arc<dyn SourceHost> = host;
        let declared = provider_capabilities();
        let granted = BTreeSet::from([
            SourceCapability::AccountRef,
            SourceCapability::PlaylistWrite,
            SourceCapability::BridgeNeteaseApiEnhanced,
        ]);
        let runtime = SourceRuntime::with_host(runtime_host, granted);
        let bridge = Arc::new(FakeProviderBridge::default());
        let provider =
            NeteaseSourceProvider::new(NETEASE_PROVIDER_ID.to_owned(), declared, bridge.clone());
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        let error = runtime
            .dispatch_request(
                &provider,
                SourceRequest::PlaylistAddTrack {
                    source: source_runtime::LX_SOURCE_WY.to_owned(),
                    account_ref: "account".to_owned(),
                    playlist_id: "10".to_owned(),
                    track: SourceTrackRef {
                        id: "347230".to_owned(),
                        source: source_runtime::LX_SOURCE_WY.to_owned(),
                    },
                },
            )
            .expect_err("missing playlist read should deny ownership verification");

        assert!(
            matches!(error, SourceRuntimeError::CapabilityDenied { .. })
                && !bridge.mutation_called.load(Ordering::Acquire)
        );
    }

    #[test]
    fn mutation_audit_should_persist_success_without_credentials() {
        let db = test_database();
        let host = Arc::new(DefaultSourceHost::new(Duration::from_secs(1), 1024));
        let bridge = NeteaseServiceBridge::with_credentials(
            db,
            host,
            Arc::new(MemoryCredentialStore::default()),
        )
        .expect("test bridge should initialize");

        bridge
            .record_mutation(
                TEST_ACCOUNT_REF,
                "10",
                "347230",
                SourcePlaylistMutationKind::Add,
                "succeeded",
                None,
            )
            .expect("audit should persist");

        assert_eq!(
            bridge
                .mutation_audit(Some(TEST_ACCOUNT_REF), 10)
                .expect("audit should load")
                .len(),
            1
        );
    }

    #[test]
    fn unsupported_track_mutation_should_fail_and_write_an_audit_record() {
        let db = test_database();
        let bridge = NeteaseServiceBridge::with_credentials(
            db,
            Arc::new(DefaultSourceHost::new(Duration::from_secs(1), 1024)),
            Arc::new(MemoryCredentialStore::default()),
        )
        .expect("test bridge should initialize");

        let error = bridge
            .mutate_playlist(
                TEST_ACCOUNT_REF,
                "10",
                &SourceTrackRef {
                    id: "external-track".to_owned(),
                    source: source_runtime::LX_SOURCE_TX.to_owned(),
                },
                SourcePlaylistMutationKind::Add,
            )
            .expect_err("non-NetEase tracks should be rejected");
        let audit = bridge
            .mutation_audit(Some(TEST_ACCOUNT_REF), 10)
            .expect("failed mutation audit should load");

        assert!(matches!(error, NeteaseBridgeError::UnsupportedTrack(_)));
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].outcome, "failed");
    }

    #[test]
    fn mutation_audit_should_bound_history_and_messages_per_account() {
        let db = test_database();
        let bridge = NeteaseServiceBridge::with_credentials(
            db,
            Arc::new(DefaultSourceHost::new(Duration::from_secs(1), 1024)),
            Arc::new(MemoryCredentialStore::default()),
        )
        .expect("test bridge should initialize");
        let long_message = "x".repeat(MAX_AUDIT_MESSAGE_CHARS + 20);

        for index in 0..MAX_AUDIT_RECORDS + 5 {
            bridge
                .record_mutation(
                    TEST_ACCOUNT_REF,
                    "10",
                    &index.to_string(),
                    SourcePlaylistMutationKind::Add,
                    "failed",
                    Some(&long_message),
                )
                .expect("audit should persist");
        }
        let audit = bridge
            .mutation_audit(Some(TEST_ACCOUNT_REF), MAX_AUDIT_RECORDS)
            .expect("bounded audit should load");

        assert_eq!(audit.len(), MAX_AUDIT_RECORDS as usize);
        assert_eq!(
            audit[0]
                .message
                .as_deref()
                .expect("failure message should exist")
                .chars()
                .count(),
            MAX_AUDIT_MESSAGE_CHARS
        );
    }
}
