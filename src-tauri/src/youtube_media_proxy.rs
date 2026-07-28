use percent_encoding::percent_decode_str;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tauri::http::header::{
    ACCEPT_RANGES, ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE,
    CONTENT_TYPE, RANGE,
};
use tauri::http::{Method, Request, Response, StatusCode};
use url::Url;

pub const YOUTUBE_MEDIA_PROTOCOL: &str = "fika-media";

const MAX_ENCODED_TARGET_BYTES: usize = 32 * 1024;
const MAX_TARGET_URL_BYTES: usize = 8 * 1024;
const MAX_MEDIA_CHUNK_BYTES: usize = 1024 * 1024;
const MAX_MEDIA_SESSIONS: usize = 128;
const MEDIA_SESSION_TTL: Duration = Duration::from_secs(30 * 60);
const UPSTREAM_ATTEMPTS: usize = 2;

static MEDIA_CLIENT: LazyLock<Result<Client, String>> = LazyLock::new(|| {
    Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("FikaMusic/0.1 YouTubeMediaProxy")
        .build()
        .map_err(|_| "could not initialize the media client".to_owned())
});
static MEDIA_SESSIONS: LazyLock<Mutex<BTreeMap<String, MediaSession>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

#[derive(Clone)]
struct MediaSession {
    headers: HeaderMap,
    expires_at: Instant,
}

#[derive(Debug, thiserror::Error)]
enum MediaProxyError {
    #[error("media proxy accepts only GET requests")]
    MethodNotAllowed,
    #[error("media proxy target is invalid")]
    InvalidTarget,
    #[error("media proxy target is not an allowed YouTube media URL")]
    ForbiddenTarget,
    #[error("media proxy range is invalid")]
    InvalidRange,
    #[error("media upstream connection failed")]
    Network,
    #[error("media upstream returned HTTP {0}")]
    UpstreamStatus(u16),
    #[error("media upstream did not return audio content")]
    InvalidContentType,
    #[error("media upstream response exceeded the chunk limit")]
    ResponseTooLarge,
}

impl MediaProxyError {
    const fn status(&self) -> StatusCode {
        match self {
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::InvalidTarget => StatusCode::BAD_REQUEST,
            Self::ForbiddenTarget => StatusCode::FORBIDDEN,
            Self::InvalidRange => StatusCode::RANGE_NOT_SATISFIABLE,
            Self::Network
            | Self::UpstreamStatus(_)
            | Self::InvalidContentType
            | Self::ResponseTooLarge => StatusCode::BAD_GATEWAY,
        }
    }
}

#[derive(Clone)]
struct MediaChunk {
    status: StatusCode,
    content_type: String,
    content_range: Option<String>,
    body: Vec<u8>,
}

trait MediaFetcher {
    fn fetch(
        &self,
        target: &Url,
        range: &str,
        headers: &HeaderMap,
    ) -> Result<MediaChunk, MediaProxyError>;
}

struct ReqwestMediaFetcher;

impl MediaFetcher for ReqwestMediaFetcher {
    fn fetch(
        &self,
        target: &Url,
        range: &str,
        headers: &HeaderMap,
    ) -> Result<MediaChunk, MediaProxyError> {
        let client = MEDIA_CLIENT
            .as_ref()
            .map_err(|_| MediaProxyError::Network)?;
        let mut last_network_error = None;
        for _ in 0..UPSTREAM_ATTEMPTS {
            match fetch_media_chunk(client, target, range, headers) {
                Ok(chunk) => return Ok(chunk),
                Err(MediaProxyError::Network) => {
                    last_network_error = Some(MediaProxyError::Network);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_network_error.unwrap_or(MediaProxyError::Network))
    }
}

pub fn protocol_response(request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    protocol_response_with(&ReqwestMediaFetcher, request)
}

fn protocol_response_with(
    fetcher: &impl MediaFetcher,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    match proxy_media(fetcher, &request) {
        Ok(chunk) => media_response(chunk),
        Err(error) => error_response(error),
    }
}

fn proxy_media(
    fetcher: &impl MediaFetcher,
    request: &Request<Vec<u8>>,
) -> Result<MediaChunk, MediaProxyError> {
    if request.method() != Method::GET {
        return Err(MediaProxyError::MethodNotAllowed);
    }
    let target = target_url(request)?;
    let range = bounded_range(request)?;
    let headers = registered_headers(target.as_str()).unwrap_or_default();
    fetcher.fetch(&target, &range, &headers)
}

fn target_url(request: &Request<Vec<u8>>) -> Result<Url, MediaProxyError> {
    let encoded = request
        .uri()
        .path()
        .strip_prefix('/')
        .ok_or(MediaProxyError::InvalidTarget)?;
    if encoded.is_empty() || encoded.len() > MAX_ENCODED_TARGET_BYTES {
        return Err(MediaProxyError::InvalidTarget);
    }
    let decoded = percent_decode_str(encoded)
        .decode_utf8()
        .map_err(|_| MediaProxyError::InvalidTarget)?;
    if decoded.len() > MAX_TARGET_URL_BYTES {
        return Err(MediaProxyError::InvalidTarget);
    }
    parse_allowed_target(&decoded)
}

fn parse_allowed_target(target: &str) -> Result<Url, MediaProxyError> {
    let target = Url::parse(target).map_err(|_| MediaProxyError::InvalidTarget)?;
    let host = target
        .host_str()
        .map(str::to_ascii_lowercase)
        .ok_or(MediaProxyError::ForbiddenTarget)?;
    let allowed_host = host == "googlevideo.com" || host.ends_with(".googlevideo.com");
    let default_port = target.port().is_none() || target.port() == Some(443);
    if target.scheme() != "https"
        || !allowed_host
        || !default_port
        || !target.username().is_empty()
        || target.password().is_some()
        || target.path() != "/videoplayback"
    {
        return Err(MediaProxyError::ForbiddenTarget);
    }
    Ok(target)
}

pub(crate) fn is_allowed_target(target: &str) -> bool {
    parse_allowed_target(target).is_ok()
}

pub(crate) fn register_media_headers(target: &str, supplied: &BTreeMap<String, String>) -> bool {
    let Ok(target) = parse_allowed_target(target) else {
        return false;
    };
    let mut headers = HeaderMap::new();
    for (name, value) in supplied {
        if headers.len() >= 16 {
            break;
        }
        let Some(name) = allowed_media_header(name) else {
            continue;
        };
        if value.len() > 4 * 1024 {
            continue;
        }
        let Ok(value) = HeaderValue::from_str(value) else {
            continue;
        };
        headers.insert(name, value);
    }
    let Ok(mut sessions) = MEDIA_SESSIONS.lock() else {
        return false;
    };
    let now = Instant::now();
    sessions.retain(|_, session| session.expires_at > now);
    if sessions.len() >= MAX_MEDIA_SESSIONS {
        let oldest = sessions
            .iter()
            .min_by_key(|(_, session)| session.expires_at)
            .map(|(url, _)| url.clone());
        if let Some(oldest) = oldest {
            sessions.remove(&oldest);
        }
    }
    sessions.insert(
        target.to_string(),
        MediaSession {
            headers,
            expires_at: now + MEDIA_SESSION_TTL,
        },
    );
    true
}

pub(crate) fn registered_headers(target: &str) -> Option<HeaderMap> {
    let target = parse_allowed_target(target).ok()?;
    let mut sessions = MEDIA_SESSIONS.lock().ok()?;
    let now = Instant::now();
    sessions.retain(|_, session| session.expires_at > now);
    sessions.get_mut(target.as_str()).map(|session| {
        session.expires_at = now + MEDIA_SESSION_TTL;
        session.headers.clone()
    })
}

fn allowed_media_header(name: &str) -> Option<HeaderName> {
    match name.to_ascii_lowercase().as_str() {
        "accept" => Some(reqwest::header::ACCEPT),
        "accept-language" => Some(reqwest::header::ACCEPT_LANGUAGE),
        "origin" => Some(reqwest::header::ORIGIN),
        "referer" => Some(reqwest::header::REFERER),
        "sec-fetch-mode" => Some(HeaderName::from_static("sec-fetch-mode")),
        "user-agent" => Some(reqwest::header::USER_AGENT),
        _ => None,
    }
}

fn bounded_range(request: &Request<Vec<u8>>) -> Result<String, MediaProxyError> {
    let Some(value) = request.headers().get(RANGE) else {
        return Ok(format!("bytes=0-{}", MAX_MEDIA_CHUNK_BYTES - 1));
    };
    let value = value.to_str().map_err(|_| MediaProxyError::InvalidRange)?;
    let range = value
        .strip_prefix("bytes=")
        .filter(|range| !range.contains(','))
        .ok_or(MediaProxyError::InvalidRange)?;
    let (start, end) = range.split_once('-').ok_or(MediaProxyError::InvalidRange)?;
    if start.is_empty() {
        let length = end
            .parse::<usize>()
            .map_err(|_| MediaProxyError::InvalidRange)?
            .clamp(1, MAX_MEDIA_CHUNK_BYTES);
        return Ok(format!("bytes=-{length}"));
    }
    let start = start
        .parse::<u64>()
        .map_err(|_| MediaProxyError::InvalidRange)?;
    let maximum_end = start.saturating_add(MAX_MEDIA_CHUNK_BYTES as u64 - 1);
    let end = if end.is_empty() {
        maximum_end
    } else {
        end.parse::<u64>()
            .map_err(|_| MediaProxyError::InvalidRange)?
            .min(maximum_end)
    };
    if end < start {
        return Err(MediaProxyError::InvalidRange);
    }
    Ok(format!("bytes={start}-{end}"))
}

fn fetch_media_chunk(
    client: &Client,
    target: &Url,
    range: &str,
    headers: &HeaderMap,
) -> Result<MediaChunk, MediaProxyError> {
    let response = client
        .get(target.clone())
        .header(RANGE, range)
        .headers(headers.clone())
        .send()
        .map_err(|_| MediaProxyError::Network)?;
    let status = response.status();
    if status != StatusCode::PARTIAL_CONTENT {
        return Err(MediaProxyError::UpstreamStatus(status.as_u16()));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.to_ascii_lowercase().starts_with("audio/"))
        .map(str::to_owned)
        .ok_or(MediaProxyError::InvalidContentType)?;
    let content_range = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or(MediaProxyError::InvalidRange)?;
    let mut body = Vec::with_capacity(MAX_MEDIA_CHUNK_BYTES.min(64 * 1024));
    response
        .take(MAX_MEDIA_CHUNK_BYTES as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|_| MediaProxyError::Network)?;
    if body.len() > MAX_MEDIA_CHUNK_BYTES {
        return Err(MediaProxyError::ResponseTooLarge);
    }
    Ok(MediaChunk {
        status,
        content_type,
        content_range: Some(content_range),
        body,
    })
}

fn media_response(chunk: MediaChunk) -> Response<Vec<u8>> {
    let mut builder = Response::builder()
        .status(chunk.status)
        .header(CONTENT_TYPE, chunk.content_type)
        .header(CONTENT_LENGTH, chunk.body.len())
        .header(ACCEPT_RANGES, "bytes")
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header("Access-Control-Expose-Headers", "Content-Range")
        .header("Cross-Origin-Resource-Policy", "cross-origin")
        .header(CACHE_CONTROL, "private, no-store");
    if let Some(content_range) = chunk.content_range {
        builder = builder.header(CONTENT_RANGE, content_range);
    }
    builder
        .body(chunk.body)
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

fn error_response(error: MediaProxyError) -> Response<Vec<u8>> {
    let status = error.status();
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header("Cross-Origin-Resource-Policy", "cross-origin")
        .header(CACHE_CONTROL, "no-store")
        .body(error.to_string().into_bytes())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    use std::sync::Mutex;

    struct StubFetcher {
        requested_range: Mutex<Option<String>>,
        requested_headers: Mutex<HeaderMap>,
    }

    impl StubFetcher {
        fn new() -> Self {
            Self {
                requested_range: Mutex::new(None),
                requested_headers: Mutex::new(HeaderMap::new()),
            }
        }
    }

    impl MediaFetcher for StubFetcher {
        fn fetch(
            &self,
            _target: &Url,
            range: &str,
            headers: &HeaderMap,
        ) -> Result<MediaChunk, MediaProxyError> {
            *self.requested_range.lock().expect("range lock should open") = Some(range.to_owned());
            *self
                .requested_headers
                .lock()
                .expect("header lock should open") = headers.clone();
            Ok(MediaChunk {
                status: StatusCode::PARTIAL_CONTENT,
                content_type: "audio/mp4".to_owned(),
                content_range: Some("bytes 10-12/100".to_owned()),
                body: vec![1, 2, 3],
            })
        }
    }

    fn proxy_request(target: &str, range: Option<&str>) -> Request<Vec<u8>> {
        let encoded = utf8_percent_encode(target, NON_ALPHANUMERIC);
        let mut builder = Request::builder()
            .method(Method::GET)
            .uri(format!("{YOUTUBE_MEDIA_PROTOCOL}://localhost/{encoded}"));
        if let Some(range) = range {
            builder = builder.header(RANGE, range);
        }
        builder.body(Vec::new()).expect("request should build")
    }

    #[test]
    fn proxy_should_reject_a_googlevideo_prefix_attack() {
        let fetcher = StubFetcher::new();
        let request = proxy_request(
            "https://googlevideo.com.attacker.test/videoplayback?id=1",
            None,
        );

        let response = protocol_response_with(&fetcher, request);

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn proxy_should_bound_an_open_ended_media_range() {
        let fetcher = StubFetcher::new();
        let request = proxy_request(
            "https://rr5---sn.example.googlevideo.com/videoplayback?id=1",
            Some("bytes=10-"),
        );

        let response = protocol_response_with(&fetcher, request);

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            fetcher
                .requested_range
                .into_inner()
                .expect("range lock should open")
                .as_deref(),
            Some("bytes=10-1048585")
        );
    }

    #[test]
    fn proxy_should_preserve_audio_range_response_headers() {
        let fetcher = StubFetcher::new();
        let request = proxy_request(
            "https://rr5---sn.example.googlevideo.com/videoplayback?id=1",
            Some("bytes=10-12"),
        );

        let response = protocol_response_with(&fetcher, request);

        assert_eq!(response.headers()[CONTENT_TYPE], "audio/mp4");
        assert_eq!(response.headers()[CONTENT_RANGE], "bytes 10-12/100");
        assert_eq!(response.body(), &[1, 2, 3]);
    }

    #[test]
    fn proxy_should_forward_registered_yt_dlp_request_headers() {
        let registered_target =
            "https://RR5---SN.EXAMPLE.GOOGLEVIDEO.COM:443/videoplayback?id=headers";
        let requested_target = "https://rr5---sn.example.googlevideo.com/videoplayback?id=headers";
        assert!(register_media_headers(
            registered_target,
            &BTreeMap::from([
                ("User-Agent".to_owned(), "yt-dlp test agent".to_owned()),
                ("Cookie".to_owned(), "must-not-forward=1".to_owned()),
            ]),
        ));
        let fetcher = StubFetcher::new();

        let response = protocol_response_with(&fetcher, proxy_request(requested_target, None));

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        let headers = fetcher
            .requested_headers
            .into_inner()
            .expect("header lock should open");
        assert_eq!(headers[reqwest::header::USER_AGENT], "yt-dlp test agent");
        assert!(!headers.contains_key(reqwest::header::COOKIE));
    }
}
