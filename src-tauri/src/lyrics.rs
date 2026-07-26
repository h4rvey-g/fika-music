use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use flate2::read::ZlibDecoder;
use lofty::config::ParseOptions;
use lofty::file::{AudioFile, TaggedFile, TaggedFileExt};
use lofty::id3::v2::{
    Frame, Id3v2Tag, SyncTextContentType, SynchronizedTextFrame, TimestampFormat,
};
use lofty::mpeg::MpegFile;
use lofty::tag::ItemKey;
use netease_music::{LyricParams, NeteaseMusicClient, SearchParams};
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use strsim::normalized_levenshtein;
use uuid::Uuid;

const NETEASE_PROVIDER_NAME: &str = "NetEase Cloud Music";
const QQ_PROVIDER_NAME: &str = "QQ Music";
const KUGOU_PROVIDER_NAME: &str = "KuGou";
const LRCLIB_PROVIDER_NAME: &str = "LRCLIB";
const QQ_API_URL: &str = "https://u.y.qq.com/cgi-bin/musicu.fcg";
const KUGOU_SEARCH_URL: &str = "https://songsearch.kugou.com/song_search_v2";
const KUGOU_LYRIC_SEARCH_URL: &str = "https://krcs.kugou.com/search";
const KUGOU_LYRIC_DOWNLOAD_URL: &str = "https://lyrics.kugou.com/download";
const LRCLIB_SEARCH_URL: &str = "https://lrclib.net/api/search";
const MATCH_SCORE_THRESHOLD: f64 = 55.0;
const MATCH_SCORE_PREFERENCE_WINDOW: f64 = 15.0;
const MAX_DURATION_DIFFERENCE_SECONDS: f64 = 4.0;
const MAX_LYRICS_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_NETWORK_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const NETWORK_PROVIDER_TIMEOUT: Duration = Duration::from_secs(6);
const NETWORK_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PROVIDER_CANDIDATES: usize = 20;
const MAX_PROVIDER_FETCH_ATTEMPTS: usize = 2;
const KUGOU_KRC_XOR_KEY: [u8; 16] = [
    0x40, 0x47, 0x61, 0x77, 0x5e, 0x32, 0x74, 0x47, 0x51, 0x36, 0x31, 0x2d, 0xce, 0xd2, 0x6e, 0x69,
];

#[derive(Debug, Clone, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct TrackLyricsQuery {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_seconds: Option<i64>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub track_id: Option<String>,
}

impl TrackLyricsQuery {
    pub fn new(
        title: String,
        artist: Option<String>,
        album: Option<String>,
        duration_seconds: Option<i64>,
    ) -> Self {
        Self {
            title,
            artist,
            album,
            duration_seconds,
            source: None,
            track_id: None,
        }
    }

    pub fn with_remote_identity(
        mut self,
        source: Option<String>,
        track_id: Option<String>,
    ) -> Self {
        self.source = source;
        self.track_id = track_id;
        self
    }

    fn is_searchable(&self) -> bool {
        !self.title.trim().is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum LyricsSource {
    Embedded,
    Sidecar,
    Network,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LyricWord {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LyricLine {
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    pub text: String,
    pub words: Vec<LyricWord>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct ResolvedLyrics {
    pub source: LyricsSource,
    pub provider: Option<String>,
    pub is_synced: bool,
    pub lines: Vec<LyricLine>,
    pub saved_path: Option<String>,
    pub match_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LocalTrackPlaybackDetails {
    pub cover_data_url: Option<String>,
    pub lyrics: Option<ResolvedLyrics>,
    pub lyrics_error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LyricsError {
    #[error("failed to create the lyrics HTTP client: {0}")]
    Client(#[source] reqwest::Error),
    #[error("lyrics request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{provider} returned an unexpected status: {status}")]
    HttpStatus {
        provider: &'static str,
        status: StatusCode,
    },
    #[error("lyrics response exceeded {MAX_NETWORK_RESPONSE_BYTES} bytes")]
    ResponseTooLarge,
    #[error("failed to read a lyrics response: {0}")]
    ResponseRead(#[from] std::io::Error),
    #[error("lyrics response contained invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{provider} returned invalid data: {message}")]
    InvalidProviderData {
        provider: &'static str,
        message: String,
    },
    #[error("network lyric providers failed: {0}")]
    ProviderFailures(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibRecord {
    id: i64,
    track_name: String,
    artist_name: String,
    album_name: String,
    duration: f64,
    #[serde(default)]
    instrumental: bool,
    #[serde(default)]
    plain_lyrics: Option<String>,
    #[serde(default)]
    synced_lyrics: Option<String>,
}

impl LrclibRecord {
    fn preferred_text(&self) -> Option<&str> {
        self.synced_lyrics
            .as_deref()
            .filter(|lyrics| !lyrics.trim().is_empty())
            .or_else(|| {
                self.plain_lyrics
                    .as_deref()
                    .filter(|lyrics| !lyrics.trim().is_empty())
            })
            .or(self.instrumental.then_some("Instrumental"))
    }

    fn has_synced_lyrics(&self) -> bool {
        self.synced_lyrics
            .as_deref()
            .is_some_and(|lyrics| !lyrics.trim().is_empty())
    }
}

struct DownloadedLyrics {
    text: String,
    match_score: f64,
    provider: String,
}

#[derive(Debug, Clone, PartialEq)]
struct ProviderCandidate {
    id: String,
    title: String,
    artist: String,
    album: String,
    duration_seconds: Option<f64>,
}

struct LrclibClient {
    client: Client,
}

impl LrclibClient {
    fn new() -> Result<Self, LyricsError> {
        let client = Client::builder()
            .timeout(NETWORK_PROVIDER_TIMEOUT)
            .user_agent(format!(
                "FikaMusic/{} (com.hvg.fika-music)",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(LyricsError::Client)?;
        Ok(Self { client })
    }

    fn search(&self, query: &TrackLyricsQuery) -> Result<Vec<LrclibRecord>, LyricsError> {
        let mut params = vec![("track_name", query.title.trim())];
        if let Some(artist) = non_empty(query.artist.as_deref()) {
            params.push(("artist_name", artist));
        }

        self.request(&params)
    }

    fn broad_search(&self, query: &TrackLyricsQuery) -> Result<Vec<LrclibRecord>, LyricsError> {
        let keyword = match non_empty(query.artist.as_deref()) {
            Some(artist) => format!("{artist} {}", query.title.trim()),
            None => query.title.trim().to_owned(),
        };
        self.request(&[("q", keyword.as_str())])
    }

    fn request(&self, params: &[(&str, &str)]) -> Result<Vec<LrclibRecord>, LyricsError> {
        let response = self.client.get(LRCLIB_SEARCH_URL).query(params).send()?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        if !response.status().is_success() {
            return Err(LyricsError::HttpStatus {
                provider: LRCLIB_PROVIDER_NAME,
                status: response.status(),
            });
        }

        if response
            .content_length()
            .is_some_and(|length| length > MAX_NETWORK_RESPONSE_BYTES as u64)
        {
            return Err(LyricsError::ResponseTooLarge);
        }
        let mut bytes = Vec::new();
        response
            .take((MAX_NETWORK_RESPONSE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_NETWORK_RESPONSE_BYTES {
            return Err(LyricsError::ResponseTooLarge);
        }

        Ok(serde_json::from_slice(&bytes)?)
    }
}

fn lyrics_http_client() -> Result<Client, LyricsError> {
    Client::builder()
        .timeout(NETWORK_PROVIDER_TIMEOUT)
        .user_agent(format!(
            "FikaMusic/{} (com.hvg.fika-music)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(LyricsError::Client)
}

fn read_bounded_response(
    response: reqwest::blocking::Response,
    provider: &'static str,
) -> Result<Vec<u8>, LyricsError> {
    if !response.status().is_success() {
        return Err(LyricsError::HttpStatus {
            provider,
            status: response.status(),
        });
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_NETWORK_RESPONSE_BYTES as u64)
    {
        return Err(LyricsError::ResponseTooLarge);
    }

    let mut bytes = Vec::new();
    response
        .take((MAX_NETWORK_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_NETWORK_RESPONSE_BYTES {
        return Err(LyricsError::ResponseTooLarge);
    }
    Ok(bytes)
}

fn read_json_response(
    response: reqwest::blocking::Response,
    provider: &'static str,
) -> Result<Value, LyricsError> {
    let bytes = read_bounded_response(response, provider)?;
    serde_json::from_slice(&bytes).map_err(|error| LyricsError::InvalidProviderData {
        provider,
        message: format!("invalid JSON: {error}"),
    })
}

fn json_string_value(value: Option<&Value>) -> Option<String> {
    let value = value?;
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        _ => return None,
    };
    non_empty(Some(text.as_str())).map(str::to_owned)
}

fn json_f64_value(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
}

fn json_name_list(value: Option<&Value>, field: &str) -> String {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| json_string_value(item.get(field)))
                .collect::<Vec<_>>()
                .join("/")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

fn provider_search_text(query: &TrackLyricsQuery) -> String {
    match non_empty(query.artist.as_deref()) {
        Some(artist) => format!("{artist} {}", query.title.trim()),
        None => query.title.trim().to_owned(),
    }
}

fn query_provider_candidate(query: &TrackLyricsQuery, id: &str) -> ProviderCandidate {
    ProviderCandidate {
        id: id.to_owned(),
        title: query.title.clone(),
        artist: query.artist.clone().unwrap_or_default(),
        album: query.album.clone().unwrap_or_default(),
        duration_seconds: query.duration_seconds.map(|duration| duration as f64),
    }
}

fn validate_netease_response(
    response: &netease_music::ApiResponse,
    operation: &'static str,
) -> Result<(), LyricsError> {
    if response.raw.len() > MAX_NETWORK_RESPONSE_BYTES {
        return Err(LyricsError::ResponseTooLarge);
    }
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if !status.is_success() {
        return Err(LyricsError::HttpStatus {
            provider: NETEASE_PROVIDER_NAME,
            status,
        });
    }
    if response
        .body
        .get("code")
        .and_then(Value::as_i64)
        .is_some_and(|code| code != 200)
    {
        return Err(LyricsError::InvalidProviderData {
            provider: NETEASE_PROVIDER_NAME,
            message: format!("{operation} returned a non-success code"),
        });
    }
    Ok(())
}

fn parse_netease_candidates(body: &Value) -> Vec<ProviderCandidate> {
    let Some(songs) = body.pointer("/result/songs").and_then(Value::as_array) else {
        return Vec::new();
    };

    songs
        .iter()
        .take(MAX_PROVIDER_CANDIDATES)
        .filter_map(|song| {
            let id = json_string_value(song.get("id"))?;
            let title = json_string_value(song.get("name").or_else(|| song.get("title")))?;
            let artist = json_name_list(song.get("ar").or_else(|| song.get("artists")), "name");
            let album = song
                .get("al")
                .or_else(|| song.get("album"))
                .and_then(|album| {
                    json_string_value(album.get("name").or_else(|| album.get("title")))
                })
                .unwrap_or_default();
            let duration_seconds = json_f64_value(
                song.get("dt")
                    .or_else(|| song.get("duration"))
                    .or_else(|| song.get("interval")),
            )
            .map(|duration| duration / 1_000.0);

            Some(ProviderCandidate {
                id,
                title,
                artist,
                album,
                duration_seconds,
            })
        })
        .collect()
}

fn fetch_netease_lyrics(
    client: &NeteaseMusicClient,
    candidate: &ProviderCandidate,
) -> Result<Option<String>, LyricsError> {
    let response = client
        .lyric(LyricParams {
            id: candidate.id.clone(),
            lv: Some(-1),
            tv: Some(-1),
        })
        .map_err(|error| LyricsError::InvalidProviderData {
            provider: NETEASE_PROVIDER_NAME,
            message: format!("lyric request failed: {error}"),
        })?;
    validate_netease_response(&response, "lyric")?;

    Ok(netease_lyric_text(&response.body))
}

fn netease_lyric_text(body: &Value) -> Option<String> {
    let enhanced = ["/yrc/lyric", "/klyric/lyric"]
        .into_iter()
        .find_map(|path| {
            body.pointer(path)
                .and_then(|value| json_string_value(Some(value)))
        });
    let original = body
        .pointer("/lrc/lyric")
        .and_then(|value| json_string_value(Some(value)));
    let translation = body
        .pointer("/tlyric/lyric")
        .and_then(|value| json_string_value(Some(value)));
    let romanization = body
        .pointer("/romalrc/lyric")
        .and_then(|value| json_string_value(Some(value)));
    combine_lyric_texts([enhanced.or(original), translation, romanization])
}

fn resolve_netease_lyrics(
    query: &TrackLyricsQuery,
) -> Result<Option<DownloadedLyrics>, LyricsError> {
    let client = NeteaseMusicClient::builder()
        .timeout(NETWORK_PROVIDER_TIMEOUT)
        .build()
        .map_err(|error| LyricsError::InvalidProviderData {
            provider: NETEASE_PROVIDER_NAME,
            message: format!("client creation failed: {error}"),
        })?;

    if query
        .source
        .as_deref()
        .is_some_and(|source| matches!(source, "wy" | "netease" | "netease-cloud-music"))
    {
        if let Some(track_id) = non_empty(query.track_id.as_deref()) {
            let candidate = query_provider_candidate(query, track_id);
            if score_provider_candidate(query, &candidate)
                .is_some_and(|score| score >= MATCH_SCORE_THRESHOLD)
            {
                if let Ok(Some(text)) = fetch_netease_lyrics(&client, &candidate) {
                    if !parse_lyrics(&text).is_empty() {
                        return Ok(Some(DownloadedLyrics {
                            text,
                            match_score: 100.0,
                            provider: format!("{NETEASE_PROVIDER_NAME} #{}", candidate.id),
                        }));
                    }
                }
            }
        }
    }

    let response = client
        .search(SearchParams {
            keywords: provider_search_text(query),
            search_type: Some("1".to_owned()),
            limit: Some(MAX_PROVIDER_CANDIDATES as u32),
            offset: Some(0),
        })
        .map_err(|error| LyricsError::InvalidProviderData {
            provider: NETEASE_PROVIDER_NAME,
            message: format!("search request failed: {error}"),
        })?;
    validate_netease_response(&response, "search")?;
    let candidates = parse_netease_candidates(&response.body);
    resolve_provider_candidates(NETEASE_PROVIDER_NAME, query, candidates, |candidate| {
        fetch_netease_lyrics(&client, candidate)
    })
}

fn parse_qq_candidates(body: &Value) -> Vec<ProviderCandidate> {
    let Some(songs) = body
        .pointer("/req_1/data/body/song/list")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    songs
        .iter()
        .take(MAX_PROVIDER_CANDIDATES)
        .filter_map(|song| {
            let id = json_string_value(song.get("mid").or_else(|| song.get("id")))?;
            let title = json_string_value(song.get("title").or_else(|| song.get("name")))?;
            let artist = json_name_list(song.get("singer"), "name");
            let album = song
                .get("album")
                .and_then(|album| {
                    json_string_value(album.get("name").or_else(|| album.get("title")))
                })
                .unwrap_or_default();
            let duration_seconds = json_f64_value(song.get("interval"));

            Some(ProviderCandidate {
                id,
                title,
                artist,
                album,
                duration_seconds,
            })
        })
        .collect()
}

fn decode_base64_lyric(value: Option<&Value>) -> Option<String> {
    let raw = json_string_value(value)?;
    let decoded = BASE64_STANDARD
        .decode(raw.trim())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or(raw);
    non_empty(Some(decoded.as_str())).map(str::to_owned)
}

fn fetch_qq_lyrics(
    client: &Client,
    candidate: &ProviderCandidate,
) -> Result<Option<String>, LyricsError> {
    if let Some(lyrics) = request_qq_lyrics(client, candidate, true)? {
        if looks_like_lyric_text(&lyrics) {
            return Ok(Some(lyrics));
        }
    }
    request_qq_lyrics(client, candidate, false)
}

fn request_qq_lyrics(
    client: &Client,
    candidate: &ProviderCandidate,
    enhanced: bool,
) -> Result<Option<String>, LyricsError> {
    let param = if enhanced {
        json!({
            "crypt": 0,
            "format": "json",
            "qrc": 1,
            "qrc_t": 0,
            "roma": 1,
            "songMID": candidate.id,
            "trans": 1,
            "type": -1
        })
    } else {
        json!({
            "crypt": 0,
            "roma": 1,
            "songMID": candidate.id,
            "trans": 1,
            "type": 0
        })
    };
    let body = json!({
        "req": {
            "method": "GetPlayLyricInfo",
            "module": "music.musichallSong.PlayLyricInfo",
            "param": param
        },
        "comm": {"ct": 24, "cv": 0}
    });
    let response = client
        .post(QQ_API_URL)
        .header("Referer", "https://y.qq.com/")
        .header("Accept", "application/json, text/plain, */*")
        .json(&body)
        .send()?;
    let response = read_json_response(response, QQ_PROVIDER_NAME)?;
    if response
        .pointer("/req/code")
        .and_then(Value::as_i64)
        .is_some_and(|code| code != 0)
    {
        return Ok(None);
    }
    let data = response.pointer("/req/data");
    let original = decode_base64_lyric(data.and_then(|value| value.get("lyric")));
    let translation = decode_base64_lyric(data.and_then(|value| value.get("trans")));
    let romanization = decode_base64_lyric(data.and_then(|value| value.get("roma")));
    Ok(combine_lyric_texts([original, translation, romanization]))
}

fn looks_like_lyric_text(text: &str) -> bool {
    text.contains("LyricContent=")
        || text.lines().any(|line| {
            parse_millisecond_line_header(line.trim_start()).is_some()
                || !parse_lrc_timestamps(line, 0).0.is_empty()
        })
}

fn resolve_qq_lyrics(query: &TrackLyricsQuery) -> Result<Option<DownloadedLyrics>, LyricsError> {
    let client = lyrics_http_client()?;
    if query
        .source
        .as_deref()
        .is_some_and(|source| matches!(source, "tx" | "qq" | "qqmusic"))
    {
        if let Some(track_id) = non_empty(query.track_id.as_deref()) {
            let candidate = query_provider_candidate(query, track_id);
            if let Ok(Some(text)) = fetch_qq_lyrics(&client, &candidate) {
                if !parse_lyrics(&text).is_empty() {
                    return Ok(Some(DownloadedLyrics {
                        text,
                        match_score: 100.0,
                        provider: format!("{QQ_PROVIDER_NAME} #{}", candidate.id),
                    }));
                }
            }
        }
    }
    let body = json!({
        "req_1": {
            "method": "DoSearchForQQMusicDesktop",
            "module": "music.search.SearchCgiService",
            "param": {
                "num_per_page": MAX_PROVIDER_CANDIDATES,
                "page_num": 1,
                "query": provider_search_text(query),
                "search_type": 0
            }
        }
    });
    let response = client
        .post(QQ_API_URL)
        .header("Referer", "https://y.qq.com/")
        .header("Accept", "application/json, text/plain, */*")
        .json(&body)
        .send()?;
    let response = read_json_response(response, QQ_PROVIDER_NAME)?;
    if response
        .pointer("/req_1/code")
        .and_then(Value::as_i64)
        .is_some_and(|code| code != 0)
    {
        return Ok(None);
    }
    let candidates = parse_qq_candidates(&response);
    resolve_provider_candidates(QQ_PROVIDER_NAME, query, candidates, |candidate| {
        fetch_qq_lyrics(&client, candidate)
    })
}

fn clean_kugou_text(value: &str) -> String {
    value
        .replace("<em>", "")
        .replace("</em>", "")
        .trim()
        .to_owned()
}

fn kugou_song_title(file_name: &str, artist: &str) -> String {
    let title = clean_kugou_text(file_name);
    let Some((prefix, remainder)) = title.split_once(" - ") else {
        return title;
    };
    if normalize_text(prefix) == normalize_text(artist) {
        remainder.trim().to_owned()
    } else {
        title
    }
}

fn parse_kugou_candidates(body: &Value) -> Vec<ProviderCandidate> {
    let Some(songs) = body
        .pointer("/data/lists")
        .or_else(|| body.pointer("/lists"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    songs
        .iter()
        .take(MAX_PROVIDER_CANDIDATES)
        .filter_map(|song| {
            let id = json_string_value(
                song.get("FileHash")
                    .or_else(|| song.get("Hash"))
                    .or_else(|| song.get("fileHash")),
            )?;
            let artist = json_string_value(song.get("SingerName"))?;
            let file_name = json_string_value(
                song.get("SongName")
                    .or_else(|| song.get("FileName"))
                    .or_else(|| song.get("songName")),
            )?;
            let title = kugou_song_title(&file_name, &artist);
            let album = json_string_value(song.get("AlbumName")).unwrap_or_default();
            let duration = json_f64_value(song.get("Duration"));
            let duration_seconds = duration.map(|value| {
                if value > 10_000.0 {
                    value / 1_000.0
                } else {
                    value
                }
            });

            Some(ProviderCandidate {
                id,
                title,
                artist,
                album,
                duration_seconds,
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
struct KugouLyricCandidate {
    id: String,
    access_key: String,
    title: String,
    artist: String,
    duration_seconds: Option<f64>,
}

fn parse_kugou_lyric_candidates(body: &Value) -> Vec<KugouLyricCandidate> {
    body.get("candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_PROVIDER_CANDIDATES)
        .filter_map(|candidate| {
            Some(KugouLyricCandidate {
                id: json_string_value(candidate.get("id"))?,
                access_key: json_string_value(candidate.get("accesskey"))?,
                title: json_string_value(candidate.get("song"))?,
                artist: json_string_value(candidate.get("singer")).unwrap_or_default(),
                duration_seconds: json_f64_value(candidate.get("duration")).map(|value| {
                    if value > 10_000.0 {
                        value / 1_000.0
                    } else {
                        value
                    }
                }),
            })
        })
        .collect()
}

fn fetch_kugou_lyrics(
    client: &Client,
    query: &TrackLyricsQuery,
    candidate: &ProviderCandidate,
) -> Result<Option<String>, LyricsError> {
    let response = client
        .get(KUGOU_LYRIC_SEARCH_URL)
        .query(&[
            ("ver", "1"),
            ("man", "yes"),
            ("client", "mobi"),
            ("keyword", ""),
            ("duration", ""),
            ("hash", candidate.id.as_str()),
            ("album_audio_id", ""),
        ])
        .header("Referer", "https://www.kugou.com/")
        .send()?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = read_json_response(response, KUGOU_PROVIDER_NAME)?;
    let mut lyric_candidates = parse_kugou_lyric_candidates(&response)
        .into_iter()
        .filter_map(|candidate| {
            let metadata = ProviderCandidate {
                id: candidate.id.clone(),
                title: candidate.title.clone(),
                artist: candidate.artist.clone(),
                album: String::new(),
                duration_seconds: candidate.duration_seconds,
            };
            let score = score_provider_candidate(query, &metadata)?;
            (score >= MATCH_SCORE_THRESHOLD).then_some((candidate, score))
        })
        .collect::<Vec<_>>();
    lyric_candidates.sort_by(|left, right| right.1.total_cmp(&left.1));
    let Some((lyric_candidate, _)) = lyric_candidates.into_iter().next() else {
        return Ok(None);
    };

    let response = client
        .get(KUGOU_LYRIC_DOWNLOAD_URL)
        .query(&[
            ("ver", "1"),
            ("client", "pc"),
            ("id", lyric_candidate.id.as_str()),
            ("accesskey", lyric_candidate.access_key.as_str()),
            ("fmt", "krc"),
            ("charset", "utf8"),
        ])
        .send()?;
    let bytes = read_bounded_response(response, KUGOU_PROVIDER_NAME)?;
    let response: Value =
        serde_json::from_slice(&bytes).map_err(|error| LyricsError::InvalidProviderData {
            provider: KUGOU_PROVIDER_NAME,
            message: format!("invalid lyric JSON: {error}"),
        })?;
    if response
        .get("status")
        .and_then(Value::as_i64)
        .is_some_and(|status| status != 200)
    {
        return Ok(None);
    }
    let Some(content) = json_string_value(response.get("content")) else {
        return Ok(None);
    };
    let decoded = BASE64_STANDARD.decode(content.trim()).map_err(|error| {
        LyricsError::InvalidProviderData {
            provider: KUGOU_PROVIDER_NAME,
            message: format!("lyric content was not valid Base64: {error}"),
        }
    })?;
    if decoded.len() as u64 > MAX_LYRICS_FILE_BYTES {
        return Err(LyricsError::ResponseTooLarge);
    }
    let text = decode_kugou_lyric(&decoded)?;
    Ok((!text.trim().is_empty()).then_some(text))
}

fn decode_kugou_lyric(bytes: &[u8]) -> Result<String, LyricsError> {
    let Some(payload) = bytes.strip_prefix(b"krc1") else {
        return Ok(decode_text_file(bytes));
    };
    let decrypted = payload
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ KUGOU_KRC_XOR_KEY[index % KUGOU_KRC_XOR_KEY.len()])
        .collect::<Vec<_>>();
    let decoder = ZlibDecoder::new(decrypted.as_slice());
    let mut decoded = Vec::new();
    decoder
        .take(MAX_LYRICS_FILE_BYTES.saturating_add(1))
        .read_to_end(&mut decoded)
        .map_err(|error| LyricsError::InvalidProviderData {
            provider: KUGOU_PROVIDER_NAME,
            message: format!("failed to decompress KRC lyrics: {error}"),
        })?;
    if decoded.len() as u64 > MAX_LYRICS_FILE_BYTES {
        return Err(LyricsError::ResponseTooLarge);
    }
    Ok(decode_text_file(&decoded))
}

fn resolve_kugou_lyrics(query: &TrackLyricsQuery) -> Result<Option<DownloadedLyrics>, LyricsError> {
    let client = lyrics_http_client()?;
    if query
        .source
        .as_deref()
        .is_some_and(|source| matches!(source, "kg" | "kugou"))
    {
        if let Some(track_id) = non_empty(query.track_id.as_deref()) {
            let candidate = query_provider_candidate(query, track_id);
            if let Ok(Some(text)) = fetch_kugou_lyrics(&client, query, &candidate) {
                if !parse_lyrics(&text).is_empty() {
                    return Ok(Some(DownloadedLyrics {
                        text,
                        match_score: 100.0,
                        provider: format!("{KUGOU_PROVIDER_NAME} #{}", candidate.id),
                    }));
                }
            }
        }
    }
    let response = client
        .get(KUGOU_SEARCH_URL)
        .query(&[
            ("keyword", provider_search_text(query)),
            ("page", "1".to_owned()),
            ("pagesize", MAX_PROVIDER_CANDIDATES.to_string()),
            ("userid", "0".to_owned()),
            ("clientver", "20549".to_owned()),
            ("platform", "WebFilter".to_owned()),
            ("tag", "em".to_owned()),
            ("filter", "10".to_owned()),
            ("iscorrection", "1".to_owned()),
            ("privilege_filter", "0".to_owned()),
        ])
        .header("Referer", "https://www.kugou.com/")
        .header("Accept", "application/json, text/plain, */*")
        .send()?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = read_json_response(response, KUGOU_PROVIDER_NAME)?;
    let candidates = parse_kugou_candidates(&response);
    resolve_provider_candidates(KUGOU_PROVIDER_NAME, query, candidates, |candidate| {
        fetch_kugou_lyrics(&client, query, candidate)
    })
}

fn combine_lyric_texts<const N: usize>(texts: [Option<String>; N]) -> Option<String> {
    let mut combined = String::new();
    for text in texts.into_iter().flatten() {
        if combined.is_empty() {
            combined = text;
        } else if combined != text {
            combined.push('\n');
            combined.push_str(&text);
        }
    }
    (!combined.trim().is_empty()).then_some(combined)
}

fn rank_provider_candidates(
    query: &TrackLyricsQuery,
    candidates: &[ProviderCandidate],
) -> Vec<(usize, f64)> {
    let mut scored = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            score_provider_candidate(query, candidate).map(|score| (index, score))
        })
        .filter(|(_, score)| *score >= MATCH_SCORE_THRESHOLD)
        .collect::<Vec<_>>();
    let Some(highest_score) = scored
        .iter()
        .map(|(_, score)| *score)
        .max_by(f64::total_cmp)
    else {
        return Vec::new();
    };
    scored.retain(|(_, score)| highest_score - score <= MATCH_SCORE_PREFERENCE_WINDOW);
    scored.sort_by(|left, right| right.1.total_cmp(&left.1));
    scored.truncate(MAX_PROVIDER_FETCH_ATTEMPTS);
    scored
}

fn resolve_provider_candidates<F>(
    provider: &'static str,
    query: &TrackLyricsQuery,
    candidates: Vec<ProviderCandidate>,
    mut fetch: F,
) -> Result<Option<DownloadedLyrics>, LyricsError>
where
    F: FnMut(&ProviderCandidate) -> Result<Option<String>, LyricsError>,
{
    let mut last_error = None;
    for (index, score) in rank_provider_candidates(query, &candidates) {
        let candidate = &candidates[index];
        match fetch(candidate) {
            Ok(Some(text)) if !parse_lyrics(&text).is_empty() => {
                return Ok(Some(DownloadedLyrics {
                    text,
                    match_score: score,
                    provider: format!("{provider} #{}", candidate.id),
                }));
            }
            Ok(_) => {}
            Err(error) => last_error = Some(error),
        }
    }
    match last_error {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

pub fn resolve_local_track(path: &Path, query: &TrackLyricsQuery) -> LocalTrackPlaybackDetails {
    let tagged_file = lofty::read_from_path(path).ok();
    let cover_data_url = crate::album_art::embedded_cover_data_url(path);

    let embedded = tagged_file
        .as_ref()
        .and_then(embedded_lyrics)
        .and_then(|text| resolved_lyrics(LyricsSource::Embedded, &text, None, None, None))
        .or_else(|| read_synchronized_id3_lyrics(path));
    if let Some(lyrics) = embedded {
        return LocalTrackPlaybackDetails {
            cover_data_url,
            lyrics: Some(lyrics),
            lyrics_error: None,
        };
    }

    let sidecar_error = match read_sidecar_lyrics(path) {
        Ok(Some((sidecar_path, text))) => {
            return LocalTrackPlaybackDetails {
                cover_data_url,
                lyrics: resolved_lyrics(
                    LyricsSource::Sidecar,
                    &text,
                    None,
                    Some(path_to_string(&sidecar_path)),
                    None,
                ),
                lyrics_error: None,
            };
        }
        Ok(None) => None,
        Err(error) => Some(error),
    };

    match resolve_network_lyrics_text(query) {
        Ok(Some(downloaded)) => {
            let saved_path = persist_downloaded_lyrics(path, &downloaded.text)
                .ok()
                .map(|path| path_to_string(&path));
            LocalTrackPlaybackDetails {
                cover_data_url,
                lyrics: resolved_lyrics(
                    LyricsSource::Network,
                    &downloaded.text,
                    Some(downloaded.provider),
                    saved_path,
                    Some(downloaded.match_score),
                ),
                lyrics_error: None,
            }
        }
        Ok(None) => LocalTrackPlaybackDetails {
            cover_data_url,
            lyrics: None,
            lyrics_error: sidecar_error,
        },
        Err(error) => LocalTrackPlaybackDetails {
            cover_data_url,
            lyrics: None,
            lyrics_error: Some(match sidecar_error {
                Some(sidecar_error) => {
                    format!("{sidecar_error}; network fallback failed: {error}")
                }
                None => error.to_string(),
            }),
        },
    }
}

pub fn resolve_network_lyrics(
    query: &TrackLyricsQuery,
) -> Result<Option<ResolvedLyrics>, LyricsError> {
    Ok(resolve_network_lyrics_text(query)?.and_then(|downloaded| {
        resolved_lyrics(
            LyricsSource::Network,
            &downloaded.text,
            Some(downloaded.provider),
            None,
            Some(downloaded.match_score),
        )
    }))
}

fn resolve_network_lyrics_text(
    query: &TrackLyricsQuery,
) -> Result<Option<DownloadedLyrics>, LyricsError> {
    if !query.is_searchable() {
        return Ok(None);
    }

    let started_at = Instant::now();
    let mut failures = Vec::new();

    // Search all primary providers together, then consume results in priority order.
    let primary_results = std::thread::scope(|scope| {
        let netease = scope.spawn(|| resolve_netease_lyrics(query));
        let qq = scope.spawn(|| resolve_qq_lyrics(query));
        let kugou = scope.spawn(|| resolve_kugou_lyrics(query));
        [
            (NETEASE_PROVIDER_NAME, netease.join()),
            (QQ_PROVIDER_NAME, qq.join()),
            (KUGOU_PROVIDER_NAME, kugou.join()),
        ]
    });
    for (provider, result) in primary_results {
        match result {
            Ok(Ok(Some(lyrics))) => return Ok(Some(lyrics)),
            Ok(Ok(None)) => {}
            Ok(Err(error)) => failures.push(format!("{provider}: {error}")),
            Err(_) => failures.push(format!("{provider}: provider worker panicked")),
        }
    }

    if started_at.elapsed() < NETWORK_RESOLUTION_TIMEOUT {
        match resolve_lrclib_lyrics(query) {
            Ok(Some(lyrics)) => return Ok(Some(lyrics)),
            Ok(None) => {}
            Err(error) => failures.push(format!("{LRCLIB_PROVIDER_NAME}: {error}")),
        }
    }

    if failures.is_empty() {
        Ok(None)
    } else {
        Err(LyricsError::ProviderFailures(failures.join("; ")))
    }
}

fn resolve_lrclib_lyrics(
    query: &TrackLyricsQuery,
) -> Result<Option<DownloadedLyrics>, LyricsError> {
    let client = LrclibClient::new()?;
    let mut records = client.search(query)?;
    if select_record(query, &records).is_none() {
        let broad_records = client.broad_search(query)?;
        for record in broad_records {
            if !records.iter().any(|candidate| candidate.id == record.id) {
                records.push(record);
            }
        }
    }

    let Some((record, match_score)) = select_record(query, &records) else {
        return Ok(None);
    };
    let Some(text) = record.preferred_text() else {
        return Ok(None);
    };

    Ok(Some(DownloadedLyrics {
        text: text.trim().to_owned(),
        match_score,
        provider: format!("{LRCLIB_PROVIDER_NAME} #{}", record.id),
    }))
}

fn select_record<'a>(
    query: &TrackLyricsQuery,
    records: &'a [LrclibRecord],
) -> Option<(&'a LrclibRecord, f64)> {
    let mut scored = records
        .iter()
        .filter(|record| record.preferred_text().is_some())
        .filter_map(|record| score_record(query, record).map(|score| (record, score)))
        .filter(|(_, score)| *score >= MATCH_SCORE_THRESHOLD)
        .collect::<Vec<_>>();

    let highest_score = scored
        .iter()
        .map(|(_, score)| *score)
        .max_by(f64::total_cmp)?;
    scored.retain(|(_, score)| highest_score - score <= MATCH_SCORE_PREFERENCE_WINDOW);
    scored.sort_by(|(left_record, left_score), (right_record, right_score)| {
        right_record
            .has_synced_lyrics()
            .cmp(&left_record.has_synced_lyrics())
            .then_with(|| right_score.total_cmp(left_score))
    });
    scored.into_iter().next()
}

fn score_record(query: &TrackLyricsQuery, record: &LrclibRecord) -> Option<f64> {
    score_metadata(
        query,
        &record.track_name,
        &record.artist_name,
        &record.album_name,
        Some(record.duration),
    )
}

fn score_provider_candidate(
    query: &TrackLyricsQuery,
    candidate: &ProviderCandidate,
) -> Option<f64> {
    score_metadata(
        query,
        &candidate.title,
        &candidate.artist,
        &candidate.album,
        candidate.duration_seconds,
    )
}

fn score_metadata(
    query: &TrackLyricsQuery,
    title: &str,
    artist_name: &str,
    album_name: &str,
    duration_seconds: Option<f64>,
) -> Option<f64> {
    if query.duration_seconds.is_some_and(|duration| {
        duration_seconds.is_some_and(|candidate_duration| {
            (duration as f64 - candidate_duration).abs() > MAX_DURATION_DIFFERENCE_SECONDS
        })
    }) {
        return None;
    }

    let mut title_score = title_similarity(&query.title, title) * 100.0;
    if non_empty(query.artist.as_deref()).is_none() {
        title_score = title_score.max(
            [
                format!("{} - {}", artist_name, title),
                format!("{} - {}", title, artist_name),
            ]
            .iter()
            .map(|candidate| text_similarity(&query.title, candidate) * 100.0)
            .fold(0.0, f64::max),
        );
    }

    let artist_score = non_empty(query.artist.as_deref())
        .map(|artist| artist_similarity(artist, artist_name) * 100.0);
    let album_score = non_empty(query.album.as_deref())
        .filter(|_| !album_name.trim().is_empty())
        .map(|album| text_similarity(album, album_name) * 100.0);

    let mut score = match (artist_score, album_score) {
        (Some(artist), Some(album)) => {
            (title_score * 0.5 + artist * 0.5).max(title_score * 0.5 + artist * 0.35 + album * 0.15)
        }
        (Some(artist), None) => title_score * 0.5 + artist * 0.5,
        (None, Some(album)) => (title_score * 0.7 + album * 0.3).max(title_score * 0.8),
        (None, None) => title_score,
    };

    if title_score < 30.0 {
        score = (score - 35.0).max(0.0);
    }

    Some(score.clamp(0.0, 100.0))
}

fn title_similarity(left: &str, right: &str) -> f64 {
    let normalized_left = normalize_text(left);
    let normalized_right = normalize_text(right);
    let full_score = similarity_normalized(&normalized_left, &normalized_right);
    let (left_core, left_had_tag) = title_core(&normalized_left);
    let (right_core, right_had_tag) = title_core(&normalized_right);
    if !left_had_tag && !right_had_tag {
        return full_score;
    }

    let core_score = similarity_normalized(&left_core, &right_core);
    let tag_weight = if left_had_tag == right_had_tag {
        0.8
    } else {
        0.75
    };
    full_score.max(core_score * tag_weight)
}

fn title_core(title: &str) -> (String, bool) {
    const VERSION_TAGS: [&str; 18] = [
        "version",
        " ver",
        "mix",
        "edit",
        "live",
        "solo",
        "style",
        "size",
        "instrumental",
        "inst.",
        "off vocal",
        "karaoke",
        "remaster",
        "acoustic",
        "伴奏",
        "纯音乐",
        "现场",
        "版",
    ];

    let Some(index) = title
        .char_indices()
        .rev()
        .find_map(|(index, character)| matches!(character, '(' | '[' | '<' | '-').then_some(index))
    else {
        return (title.to_owned(), false);
    };
    let suffix = &title[index..];
    if VERSION_TAGS.iter().any(|tag| suffix.contains(tag)) {
        (title[..index].trim().to_owned(), true)
    } else {
        (title.to_owned(), false)
    }
}

fn artist_similarity(left: &str, right: &str) -> f64 {
    let left_parts = artist_parts(left);
    let right_parts = artist_parts(right);
    if left_parts.is_empty() || right_parts.is_empty() {
        return text_similarity(left, right);
    }

    let mut pair_scores = left_parts
        .iter()
        .enumerate()
        .flat_map(|(left_index, left)| {
            right_parts
                .iter()
                .enumerate()
                .map(move |(right_index, right)| {
                    (left_index, right_index, similarity_normalized(left, right))
                })
        })
        .collect::<Vec<_>>();
    pair_scores.sort_by(|left, right| right.2.total_cmp(&left.2));

    let mut used_left = vec![false; left_parts.len()];
    let mut used_right = vec![false; right_parts.len()];
    let mut total = 0.0;
    for (left_index, right_index, score) in pair_scores {
        if used_left[left_index] || used_right[right_index] {
            continue;
        }
        used_left[left_index] = true;
        used_right[right_index] = true;
        total += score;
    }

    let matched_score = total / left_parts.len().max(right_parts.len()) as f64;
    matched_score.max(text_similarity(left, right))
}

fn artist_parts(artist: &str) -> Vec<String> {
    let normalized = normalize_text(artist)
        .replace(" featuring ", "/")
        .replace(" feat. ", "/")
        .replace(" feat ", "/")
        .replace(" ft. ", "/")
        .replace(" x ", "/");
    normalized
        .split([',', '、', '/', '\\', '&', '・', ';'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn text_similarity(left: &str, right: &str) -> f64 {
    similarity_normalized(&normalize_text(left), &normalize_text(right))
}

fn similarity_normalized(left: &str, right: &str) -> f64 {
    if left == right && !left.is_empty() {
        1.0
    } else if left.is_empty() || right.is_empty() {
        0.0
    } else {
        normalized_levenshtein(left, right)
    }
}

fn normalize_text(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut previous_was_space = false;
    for character in value.trim().chars() {
        let character = match character {
            '（' => '(',
            '）' => ')',
            '：' => ':',
            '！' => '!',
            '？' => '?',
            '／' => '/',
            '＆' => '&',
            '＊' => '*',
            '＠' => '@',
            '＃' => '#',
            '＄' => '$',
            '％' => '%',
            '＼' => '\\',
            '｜' => '|',
            '＝' => '=',
            '＋' => '+',
            '－' => '-',
            '＜' => '<',
            '＞' => '>',
            '［' => '[',
            '］' => ']',
            '｛' => '{',
            '｝' => '}',
            other => other,
        };
        if character.is_whitespace() {
            if !previous_was_space {
                normalized.push(' ');
                previous_was_space = true;
            }
        } else {
            normalized.extend(character.to_lowercase());
            previous_was_space = false;
        }
    }
    normalized.trim().to_owned()
}

fn resolved_lyrics(
    source: LyricsSource,
    text: &str,
    provider: Option<String>,
    saved_path: Option<String>,
    match_score: Option<f64>,
) -> Option<ResolvedLyrics> {
    let lines = parse_lyrics(text);
    if lines.is_empty() {
        return None;
    }
    let is_synced = lines.iter().any(|line| line.start_ms.is_some());
    Some(ResolvedLyrics {
        source,
        provider,
        is_synced,
        lines,
        saved_path,
        match_score: match_score.map(|score| (score * 10.0).round() / 10.0),
    })
}

fn parse_lyrics(text: &str) -> Vec<LyricLine> {
    let qrc_content = extract_qrc_lyric_content(text);
    let normalized = qrc_content
        .as_deref()
        .unwrap_or(text)
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n");
    let offset_ms = normalized
        .lines()
        .find_map(|line| parse_offset(line.trim()))
        .unwrap_or_default();
    let mut timed_lines = BTreeMap::<u64, Vec<ParsedTimedLine>>::new();
    let mut plain_lines = Vec::new();

    for raw_line in normalized.lines() {
        let parsed_lines = parse_timed_lyric_line(raw_line, offset_ms);
        if parsed_lines.is_empty() {
            let content = raw_line.trim();
            if !content.is_empty() && !is_metadata_line(content) {
                plain_lines.push(LyricLine {
                    start_ms: None,
                    end_ms: None,
                    text: content.to_owned(),
                    words: Vec::new(),
                });
            }
            continue;
        }

        for parsed_line in parsed_lines {
            if parsed_line.text.is_empty() {
                continue;
            }
            let lines = timed_lines.entry(parsed_line.start_ms).or_default();
            if let Some(existing) = lines
                .iter_mut()
                .find(|existing| existing.text == parsed_line.text)
            {
                if existing.words.is_empty() && !parsed_line.words.is_empty() {
                    *existing = parsed_line;
                }
            } else {
                lines.push(parsed_line);
            }
        }
    }

    if timed_lines.is_empty() {
        return plain_lines;
    }

    let mut lines = timed_lines
        .into_iter()
        .map(|(start_ms, parsed_lines)| {
            let end_ms = parsed_lines.iter().filter_map(|line| line.end_ms).max();
            let words = if parsed_lines.iter().any(|line| !line.words.is_empty()) {
                merge_timed_words(&parsed_lines, start_ms, end_ms)
            } else {
                Vec::new()
            };
            LyricLine {
                start_ms: Some(start_ms),
                end_ms,
                text: parsed_lines
                    .iter()
                    .map(|line| line.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                words,
            }
        })
        .collect::<Vec<_>>();
    set_line_end_times(&mut lines);
    lines
}

fn extract_qrc_lyric_content(text: &str) -> Option<String> {
    if !text.contains("LyricContent=") {
        return None;
    }
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);
    loop {
        match reader.read_event().ok()? {
            Event::Start(element) | Event::Empty(element) => {
                for attribute in element.attributes().with_checks(false).flatten() {
                    if attribute.key.as_ref() == b"LyricContent" {
                        return attribute
                            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                            .ok()
                            .map(|value| value.into_owned());
                    }
                }
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}

#[derive(Debug, Clone)]
struct ParsedTimedLine {
    start_ms: u64,
    end_ms: Option<u64>,
    text: String,
    words: Vec<LyricWord>,
}

#[derive(Debug, Clone, Copy)]
enum WordTimeOrigin {
    Absolute,
    Relative,
}

#[derive(Debug)]
struct WordTimingMarker {
    start: u64,
    duration: u64,
    range_start: usize,
    range_end: usize,
}

fn parse_timed_lyric_line(line: &str, offset_ms: i64) -> Vec<ParsedTimedLine> {
    let line = line.trim_start();
    if let Some((start_ms, duration_ms, content)) = parse_millisecond_line_header(line) {
        let adjusted_start_ms = start_ms.saturating_add_signed(offset_ms);
        let words = if content.contains('(') {
            parse_marked_words(
                content,
                '(',
                ')',
                WordTimeOrigin::Absolute,
                adjusted_start_ms,
                offset_ms,
            )
        } else {
            parse_marked_words(
                content,
                '<',
                '>',
                WordTimeOrigin::Relative,
                adjusted_start_ms,
                offset_ms,
            )
        };
        return vec![ParsedTimedLine {
            start_ms: adjusted_start_ms,
            end_ms: Some(adjusted_start_ms.saturating_add(duration_ms)),
            text: strip_word_timing(content, &[('<', '>'), ('(', ')')])
                .trim()
                .to_owned(),
            words,
        }];
    }

    let (timestamps, content) = parse_lrc_timestamps(line, offset_ms);
    if timestamps.is_empty() {
        return Vec::new();
    }
    let text = strip_word_timing(content, &[('<', '>')]).trim().to_owned();
    timestamps
        .into_iter()
        .map(|start_ms| {
            let words = parse_marked_words(
                content,
                '<',
                '>',
                WordTimeOrigin::Relative,
                start_ms,
                offset_ms,
            );
            let end_ms = words.iter().map(|word| word.end_ms).max();
            ParsedTimedLine {
                start_ms,
                end_ms,
                text: text.clone(),
                words,
            }
        })
        .collect()
}

fn parse_millisecond_line_header(line: &str) -> Option<(u64, u64, &str)> {
    let after_open = line.strip_prefix('[')?;
    let close_index = after_open.find(']')?;
    let mut parts = after_open[..close_index].split(',');
    let start_ms = parts.next()?.trim().parse().ok()?;
    let duration_ms = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((start_ms, duration_ms, &after_open[close_index + 1..]))
}

fn parse_lrc_timestamps(line: &str, offset_ms: i64) -> (Vec<u64>, &str) {
    let mut timestamps = Vec::new();
    let mut remainder = line.trim_start();
    while let Some(after_open) = remainder.strip_prefix('[') {
        let Some(close_index) = after_open.find(']') else {
            break;
        };
        let tag = &after_open[..close_index];
        let Some(timestamp) = parse_timestamp(tag) else {
            break;
        };
        timestamps.push(timestamp.saturating_add_signed(offset_ms));
        remainder = &after_open[close_index + 1..];
    }
    (timestamps, remainder)
}

fn parse_marked_words(
    content: &str,
    open: char,
    close: char,
    origin: WordTimeOrigin,
    line_start_ms: u64,
    offset_ms: i64,
) -> Vec<LyricWord> {
    let markers = word_timing_markers(content, open, close);
    let Some(first_marker) = markers.first() else {
        return Vec::new();
    };
    let markers_precede_text = content[..first_marker.range_start].trim().is_empty();
    let mut words = markers
        .iter()
        .enumerate()
        .filter_map(|(index, marker)| {
            let text_range = if markers_precede_text {
                marker.range_end
                    ..markers
                        .get(index + 1)
                        .map_or(content.len(), |next| next.range_start)
            } else {
                let text_start = if index > 0 {
                    markers[index - 1].range_end
                } else {
                    0
                };
                text_start..marker.range_start
            };
            let text = content[text_range].to_owned();
            if text.is_empty() {
                return None;
            }
            let start_ms = match origin {
                WordTimeOrigin::Absolute => marker.start.saturating_add_signed(offset_ms),
                WordTimeOrigin::Relative => line_start_ms.saturating_add(marker.start),
            };
            Some(LyricWord {
                start_ms,
                end_ms: start_ms.saturating_add(marker.duration),
                text,
            })
        })
        .collect::<Vec<_>>();

    if !markers_precede_text {
        if let (Some(last_marker), Some(last_word)) = (markers.last(), words.last_mut()) {
            last_word.text.push_str(&content[last_marker.range_end..]);
        }
    }
    if let Some(first_word) = words.first_mut() {
        first_word.text = first_word.text.trim_start().to_owned();
    }
    if let Some(last_word) = words.last_mut() {
        last_word.text = last_word.text.trim_end().to_owned();
    }
    words.retain(|word| !word.text.is_empty());
    words
}

fn word_timing_markers(content: &str, open: char, close: char) -> Vec<WordTimingMarker> {
    let mut markers = Vec::new();
    let mut cursor = 0;
    while let Some(relative_open) = content[cursor..].find(open) {
        let range_start = cursor + relative_open;
        let values_start = range_start + open.len_utf8();
        let Some(relative_close) = content[values_start..].find(close) else {
            break;
        };
        let range_end = values_start + relative_close + close.len_utf8();
        let mut values = content[values_start..values_start + relative_close].split(',');
        if let (Some(start), Some(duration)) = (
            values.next().and_then(|value| value.trim().parse().ok()),
            values.next().and_then(|value| value.trim().parse().ok()),
        ) {
            markers.push(WordTimingMarker {
                start,
                duration,
                range_start,
                range_end,
            });
        }
        cursor = range_end;
    }
    markers
}

fn strip_word_timing(content: &str, delimiters: &[(char, char)]) -> String {
    let mut stripped = content.to_owned();
    for &(open, close) in delimiters {
        let markers = word_timing_markers(&stripped, open, close);
        for marker in markers.into_iter().rev() {
            stripped.replace_range(marker.range_start..marker.range_end, "");
        }
    }
    stripped
}

fn merge_timed_words(
    parsed_lines: &[ParsedTimedLine],
    line_start_ms: u64,
    line_end_ms: Option<u64>,
) -> Vec<LyricWord> {
    let mut words = Vec::new();
    for parsed_line in parsed_lines {
        if parsed_line.words.is_empty() {
            continue;
        }
        if !words.is_empty() {
            words.push(LyricWord {
                start_ms: line_start_ms,
                end_ms: line_end_ms.unwrap_or(line_start_ms),
                text: "\n".to_owned(),
            });
        }
        words.extend(parsed_line.words.iter().cloned());
    }
    words
}

fn set_line_end_times(lines: &mut [LyricLine]) {
    for index in 0..lines.len() {
        let Some(start_ms) = lines[index].start_ms else {
            continue;
        };
        let next_start_ms = lines.get(index + 1).and_then(|line| line.start_ms);
        let explicit_end_ms = lines[index].end_ms.filter(|end_ms| *end_ms > start_ms);
        lines[index].end_ms = match (explicit_end_ms, next_start_ms) {
            (Some(explicit), Some(next)) => Some(explicit.min(next).max(start_ms)),
            (Some(explicit), None) => Some(explicit),
            (None, Some(next)) => Some(next.max(start_ms)),
            (None, None) => None,
        };
    }
}

fn parse_timestamp(value: &str) -> Option<u64> {
    let parts = value.split(':').collect::<Vec<_>>();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (0_u64, minutes.parse::<u64>().ok()?, *seconds),
        [hours, minutes, seconds] => (
            hours.parse::<u64>().ok()?,
            minutes.parse::<u64>().ok()?,
            *seconds,
        ),
        _ => return None,
    };
    let (whole_seconds, fraction_ms) = parse_second_component(seconds)?;
    Some(
        hours
            .saturating_mul(3_600_000)
            .saturating_add(minutes.saturating_mul(60_000))
            .saturating_add(whole_seconds.saturating_mul(1_000))
            .saturating_add(fraction_ms),
    )
}

fn parse_second_component(value: &str) -> Option<(u64, u64)> {
    let mut parts = value.splitn(2, '.');
    let seconds = parts.next()?.parse().ok()?;
    let fraction = parts.next().unwrap_or_default();
    if fraction.is_empty() {
        return Some((seconds, 0));
    }
    if !fraction.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let digits = fraction.chars().take(3).collect::<String>();
    let value: u64 = digits.parse().ok()?;
    let milliseconds = match digits.len() {
        1 => value * 100,
        2 => value * 10,
        _ => value,
    };
    Some((seconds, milliseconds))
}

fn parse_offset(line: &str) -> Option<i64> {
    line.strip_prefix("[offset:")?
        .strip_suffix(']')?
        .trim()
        .parse()
        .ok()
}

fn is_metadata_line(line: &str) -> bool {
    const METADATA_PREFIXES: [&str; 11] = [
        "[ar:", "[al:", "[ti:", "[au:", "[by:", "[length:", "[offset:", "[re:", "[ve:", "[tool:",
        "[id:",
    ];
    let lowercase = line.to_ascii_lowercase();
    METADATA_PREFIXES
        .iter()
        .any(|prefix| lowercase.starts_with(prefix))
}

fn embedded_lyrics(tagged_file: &TaggedFile) -> Option<String> {
    [ItemKey::Lyrics, ItemKey::UnsyncLyrics]
        .into_iter()
        .find_map(|key| {
            tagged_file.tags().iter().find_map(|tag| {
                tag.get_strings(key)
                    .map(str::trim)
                    .find(|lyrics| !lyrics.is_empty())
                    .map(str::to_owned)
            })
        })
}

fn read_synchronized_id3_lyrics(path: &Path) -> Option<ResolvedLyrics> {
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp3"))
    {
        return None;
    }
    let mut file = fs::File::open(path).ok()?;
    let mpeg_file =
        MpegFile::read_from(&mut file, ParseOptions::new().read_properties(false)).ok()?;
    synchronized_id3_lyrics(mpeg_file.id3v2()?)
}

fn synchronized_id3_lyrics(tag: &Id3v2Tag) -> Option<ResolvedLyrics> {
    tag.into_iter()
        .filter_map(|frame| {
            let Frame::Binary(binary) = frame else {
                return None;
            };
            (binary.id().as_str() == "SYLT")
                .then(|| SynchronizedTextFrame::parse(&binary.data, binary.flags()).ok())
                .flatten()
        })
        .filter(|frame| frame.timestamp_format == TimestampFormat::MS)
        .filter(|frame| {
            matches!(
                frame.content_type,
                SyncTextContentType::Lyrics | SyncTextContentType::TextTranscription
            )
        })
        .find_map(|frame| {
            let mut timed_lines = BTreeMap::<u64, Vec<String>>::new();
            for (timestamp, text) in frame.content {
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                let texts = timed_lines.entry(u64::from(timestamp)).or_default();
                if !texts.iter().any(|candidate| candidate == text) {
                    texts.push(text.to_owned());
                }
            }
            (!timed_lines.is_empty()).then(|| ResolvedLyrics {
                source: LyricsSource::Embedded,
                provider: None,
                is_synced: true,
                lines: timed_lines
                    .into_iter()
                    .map(|(start_ms, texts)| LyricLine {
                        start_ms: Some(start_ms),
                        end_ms: None,
                        text: texts.join("\n"),
                        words: Vec::new(),
                    })
                    .collect(),
                saved_path: None,
                match_score: None,
            })
        })
}

fn read_sidecar_lyrics(audio_path: &Path) -> Result<Option<(PathBuf, String)>, String> {
    let Some(path) = find_sidecar_lyrics(audio_path) else {
        return Ok(None);
    };
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("failed to inspect lyrics file {}: {error}", path.display()))?;
    if metadata.len() > MAX_LYRICS_FILE_BYTES {
        return Err(format!(
            "lyrics file {} exceeds {} bytes",
            path.display(),
            MAX_LYRICS_FILE_BYTES
        ));
    }
    let bytes = fs::read(&path)
        .map_err(|error| format!("failed to read lyrics file {}: {error}", path.display()))?;
    let text = decode_text_file(&bytes);
    if text.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some((path, text)))
}

fn find_sidecar_lyrics(audio_path: &Path) -> Option<PathBuf> {
    let directory = audio_path.parent()?;
    let audio_stem = audio_path.file_stem()?.to_string_lossy().to_lowercase();
    let mut candidates = fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter_map(|path| {
            let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
            let extension_rank = match extension.as_str() {
                "lrc" => 0_u8,
                "txt" => 1_u8,
                _ => return None,
            };
            let candidate_stem = path.file_stem()?.to_string_lossy().to_lowercase();
            let name_rank = if candidate_stem == audio_stem {
                0_u8
            } else if candidate_stem
                .strip_prefix(&audio_stem)
                .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with(" -"))
            {
                1_u8
            } else {
                return None;
            };
            Some((name_rank, extension_rank, path))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        (left.0, left.1)
            .cmp(&(right.0, right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    candidates.into_iter().next().map(|(_, _, path)| path)
}

fn decode_text_file(bytes: &[u8]) -> String {
    if let Some(bytes) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(bytes, u16::from_le_bytes);
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(bytes, u16::from_be_bytes);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn decode_utf16(bytes: &[u8], decode: fn([u8; 2]) -> u16) -> String {
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| decode([chunk[0], chunk[1]]));
    char::decode_utf16(units)
        .map(|character| character.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn persist_downloaded_lyrics(audio_path: &Path, text: &str) -> Result<PathBuf, std::io::Error> {
    let target = audio_path.with_extension("lrc");
    if target.is_file() {
        return Ok(target);
    }
    let temporary = target.with_extension(format!("lrc.{}.tmp", Uuid::new_v4()));
    fs::write(&temporary, text)?;
    match fs::rename(&temporary, &target) {
        Ok(()) => Ok(target),
        Err(_error) if target.is_file() => {
            let _ = fs::remove_file(temporary);
            Ok(target)
        }
        Err(error) => {
            let _ = fs::remove_file(temporary);
            Err(error)
        }
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::ZlibEncoder, Compression};
    use lofty::config::WriteOptions;
    use lofty::id3::v2::{BinaryFrame, FrameId};
    use lofty::TextEncoding;
    use serde_json::json;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

    static NEXT_TEST_DIR_ID: AtomicU64 = AtomicU64::new(0);

    fn query() -> TrackLyricsQuery {
        TrackLyricsQuery::new(
            "Song Title".to_owned(),
            Some("Main Artist".to_owned()),
            Some("Album".to_owned()),
            Some(180),
        )
    }

    fn record(title: &str, artist: &str, duration: f64) -> LrclibRecord {
        LrclibRecord {
            id: 1,
            track_name: title.to_owned(),
            artist_name: artist.to_owned(),
            album_name: "Album".to_owned(),
            duration,
            instrumental: false,
            plain_lyrics: Some("Plain lyrics".to_owned()),
            synced_lyrics: None,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let id = NEXT_TEST_DIR_ID.fetch_add(1, AtomicOrdering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("fika-lyrics-{name}-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("test directory should be created");
        path
    }

    #[test]
    fn parse_lyrics_should_parse_multiple_timestamps_and_apply_offset() {
        let lyrics = "[offset:250]\n[00:01.20][00:03.000]First\n[00:05]Second";

        let lines = parse_lyrics(lyrics);

        assert_eq!(
            lines,
            vec![
                LyricLine {
                    start_ms: Some(1_450),
                    end_ms: Some(3_250),
                    text: "First".to_owned(),
                    words: Vec::new(),
                },
                LyricLine {
                    start_ms: Some(3_250),
                    end_ms: Some(5_250),
                    text: "First".to_owned(),
                    words: Vec::new(),
                },
                LyricLine {
                    start_ms: Some(5_250),
                    end_ms: None,
                    text: "Second".to_owned(),
                    words: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn parse_lyrics_should_merge_translation_lines_at_the_same_timestamp() {
        let lines = parse_lyrics("[00:01.00]Original\n[00:01.00]Translation");

        assert_eq!(lines[0].text, "Original\nTranslation");
    }

    #[test]
    fn parse_lyrics_should_not_assign_source_timing_to_untimed_translation() {
        let lines =
            parse_lyrics("[00:01.00]<0,500>Original\n[00:01.00]Translation\n[00:02.00]Next");

        assert_eq!(lines[0].text, "Original\nTranslation");
        assert_eq!(
            lines[0]
                .words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<String>(),
            "Original"
        );
    }

    #[test]
    fn parse_lyrics_should_parse_enhanced_lrc_word_timings() {
        let lines = parse_lyrics("[00:01.000]<0,400>Hello <400,600>world\n[00:02.000]<0,500>Next");

        assert_eq!(lines[0].text, "Hello world");
        assert_eq!(
            lines[0].words,
            vec![
                LyricWord {
                    start_ms: 1_000,
                    end_ms: 1_400,
                    text: "Hello ".to_owned(),
                },
                LyricWord {
                    start_ms: 1_400,
                    end_ms: 2_000,
                    text: "world".to_owned(),
                },
            ]
        );
        assert_eq!(lines[0].end_ms, Some(2_000));
    }

    #[test]
    fn parse_lyrics_should_parse_yrc_absolute_word_timings() {
        let lines = parse_lyrics(
            "[1000,1000](1000,400,0)Coffee (1400,600,0)cools\n[2000,500](2000,500,0)Next",
        );

        assert_eq!(lines[0].text, "Coffee cools");
        assert_eq!(lines[0].words[0].start_ms, 1_000);
        assert_eq!(lines[0].words[0].end_ms, 1_400);
        assert_eq!(lines[0].words[1].text, "cools");
        assert_eq!(lines[0].words[1].end_ms, 2_000);
    }

    #[test]
    fn parse_lyrics_should_parse_qrc_trailing_word_timings() {
        let lines =
            parse_lyrics("[1000,1000]Coffee (1000,400)cools(1400,600)\n[2000,500]Next(2000,500)");

        assert_eq!(lines[0].text, "Coffee cools");
        assert_eq!(lines[0].words[0].text, "Coffee ");
        assert_eq!(lines[0].words[0].start_ms, 1_000);
        assert_eq!(lines[0].words[1].text, "cools");
        assert_eq!(lines[0].words[1].end_ms, 2_000);
    }

    #[test]
    fn parse_lyrics_should_extract_qrc_xml_content_and_entities() {
        let lines = parse_lyrics(
            r#"<?xml version="1.0"?><QrcInfos><LyricInfo LyricContent="[1000,1000]A &amp; B(1000,1000)&#10;[2000,500]Next(2000,500)"/></QrcInfos>"#,
        );

        assert_eq!(lines[0].text, "A & B");
        assert_eq!(lines[0].words[0].text, "A & B");
        assert_eq!(lines[0].words[0].end_ms, 2_000);
    }

    #[test]
    fn parse_lyrics_should_preserve_non_timing_parentheses() {
        let lines = parse_lyrics("[00:01.00]Song (live)\n[00:02.00]Next");

        assert_eq!(lines[0].text, "Song (live)");
        assert!(lines[0].words.is_empty());
    }

    #[test]
    fn parse_lyrics_should_parse_krc_relative_word_timings() {
        let lines =
            parse_lyrics("[1000,1000]<0,400,0>Coffee <400,600,0>cools\n[2000,500]<0,500,0>Next");

        assert_eq!(lines[0].text, "Coffee cools");
        assert_eq!(lines[0].words[0].start_ms, 1_000);
        assert_eq!(lines[0].words[1].start_ms, 1_400);
        assert_eq!(lines[0].words[1].end_ms, 2_000);
    }

    #[test]
    fn decode_kugou_lyric_should_decrypt_and_inflate_krc_payloads() {
        let original = b"[1000,500]<0,500,0>Line";
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(original)
            .expect("KRC fixture should compress");
        let compressed = encoder.finish().expect("KRC fixture should finish");
        let encrypted = compressed
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ KUGOU_KRC_XOR_KEY[index % KUGOU_KRC_XOR_KEY.len()]);
        let payload = b"krc1".iter().copied().chain(encrypted).collect::<Vec<_>>();

        assert_eq!(
            decode_kugou_lyric(&payload).expect("KRC fixture should decode"),
            String::from_utf8_lossy(original)
        );
    }

    #[test]
    fn parse_lyrics_should_keep_plain_lines_and_ignore_metadata() {
        let lines = parse_lyrics("[ar:Artist]\nFirst line\nSecond line");

        assert_eq!(
            lines,
            vec![
                LyricLine {
                    start_ms: None,
                    end_ms: None,
                    text: "First line".to_owned(),
                    words: Vec::new(),
                },
                LyricLine {
                    start_ms: None,
                    end_ms: None,
                    text: "Second line".to_owned(),
                    words: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn synchronized_id3_lyrics_should_parse_millisecond_sylt_frames() {
        let synchronized_text = SynchronizedTextFrame::new(
            TextEncoding::UTF8,
            *b"eng",
            TimestampFormat::MS,
            SyncTextContentType::Lyrics,
            None,
            vec![
                (1_000, "First line".to_owned()),
                (3_250, "Second line".to_owned()),
            ],
        );
        let bytes = synchronized_text
            .as_bytes(WriteOptions::default())
            .expect("SYLT fixture should serialize");
        let mut tag = Id3v2Tag::new();
        let _ = tag.insert(Frame::Binary(BinaryFrame::new(
            FrameId::new("SYLT").expect("SYLT should be a valid frame id"),
            bytes,
        )));

        let lyrics = synchronized_id3_lyrics(&tag).expect("SYLT lyrics should resolve");

        assert_eq!(
            lyrics.lines,
            vec![
                LyricLine {
                    start_ms: Some(1_000),
                    end_ms: None,
                    text: "First line".to_owned(),
                    words: Vec::new(),
                },
                LyricLine {
                    start_ms: Some(3_250),
                    end_ms: None,
                    text: "Second line".to_owned(),
                    words: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn parse_netease_candidates_should_normalize_song_metadata() {
        let body = json!({
            "result": {
                "songs": [{
                    "id": 338943,
                    "name": "Song Title",
                    "ar": [{"name": "Main Artist"}],
                    "al": {"name": "Album"},
                    "dt": 180000
                }]
            }
        });

        let candidates = parse_netease_candidates(&body);

        assert_eq!(
            candidates,
            vec![ProviderCandidate {
                id: "338943".to_owned(),
                title: "Song Title".to_owned(),
                artist: "Main Artist".to_owned(),
                album: "Album".to_owned(),
                duration_seconds: Some(180.0),
            }]
        );
    }

    #[test]
    fn netease_lyric_text_should_prefer_word_timed_lyrics() {
        let body = json!({
            "yrc": {"lyric": "[1000,500](1000,500,0)Timed"},
            "lrc": {"lyric": "[00:01.00]Plain"},
            "tlyric": {"lyric": "[00:01.00]Translation"}
        });

        let text = netease_lyric_text(&body).expect("lyrics should resolve");

        assert!(text.contains("Timed"));
        assert!(!text.contains("Plain"));
        assert!(text.contains("Translation"));
    }

    #[test]
    fn parse_qq_candidates_should_use_song_mid_for_lyric_lookup() {
        let body = json!({
            "req_1": {
                "data": {
                    "body": {
                        "song": {
                            "list": [{
                                "id": 449205,
                                "mid": "003aAYrm3GE0Ac",
                                "title": "Song Title",
                                "interval": 180,
                                "singer": [{"name": "Main Artist"}],
                                "album": {"name": "Album"}
                            }]
                        }
                    }
                }
            }
        });

        let candidates = parse_qq_candidates(&body);

        assert_eq!(candidates[0].id, "003aAYrm3GE0Ac");
    }

    #[test]
    fn parse_kugou_candidates_should_strip_artist_prefix_and_markup() {
        let body = json!({
            "status": 1,
            "lists": [{
                "FileHash": "ABC123",
                "FileName": "<em>Main Artist</em> - <em>Song Title</em>",
                "SingerName": "Main Artist",
                "AlbumName": "Album",
                "Duration": 180
            }]
        });

        let candidates = parse_kugou_candidates(&body);

        assert_eq!(candidates[0].title, "Song Title");
    }

    #[test]
    fn decode_base64_lyric_should_accept_qq_lrc_payloads() {
        let encoded = BASE64_STANDARD.encode("[00:01.00]First line");

        let decoded = decode_base64_lyric(Some(&Value::String(encoded)));

        assert_eq!(decoded.as_deref(), Some("[00:01.00]First line"));
    }

    #[test]
    fn looks_like_lyric_text_should_reject_encrypted_qq_qrc_payloads() {
        assert!(!looks_like_lyric_text(
            "F5AF80D82C4F4BA241E179EB82BC410A27430E82B5546E6E"
        ));
        assert!(looks_like_lyric_text(
            "<QrcInfos><LyricInfo LyricContent=\"[1000,500]Line(1000,500)\"/></QrcInfos>"
        ));
        assert!(looks_like_lyric_text("[00:01.00]Line"));
    }

    #[test]
    fn rank_provider_candidates_should_reject_duration_mismatches() {
        let candidates = vec![ProviderCandidate {
            id: "1".to_owned(),
            title: "Song Title".to_owned(),
            artist: "Main Artist".to_owned(),
            album: "Album".to_owned(),
            duration_seconds: Some(185.0),
        }];

        let ranked = rank_provider_candidates(&query(), &candidates);

        assert!(ranked.is_empty());
    }

    #[test]
    fn resolve_provider_candidates_should_try_the_next_match_after_fetch_failure() {
        let candidates = vec![
            ProviderCandidate {
                id: "first".to_owned(),
                title: "Song Title".to_owned(),
                artist: "Main Artist".to_owned(),
                album: "Album".to_owned(),
                duration_seconds: Some(180.0),
            },
            ProviderCandidate {
                id: "second".to_owned(),
                title: "Song Title".to_owned(),
                artist: "Main Artist".to_owned(),
                album: "Album".to_owned(),
                duration_seconds: Some(180.0),
            },
        ];

        let resolved = resolve_provider_candidates("Test", &query(), candidates, |candidate| {
            if candidate.id == "first" {
                Err(LyricsError::ProviderFailures(
                    "temporary failure".to_owned(),
                ))
            } else {
                Ok(Some("[00:01.00]Resolved".to_owned()))
            }
        })
        .expect("the second candidate should recover the provider result");

        assert_eq!(
            resolved.map(|lyrics| lyrics.provider),
            Some("Test #second".to_owned())
        );
    }

    #[test]
    fn score_record_should_reject_duration_differences_over_four_seconds() {
        let score = score_record(&query(), &record("Song Title", "Main Artist", 184.1));

        assert!(score.is_none());
    }

    #[test]
    fn select_record_should_prefer_synced_lyrics_within_the_score_window() {
        let best_plain = record("Song Title", "Main Artist", 180.0);
        let mut slightly_weaker_synced = record("Song Title (Live)", "Main Artist", 180.0);
        slightly_weaker_synced.id = 2;
        slightly_weaker_synced.synced_lyrics = Some("[00:01.00]Synced".to_owned());

        let records = [best_plain, slightly_weaker_synced];
        let (selected, _) = select_record(&query(), &records).expect("a candidate should match");

        assert_eq!(selected.id, 2);
    }

    #[test]
    fn find_sidecar_lyrics_should_prefer_exact_lrc_over_decorated_or_text_files() {
        let root = temp_dir("sidecar-priority");
        let audio = root.join("Track.mp3");
        fs::write(&audio, []).expect("audio placeholder should be written");
        fs::write(root.join("Track.zh.lrc"), "decorated").expect("decorated lyrics should write");
        fs::write(root.join("Track.txt"), "plain").expect("text lyrics should write");
        fs::write(root.join("track.LRC"), "exact").expect("exact lyrics should write");

        let selected = find_sidecar_lyrics(&audio).expect("lyrics should be found");

        fs::remove_dir_all(root).expect("test directory should be removed");
        assert_eq!(
            selected.file_name().and_then(|name| name.to_str()),
            Some("track.LRC")
        );
    }

    #[test]
    fn decode_text_file_should_support_utf16_little_endian_bom() {
        let bytes = [0xFF, 0xFE, b'A', 0, b'B', 0];

        assert_eq!(decode_text_file(&bytes), "AB");
    }

    #[test]
    fn normalize_text_should_unify_full_width_symbols_and_whitespace() {
        assert_eq!(normalize_text("  Song（Live）\tMix  "), "song(live) mix");
    }
}
