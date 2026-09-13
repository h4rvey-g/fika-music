use std::collections::{HashMap, HashSet, VecDeque};

use rand::seq::SliceRandom;
use rand::Rng;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::online_music::OnlineTrack;
use super::LocalTrack;

const ACTIVE_SESSION_ID: i64 = 1;
const HISTORY_LIMIT: usize = 100;
const MAX_QUEUE_PAGE_SIZE: usize = 200;
const MAX_CONSECUTIVE_FAILURES: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum PlaybackMode {
    Sequential,
    Shuffle,
    Repeat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export_to = "bindings.ts")]
pub enum PlaybackTrackInput {
    Local { track_id: i64 },
    Online { track: Box<OnlineTrack> },
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export_to = "bindings.ts")]
pub enum PlaybackQueueItem {
    Local { id: String, track: Box<LocalTrack> },
    Online { id: String, track: Box<OnlineTrack> },
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct PlaybackSessionSnapshot {
    pub revision: u64,
    pub mode: PlaybackMode,
    pub current: Option<PlaybackQueueItem>,
    pub upcoming: Vec<PlaybackQueueItem>,
    pub upcoming_offset: usize,
    pub upcoming_total: usize,
    pub history_count: usize,
    pub can_go_previous: bool,
    pub can_go_next: bool,
    pub position_seconds: f64,
    pub paused: bool,
    pub stream_open: bool,
    pub consecutive_failures: u8,
}

#[derive(Debug, thiserror::Error)]
pub enum PlaybackSessionError {
    #[error("playback context is empty")]
    EmptyContext,
    #[error("playback context start index is out of range")]
    StartOutOfRange,
    #[error("playback queue index is out of range")]
    QueueIndexOutOfRange,
    #[error("playback session data is invalid: {0}")]
    InvalidData(#[from] serde_json::Error),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum StoredTrack {
    Local { track_id: i64 },
    Online { track: Box<OnlineTrack> },
}

impl StoredTrack {
    fn identity(&self) -> String {
        match self {
            Self::Local { track_id } => format!("local:{track_id}"),
            Self::Online { track } => format!("online:{}", track.key),
        }
    }
}

impl From<PlaybackTrackInput> for StoredTrack {
    fn from(value: PlaybackTrackInput) -> Self {
        match value {
            PlaybackTrackInput::Local { track_id } => Self::Local { track_id },
            PlaybackTrackInput::Online { track } => Self::Online { track },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlaybackEntry {
    id: String,
    track: StoredTrack,
    context_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlaybackSession {
    revision: u64,
    mode: PlaybackMode,
    current: Option<PlaybackEntry>,
    upcoming: Vec<PlaybackEntry>,
    baseline: Vec<String>,
    history: VecDeque<PlaybackEntry>,
    context: Vec<StoredTrack>,
    context_cursor: Option<usize>,
    position_seconds: f64,
    paused: bool,
    continuation_enabled: bool,
    stream_open: bool,
    consecutive_failures: u8,
}

impl Default for PlaybackSession {
    fn default() -> Self {
        Self {
            revision: 0,
            mode: PlaybackMode::Sequential,
            current: None,
            upcoming: Vec::new(),
            baseline: Vec::new(),
            history: VecDeque::new(),
            context: Vec::new(),
            context_cursor: None,
            position_seconds: 0.0,
            paused: true,
            continuation_enabled: false,
            stream_open: false,
            consecutive_failures: 0,
        }
    }
}

impl PlaybackSession {
    pub(crate) fn load(connection: &Connection) -> Result<Self, PlaybackSessionError> {
        let state_json = connection
            .query_row(
                "SELECT state_json FROM playback_session_state WHERE id = ?1",
                [ACTIVE_SESSION_ID],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let mut session = state_json
            .map(|json| serde_json::from_str::<Self>(&json).map_err(PlaybackSessionError::from))
            .unwrap_or_else(|| Ok(Self::default()))?;
        session.paused = true;
        session.stream_open = false;
        Ok(session)
    }

    pub(crate) fn persist(&self, connection: &Connection) -> Result<(), PlaybackSessionError> {
        let state_json = serde_json::to_string(self)?;
        connection.execute(
            "INSERT INTO playback_session_state (id, state_json, updated_at)
             VALUES (?1, ?2, unixepoch())
             ON CONFLICT(id) DO UPDATE SET
                 state_json = excluded.state_json,
                 updated_at = excluded.updated_at",
            params![ACTIVE_SESSION_ID, state_json],
        )?;
        Ok(())
    }

    pub(crate) fn prune_unavailable_local_tracks(
        &mut self,
        mut exists: impl FnMut(i64) -> bool,
    ) -> bool {
        let local_ids: HashSet<i64> = self
            .context
            .iter()
            .chain(self.current.iter().map(|entry| &entry.track))
            .chain(self.upcoming.iter().map(|entry| &entry.track))
            .chain(self.history.iter().map(|entry| &entry.track))
            .filter_map(|track| match track {
                StoredTrack::Local { track_id } => Some(*track_id),
                StoredTrack::Online { .. } => None,
            })
            .collect();
        let available: HashSet<i64> = local_ids.into_iter().filter(|id| exists(*id)).collect();
        let original_context_len = self.context.len();
        let original_event_count =
            usize::from(self.current.is_some()) + self.upcoming.len() + self.history.len();
        let mut context_index_map = vec![None; original_context_len];
        let mut context = Vec::with_capacity(original_context_len);
        for (old_index, track) in std::mem::take(&mut self.context).into_iter().enumerate() {
            if stored_track_is_available(&track, &available) {
                context_index_map[old_index] = Some(context.len());
                context.push(track);
            }
        }
        self.context = context;

        let had_current = self.current.is_some();
        if self
            .current
            .as_mut()
            .is_some_and(|entry| !remap_available_entry(entry, &context_index_map, &available))
        {
            self.current = None;
        }
        self.upcoming
            .retain_mut(|entry| remap_available_entry(entry, &context_index_map, &available));
        self.history = std::mem::take(&mut self.history)
            .into_iter()
            .filter_map(|mut entry| {
                remap_available_entry(&mut entry, &context_index_map, &available).then_some(entry)
            })
            .collect();
        self.context_cursor = self
            .context_cursor
            .and_then(|index| context_index_map.get(index).copied().flatten());

        if had_current && self.current.is_none() {
            if let Some(next) = self.upcoming.first().cloned() {
                self.upcoming.remove(0);
                self.context_cursor = next.context_index.or(self.context_cursor);
                self.current = Some(next);
                self.position_seconds = 0.0;
            }
        }
        let upcoming_ids: HashSet<&str> = self
            .upcoming
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();
        self.baseline
            .retain(|id| upcoming_ids.contains(id.as_str()));

        let event_count =
            usize::from(self.current.is_some()) + self.upcoming.len() + self.history.len();
        let changed =
            self.context.len() != original_context_len || event_count != original_event_count;
        if changed {
            self.bump_revision();
        }
        changed
    }

    pub(crate) fn replace_context<R: Rng + ?Sized>(
        &mut self,
        tracks: Vec<PlaybackTrackInput>,
        start_index: usize,
        autoplay: bool,
        mode: PlaybackMode,
        stream_open: bool,
        rng: &mut R,
    ) -> Result<(), PlaybackSessionError> {
        let tracks: Vec<StoredTrack> = tracks.into_iter().map(Into::into).collect();
        self.replace_stored_context(tracks, start_index, autoplay, mode, stream_open, rng)
    }

    pub(crate) fn replace_local_context<R: Rng + ?Sized>(
        &mut self,
        track_ids: Vec<i64>,
        start_index: usize,
        autoplay: bool,
        mode: PlaybackMode,
        rng: &mut R,
    ) -> Result<(), PlaybackSessionError> {
        let tracks = track_ids
            .into_iter()
            .map(|track_id| StoredTrack::Local { track_id })
            .collect();
        self.replace_stored_context(tracks, start_index, autoplay, mode, false, rng)
    }

    fn replace_stored_context<R: Rng + ?Sized>(
        &mut self,
        tracks: Vec<StoredTrack>,
        start_index: usize,
        autoplay: bool,
        mode: PlaybackMode,
        stream_open: bool,
        rng: &mut R,
    ) -> Result<(), PlaybackSessionError> {
        if tracks.is_empty() {
            return Err(PlaybackSessionError::EmptyContext);
        }
        if start_index >= tracks.len() {
            return Err(PlaybackSessionError::StartOutOfRange);
        }

        self.mode = mode;
        self.context = tracks;
        self.upcoming.clear();
        self.baseline.clear();
        self.continuation_enabled = true;
        self.stream_open = stream_open;
        self.consecutive_failures = 0;

        if autoplay {
            self.position_seconds = 0.0;
            self.paused = false;
            self.current = Some(self.context_entry(start_index));
            self.context_cursor = Some(start_index);
            self.history.clear();
            self.plan_after_current(start_index, rng);
        } else {
            if let Some(current) = &mut self.current {
                current.context_index = None;
            }
            self.context_cursor = None;
            self.plan_queued_context(start_index, rng);
        }
        self.bump_revision();
        Ok(())
    }

    fn plan_after_current<R: Rng + ?Sized>(&mut self, start_index: usize, rng: &mut R) {
        let indices = match self.mode {
            PlaybackMode::Sequential => ((start_index + 1)..self.context.len()).collect(),
            PlaybackMode::Repeat if !self.stream_open => ((start_index + 1)..self.context.len())
                .chain(0..=start_index)
                .collect(),
            PlaybackMode::Repeat => ((start_index + 1)..self.context.len()).collect(),
            PlaybackMode::Shuffle => (0..self.context.len())
                .filter(|index| *index != start_index)
                .collect(),
        };
        self.install_context_entries(indices, self.mode == PlaybackMode::Shuffle, rng);
    }

    fn plan_queued_context<R: Rng + ?Sized>(&mut self, start_index: usize, rng: &mut R) {
        let mut indices: Vec<usize> = match self.mode {
            PlaybackMode::Sequential => (start_index..self.context.len()).collect(),
            PlaybackMode::Repeat if !self.stream_open => (start_index..self.context.len())
                .chain(0..start_index)
                .collect(),
            PlaybackMode::Repeat => (start_index..self.context.len()).collect(),
            PlaybackMode::Shuffle => (0..self.context.len())
                .filter(|index| *index != start_index)
                .collect(),
        };
        if self.mode == PlaybackMode::Shuffle {
            indices.shuffle(rng);
            indices.insert(0, start_index);
        }
        self.install_context_entries(indices, false, rng);
    }

    fn install_context_entries<R: Rng + ?Sized>(
        &mut self,
        indices: Vec<usize>,
        shuffle: bool,
        rng: &mut R,
    ) {
        let mut entries: Vec<PlaybackEntry> = indices
            .into_iter()
            .map(|index| self.context_entry(index))
            .collect();
        self.baseline = entries.iter().map(|entry| entry.id.clone()).collect();
        if shuffle {
            entries.shuffle(rng);
        }
        self.upcoming = entries;
    }

    pub(crate) fn play_next(&mut self, tracks: Vec<PlaybackTrackInput>) {
        if tracks.is_empty() {
            return;
        }
        let entries: Vec<PlaybackEntry> = tracks
            .into_iter()
            .map(|track| PlaybackEntry {
                id: Uuid::new_v4().to_string(),
                track: track.into(),
                context_index: None,
            })
            .collect();
        let ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
        self.upcoming.splice(0..0, entries);
        self.baseline.splice(0..0, ids);
        self.continuation_enabled = true;
        self.consecutive_failures = 0;
        self.bump_revision();
    }

    pub(crate) fn append_stream_batch<R: Rng + ?Sized>(
        &mut self,
        tracks: Vec<PlaybackTrackInput>,
        rng: &mut R,
    ) {
        let mut seen: HashSet<String> = self.context.iter().map(StoredTrack::identity).collect();
        let start_index = self.context.len();
        let additions: Vec<StoredTrack> = tracks
            .into_iter()
            .map(StoredTrack::from)
            .filter(|track| seen.insert(track.identity()))
            .collect();
        if additions.is_empty() {
            return;
        }
        self.context.extend(additions);
        let mut entries: Vec<PlaybackEntry> = (start_index..self.context.len())
            .map(|index| self.context_entry(index))
            .collect();
        if self.mode == PlaybackMode::Shuffle {
            entries.shuffle(rng);
        }
        self.baseline
            .extend(entries.iter().map(|entry| entry.id.clone()));
        self.upcoming.extend(entries);
        self.bump_revision();
    }

    pub(crate) fn close_stream<R: Rng + ?Sized>(&mut self, rng: &mut R) {
        if !self.stream_open {
            return;
        }
        self.stream_open = false;
        self.ensure_continuation(rng);
        self.bump_revision();
    }

    pub(crate) fn set_mode<R: Rng + ?Sized>(&mut self, mode: PlaybackMode, rng: &mut R) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        if !self.continuation_enabled && self.upcoming.is_empty() {
            self.bump_revision();
            return;
        }

        match mode {
            PlaybackMode::Sequential => self.sort_by_baseline(),
            PlaybackMode::Shuffle => self.upcoming.shuffle(rng),
            PlaybackMode::Repeat => {
                self.sort_by_baseline();
                if !self.stream_open {
                    self.append_repeat_wrap();
                }
            }
        }
        self.bump_revision();
    }

    pub(crate) fn move_upcoming(
        &mut self,
        from: usize,
        to: usize,
    ) -> Result<(), PlaybackSessionError> {
        if from >= self.upcoming.len() || to >= self.upcoming.len() {
            return Err(PlaybackSessionError::QueueIndexOutOfRange);
        }
        if from == to {
            return Ok(());
        }
        let entry = self.upcoming.remove(from);
        self.upcoming.insert(to, entry);
        self.baseline = self.upcoming.iter().map(|entry| entry.id.clone()).collect();
        self.bump_revision();
        Ok(())
    }

    pub(crate) fn remove_upcoming(&mut self, index: usize) -> Result<(), PlaybackSessionError> {
        if index >= self.upcoming.len() {
            return Err(PlaybackSessionError::QueueIndexOutOfRange);
        }
        let removed = self.upcoming.remove(index);
        self.baseline.retain(|id| id != &removed.id);
        if self.upcoming.is_empty() {
            self.continuation_enabled = false;
            self.stream_open = false;
        }
        self.bump_revision();
        Ok(())
    }

    pub(crate) fn clear_upcoming(&mut self) {
        self.upcoming.clear();
        self.baseline.clear();
        self.context.clear();
        self.context_cursor = None;
        self.continuation_enabled = false;
        self.stream_open = false;
        self.consecutive_failures = 0;
        self.bump_revision();
    }

    pub(crate) fn select_upcoming<R: Rng + ?Sized>(
        &mut self,
        index: usize,
        rng: &mut R,
    ) -> Result<bool, PlaybackSessionError> {
        if index >= self.upcoming.len() {
            return Err(PlaybackSessionError::QueueIndexOutOfRange);
        }
        let discarded: HashSet<String> = self
            .upcoming
            .drain(0..index)
            .map(|entry| entry.id)
            .collect();
        self.baseline.retain(|id| !discarded.contains(id));
        self.advance(rng)
    }

    pub(crate) fn advance<R: Rng + ?Sized>(
        &mut self,
        rng: &mut R,
    ) -> Result<bool, PlaybackSessionError> {
        self.advance_inner(true, rng)
    }

    fn advance_inner<R: Rng + ?Sized>(
        &mut self,
        include_current_in_history: bool,
        rng: &mut R,
    ) -> Result<bool, PlaybackSessionError> {
        self.ensure_continuation(rng);
        if self.upcoming.is_empty() {
            self.paused = true;
            self.bump_revision();
            return Ok(false);
        }

        let next = self.upcoming.remove(0);
        self.baseline.retain(|id| id != &next.id);
        if include_current_in_history {
            if let Some(current) = self.current.take() {
                self.push_history(current);
            }
        } else {
            self.current = None;
        }
        self.context_cursor = next.context_index.or(self.context_cursor);
        self.current = Some(next);
        self.position_seconds = 0.0;
        self.paused = false;
        self.ensure_continuation(rng);
        self.bump_revision();
        Ok(true)
    }

    pub(crate) fn previous(&mut self) -> bool {
        let Some(previous) = self.history.pop_back() else {
            return false;
        };
        if let Some(current) = self.current.replace(previous) {
            self.baseline.insert(0, current.id.clone());
            self.upcoming.insert(0, current);
        }
        self.context_cursor = self
            .current
            .as_ref()
            .and_then(|entry| entry.context_index)
            .or(self.context_cursor);
        self.position_seconds = 0.0;
        self.paused = false;
        self.consecutive_failures = 0;
        self.bump_revision();
        true
    }

    pub(crate) fn playback_failed<R: Rng + ?Sized>(
        &mut self,
        rng: &mut R,
    ) -> Result<bool, PlaybackSessionError> {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            self.current = None;
            self.paused = true;
            self.bump_revision();
            return Ok(false);
        }
        self.advance_inner(false, rng)
    }

    pub(crate) fn mark_started(&mut self) {
        self.paused = false;
        self.consecutive_failures = 0;
        self.bump_revision();
    }

    pub(crate) fn reset_failures(&mut self) {
        self.consecutive_failures = 0;
    }

    pub(crate) fn set_paused(&mut self, paused: bool) {
        if self.paused != paused {
            self.paused = paused;
            self.bump_revision();
        }
    }

    pub(crate) fn save_progress(&mut self, position_seconds: f64) {
        if position_seconds.is_finite() && position_seconds >= 0.0 {
            self.position_seconds = position_seconds;
        }
    }

    pub(crate) fn snapshot<F>(
        &self,
        offset: usize,
        limit: usize,
        mut local_track: F,
    ) -> PlaybackSessionSnapshot
    where
        F: FnMut(i64) -> Option<LocalTrack>,
    {
        let limit = limit.clamp(1, MAX_QUEUE_PAGE_SIZE);
        let current = self
            .current
            .as_ref()
            .and_then(|entry| materialize_entry(entry, &mut local_track));
        let upcoming = self
            .upcoming
            .iter()
            .skip(offset)
            .take(limit)
            .filter_map(|entry| materialize_entry(entry, &mut local_track))
            .collect();
        PlaybackSessionSnapshot {
            revision: self.revision,
            mode: self.mode,
            current,
            upcoming,
            upcoming_offset: offset,
            upcoming_total: self.upcoming.len(),
            history_count: self.history.len(),
            can_go_previous: !self.history.is_empty(),
            can_go_next: !self.upcoming.is_empty()
                || (self.continuation_enabled
                    && (self.stream_open || self.mode != PlaybackMode::Sequential)),
            position_seconds: self.position_seconds,
            paused: self.paused,
            stream_open: self.stream_open,
            consecutive_failures: self.consecutive_failures,
        }
    }

    fn append_repeat_wrap(&mut self) {
        let Some(cursor) = self.context_cursor else {
            return;
        };
        let existing: HashSet<usize> = self
            .upcoming
            .iter()
            .filter_map(|entry| entry.context_index)
            .collect();
        let entries: Vec<PlaybackEntry> = (0..=cursor)
            .filter(|index| !existing.contains(index))
            .map(|index| self.context_entry(index))
            .collect();
        self.baseline
            .extend(entries.iter().map(|entry| entry.id.clone()));
        self.upcoming.extend(entries);
    }

    fn ensure_continuation<R: Rng + ?Sized>(&mut self, rng: &mut R) {
        if !self.upcoming.is_empty()
            || !self.continuation_enabled
            || self.stream_open
            || self.context.is_empty()
        {
            return;
        }
        let indices: Vec<usize> = match self.mode {
            PlaybackMode::Sequential => return,
            PlaybackMode::Repeat => match self.context_cursor {
                Some(cursor) => ((cursor + 1)..self.context.len())
                    .chain(0..=cursor)
                    .collect(),
                None => (0..self.context.len()).collect(),
            },
            PlaybackMode::Shuffle => (0..self.context.len()).collect(),
        };
        self.install_context_entries(indices, self.mode == PlaybackMode::Shuffle, rng);
    }

    fn sort_by_baseline(&mut self) {
        let positions: HashMap<&str, usize> = self
            .baseline
            .iter()
            .enumerate()
            .map(|(index, id)| (id.as_str(), index))
            .collect();
        self.upcoming.sort_by_key(|entry| {
            positions
                .get(entry.id.as_str())
                .copied()
                .unwrap_or(usize::MAX)
        });
    }

    fn context_entry(&self, index: usize) -> PlaybackEntry {
        PlaybackEntry {
            id: Uuid::new_v4().to_string(),
            track: self.context[index].clone(),
            context_index: Some(index),
        }
    }

    fn push_history(&mut self, entry: PlaybackEntry) {
        self.history.push_back(entry);
        while self.history.len() > HISTORY_LIMIT {
            self.history.pop_front();
        }
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

fn stored_track_is_available(track: &StoredTrack, available_local_ids: &HashSet<i64>) -> bool {
    match track {
        StoredTrack::Local { track_id } => available_local_ids.contains(track_id),
        StoredTrack::Online { .. } => true,
    }
}

fn remap_available_entry(
    entry: &mut PlaybackEntry,
    context_index_map: &[Option<usize>],
    available_local_ids: &HashSet<i64>,
) -> bool {
    if !stored_track_is_available(&entry.track, available_local_ids) {
        return false;
    }
    let Some(old_index) = entry.context_index else {
        return true;
    };
    let Some(new_index) = context_index_map.get(old_index).copied().flatten() else {
        return false;
    };
    entry.context_index = Some(new_index);
    true
}

fn materialize_entry(
    entry: &PlaybackEntry,
    local_track: &mut impl FnMut(i64) -> Option<LocalTrack>,
) -> Option<PlaybackQueueItem> {
    match &entry.track {
        StoredTrack::Local { track_id } => {
            local_track(*track_id).map(|track| PlaybackQueueItem::Local {
                id: entry.id.clone(),
                track: Box::new(track),
            })
        }
        StoredTrack::Online { track } => Some(PlaybackQueueItem::Online {
            id: entry.id.clone(),
            track: track.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    use super::*;

    fn local(track_id: i64) -> PlaybackTrackInput {
        PlaybackTrackInput::Local { track_id }
    }

    fn current_id(session: &PlaybackSession) -> Option<i64> {
        match &session.current.as_ref()?.track {
            StoredTrack::Local { track_id } => Some(*track_id),
            StoredTrack::Online { .. } => None,
        }
    }

    fn upcoming_ids(session: &PlaybackSession) -> Vec<i64> {
        session
            .upcoming
            .iter()
            .filter_map(|entry| match entry.track {
                StoredTrack::Local { track_id } => Some(track_id),
                StoredTrack::Online { .. } => None,
            })
            .collect()
    }

    fn session(mode: PlaybackMode) -> PlaybackSession {
        let mut session = PlaybackSession::default();
        session
            .replace_context(
                vec![local(1), local(2), local(3), local(4)],
                1,
                true,
                mode,
                false,
                &mut StdRng::seed_from_u64(7),
            )
            .expect("context should be valid");
        session
    }

    #[test]
    fn sequential_should_plan_from_current_to_the_end() {
        let session = session(PlaybackMode::Sequential);

        assert_eq!(upcoming_ids(&session), vec![3, 4]);
    }

    #[test]
    fn repeat_should_plan_the_tail_then_wrap_to_current() {
        let session = session(PlaybackMode::Repeat);

        assert_eq!(upcoming_ids(&session), vec![3, 4, 1, 2]);
    }

    #[test]
    fn shuffle_should_exclude_the_current_occurrence() {
        let session = session(PlaybackMode::Shuffle);
        let upcoming = upcoming_ids(&session);

        assert_eq!(upcoming.len(), 3);
        assert!(!upcoming.contains(&2));
    }

    #[test]
    fn play_next_should_put_the_latest_request_first() {
        let mut session = session(PlaybackMode::Sequential);
        session.play_next(vec![local(8)]);
        session.play_next(vec![local(9)]);

        assert_eq!(upcoming_ids(&session), vec![9, 8, 3, 4]);
    }

    #[test]
    fn previous_should_restore_the_departed_current_at_the_queue_head() {
        let mut session = session(PlaybackMode::Sequential);
        let mut rng = StdRng::seed_from_u64(7);
        session.advance(&mut rng).expect("advance should succeed");
        session.previous();

        assert_eq!(
            (current_id(&session), upcoming_ids(&session)),
            (Some(2), vec![3, 4])
        );
    }

    #[test]
    fn sequential_should_restore_the_user_baseline_after_shuffle() {
        let mut session = session(PlaybackMode::Sequential);
        session
            .move_upcoming(1, 0)
            .expect("queue move should succeed");
        let mut rng = StdRng::seed_from_u64(11);
        session.set_mode(PlaybackMode::Shuffle, &mut rng);
        session.set_mode(PlaybackMode::Sequential, &mut rng);

        assert_eq!(upcoming_ids(&session), vec![4, 3]);
    }

    #[test]
    fn deleted_context_item_should_return_only_in_the_next_cycle() {
        let mut session = session(PlaybackMode::Sequential);
        session
            .remove_upcoming(0)
            .expect("queue removal should succeed");
        let mut rng = StdRng::seed_from_u64(3);
        session.set_mode(PlaybackMode::Repeat, &mut rng);

        assert_eq!(upcoming_ids(&session), vec![4, 1, 2]);
    }

    #[test]
    fn sequential_mode_should_keep_all_unconsumed_repeat_members() {
        let mut session = session(PlaybackMode::Repeat);
        session.set_mode(PlaybackMode::Sequential, &mut StdRng::seed_from_u64(3));

        assert_eq!(upcoming_ids(&session), vec![3, 4, 1, 2]);
    }

    #[test]
    fn later_shuffle_cycles_should_include_the_complete_context() {
        let mut session = session(PlaybackMode::Shuffle);
        let mut rng = StdRng::seed_from_u64(3);
        for _ in 0..3 {
            session.advance(&mut rng).expect("advance should succeed");
        }

        let mut upcoming = upcoming_ids(&session);
        upcoming.sort_unstable();
        assert_eq!(upcoming, vec![1, 2, 3, 4]);
    }

    #[test]
    fn unavailable_local_tracks_should_be_removed_and_current_should_advance() {
        let mut session = session(PlaybackMode::Sequential);

        assert!(
            session.prune_unavailable_local_tracks(|track_id| { track_id != 2 && track_id != 3 })
        );

        assert_eq!(current_id(&session), Some(4));
        assert!(upcoming_ids(&session).is_empty());
    }

    #[test]
    fn replacing_only_the_future_should_preserve_current_progress_and_play_state() {
        let mut session = session(PlaybackMode::Sequential);
        session.save_progress(42.0);

        session
            .replace_context(
                vec![local(8), local(9)],
                0,
                false,
                PlaybackMode::Sequential,
                false,
                &mut StdRng::seed_from_u64(3),
            )
            .expect("replacement context should be valid");

        assert_eq!(current_id(&session), Some(2));
        assert_eq!(upcoming_ids(&session), vec![8, 9]);
        assert_eq!(session.position_seconds, 42.0);
        assert!(!session.paused);
    }

    #[test]
    fn persisted_session_should_restore_paused_and_freeze_an_open_stream() {
        let connection = Connection::open_in_memory().expect("database should open");
        connection
            .execute_batch(
                "CREATE TABLE playback_session_state (
                    id INTEGER PRIMARY KEY CHECK(id = 1),
                    state_json TEXT NOT NULL,
                    updated_at INTEGER NOT NULL
                );",
            )
            .expect("session table should be created");
        let mut session = PlaybackSession::default();
        session
            .replace_context(
                vec![local(1), local(2)],
                0,
                true,
                PlaybackMode::Sequential,
                true,
                &mut StdRng::seed_from_u64(3),
            )
            .expect("stream context should be valid");
        session.save_progress(37.0);
        session
            .persist(&connection)
            .expect("session should persist");

        let restored = PlaybackSession::load(&connection).expect("session should restore");

        assert_eq!(current_id(&restored), Some(1));
        assert_eq!(restored.position_seconds, 37.0);
        assert!(restored.paused);
        assert!(!restored.stream_open);
    }

    #[test]
    fn playback_failure_should_stop_after_three_consecutive_tracks() {
        let mut session = session(PlaybackMode::Repeat);
        let mut rng = StdRng::seed_from_u64(3);
        assert!(session.playback_failed(&mut rng).unwrap());
        assert!(session.playback_failed(&mut rng).unwrap());

        assert!(!session.playback_failed(&mut rng).unwrap());
        assert_eq!(current_id(&session), None);
    }
}
