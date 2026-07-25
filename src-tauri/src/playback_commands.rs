use super::*;

#[tauri::command]
pub(crate) fn local_track_media_source(
    app: AppHandle,
    state: State<'_, AppState>,
    track_id: i64,
) -> CommandResult<MediaSource> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::StatePoisoned("db").to_string())?;

    let file_path: String = db
        .query_row(
            "SELECT file_path FROM local_tracks WHERE id = ?1",
            params![track_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| AppError::TrackNotFound(track_id).to_string())?;

    if !Path::new(&file_path).is_file() {
        return Err(AppError::TrackFileMissing(file_path).to_string());
    }

    app.asset_protocol_scope()
        .allow_file(&file_path)
        .map_err(|error| error.to_string())?;

    Ok(MediaSource { file_path })
}

#[tauri::command]
pub(crate) async fn local_track_playback_details(
    state: State<'_, AppState>,
    track_id: i64,
) -> CommandResult<lyrics::LocalTrackPlaybackDetails> {
    let track = {
        let db = state
            .db
            .lock()
            .map_err(|_| AppError::StatePoisoned("db").to_string())?;
        track_by_id(&db, track_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| AppError::TrackNotFound(track_id).to_string())?
    };
    let path = PathBuf::from(&track.file_path);
    if !path.is_file() {
        return Err(AppError::TrackFileMissing(track.file_path).to_string());
    }
    let query = lyrics::TrackLyricsQuery::new(
        track.title,
        track.artist,
        track.album,
        track.duration_seconds,
    );

    tauri::async_runtime::spawn_blocking(move || lyrics::resolve_local_track(&path, &query))
        .await
        .map_err(|error| format!("playback details task failed: {error}"))
}

#[tauri::command]
pub(crate) async fn resolve_remote_track_lyrics(
    query: lyrics::TrackLyricsQuery,
) -> CommandResult<Option<lyrics::ResolvedLyrics>> {
    tauri::async_runtime::spawn_blocking(move || lyrics::resolve_network_lyrics(&query))
        .await
        .map_err(|error| format!("remote lyrics task failed: {error}"))?
        .map_err(|error| error.to_string())
}
