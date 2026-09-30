use super::*;

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct NeteaseCommandError {
    code: String,
    message: String,
    verification: Option<netease::NeteaseVerificationChallenge>,
}

impl From<netease::NeteaseBridgeError> for NeteaseCommandError {
    fn from(error: netease::NeteaseBridgeError) -> Self {
        let verification = match &error {
            netease::NeteaseBridgeError::VerificationRequired { verification, .. } => {
                verification.clone()
            }
            _ => None,
        };
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
            verification,
        }
    }
}

async fn run_netease_task<T, F>(task: F) -> Result<T, NeteaseCommandError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, netease::NeteaseBridgeError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| NeteaseCommandError {
            code: "bridge-failure".to_owned(),
            message: format!("NetEase bridge task failed: {error}"),
            verification: None,
        })?
        .map_err(Into::into)
}

#[tauri::command]
pub(crate) fn netease_web_login_supported() -> bool {
    netease_web_login::supported()
}

#[tauri::command]
pub(crate) async fn start_netease_web_login(
    app: AppHandle,
    web_login: State<'_, Arc<netease_web_login::WebLoginState>>,
) -> Result<netease::NeteaseWebLoginStart, NeteaseCommandError> {
    let web_login = Arc::clone(&web_login);
    run_netease_task(move || web_login.start(&app)).await
}

#[tauri::command]
pub(crate) async fn poll_netease_web_login(
    app: AppHandle,
    state: State<'_, AppState>,
    web_login: State<'_, Arc<netease_web_login::WebLoginState>>,
    session_id: String,
) -> Result<netease::NeteaseWebLoginPoll, NeteaseCommandError> {
    let web_login = Arc::clone(&web_login);
    let bridge = Arc::clone(&state.netease_bridge);
    let poll = run_netease_task(move || web_login.poll(&app, session_id.trim(), &bridge)).await?;
    if poll.account.is_some() {
        state.online_music_cache.invalidate();
    }
    Ok(poll)
}

#[tauri::command]
pub(crate) async fn cancel_netease_web_login(
    app: AppHandle,
    web_login: State<'_, Arc<netease_web_login::WebLoginState>>,
    session_id: String,
) -> Result<(), NeteaseCommandError> {
    let web_login = Arc::clone(&web_login);
    run_netease_task(move || web_login.cancel(&app, session_id.trim())).await
}

#[tauri::command]
pub(crate) async fn start_netease_qr_login(
    state: State<'_, AppState>,
) -> Result<netease::NeteaseQrLoginStart, NeteaseCommandError> {
    let bridge = Arc::clone(&state.netease_bridge);
    run_netease_task(move || bridge.start_qr_login()).await
}

#[tauri::command]
pub(crate) async fn poll_netease_qr_login(
    state: State<'_, AppState>,
    session_id: String,
    secure_captcha: Option<String>,
    yd_device_token: Option<String>,
) -> Result<netease::NeteaseQrLoginPoll, NeteaseCommandError> {
    let bridge = Arc::clone(&state.netease_bridge);
    let poll = run_netease_task(move || {
        bridge.poll_qr_login(
            session_id.trim(),
            secure_captcha.as_deref(),
            yd_device_token.as_deref(),
        )
    })
    .await?;
    if poll.account.is_some() {
        state.online_music_cache.invalidate();
    }
    Ok(poll)
}

#[tauri::command]
pub(crate) async fn cancel_netease_qr_login(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), NeteaseCommandError> {
    let bridge = Arc::clone(&state.netease_bridge);
    run_netease_task(move || bridge.cancel_qr_login(session_id.trim())).await
}

#[tauri::command]
pub(crate) async fn start_netease_phone_login(
    state: State<'_, AppState>,
    phone: String,
) -> Result<netease::NeteasePhoneLoginStart, NeteaseCommandError> {
    let bridge = Arc::clone(&state.netease_bridge);
    run_netease_task(move || bridge.start_phone_login(phone.trim())).await
}

#[tauri::command]
pub(crate) async fn complete_netease_phone_login(
    state: State<'_, AppState>,
    session_id: String,
    verification_code: String,
) -> Result<netease::NeteaseAccount, NeteaseCommandError> {
    let bridge = Arc::clone(&state.netease_bridge);
    let account = run_netease_task(move || {
        bridge.complete_phone_login(session_id.trim(), verification_code.trim())
    })
    .await?;
    state.online_music_cache.invalidate();
    Ok(account)
}

#[tauri::command]
pub(crate) async fn login_netease_password(
    state: State<'_, AppState>,
    account: String,
    password: String,
) -> Result<netease::NeteaseAccount, NeteaseCommandError> {
    let bridge = Arc::clone(&state.netease_bridge);
    let account = account.trim().to_owned();
    let account = run_netease_task(move || bridge.login_with_password(&account, &password)).await?;
    state.online_music_cache.invalidate();
    Ok(account)
}

#[tauri::command]
pub(crate) async fn complete_netease_password_login(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<netease::NeteaseAccount, NeteaseCommandError> {
    let bridge = Arc::clone(&state.netease_bridge);
    let account =
        run_netease_task(move || bridge.complete_password_login(session_id.trim())).await?;
    state.online_music_cache.invalidate();
    Ok(account)
}

#[tauri::command]
pub(crate) async fn cancel_netease_password_login(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), NeteaseCommandError> {
    let bridge = Arc::clone(&state.netease_bridge);
    run_netease_task(move || bridge.cancel_password_login(session_id.trim())).await
}

#[tauri::command]
pub(crate) async fn cancel_netease_phone_login(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), NeteaseCommandError> {
    let bridge = Arc::clone(&state.netease_bridge);
    run_netease_task(move || bridge.cancel_phone_login(session_id.trim())).await
}

#[tauri::command]
pub(crate) fn list_netease_accounts(
    state: State<'_, AppState>,
) -> Result<Vec<netease::NeteaseAccount>, NeteaseCommandError> {
    state.netease_bridge.accounts().map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn disconnect_netease_account(
    state: State<'_, AppState>,
    account_ref: String,
) -> Result<(), NeteaseCommandError> {
    let bridge = Arc::clone(&state.netease_bridge);
    run_netease_task(move || bridge.disconnect_account(account_ref.trim())).await?;
    state.online_music_cache.invalidate();
    Ok(())
}

#[tauri::command]
pub(crate) fn list_netease_mutation_audit(
    state: State<'_, AppState>,
    account_ref: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<netease::NeteaseMutationAudit>, NeteaseCommandError> {
    state
        .netease_bridge
        .mutation_audit(account_ref.as_deref(), limit.unwrap_or(50))
        .map_err(Into::into)
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct KugouCommandError {
    code: String,
    message: String,
}

impl From<kugou::KugouBridgeError> for KugouCommandError {
    fn from(error: kugou::KugouBridgeError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

async fn run_kugou_task<T, F>(task: F) -> Result<T, KugouCommandError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, kugou::KugouBridgeError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| KugouCommandError {
            code: "bridge-failure".to_owned(),
            message: format!("KuGou bridge task failed: {error}"),
        })?
        .map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn start_kugou_qr_login(
    state: State<'_, AppState>,
) -> Result<kugou::KugouQrLoginStart, KugouCommandError> {
    let bridge = Arc::clone(&state.kugou_bridge);
    run_kugou_task(move || bridge.start_qr_login()).await
}

#[tauri::command]
pub(crate) async fn poll_kugou_qr_login(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<kugou::KugouQrLoginPoll, KugouCommandError> {
    let bridge = Arc::clone(&state.kugou_bridge);
    let poll = run_kugou_task(move || bridge.poll_qr_login(session_id.trim())).await?;
    if poll.account.is_some() {
        state.online_music_cache.invalidate();
    }
    Ok(poll)
}

#[tauri::command]
pub(crate) async fn cancel_kugou_qr_login(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), KugouCommandError> {
    let bridge = Arc::clone(&state.kugou_bridge);
    run_kugou_task(move || bridge.cancel_qr_login(session_id.trim())).await
}

#[tauri::command]
pub(crate) async fn start_kugou_phone_login(
    state: State<'_, AppState>,
    phone: String,
) -> Result<kugou::KugouPhoneLoginStart, KugouCommandError> {
    let bridge = Arc::clone(&state.kugou_bridge);
    run_kugou_task(move || bridge.start_phone_login(phone.trim())).await
}

#[tauri::command]
pub(crate) async fn complete_kugou_phone_login(
    state: State<'_, AppState>,
    session_id: String,
    verification_code: String,
) -> Result<kugou::KugouAccount, KugouCommandError> {
    let bridge = Arc::clone(&state.kugou_bridge);
    let account = run_kugou_task(move || {
        bridge.complete_phone_login(session_id.trim(), verification_code.trim())
    })
    .await?;
    state.online_music_cache.invalidate();
    Ok(account)
}

#[tauri::command]
pub(crate) async fn cancel_kugou_phone_login(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), KugouCommandError> {
    let bridge = Arc::clone(&state.kugou_bridge);
    run_kugou_task(move || bridge.cancel_phone_login(session_id.trim())).await
}

#[tauri::command]
pub(crate) fn list_kugou_accounts(
    state: State<'_, AppState>,
) -> Result<Vec<kugou::KugouAccount>, KugouCommandError> {
    state.kugou_bridge.accounts().map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn disconnect_kugou_account(
    state: State<'_, AppState>,
    account_ref: String,
) -> Result<(), KugouCommandError> {
    let bridge = Arc::clone(&state.kugou_bridge);
    run_kugou_task(move || bridge.disconnect_account(account_ref.trim())).await?;
    state.online_music_cache.invalidate();
    Ok(())
}
