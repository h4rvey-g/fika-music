//! Login request behavior from api-enhanced PRs #201 and #243.
//! Keep this adapter separate from the pinned crate's music-query APIs.

use super::{
    new_client, ApiResponse, NeteaseBridgeError, NeteaseMusicClient, API_TIMEOUT,
    MAX_API_RESPONSE_BYTES,
};
use aes::Aes128;
use cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyInit};
use flate2::read::GzDecoder;
use netease_music::{eapi_params, weapi_params, Cookie};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use rand::Rng;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, COOKIE, SET_COOKIE};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const MOBILE_HOST: &str = "https://interface3.music.163.com";
const WEB_HOST: &str = "https://music.163.com";
const IOS_APPVER: &str = "9.5.37";
const IOS_OSVER: &str = "18.7.2";
const IOS_BUILDVER: &str = "7010";
const IOS_UA: &str = "neteasemusic/9.5.37 (iPhone; iOS 18.7.2; Scale/3.00)";
const WEB_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:152.0) Gecko/20100101 Firefox/152.0";
const EAPI_KEY: &[u8; 16] = b"e82ckenh8dichen8";
const WEB_TOKEN_CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ-_";
const COOKIE_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoginMode {
    Web,
    Mobile,
}

#[derive(Clone)]
pub(super) struct LoginClient {
    api: NeteaseMusicClient,
    http: Client,
    mode: LoginMode,
    nmtid_probes: Arc<AtomicUsize>,
    #[cfg(test)]
    test_host: Option<String>,
}

impl fmt::Debug for LoginClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LoginClient(<redacted>)")
    }
}

impl LoginClient {
    pub(super) fn web() -> Result<Self, NeteaseBridgeError> {
        Self::new(LoginMode::Web)
    }

    pub(super) fn mobile() -> Result<Self, NeteaseBridgeError> {
        Self::new(LoginMode::Mobile)
    }

    fn new(mode: LoginMode) -> Result<Self, NeteaseBridgeError> {
        let api = new_client()?;
        let http = Client::builder()
            .timeout(API_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| {
                NeteaseBridgeError::Bridge("create login HTTP client failed".to_owned())
            })?;
        let client = Self {
            api,
            http,
            mode,
            nmtid_probes: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            test_host: None,
        };
        client.prepare_cookies();
        Ok(client)
    }

    fn prepare_cookies(&self) {
        let nuid = uuid::Uuid::new_v4().simple().to_string();
        for (name, value) in [
            ("_ntes_nuid", nuid.clone()),
            ("_ntes_nnid", format!("{nuid},{}", now_millis())),
            (
                "WNMCID",
                format!(
                    "{}.{}.01.0",
                    random_chars(b"abcdefghijklmnopqrstuvwxyz", 6),
                    now_millis()
                ),
            ),
            ("WEVNSM", "1.0.0".to_owned()),
            ("deviceId", uuid::Uuid::new_v4().simple().to_string()),
        ] {
            self.api.set_cookie(name, value);
        }
        match self.mode {
            LoginMode::Web => {
                self.api
                    .set_cookie("JSESSIONID-WYYY", random_chars(WEB_TOKEN_CHARS, 190));
                self.api.set_cookie("_iuqxldmzr_", "33");
                self.api
                    .set_cookie("NMTID", format!("00{}", random_chars(WEB_TOKEN_CHARS, 39)));
            }
            LoginMode::Mobile => {
                for (name, value) in [
                    ("os", "iPhone OS"),
                    ("osver", IOS_OSVER),
                    ("appver", IOS_APPVER),
                    ("buildver", IOS_BUILDVER),
                    ("channel", "distribution"),
                ] {
                    self.api.set_cookie(name, value);
                }
                // PR #243: the first eapi requests must omit NMTID so the server can issue it.
            }
        }
    }

    pub(super) fn api(&self) -> &NeteaseMusicClient {
        &self.api
    }

    #[cfg(test)]
    pub(super) fn with_test_host(mut self, host: String) -> Self {
        self.test_host = Some(host);
        self
    }

    pub(super) fn account(&self) -> Result<ApiResponse, NeteaseBridgeError> {
        self.post(
            WEB_HOST,
            "/api/w/nuser/account/get",
            json!({}),
            None,
            false,
            "verify login account",
        )
    }

    pub(super) fn chain_id(&self) -> String {
        self.api.chain_id()
    }

    pub(super) fn start_qr(&self) -> Result<ApiResponse, NeteaseBridgeError> {
        self.request(
            "/api/login/qrcode/unikey",
            web_key_data(),
            None,
            "start QR login",
        )
    }

    pub(super) fn poll_qr(
        &self,
        key: &str,
        chain_id: &str,
        secure_captcha: Option<&str>,
        yd_device_token: Option<&str>,
    ) -> Result<ApiResponse, NeteaseBridgeError> {
        let data = web_poll_data(key, secure_captcha, yd_device_token)?;
        self.request(
            "/api/login/qrcode/client/login",
            data,
            Some(chain_id),
            "poll QR login",
        )
    }

    pub(super) fn send_code(&self, phone: &str) -> Result<ApiResponse, NeteaseBridgeError> {
        self.request(
            "/api/sms/captcha/sent",
            mobile_captcha_data(phone),
            None,
            "send verification code",
        )
    }

    pub(super) fn complete_phone(
        &self,
        phone: &str,
        code: &str,
    ) -> Result<ApiResponse, NeteaseBridgeError> {
        self.request(
            "/api/login/cellphone",
            mobile_login_data(phone, code),
            None,
            "log in with verification code",
        )
    }

    pub(super) fn verification_qr(&self, data: Value) -> Result<ApiResponse, NeteaseBridgeError> {
        // Front-risk verification is a web endpoint even for a mobile login session.
        self.post(
            WEB_HOST,
            "/api/frontrisk/verify/getqrcode",
            data,
            None,
            false,
            "create security verification QR code",
        )
    }

    fn request(
        &self,
        path: &str,
        data: Value,
        chain_id: Option<&str>,
        operation: &'static str,
    ) -> Result<ApiResponse, NeteaseBridgeError> {
        let mobile = self.mode == LoginMode::Mobile;
        self.post(
            if mobile { MOBILE_HOST } else { WEB_HOST },
            path,
            data,
            chain_id,
            mobile,
            operation,
        )
    }

    fn mobile_header(&self) -> BTreeMap<String, String> {
        let mut header = BTreeMap::from([
            ("os".to_owned(), "iPhone OS".to_owned()),
            ("osver".to_owned(), IOS_OSVER.to_owned()),
            ("appver".to_owned(), IOS_APPVER.to_owned()),
            ("buildver".to_owned(), IOS_BUILDVER.to_owned()),
            ("channel".to_owned(), "distribution".to_owned()),
            ("versioncode".to_owned(), "140".to_owned()),
            ("mobilename".to_owned(), "".to_owned()),
            ("resolution".to_owned(), "1920x1080".to_owned()),
            ("__csrf".to_owned(), self.api.csrf_token()),
            (
                "deviceId".to_owned(),
                self.api.cookie("deviceId").unwrap_or_default(),
            ),
            (
                "requestId".to_owned(),
                format!(
                    "{}_{:04}",
                    now_millis(),
                    rand::thread_rng().gen_range(0..1000)
                ),
            ),
        ]);
        for name in ["MUSIC_U", "MUSIC_A", "NMTID"] {
            if let Some(value) = self.api.cookie(name).filter(|value| !value.is_empty()) {
                header.insert(name.to_owned(), value);
            }
        }
        if !header.contains_key("NMTID") && self.nmtid_probes.load(Ordering::Relaxed) >= 3 {
            let value = format!("00O{}", random_chars(b"0123456789abcdef", 38));
            self.api.set_cookie("NMTID", &value);
            header.insert("NMTID".to_owned(), value);
        }
        header
    }

    fn post(
        &self,
        host: &str,
        path: &str,
        mut data: Value,
        chain_id: Option<&str>,
        mobile: bool,
        operation: &'static str,
    ) -> Result<ApiResponse, NeteaseBridgeError> {
        #[cfg(test)]
        let host = self.test_host.as_deref().unwrap_or(host);
        let mut request = self.http.post(format!(
            "{host}{}",
            if mobile {
                path.replacen("/api/", "/eapi/", 1)
            } else {
                path.replacen("/api/", "/weapi/", 1)
            }
        ));
        let probing_nmtid;
        let form = if mobile {
            let header = self.mobile_header();
            probing_nmtid = !header.contains_key("NMTID");
            request = request
                .header(COOKIE, cookie_header(&header, operation)?)
                .header("User-Agent", IOS_UA)
                .header("x-aeapi", "true")
                .header("x-os", "iPhone OS")
                .header("x-osver", IOS_OSVER)
                .header("x-appver", IOS_APPVER)
                .header("x-buildver", IOS_BUILDVER)
                .header(
                    "x-deviceid",
                    self.api.cookie("deviceId").unwrap_or_default(),
                )
                .header(
                    "x-sdeviceid",
                    self.api.cookie("sDeviceId").unwrap_or_default(),
                );
            if let Some(value) = self.api.cookie("MUSIC_U") {
                request = request.header("x-music-u", value);
            }
            // PR #201 mobile eapi uses an empty encrypted header and HTTP device headers.
            data["header"] = json!({});
            data["deviceId"] = json!(self.api.cookie("deviceId").unwrap_or_default());
            eapi_params(path, &data)
        } else {
            probing_nmtid = false;
            let cookies = self
                .api
                .cookies()
                .into_iter()
                .map(|cookie| (cookie.name, cookie.value))
                .collect();
            request = request
                .header(COOKIE, cookie_header(&cookies, operation)?)
                .header("User-Agent", WEB_UA)
                .header("Referer", "https://music.163.com/")
                .header("Origin", WEB_HOST)
                .header("x-os", "web")
                .header("X-channelSource", "undefined")
                .header("Nm-GCore-Status", "1");
            if let Some(chain_id) = chain_id {
                request = request
                    .header("X-loginMethod", "QrCode")
                    .header("x-login-chain-id", chain_id);
            }
            data["csrf_token"] = json!(self.api.csrf_token());
            weapi_params(&data)
        }
        .map_err(|_| invalid_response(operation, "login request encryption failed"))?;

        // Do not retry login or SMS requests: duplicate attempts can trigger risk control.
        let response = request.form(&form).send().map_err(|error| {
            NeteaseBridgeError::Bridge(format!("{operation}: {}", error.without_url()))
        })?;
        let status = response.status().as_u16();
        if status == 429 {
            return Err(NeteaseBridgeError::RateLimited);
        }
        let headers = response.headers().clone();
        let raw = read_limited(response, operation)?;
        let body = decode_response(&raw, mobile, operation)?;
        if !body.is_object() {
            return Err(invalid_response(
                operation,
                "login response was not an object",
            ));
        }
        let cookies = response_cookies(&headers, &body);
        for cookie in &cookies {
            self.api.set_cookie(&cookie.name, &cookie.value);
        }
        if probing_nmtid {
            self.nmtid_probes.fetch_add(1, Ordering::Relaxed);
        }
        let code = body
            .get("code")
            .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()));
        Ok(ApiResponse {
            status,
            code,
            body,
            raw: raw.into(),
            cookies,
        })
    }
}

fn cookie_header(
    cookies: &BTreeMap<String, String>,
    operation: &'static str,
) -> Result<HeaderValue, NeteaseBridgeError> {
    let header = cookies
        .iter()
        .map(|(name, value)| {
            cookie::Cookie::new(
                utf8_percent_encode(name, COOKIE_ENCODE_SET).to_string(),
                utf8_percent_encode(value, COOKIE_ENCODE_SET).to_string(),
            )
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("; ");
    let mut value = HeaderValue::from_str(&header)
        .map_err(|_| invalid_response(operation, "invalid login cookie header"))?;
    value.set_sensitive(true);
    Ok(value)
}

fn response_cookies(headers: &HeaderMap, body: &Value) -> Vec<Cookie> {
    let mut cookies = BTreeMap::new();
    for header in headers.get_all(SET_COOKIE) {
        if let Ok(value) = header.to_str() {
            if let Ok(cookie) = cookie::Cookie::parse(value) {
                cookies.insert(cookie.name().to_owned(), cookie.value().to_owned());
            }
        }
    }
    if let Some(value) = body.get("cookie").and_then(Value::as_str) {
        for cookie in cookie::Cookie::split_parse(value).flatten() {
            if !matches!(
                cookie.name().to_ascii_lowercase().as_str(),
                "path" | "domain" | "expires" | "max-age" | "samesite"
            ) {
                cookies.insert(cookie.name().to_owned(), cookie.value().to_owned());
            }
        }
    }
    cookies
        .into_iter()
        .map(|(name, value)| Cookie::new(name, value))
        .collect()
}

fn web_key_data() -> Value {
    json!({ "type": 1, "noCheckToken": true })
}

fn web_poll_data(
    key: &str,
    secure_captcha: Option<&str>,
    yd_device_token: Option<&str>,
) -> Result<Value, NeteaseBridgeError> {
    for value in [secure_captcha, yd_device_token].into_iter().flatten() {
        if value.len() > 4096 || value.chars().any(char::is_control) {
            return Err(invalid_response(
                "poll QR login",
                "invalid security verification result",
            ));
        }
    }
    let mut data = json!({ "key": key, "type": 1, "noCheckToken": true, "ydDeviceToken": yd_device_token.unwrap_or_default() });
    if let Some(value) = secure_captcha.filter(|value| !value.is_empty()) {
        data["secureCaptcha"] = json!(value);
    }
    Ok(data)
}

pub(super) fn qr_url(key: &str, chain_id: &str) -> Result<String, NeteaseBridgeError> {
    let mut url = reqwest::Url::parse("https://music.163.com/st/platform/scanlogin")
        .map_err(|_| invalid_response("start QR login", "invalid QR login URL"))?;
    url.query_pairs_mut().extend_pairs([
        ("codekey", key),
        ("chainId", chain_id),
        ("hdw_device", "web"),
        ("hdw_appid", "web"),
        ("hitExp", "1"),
    ]);
    Ok(url.to_string())
}

fn mobile_captcha_data(phone: &str) -> Value {
    json!({ "cellphone": phone, "ctcode": "86", "os": "iOS", "fromPage": "RN", "rnBundleVersion": "0.0.5", "rnBundleName": "new-rn-login", "verifyId": 1, "e_r": true })
}

fn mobile_login_data(phone: &str, code: &str) -> Value {
    json!({ "phone": phone, "countrycode": "86", "type": "1", "https": "true", "remember": "true", "rememberLogin": "true", "captcha": code, "os": "iOS", "fromPage": "RN", "rnBundleVersion": "0.0.5", "rnBundleName": "new-rn-login", "verifyId": 1, "e_r": true })
}

fn read_limited(reader: impl Read, operation: &'static str) -> Result<Vec<u8>, NeteaseBridgeError> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_API_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid_response(operation, "login response could not be read"))?;
    if bytes.len() > MAX_API_RESPONSE_BYTES {
        return Err(invalid_response(
            operation,
            "login response exceeded the byte limit",
        ));
    }
    Ok(bytes)
}

fn gunzip(bytes: &[u8], operation: &'static str) -> Result<Vec<u8>, NeteaseBridgeError> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        read_limited(GzDecoder::new(bytes), operation)
    } else {
        Ok(bytes.to_vec())
    }
}

fn decode_response(
    raw: &[u8],
    encrypted: bool,
    operation: &'static str,
) -> Result<Value, NeteaseBridgeError> {
    let bytes = gunzip(raw, operation)?;
    if let Ok(body) = serde_json::from_slice(&bytes) {
        return Ok(body);
    }
    if !encrypted {
        return Err(invalid_response(operation, "login response was not JSON"));
    }
    let decoded = ecb::Decryptor::<Aes128>::new(EAPI_KEY.into())
        .decrypt_padded_vec_mut::<Pkcs7>(&bytes)
        .map_err(|_| invalid_response(operation, "encrypted login response was invalid"))?;
    let decoded = gunzip(&decoded, operation)?;
    serde_json::from_slice(&decoded)
        .map_err(|_| invalid_response(operation, "decoded login response was not JSON"))
}

fn invalid_response(operation: &'static str, message: &str) -> NeteaseBridgeError {
    NeteaseBridgeError::InvalidResponse {
        operation,
        message: message.to_owned(),
    }
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

fn random_chars(alphabet: &[u8], length: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| char::from(alphabet[rng.gen_range(0..alphabet.len())]))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cipher::BlockEncryptMut;
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;
    use std::io::{BufRead, BufReader};
    use std::net::{TcpListener, TcpStream};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    pub(crate) struct RecordedRequest {
        pub line: String,
        pub headers: BTreeMap<String, String>,
        pub body: String,
    }

    pub(crate) struct TestReply {
        pub status: u16,
        pub headers: Vec<(&'static str, &'static str)>,
        pub body: Vec<u8>,
    }

    impl TestReply {
        pub(crate) fn json(body: Value) -> Self {
            Self {
                status: 200,
                headers: Vec::new(),
                body: serde_json::to_vec(&body).unwrap(),
            }
        }

        pub(crate) fn encrypted(body: Value) -> Self {
            Self {
                status: 200,
                headers: Vec::new(),
                body: encrypt(&gzip(&serde_json::to_vec(&body).unwrap())),
            }
        }

        pub(crate) fn cookie(mut self, value: &'static str) -> Self {
            self.headers.push(("Set-Cookie", value));
            self
        }
    }

    pub(crate) fn mock_server(
        replies: Vec<TestReply>,
    ) -> (String, JoinHandle<Vec<RecordedRequest>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        Err(_) => panic!("fixture server did not receive the expected request"),
                    }
                };
                // Accepted sockets inherit nonblocking mode on some platforms, including macOS.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                requests.push(read_request(&mut stream));
                write!(
                    stream,
                    "HTTP/1.1 {} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n",
                    reply.status,
                    reply.body.len()
                )
                .unwrap();
                for (name, value) in reply.headers {
                    write!(stream, "{name}: {value}\r\n").unwrap();
                }
                stream.write_all(b"\r\n").unwrap();
                stream.write_all(&reply.body).unwrap();
            }
            requests
        });
        (host, handle)
    }

    fn read_request(stream: &mut TcpStream) -> RecordedRequest {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let mut headers = BTreeMap::new();
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).unwrap();
            if header == "\r\n" {
                break;
            }
            let (name, value) = header.split_once(':').expect("fixture request header");
            headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
        }
        let length = headers.get("content-length").unwrap().parse().unwrap();
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        RecordedRequest {
            line: line.trim().to_owned(),
            headers,
            body: String::from_utf8(body).unwrap(),
        }
    }

    #[test]
    fn fixture_server_should_wait_for_delayed_request_bytes() {
        let (host, server) = mock_server(vec![TestReply::json(json!({ "code": 200 }))]);
        let mut stream = TcpStream::connect(host.strip_prefix("http://").unwrap()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        stream
            .write_all(b"POST /fixture HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        assert_eq!(server.join().unwrap()[0].line, "POST /fixture HTTP/1.1");
    }

    fn encrypt(bytes: &[u8]) -> Vec<u8> {
        ecb::Encryptor::<Aes128>::new(EAPI_KEY.into()).encrypt_padded_vec_mut::<Pkcs7>(bytes)
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn web_qr_requests_should_use_type_one_and_real_verification_result() {
        assert_eq!(web_key_data(), json!({ "type": 1, "noCheckToken": true }));
        assert_eq!(
            web_poll_data(
                "fixture-key",
                Some("fixture-validate"),
                Some("fixture-device")
            )
            .unwrap(),
            json!({ "key": "fixture-key", "type": 1, "noCheckToken": true, "secureCaptcha": "fixture-validate", "ydDeviceToken": "fixture-device" })
        );
    }

    #[test]
    fn web_qr_requests_should_not_invent_security_verification() {
        let data = web_poll_data("fixture-key", None, None).unwrap();
        assert!(data.get("secureCaptcha").is_none());
        assert_eq!(data["ydDeviceToken"], "");
    }

    #[test]
    fn web_qr_requests_should_reject_oversized_verification_without_values() {
        let error = web_poll_data("fixture-key", Some(&"secret".repeat(1000)), None).unwrap_err();
        assert!(!error.to_string().contains("secret"));
    }

    #[test]
    fn qr_url_should_preserve_the_same_chain_id_and_encode_parameters() {
        let url = reqwest::Url::parse(&qr_url("fixture+&key", "fixture+&chain").unwrap()).unwrap();
        assert_eq!(url.path(), "/st/platform/scanlogin");
        let query: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["codekey"], "fixture+&key");
        assert_eq!(query["chainId"], "fixture+&chain");
        assert_eq!(query["hdw_device"], "web");
    }

    #[test]
    fn mobile_context_should_omit_nmtid_until_issued_by_the_server() {
        let client = LoginClient::mobile().unwrap();
        assert!(!client.mobile_header().contains_key("NMTID"));
        client.api.set_cookie("NMTID", "fixture-server-nmtid");
        assert_eq!(
            client.clone().mobile_header()["NMTID"],
            "fixture-server-nmtid"
        );
        assert_eq!(client.api.cookie("os").as_deref(), Some("iPhone OS"));
    }

    #[test]
    fn nmtid_fallback_should_be_bounded_and_stable() {
        let client = LoginClient::mobile().unwrap();
        client.nmtid_probes.store(3, Ordering::Relaxed);
        let first = client.mobile_header()["NMTID"].clone();
        assert!(first.starts_with("00O"));
        assert_eq!(client.mobile_header()["NMTID"], first);
    }

    #[test]
    fn login_context_should_be_stable_isolated_and_redacted() {
        let client = LoginClient::web().unwrap();
        let clone = client.clone();
        assert_eq!(client.api.cookie("NMTID"), clone.api.cookie("NMTID"));
        assert_ne!(
            client.api.cookie("NMTID"),
            LoginClient::web().unwrap().api.cookie("NMTID")
        );
        client.api.set_cookie("MUSIC_U", "fixture-secret");
        assert!(!format!("{client:?}").contains("fixture-secret"));
    }

    #[test]
    fn response_cookies_should_merge_http_and_body_without_attributes() {
        let mut headers = HeaderMap::new();
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("NMTID=fixture-server; Path=/; HttpOnly"),
        );
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("MUSIC_U=fixture-old; Path=/"),
        );
        let cookies = response_cookies(
            &headers,
            &json!({ "cookie": "MUSIC_U=fixture-new; __csrf=fixture-csrf; Path=/; Max-Age=10" }),
        );
        assert_eq!(
            cookies,
            vec![
                Cookie::new("MUSIC_U", "fixture-new"),
                Cookie::new("NMTID", "fixture-server"),
                Cookie::new("__csrf", "fixture-csrf")
            ]
        );
    }

    #[test]
    fn mobile_login_should_match_pr201_instead_of_the_old_web_endpoint() {
        let data = mobile_login_data("13800138000", "123456");
        assert_eq!(data["rememberLogin"], "true");
        assert_eq!(data["rnBundleName"], "new-rn-login");
        assert_eq!(data["verifyId"], 1);
        assert_eq!(data["e_r"], true);
        assert!(data.get("password").is_none());
        assert_eq!(
            mobile_captcha_data("13800138000")["cellphone"],
            "13800138000"
        );
    }

    #[test]
    fn eapi_response_should_decode_plain_encrypted_and_both_gzip_orders() {
        let body = json!({ "code": 200, "data": { "fixture": true } });
        let bytes = serde_json::to_vec(&body).unwrap();
        for raw in [
            bytes.clone(),
            encrypt(&bytes),
            encrypt(&gzip(&bytes)),
            gzip(&encrypt(&bytes)),
            gzip(&encrypt(&gzip(&bytes))),
        ] {
            assert_eq!(decode_response(&raw, true, "fixture").unwrap(), body);
        }
    }

    #[test]
    fn decoded_response_should_reject_malformed_data_without_secret_values() {
        let error = decode_response(b"fixture-secret", true, "fixture").unwrap_err();
        assert!(!error.to_string().contains("fixture-secret"));
    }

    #[test]
    fn decoded_response_should_bound_decompression() {
        let raw = gzip(&vec![b'a'; MAX_API_RESPONSE_BYTES + 1]);
        assert!(decode_response(&raw, true, "fixture").is_err());
    }

    #[test]
    fn mobile_transport_should_reuse_server_nmtid_and_device_headers() {
        let (host, server) = mock_server(vec![
            TestReply::encrypted(json!({ "code": 200 })).cookie("NMTID=fixture-issued; Path=/"),
            TestReply::encrypted(
                json!({ "code": 200, "cookie": "MUSIC_U=fixture-session; __csrf=fixture-csrf" }),
            ),
        ]);
        let client = LoginClient::mobile().unwrap().with_test_host(host);
        assert_eq!(client.send_code("13800138000").unwrap().code, Some(200));
        assert_eq!(
            client.complete_phone("13800138000", "123456").unwrap().code,
            Some(200)
        );
        let requests = server.join().unwrap();
        assert_eq!(requests[0].line, "POST /eapi/sms/captcha/sent HTTP/1.1");
        assert_eq!(requests[1].line, "POST /eapi/login/cellphone HTTP/1.1");
        assert!(!requests[0].headers["cookie"].contains("NMTID="));
        assert!(requests[1].headers["cookie"].contains("NMTID=fixture-issued"));
        assert!(requests[0].headers["cookie"].contains("os=iPhone%20OS"));
        assert_eq!(
            requests[0].headers["x-deviceid"],
            requests[1].headers["x-deviceid"]
        );
        assert_eq!(
            requests[0].headers["x-sdeviceid"],
            requests[1].headers["x-sdeviceid"]
        );
        assert_eq!(requests[0].headers["x-aeapi"], "true");
        assert_eq!(requests[0].headers["user-agent"], IOS_UA);
        assert!(
            requests[0].headers["content-type"].starts_with("application/x-www-form-urlencoded")
        );
        let form: BTreeMap<_, _> = url::form_urlencoded::parse(requests[1].body.as_bytes())
            .into_owned()
            .collect();
        assert_eq!(form.keys().collect::<Vec<_>>(), vec!["params"]);
        assert!(!requests[1].body.contains("123456"));
        assert_eq!(
            client.api.cookie("MUSIC_U").as_deref(),
            Some("fixture-session")
        );
    }

    #[test]
    fn web_transport_should_keep_chain_headers_and_verification_response() {
        let (host, server) = mock_server(vec![
            TestReply::json(json!({ "code": 200, "unikey": "fixture-key" }))
                .cookie("NMTID=fixture-issued; Path=/"),
            TestReply::json(json!({ "code": 8821, "message": "verification required" })),
        ]);
        let client = LoginClient::web().unwrap().with_test_host(host);
        client.start_qr().unwrap();
        assert_eq!(
            client
                .poll_qr("fixture-key", "fixture-chain", None, None)
                .unwrap()
                .code,
            Some(8821)
        );
        let requests = server.join().unwrap();
        assert_eq!(requests[0].line, "POST /weapi/login/qrcode/unikey HTTP/1.1");
        assert_eq!(requests[1].headers["x-login-chain-id"], "fixture-chain");
        assert_eq!(requests[1].headers["x-loginmethod"], "QrCode");
        assert_eq!(requests[1].headers["origin"], WEB_HOST);
        assert_eq!(requests[1].headers["x-os"], "web");
        assert!(requests[1].headers["cookie"].contains("NMTID=fixture-issued"));
        let form: BTreeMap<_, _> = url::form_urlencoded::parse(requests[1].body.as_bytes())
            .into_owned()
            .collect();
        assert_eq!(form.keys().collect::<Vec<_>>(), vec!["encSecKey", "params"]);
    }

    #[test]
    fn transport_should_not_follow_redirects_or_resend_sms() {
        let mut reply =
            TestReply::json(json!({ "code": -462, "message": "verification required" }));
        reply.status = 302;
        reply
            .headers
            .push(("Location", "https://example.invalid/collect"));
        let (host, server) = mock_server(vec![reply]);
        let client = LoginClient::mobile().unwrap().with_test_host(host);
        let response = client.send_code("13800138000").unwrap();
        assert_eq!(response.status, 302);
        assert_eq!(response.code, Some(-462));
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn transport_should_not_accept_a_null_login_response_as_success() {
        let (host, server) = mock_server(vec![TestReply::json(Value::Null)]);
        let client = LoginClient::mobile().unwrap().with_test_host(host);
        assert!(client.send_code("13800138000").is_err());
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn cookie_header_should_match_encode_uri_component_and_mark_credentials_sensitive() {
        let cookies = BTreeMap::from([("fixture".to_owned(), "space +/&=!~*'()".to_owned())]);
        let header = cookie_header(&cookies, "fixture").unwrap();
        assert_eq!(
            header.to_str().unwrap(),
            "fixture=space%20%2B%2F%26%3D!~*'()"
        );
        assert!(header.is_sensitive());
    }

    #[test]
    fn transport_should_classify_http_rate_limits_even_without_json() {
        let reply = TestReply {
            status: 429,
            headers: Vec::new(),
            body: b"rate limited".to_vec(),
        };
        let (host, server) = mock_server(vec![reply]);
        let client = LoginClient::mobile().unwrap().with_test_host(host);
        assert!(matches!(
            client.send_code("13800138000"),
            Err(NeteaseBridgeError::RateLimited)
        ));
        assert_eq!(server.join().unwrap().len(), 1);
    }
}
