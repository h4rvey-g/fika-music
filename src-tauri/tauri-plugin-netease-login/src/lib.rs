use serde::{Deserialize, Serialize};
use tauri::{plugin::PluginHandle, Manager, Runtime};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_netease_login);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Session<'a> {
    session_id: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPoll {
    pub cancelled: bool,
    pub cookie_header: Option<String>,
}

pub struct LoginBrowser<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> LoginBrowser<R> {
    pub fn start(&self, session_id: &str) -> Result<(), tauri::plugin::mobile::PluginInvokeError> {
        self.0.run_mobile_plugin("start", Session { session_id })
    }

    pub fn poll(
        &self,
        session_id: &str,
    ) -> Result<BrowserPoll, tauri::plugin::mobile::PluginInvokeError> {
        self.0.run_mobile_plugin("poll", Session { session_id })
    }

    pub fn cancel(&self, session_id: &str) -> Result<(), tauri::plugin::mobile::PluginInvokeError> {
        self.0.run_mobile_plugin("cancel", Session { session_id })
    }
}

pub trait LoginBrowserExt<R: Runtime> {
    fn login_browser(&self) -> tauri::State<'_, LoginBrowser<R>>;
}

impl<R: Runtime, T: Manager<R>> LoginBrowserExt<R> for T {
    fn login_browser(&self) -> tauri::State<'_, LoginBrowser<R>> {
        self.state::<LoginBrowser<R>>()
    }
}

pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("netease-login")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let handle =
                api.register_android_plugin("com.hvg.neteaselogin", "NeteaseLoginPlugin")?;
            #[cfg(target_os = "ios")]
            let handle = api.register_ios_plugin(init_plugin_netease_login)?;
            app.manage(LoginBrowser(handle));
            Ok(())
        })
        .build()
}
