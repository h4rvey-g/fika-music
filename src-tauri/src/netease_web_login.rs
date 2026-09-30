use crate::netease::{
    NeteaseBridgeError, NeteaseServiceBridge, NeteaseWebLoginPoll, NeteaseWebLoginStart,
    NeteaseWebLoginStatus,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const LOGIN_URL: &str = "https://music.163.com/#/login";
const COOKIE_URL: &str = "https://music.163.com/";
const SESSION_TTL: Duration = Duration::from_secs(600);
const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

pub(crate) const fn supported() -> bool {
    !cfg!(any(target_os = "android", target_os = "ios"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Waiting,
    Verifying,
    Committing,
    Cancelled,
    Expired,
}

struct Attempt {
    phase: Mutex<Phase>,
    changed: Condvar,
}

impl Attempt {
    fn new() -> Self {
        Self {
            phase: Mutex::new(Phase::Waiting),
            changed: Condvar::new(),
        }
    }

    fn transition(&self, from: Phase, to: Phase) -> bool {
        let Ok(mut phase) = self.phase.lock() else {
            return false;
        };
        if *phase != from {
            return false;
        }
        *phase = to;
        self.changed.notify_all();
        true
    }

    fn cancel(&self) {
        let Ok(mut phase) = self.phase.lock() else {
            return;
        };
        if matches!(*phase, Phase::Waiting | Phase::Verifying) {
            *phase = Phase::Cancelled;
            self.changed.notify_all();
        }
    }

    fn phase(&self) -> Result<Phase, NeteaseBridgeError> {
        self.phase
            .lock()
            .map(|phase| *phase)
            .map_err(|_| failure("login state unavailable"))
    }

    fn wait_for_expiry(&self) -> bool {
        let Ok(phase) = self.phase.lock() else {
            return true;
        };
        let Ok((mut phase, timeout)) =
            self.changed
                .wait_timeout_while(phase, SESSION_TTL, |phase| {
                    matches!(*phase, Phase::Waiting | Phase::Verifying)
                })
        else {
            return true;
        };
        if timeout.timed_out() && matches!(*phase, Phase::Waiting | Phase::Verifying) {
            *phase = Phase::Expired;
            return true;
        }
        false
    }
}

#[derive(Clone)]
struct PendingLogin {
    id: String,
    label: String,
    attempt: Arc<Attempt>,
}

#[derive(Default)]
pub(crate) struct WebLoginState {
    pending: Mutex<Option<PendingLogin>>,
}

impl WebLoginState {
    pub(crate) fn start(
        &self,
        app: &AppHandle,
    ) -> Result<NeteaseWebLoginStart, NeteaseBridgeError> {
        if !supported() {
            return Err(NeteaseBridgeError::WebLoginUnsupported);
        }
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| failure("login state unavailable"))?;
        if let Some(previous) = pending.as_ref() {
            if matches!(
                previous.attempt.phase()?,
                Phase::Waiting | Phase::Verifying | Phase::Committing
            ) {
                return Err(failure("an official website login is already open"));
            }
            close_window(app, &previous.label);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let label = format!("netease-web-login-{id}");
        let attempt = Arc::new(Attempt::new());
        let window = WebviewWindowBuilder::new(
            app,
            &label,
            WebviewUrl::External(
                LOGIN_URL
                    .parse()
                    .map_err(|_| failure("login URL invalid"))?,
            ),
        )
        .title("NetEase Cloud Music - music.163.com")
        .inner_size(1024.0, 720.0)
        .min_inner_size(800.0, 600.0)
        .center()
        .incognito(true)
        .user_agent(BROWSER_UA)
        .on_navigation(allowed_navigation)
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .on_download(|_, _| false)
        .build()
        .map_err(|_| failure("could not open the official website"))?;
        let closed_attempt = Arc::clone(&attempt);
        window.on_window_event(move |event| {
            if matches!(
                event,
                tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
            ) {
                closed_attempt.cancel();
            }
        });
        let expiry_app = app.clone();
        let expiry_label = label.clone();
        let expiry_attempt = Arc::clone(&attempt);
        // One waiter per active window; cancellation wakes it immediately.
        std::thread::Builder::new()
            .name("netease-web-login-expiry".into())
            .spawn(move || {
                if expiry_attempt.wait_for_expiry() {
                    close_window(&expiry_app, &expiry_label);
                }
            })
            .map_err(|_| {
                attempt.cancel();
                close_window(app, &label);
                failure("could not start login expiry timer")
            })?;
        *pending = Some(PendingLogin {
            id: id.clone(),
            label,
            attempt,
        });
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + SESSION_TTL.as_secs();
        Ok(NeteaseWebLoginStart {
            session_id: id,
            expires_at: expires_at as i64,
        })
    }

    pub(crate) fn poll(
        &self,
        app: &AppHandle,
        id: &str,
        bridge: &NeteaseServiceBridge,
    ) -> Result<NeteaseWebLoginPoll, NeteaseBridgeError> {
        let session = self.session(id)?;
        match session.attempt.phase()? {
            Phase::Cancelled => return Ok(result(NeteaseWebLoginStatus::Cancelled)),
            Phase::Expired => return Ok(result(NeteaseWebLoginStatus::Expired)),
            Phase::Verifying | Phase::Committing => {
                return Ok(result(NeteaseWebLoginStatus::Waiting))
            }
            Phase::Waiting => {}
        }
        let Some(window) = app.get_webview_window(&session.label) else {
            session.attempt.cancel();
            return Ok(result(NeteaseWebLoginStatus::Cancelled));
        };
        let url = window
            .url()
            .map_err(|_| failure("could not inspect the login window"))?;
        if !allowed_navigation(&url) {
            self.cancel(app, id)?;
            return Err(failure("untrusted login navigation was rejected"));
        }
        let cookies = window
            .cookies_for_url(
                COOKIE_URL
                    .parse()
                    .map_err(|_| failure("cookie URL invalid"))?,
            )
            .map_err(|_| failure("could not read the official website session"))?;
        let cookies = session_cookies(&cookies)?;
        if !cookies.contains_key("MUSIC_U")
            || !session.attempt.transition(Phase::Waiting, Phase::Verifying)
        {
            return Ok(result(NeteaseWebLoginStatus::Waiting));
        }
        let account = bridge.import_web_session(cookies, || {
            session
                .attempt
                .transition(Phase::Verifying, Phase::Committing)
        });
        close_window(app, &session.label);
        self.remove(id)?;
        match account {
            Ok(account) => Ok(NeteaseWebLoginPoll {
                status: NeteaseWebLoginStatus::Connected,
                account: Some(account),
            }),
            Err(NeteaseBridgeError::WebLoginCancelled) => {
                Ok(result(NeteaseWebLoginStatus::Cancelled))
            }
            Err(error) => {
                session.attempt.cancel();
                Err(error)
            }
        }
    }

    pub(crate) fn cancel(&self, app: &AppHandle, id: &str) -> Result<(), NeteaseBridgeError> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| failure("login state unavailable"))?;
        if pending.as_ref().is_some_and(|session| session.id == id) {
            if let Some(session) = pending.take() {
                session.attempt.cancel();
                close_window(app, &session.label);
            }
        }
        Ok(())
    }

    fn session(&self, id: &str) -> Result<PendingLogin, NeteaseBridgeError> {
        self.pending
            .lock()
            .map_err(|_| failure("login state unavailable"))?
            .as_ref()
            .filter(|session| session.id == id)
            .cloned()
            .ok_or(NeteaseBridgeError::WebLoginSessionExpired)
    }

    fn remove(&self, id: &str) -> Result<(), NeteaseBridgeError> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| failure("login state unavailable"))?;
        if pending.as_ref().is_some_and(|session| session.id == id) {
            *pending = None;
        }
        Ok(())
    }
}

fn allowed_navigation(url: &tauri::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(url.host_str(), Some("music.163.com" | "st.music.163.com"))
}

fn session_cookies(
    cookies: &[tauri::webview::Cookie<'_>],
) -> Result<BTreeMap<String, String>, NeteaseBridgeError> {
    let mut session = BTreeMap::new();
    for cookie in cookies {
        if !matches!(cookie.domain(), Some("music.163.com" | ".music.163.com"))
            || cookie.path() != Some("/")
            || !["MUSIC_U", "__csrf", "NMTID", "MUSIC_A"].contains(&cookie.name())
            || cookie.value().is_empty()
        {
            continue;
        }
        if cookie.value().len() > 8192 || cookie.value().chars().any(char::is_control) {
            return Err(failure("official website session was invalid"));
        }
        if session
            .get(cookie.name())
            .is_some_and(|value| value != cookie.value())
        {
            return Err(failure(
                "official website session contained conflicting cookies",
            ));
        }
        session.insert(cookie.name().to_owned(), cookie.value().to_owned());
    }
    Ok(session)
}

fn close_window(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.destroy();
    }
}

fn failure(message: &str) -> NeteaseBridgeError {
    NeteaseBridgeError::Bridge(format!("official website login: {message}"))
}

fn result(status: NeteaseWebLoginStatus) -> NeteaseWebLoginPoll {
    NeteaseWebLoginPoll {
        status,
        account: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::webview::Cookie;

    #[test]
    fn navigation_should_only_allow_trusted_https_origins() {
        for url in [LOGIN_URL, "https://st.music.163.com/encrypt-pages"] {
            assert!(allowed_navigation(&url.parse().unwrap()));
        }
        for url in [
            "http://music.163.com/",
            "https://music.163.com.evil.test/",
            "https://evil.test/",
            "https://music.163.com:444/",
            "https://user@music.163.com/",
            "tauri://localhost/",
            "javascript:alert(1)",
        ] {
            assert!(!allowed_navigation(&url.parse().unwrap()), "{url}");
        }
    }

    #[test]
    fn cookies_should_only_import_root_session_keys_from_music_origin() {
        let cookies = vec![
            Cookie::build(("MUSIC_U", "fixture-session"))
                .domain(".music.163.com")
                .path("/")
                .http_only(true)
                .build(),
            Cookie::build(("__csrf", "fixture-csrf"))
                .domain("music.163.com")
                .path("/")
                .build(),
            Cookie::build(("other", "fixture-private"))
                .domain("music.163.com")
                .path("/")
                .build(),
            Cookie::build(("MUSIC_U", "wrong-domain"))
                .domain("evil.test")
                .path("/")
                .build(),
            Cookie::build(("MUSIC_U", "wrong-path"))
                .domain("music.163.com")
                .path("/other")
                .build(),
        ];
        assert_eq!(
            session_cookies(&cookies).unwrap(),
            BTreeMap::from([
                ("MUSIC_U".to_owned(), "fixture-session".to_owned()),
                ("__csrf".to_owned(), "fixture-csrf".to_owned())
            ])
        );
    }

    #[test]
    fn cookies_should_reject_conflicting_authentication_values() {
        let cookies = vec![
            Cookie::build(("MUSIC_U", "fixture-one"))
                .domain("music.163.com")
                .path("/")
                .build(),
            Cookie::build(("MUSIC_U", "fixture-two"))
                .domain(".music.163.com")
                .path("/")
                .build(),
        ];
        assert!(session_cookies(&cookies).is_err());
    }

    #[test]
    fn closing_during_verification_should_prevent_commit() {
        let attempt = Attempt::new();
        assert!(attempt.transition(Phase::Waiting, Phase::Verifying));
        attempt.cancel();
        assert!(!attempt.transition(Phase::Verifying, Phase::Committing));
    }

    #[test]
    fn verification_should_only_start_once() {
        let attempt = Attempt::new();
        assert!(attempt.transition(Phase::Waiting, Phase::Verifying));
        assert!(!attempt.transition(Phase::Waiting, Phase::Verifying));
    }

    #[test]
    fn cancellation_should_wake_expiry_waiter_without_waiting_for_timeout() {
        let attempt = Attempt::new();
        attempt.cancel();
        assert!(!attempt.wait_for_expiry());
    }
}
