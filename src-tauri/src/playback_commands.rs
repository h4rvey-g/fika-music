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

const PLAYBACK_SESSION_PAGE_SIZE: usize = 200;

fn playback_session_snapshot(
    state: &AppState,
    offset: usize,
    limit: usize,
) -> CommandResult<PlaybackSessionSnapshot> {
    let session = state
        .playback_session
        .lock()
        .map_err(|_| AppError::StatePoisoned("playback_session").to_string())?
        .clone();
    let library = state
        .library
        .lock()
        .map_err(|_| AppError::StatePoisoned("library").to_string())?;
    Ok(session.snapshot(offset, limit, |track_id| library.track(track_id).cloned()))
}

fn update_playback_session(
    state: &AppState,
    update: impl FnOnce(
        &mut playback_session::PlaybackSession,
    ) -> Result<(), playback_session::PlaybackSessionError>,
) -> CommandResult<PlaybackSessionSnapshot> {
    let mut session = state
        .playback_session
        .lock()
        .map_err(|_| AppError::StatePoisoned("playback_session").to_string())?;
    let mut candidate = session.clone();
    update(&mut candidate).map_err(|error| error.to_string())?;
    {
        let db = state
            .db
            .lock()
            .map_err(|_| AppError::StatePoisoned("db").to_string())?;
        candidate.persist(&db).map_err(|error| error.to_string())?;
    }
    *session = candidate;
    drop(session);
    playback_session_snapshot(state, 0, PLAYBACK_SESSION_PAGE_SIZE)
}

#[tauri::command]
pub(crate) fn get_playback_session(
    state: State<'_, AppState>,
    offset: usize,
    limit: usize,
) -> CommandResult<PlaybackSessionSnapshot> {
    playback_session_snapshot(state.inner(), offset, limit)
}

#[tauri::command]
pub(crate) fn replace_playback_session(
    state: State<'_, AppState>,
    tracks: Vec<PlaybackTrackInput>,
    start_index: usize,
    autoplay: bool,
    mode: PlaybackMode,
    stream_open: bool,
) -> CommandResult<PlaybackSessionSnapshot> {
    update_playback_session(state.inner(), move |session| {
        session.replace_context(
            tracks,
            start_index,
            autoplay,
            mode,
            stream_open,
            &mut rand::thread_rng(),
        )
    })
}

#[tauri::command]
pub(crate) fn replace_local_playback_session(
    state: State<'_, AppState>,
    queue_id: String,
    start_index: usize,
    autoplay: bool,
    mode: PlaybackMode,
) -> CommandResult<PlaybackSessionSnapshot> {
    let track_ids = {
        let library = state
            .library
            .lock()
            .map_err(|_| AppError::StatePoisoned("library").to_string())?;
        library
            .playback_queue_track_ids(queue_id.trim())
            .map_err(|error| error.to_string())?
    };
    update_playback_session(state.inner(), move |session| {
        session.replace_local_context(
            track_ids,
            start_index,
            autoplay,
            mode,
            &mut rand::thread_rng(),
        )
    })
}

#[tauri::command]
pub(crate) fn play_next_in_playback_session(
    state: State<'_, AppState>,
    tracks: Vec<PlaybackTrackInput>,
) -> CommandResult<PlaybackSessionSnapshot> {
    update_playback_session(state.inner(), move |session| {
        session.play_next(tracks);
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn append_playback_session_stream(
    state: State<'_, AppState>,
    tracks: Vec<PlaybackTrackInput>,
) -> CommandResult<PlaybackSessionSnapshot> {
    update_playback_session(state.inner(), move |session| {
        session.append_stream_batch(tracks, &mut rand::thread_rng());
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn close_playback_session_stream(
    state: State<'_, AppState>,
) -> CommandResult<PlaybackSessionSnapshot> {
    update_playback_session(state.inner(), |session| {
        session.close_stream(&mut rand::thread_rng());
        Ok(())
    })
}

#[tauri::command]
pub(crate) async fn set_playback_session_mode(
    app: AppHandle,
    mode: PlaybackMode,
) -> CommandResult<PlaybackSessionSnapshot> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        update_playback_session(state.inner(), move |session| {
            session.set_mode(mode, &mut rand::thread_rng());
            Ok(())
        })
    })
    .await
    .map_err(|error| format!("playback mode update task failed: {error}"))?
}

#[tauri::command]
pub(crate) fn move_playback_session_item(
    state: State<'_, AppState>,
    from: usize,
    to: usize,
) -> CommandResult<PlaybackSessionSnapshot> {
    update_playback_session(state.inner(), move |session| {
        session.move_upcoming(from, to)
    })
}

#[tauri::command]
pub(crate) fn remove_playback_session_item(
    state: State<'_, AppState>,
    index: usize,
) -> CommandResult<PlaybackSessionSnapshot> {
    update_playback_session(state.inner(), move |session| session.remove_upcoming(index))
}

#[tauri::command]
pub(crate) fn clear_playback_session_upcoming(
    state: State<'_, AppState>,
) -> CommandResult<PlaybackSessionSnapshot> {
    update_playback_session(state.inner(), |session| {
        session.clear_upcoming();
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn select_playback_session_item(
    state: State<'_, AppState>,
    index: usize,
) -> CommandResult<PlaybackSessionSnapshot> {
    update_playback_session(state.inner(), move |session| {
        session.reset_failures();
        session.select_upcoming(index, &mut rand::thread_rng())?;
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn next_playback_session_item(
    state: State<'_, AppState>,
    manual: bool,
) -> CommandResult<PlaybackSessionSnapshot> {
    update_playback_session(state.inner(), |session| {
        if manual {
            session.reset_failures();
        }
        session.advance(&mut rand::thread_rng())?;
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn previous_playback_session_item(
    state: State<'_, AppState>,
) -> CommandResult<PlaybackSessionSnapshot> {
    update_playback_session(state.inner(), |session| {
        session.previous();
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn fail_playback_session_current(
    state: State<'_, AppState>,
) -> CommandResult<PlaybackSessionSnapshot> {
    update_playback_session(state.inner(), |session| {
        session.playback_failed(&mut rand::thread_rng())?;
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn mark_playback_session_started(
    state: State<'_, AppState>,
) -> CommandResult<PlaybackSessionSnapshot> {
    update_playback_session(state.inner(), |session| {
        session.mark_started();
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn set_playback_session_paused(
    state: State<'_, AppState>,
    paused: bool,
) -> CommandResult<PlaybackSessionSnapshot> {
    update_playback_session(state.inner(), move |session| {
        session.set_paused(paused);
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn save_playback_session_progress(
    state: State<'_, AppState>,
    position_seconds: f64,
) -> CommandResult<()> {
    let mut session = state
        .playback_session
        .lock()
        .map_err(|_| AppError::StatePoisoned("playback_session").to_string())?;
    session.save_progress(position_seconds);
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::StatePoisoned("db").to_string())?;
    session.persist(&db).map_err(|error| error.to_string())
}
