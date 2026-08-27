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
    let playback_cache_max_mb = settings.playback_cache_max_mb;
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock was poisoned".to_owned())?;
    online_music::save_settings(&db, &settings, now_timestamp())
        .map_err(|error| error.to_string())?;
    drop(db);
    state.online_music_cache.invalidate();
    state.playback_cache.set_max_size_mb(playback_cache_max_mb);
    let playback_cache = Arc::clone(&state.playback_cache);
    let _task = tauri::async_runtime::spawn_blocking(move || {
        let _ = playback_cache.prune();
    });
    Ok(settings)
}

#[tauri::command]
pub(crate) fn get_cached_online_playback(
    state: State<'_, AppState>,
    track_key: String,
    qualities: Vec<source_runtime::SourceQuality>,
) -> CommandResult<Option<playback_cache::CachedOnlinePlayback>> {
    state
        .playback_cache
        .lookup(&track_key, &qualities)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn cache_online_playback(
    state: State<'_, AppState>,
    request: playback_cache::CacheOnlinePlaybackRequest,
) -> CommandResult<()> {
    let Some(cache_id) = state
        .playback_cache
        .reserve(&request)
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let playback_cache = Arc::clone(&state.playback_cache);
    let _task = tauri::async_runtime::spawn_blocking(move || {
        let _ = playback_cache.store(&request, &cache_id);
        playback_cache.release(&cache_id);
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn remove_cached_online_playback(
    state: State<'_, AppState>,
    cache_id: String,
) -> CommandResult<()> {
    state
        .playback_cache
        .remove(&cache_id)
        .map_err(|error| error.to_string())
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
#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) async fn select_online_download_directory(
    _initial_directory: Option<String>,
) -> CommandResult<Option<String>> {
    Ok(None)
}

#[tauri::command]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
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
