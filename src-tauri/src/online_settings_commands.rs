use super::*;

#[tauri::command]
pub(crate) fn get_online_music_settings(
    state: State<'_, AppState>,
) -> CommandResult<online_music::OnlineMusicSettings> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock was poisoned".to_owned())?;
    online_music::load_settings(&db).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn update_online_music_settings(
    state: State<'_, AppState>,
    settings: online_music::OnlineMusicSettings,
) -> CommandResult<online_music::OnlineMusicSettings> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock was poisoned".to_owned())?;
    online_music::save_settings(&db, &settings, now_timestamp())
        .map_err(|error| error.to_string())?;
    state.online_music_cache.invalidate();
    Ok(settings)
}

#[tauri::command]
pub(crate) fn clear_online_search_history(state: State<'_, AppState>) -> CommandResult<()> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock was poisoned".to_owned())?;
    online_music::clear_search_history(&db).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn select_online_download_directory(
    initial_directory: Option<String>,
) -> CommandResult<Option<String>> {
    let mut dialog =
        rfd::AsyncFileDialog::new().set_title("Choose an Online Music download folder");
    if let Some(directory) = initial_directory
        .as_deref()
        .map(str::trim)
        .filter(|directory| Path::new(directory).is_dir())
    {
        dialog = dialog.set_directory(directory);
    }
    let folder = dialog.pick_folder().await;
    Ok(folder.map(|handle| handle.path().to_string_lossy().into_owned()))
}
