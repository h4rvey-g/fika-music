use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use natord::compare as natural_compare;
use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
use nucleo_matcher::{Config, Matcher, Utf32String};
use pinyin::ToPinyin;
use rayon::prelude::*;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use smallvec::SmallVec;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};
use uuid::Uuid;

use super::{list_tracks, LocalTrack, LIBRARY_METADATA_VERSION};

const MAX_PAGE_SIZE: usize = 200;
const MAX_SNAPSHOTS: usize = 4;
const MAX_PLAYBACK_QUEUES: usize = 8;
const ALBUM_HEADER_SLOTS: usize = 2;
const UNGROUPED_ID: &str = "ungrouped";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum LibraryTextField {
    Title,
    Artist,
    Album,
    AlbumArtist,
    Genre,
    Codec,
    FileName,
    FilePath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum LibrarySortField {
    Relevance,
    Title,
    Artist,
    Album,
    AlbumArtist,
    Genre,
    Year,
    Codec,
    BitrateKbps,
    SampleRateHz,
    DurationSeconds,
    TrackNumber,
    DiscNumber,
    FileName,
    FilePath,
    FileSizeBytes,
    ModifiedAt,
    IndexedAt,
    PlayCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum LibrarySortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LibraryQueryRequest {
    pub search: String,
    pub search_fields: Vec<LibraryTextField>,
    pub sort_field: LibrarySortField,
    pub sort_direction: LibrarySortDirection,
    #[serde(default)]
    pub collapsed_group_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum LibraryViewItemKind {
    AlbumHeader,
    AlbumContinuation,
    Track,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LibraryAlbumGroup {
    pub id: String,
    pub title: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<i64>,
    pub matched_tracks: usize,
    pub total_tracks: usize,
    pub total_duration_seconds: i64,
    pub start_index: usize,
    pub end_index: usize,
    pub is_ungrouped: bool,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LibraryViewItem {
    pub index: usize,
    pub kind: LibraryViewItemKind,
    pub group: Option<LibraryAlbumGroup>,
    pub track: Option<LocalTrack>,
    pub track_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LibraryQueryPage {
    pub snapshot_id: String,
    pub total: usize,
    pub library_total: usize,
    pub total_duration_seconds: i64,
    pub needs_reindex: bool,
    pub group_total: usize,
    pub virtual_total: usize,
    pub offset: usize,
    pub items: Vec<LibraryViewItem>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LibraryViewRange {
    pub snapshot_id: String,
    pub offset: usize,
    pub items: Vec<LibraryViewItem>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LibraryGroupToggleResult {
    pub snapshot_id: String,
    pub group_id: String,
    pub collapsed: bool,
    pub virtual_total: usize,
    pub group_virtual_index: usize,
    pub offset: usize,
    pub items: Vec<LibraryViewItem>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LibrarySelectionRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LibrarySelectionRequest {
    pub select_all: bool,
    pub ranges: Vec<LibrarySelectionRange>,
    pub excluded_ranges: Vec<LibrarySelectionRange>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LibraryPlaybackQueue {
    pub queue_id: String,
    pub total: usize,
    pub current_index: usize,
    pub track: LocalTrack,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LibraryQueueTrack {
    pub index: usize,
    pub track: LocalTrack,
}

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("library query snapshot expired; refresh the library view")]
    SnapshotExpired,
    #[error("library playback queue expired; start playback from the library again")]
    QueueExpired,
    #[error("library range starts beyond the available tracks")]
    RangeOutOfBounds,
    #[error("the selected library result is empty")]
    EmptySelection,
    #[error("the selected track no longer exists in the library")]
    TrackMissing,
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
}

#[derive(Debug, Clone)]
struct SearchText {
    searchable: Utf32String,
    sort_key: String,
}

impl SearchText {
    fn new(value: &str) -> Self {
        let normalized = normalize_text(value);
        let (full_pinyin, initials) = pinyin_keys(value);
        let searchable = format!("{normalized}\u{1f}{full_pinyin}\u{1f}{initials}").into();
        let sort_key = if full_pinyin.is_empty() {
            normalized.clone()
        } else {
            format!("{full_pinyin}\u{1f}{normalized}")
        };
        Self {
            searchable,
            sort_key,
        }
    }
}

#[derive(Debug, Clone)]
struct TrackSearchIndex {
    title: SearchText,
    artist: Option<SearchText>,
    album: Option<SearchText>,
    album_artist: Option<SearchText>,
    genre: Option<SearchText>,
    file_name: SearchText,
    file_path: SearchText,
    codec: Option<SearchText>,
}

impl TrackSearchIndex {
    fn new(track: &LocalTrack) -> Self {
        Self {
            title: SearchText::new(&track.title),
            artist: track.artist.as_deref().map(SearchText::new),
            album: track.album.as_deref().map(SearchText::new),
            album_artist: track.album_artist.as_deref().map(SearchText::new),
            genre: track.genre.as_deref().map(SearchText::new),
            file_name: SearchText::new(&track.file_name),
            file_path: SearchText::new(&track.file_path),
            codec: track.codec.as_deref().map(SearchText::new),
        }
    }

    fn text(&self, field: LibraryTextField) -> Option<&SearchText> {
        match field {
            LibraryTextField::Title => Some(&self.title),
            LibraryTextField::Artist => self.artist.as_ref(),
            LibraryTextField::Album => self.album.as_ref(),
            LibraryTextField::AlbumArtist => self.album_artist.as_ref(),
            LibraryTextField::Genre => self.genre.as_ref(),
            LibraryTextField::Codec => self.codec.as_ref(),
            LibraryTextField::FileName => Some(&self.file_name),
            LibraryTextField::FilePath => Some(&self.file_path),
        }
    }
}

#[derive(Debug, Clone)]
struct IndexedTrack {
    track: LocalTrack,
    search: TrackSearchIndex,
    group_id: String,
}

impl IndexedTrack {
    fn new(track: LocalTrack) -> Self {
        let search = TrackSearchIndex::new(&track);
        let group_id = album_identity(&track).id;
        Self {
            track,
            search,
            group_id,
        }
    }
}

#[derive(Debug, Clone)]
struct AlbumIdentity {
    id: String,
    title: Option<String>,
    album_artist: Option<String>,
    is_ungrouped: bool,
}

#[derive(Debug)]
struct AlbumIndex {
    identity: AlbumIdentity,
    track_indices: Vec<usize>,
    year: Option<i64>,
    total_duration_seconds: i64,
    default_sort_rank: usize,
}

#[derive(Debug)]
struct QueryAlbum<'a> {
    album: &'a AlbumIndex,
    matches: SmallVec<[(usize, u64); 16]>,
    max_score: u64,
    representative_index: usize,
}

#[derive(Debug, Clone, Copy)]
enum SnapshotViewItem {
    AlbumHeader(usize),
    AlbumContinuation(usize),
    Track(usize),
}

#[derive(Debug)]
struct QuerySnapshot {
    id: String,
    order: Vec<usize>,
    groups: Vec<LibraryAlbumGroup>,
    collapsed_group_ids: HashSet<String>,
    view: Vec<SnapshotViewItem>,
}

#[derive(Debug)]
struct PlaybackQueueSnapshot {
    id: String,
    track_ids: Vec<i64>,
}

#[derive(Debug, Default)]
pub struct LibraryService {
    tracks: Vec<IndexedTrack>,
    track_by_id: HashMap<i64, usize>,
    albums: HashMap<String, AlbumIndex>,
    snapshots: VecDeque<QuerySnapshot>,
    playback_queues: VecDeque<PlaybackQueueSnapshot>,
    needs_reindex: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct LibraryAlbumTarget {
    pub group_id: String,
    pub title: String,
    pub album_artist: String,
    pub year: Option<i64>,
    pub tracks: Vec<LocalTrack>,
}

impl LibraryService {
    pub fn load(connection: &Connection) -> Result<Self, LibraryError> {
        let tracks = list_tracks(connection)?;
        let needs_reindex = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM local_tracks WHERE metadata_version < ?1 LIMIT 1
            )",
            [LIBRARY_METADATA_VERSION],
            |row| row.get(0),
        )?;
        let mut service = Self {
            needs_reindex,
            ..Self::default()
        };
        service.replace_tracks(tracks);
        Ok(service)
    }

    pub fn reload(&mut self, connection: &Connection) -> Result<(), LibraryError> {
        let tracks = list_tracks(connection)?;
        self.needs_reindex = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM local_tracks WHERE metadata_version < ?1 LIMIT 1
            )",
            [LIBRARY_METADATA_VERSION],
            |row| row.get(0),
        )?;
        self.replace_tracks(tracks);
        Ok(())
    }

    pub fn query(&mut self, mut request: LibraryQueryRequest) -> LibraryQueryPage {
        if request.search_fields.is_empty() {
            request.search_fields = default_search_fields();
        }
        request
            .search_fields
            .sort_by_key(|field| text_field_rank(*field));
        request.search_fields.dedup();

        let search = request.search.trim();
        let atoms = search
            .split_whitespace()
            .map(|token| {
                Atom::new(
                    &normalize_text(token),
                    CaseMatching::Ignore,
                    Normalization::Smart,
                    AtomKind::Fuzzy,
                    false,
                )
            })
            .collect::<Vec<_>>();

        let mut matches = if atoms.is_empty() {
            (0..self.tracks.len())
                .map(|index| (index, 0_u64))
                .collect::<Vec<_>>()
        } else {
            self.tracks
                .par_iter()
                .enumerate()
                .map_init(
                    || Matcher::new(Config::DEFAULT.match_paths()),
                    |matcher, (index, track)| {
                        score_track(track, &request.search_fields, &atoms, matcher)
                            .map(|score| (index, score))
                    },
                )
                .filter_map(|matched| matched)
                .collect::<Vec<_>>()
        };

        let matched_total = matches.len();
        let mut grouped = HashMap::<&str, QueryAlbum<'_>>::new();
        for (index, score) in matches.drain(..) {
            let group_id = self.tracks[index].group_id.as_str();
            let group = grouped.entry(group_id).or_insert_with(|| {
                let album = self
                    .albums
                    .get(group_id)
                    .expect("indexed track should belong to an album");
                QueryAlbum {
                    album,
                    matches: SmallVec::new(),
                    max_score: 0,
                    representative_index: album.track_indices.first().copied().unwrap_or(index),
                }
            });
            group.max_score = group.max_score.max(score);
            group.matches.push((index, score));
        }
        let has_search = !atoms.is_empty();
        let mut query_albums = grouped.into_values().collect::<Vec<_>>();
        for album in &mut query_albums {
            if album.matches.len() > 1 {
                sort_group_tracks(
                    album.matches.as_mut_slice(),
                    &self.tracks,
                    &request,
                    has_search,
                );
            }
        }
        sort_query_albums(&mut query_albums, &self.tracks, &request, has_search);

        let mut order = Vec::with_capacity(matched_total);
        let mut groups = Vec::with_capacity(query_albums.len());
        for album in query_albums {
            let start_index = order.len();
            order.extend(album.matches.iter().map(|(index, _)| *index));
            let end_index = order.len().saturating_sub(1);
            groups.push(LibraryAlbumGroup {
                id: album.album.identity.id.clone(),
                title: album.album.identity.title.clone(),
                album_artist: album.album.identity.album_artist.clone(),
                year: album.album.year,
                matched_tracks: album.matches.len(),
                total_tracks: album.album.track_indices.len(),
                total_duration_seconds: album.album.total_duration_seconds,
                start_index,
                end_index,
                is_ungrouped: album.album.identity.is_ungrouped,
            });
        }
        let total_duration_seconds = order
            .iter()
            .filter_map(|index| self.tracks[*index].track.duration_seconds)
            .sum();
        let id = Uuid::new_v4().to_string();
        let collapsed_group_ids = request
            .collapsed_group_ids
            .into_iter()
            .collect::<HashSet<_>>();
        let view = build_snapshot_view(&groups, &collapsed_group_ids);
        let snapshot = QuerySnapshot {
            id: id.clone(),
            order,
            groups,
            collapsed_group_ids,
            view,
        };
        let items = view_items_for_range(&self.tracks, &snapshot, 0, MAX_PAGE_SIZE);
        let page = LibraryQueryPage {
            snapshot_id: id.clone(),
            total: snapshot.order.len(),
            library_total: self.tracks.len(),
            total_duration_seconds,
            needs_reindex: self.needs_reindex,
            group_total: snapshot.groups.len(),
            virtual_total: snapshot.view.len(),
            offset: 0,
            items,
        };
        self.snapshots.push_back(snapshot);
        trim_front(&mut self.snapshots, MAX_SNAPSHOTS);
        page
    }

    pub fn view_in_range(
        &self,
        snapshot_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<LibraryViewRange, LibraryError> {
        let snapshot = self.snapshot(snapshot_id)?;
        if offset > snapshot.view.len() {
            return Err(LibraryError::RangeOutOfBounds);
        }
        Ok(LibraryViewRange {
            snapshot_id: snapshot.id.clone(),
            offset,
            items: view_items_for_range(
                &self.tracks,
                snapshot,
                offset,
                limit.clamp(1, MAX_PAGE_SIZE),
            ),
        })
    }

    pub fn set_group_collapsed(
        &mut self,
        snapshot_id: &str,
        group_id: &str,
        collapsed: bool,
    ) -> Result<LibraryGroupToggleResult, LibraryError> {
        let snapshot_index = self
            .snapshots
            .iter()
            .position(|snapshot| snapshot.id == snapshot_id)
            .ok_or(LibraryError::SnapshotExpired)?;
        {
            let snapshot = &mut self.snapshots[snapshot_index];
            if !snapshot.groups.iter().any(|group| group.id == group_id) {
                return Err(LibraryError::RangeOutOfBounds);
            }
            if collapsed {
                snapshot.collapsed_group_ids.insert(group_id.to_owned());
            } else {
                snapshot.collapsed_group_ids.remove(group_id);
            }
            snapshot.view = build_snapshot_view(&snapshot.groups, &snapshot.collapsed_group_ids);
        }
        let snapshot = &self.snapshots[snapshot_index];
        let group_virtual_index = snapshot
            .view
            .iter()
            .position(|item| {
                matches!(
                    item,
                    SnapshotViewItem::AlbumHeader(index) if snapshot.groups[*index].id == group_id
                )
            })
            .ok_or(LibraryError::RangeOutOfBounds)?;
        let offset = (group_virtual_index / 100) * 100;
        Ok(LibraryGroupToggleResult {
            snapshot_id: snapshot.id.clone(),
            group_id: group_id.to_owned(),
            collapsed,
            virtual_total: snapshot.view.len(),
            group_virtual_index,
            offset,
            items: view_items_for_range(&self.tracks, snapshot, offset, MAX_PAGE_SIZE),
        })
    }

    pub fn create_playback_queue(
        &mut self,
        snapshot_id: &str,
        start_index: usize,
        selection: Option<&LibrarySelectionRequest>,
    ) -> Result<LibraryPlaybackQueue, LibraryError> {
        let snapshot = self.snapshot(snapshot_id)?;
        let start_track_id = snapshot
            .order
            .get(start_index)
            .map(|index| self.tracks[*index].track.id)
            .ok_or(LibraryError::RangeOutOfBounds)?;
        let track_ids = match selection {
            Some(selection) => self.selected_track_ids(snapshot, selection),
            None => snapshot
                .order
                .iter()
                .map(|index| self.tracks[*index].track.id)
                .collect(),
        };
        if track_ids.is_empty() {
            return Err(LibraryError::EmptySelection);
        }
        let current_index = track_ids
            .iter()
            .position(|track_id| *track_id == start_track_id)
            .unwrap_or(0);
        let track = self
            .track(track_ids[current_index])
            .cloned()
            .ok_or(LibraryError::TrackMissing)?;
        let id = Uuid::new_v4().to_string();
        let queue = LibraryPlaybackQueue {
            queue_id: id.clone(),
            total: track_ids.len(),
            current_index,
            track,
        };
        self.playback_queues
            .push_back(PlaybackQueueSnapshot { id, track_ids });
        trim_front(&mut self.playback_queues, MAX_PLAYBACK_QUEUES);
        Ok(queue)
    }

    pub fn queue_track(
        &self,
        queue_id: &str,
        index: usize,
    ) -> Result<LibraryQueueTrack, LibraryError> {
        let queue = self
            .playback_queues
            .iter()
            .find(|queue| queue.id == queue_id)
            .ok_or(LibraryError::QueueExpired)?;
        let track_id = *queue
            .track_ids
            .get(index)
            .ok_or(LibraryError::RangeOutOfBounds)?;
        let track = self
            .track(track_id)
            .cloned()
            .ok_or(LibraryError::TrackMissing)?;
        Ok(LibraryQueueTrack { index, track })
    }

    pub fn update_play_count(&mut self, track_id: i64, play_count: i64) {
        if let Some(index) = self.track_by_id.get(&track_id).copied() {
            self.tracks[index].track.play_count = play_count;
        }
    }

    pub fn album_target(&self, group_id: &str) -> Result<LibraryAlbumTarget, LibraryError> {
        let album = self
            .albums
            .get(group_id)
            .ok_or(LibraryError::TrackMissing)?;
        if album.identity.is_ungrouped {
            return Err(LibraryError::TrackMissing);
        }
        Ok(LibraryAlbumTarget {
            group_id: album.identity.id.clone(),
            title: album.identity.title.clone().unwrap_or_default(),
            album_artist: album.identity.album_artist.clone().unwrap_or_default(),
            year: album.year,
            tracks: album
                .track_indices
                .iter()
                .map(|index| self.tracks[*index].track.clone())
                .collect(),
        })
    }

    pub fn album_targets(&self) -> Vec<LibraryAlbumTarget> {
        self.albums
            .values()
            .filter(|album| !album.identity.is_ungrouped)
            .map(|album| LibraryAlbumTarget {
                group_id: album.identity.id.clone(),
                title: album.identity.title.clone().unwrap_or_default(),
                album_artist: album.identity.album_artist.clone().unwrap_or_default(),
                year: album.year,
                tracks: album
                    .track_indices
                    .iter()
                    .map(|index| self.tracks[*index].track.clone())
                    .collect(),
            })
            .collect()
    }

    pub fn selected_tracks(
        &self,
        snapshot_id: &str,
        selection: &LibrarySelectionRequest,
    ) -> Result<Vec<LocalTrack>, LibraryError> {
        let snapshot = self.snapshot(snapshot_id)?;
        Ok(self
            .selected_track_ids(snapshot, selection)
            .into_iter()
            .filter_map(|track_id| self.track(track_id).cloned())
            .collect())
    }

    fn replace_tracks(&mut self, tracks: Vec<LocalTrack>) {
        self.tracks = tracks
            .into_par_iter()
            .map(IndexedTrack::new)
            .collect::<Vec<_>>();
        self.track_by_id = self
            .tracks
            .iter()
            .enumerate()
            .map(|(index, track)| (track.track.id, index))
            .collect();
        let mut albums = HashMap::<String, AlbumIndex>::new();
        for (index, track) in self.tracks.iter().enumerate() {
            let identity = album_identity(&track.track);
            albums
                .entry(identity.id.clone())
                .or_insert_with(|| AlbumIndex {
                    identity,
                    track_indices: Vec::new(),
                    year: None,
                    total_duration_seconds: 0,
                    default_sort_rank: 0,
                })
                .track_indices
                .push(index);
        }
        for album in albums.values_mut() {
            album.year = representative_year(
                album
                    .track_indices
                    .iter()
                    .filter_map(|index| self.tracks[*index].track.year),
            );
            album.total_duration_seconds = album
                .track_indices
                .iter()
                .filter_map(|index| self.tracks[*index].track.duration_seconds)
                .sum();
        }
        let mut representatives = albums
            .values()
            .filter_map(|album| album.track_indices.first().copied())
            .collect::<Vec<_>>();
        representatives.par_sort_unstable_by(|left, right| {
            default_album_track_order(&self.tracks[*left], &self.tracks[*right]).then_with(|| {
                self.tracks[*left]
                    .group_id
                    .cmp(&self.tracks[*right].group_id)
            })
        });
        for (rank, representative) in representatives.into_iter().enumerate() {
            if let Some(album) = albums.get_mut(self.tracks[representative].group_id.as_str()) {
                album.default_sort_rank = rank;
            }
        }
        self.albums = albums;
        self.snapshots.clear();
    }

    fn snapshot(&self, id: &str) -> Result<&QuerySnapshot, LibraryError> {
        self.snapshots
            .iter()
            .find(|snapshot| snapshot.id == id)
            .ok_or(LibraryError::SnapshotExpired)
    }

    fn track(&self, id: i64) -> Option<&LocalTrack> {
        self.track_by_id
            .get(&id)
            .map(|index| &self.tracks[*index].track)
    }

    fn selected_track_ids(
        &self,
        snapshot: &QuerySnapshot,
        selection: &LibrarySelectionRequest,
    ) -> Vec<i64> {
        if selection.select_all {
            return snapshot
                .order
                .iter()
                .enumerate()
                .filter(|(position, _)| !range_list_contains(&selection.excluded_ranges, *position))
                .map(|(_, index)| self.tracks[*index].track.id)
                .collect();
        }

        let mut positions = selection
            .ranges
            .iter()
            .flat_map(|range| {
                let start = range.start.min(range.end);
                let end = range
                    .start
                    .max(range.end)
                    .min(snapshot.order.len().saturating_sub(1));
                start..=end
            })
            .collect::<Vec<_>>();
        positions.sort_unstable();
        positions.dedup();
        positions
            .into_iter()
            .filter(|position| !range_list_contains(&selection.excluded_ranges, *position))
            .filter_map(|position| snapshot.order.get(position))
            .map(|index| self.tracks[*index].track.id)
            .collect()
    }
}

fn score_track(
    track: &IndexedTrack,
    fields: &[LibraryTextField],
    atoms: &[Atom],
    matcher: &mut Matcher,
) -> Option<u64> {
    atoms.iter().try_fold(0_u64, |total, atom| {
        fields
            .iter()
            .filter_map(|field| track.search.text(*field))
            .filter_map(|text| atom.score(text.searchable.slice(..), matcher))
            .max()
            .map(|score| total + u64::from(score))
    })
}

fn sort_group_tracks(
    matches: &mut [(usize, u64)],
    tracks: &[IndexedTrack],
    request: &LibraryQueryRequest,
    has_search: bool,
) {
    matches.par_sort_unstable_by(|(left_index, left_score), (right_index, right_score)| {
        let left = &tracks[*left_index];
        let right = &tracks[*right_index];
        let ordering = if request.sort_field == LibrarySortField::Relevance && has_search {
            right_score.cmp(left_score)
        } else if request.sort_field == LibrarySortField::Relevance
            || is_album_sort_field(request.sort_field)
        {
            default_track_order(left, right)
        } else {
            field_order(left, right, request.sort_field, request.sort_direction)
        };
        ordering
            .then_with(|| default_track_order(left, right))
            .then_with(|| left.track.id.cmp(&right.track.id))
    });
}

fn sort_query_albums(
    albums: &mut [QueryAlbum<'_>],
    tracks: &[IndexedTrack],
    request: &LibraryQueryRequest,
    has_search: bool,
) {
    albums.par_sort_unstable_by(|left, right| {
        if left.album.identity.is_ungrouped != right.album.identity.is_ungrouped {
            return if left.album.identity.is_ungrouped {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }
        let ordering = if request.sort_field == LibrarySortField::Relevance && has_search {
            right.max_score.cmp(&left.max_score)
        } else if is_album_sort_field(request.sort_field) {
            album_field_order(
                left,
                right,
                tracks,
                request.sort_field,
                request.sort_direction,
            )
        } else {
            Ordering::Equal
        };
        ordering.then_with(|| {
            left.album
                .default_sort_rank
                .cmp(&right.album.default_sort_rank)
        })
    });
}

fn is_album_sort_field(field: LibrarySortField) -> bool {
    matches!(
        field,
        LibrarySortField::Album | LibrarySortField::AlbumArtist | LibrarySortField::Year
    )
}

fn album_field_order(
    left: &QueryAlbum<'_>,
    right: &QueryAlbum<'_>,
    tracks: &[IndexedTrack],
    field: LibrarySortField,
    direction: LibrarySortDirection,
) -> Ordering {
    let left_track = &tracks[left.representative_index];
    let right_track = &tracks[right.representative_index];
    let ordering = match field {
        LibrarySortField::Album => optional_search_order(
            left_track.search.album.as_ref(),
            right_track.search.album.as_ref(),
        ),
        LibrarySortField::AlbumArtist => optional_search_order(
            left_track
                .search
                .album_artist
                .as_ref()
                .or(left_track.search.artist.as_ref()),
            right_track
                .search
                .album_artist
                .as_ref()
                .or(right_track.search.artist.as_ref()),
        ),
        LibrarySortField::Year => optional_value_order(left.album.year, right.album.year),
        _ => Ordering::Equal,
    };
    match direction {
        LibrarySortDirection::Ascending => ordering,
        LibrarySortDirection::Descending => {
            let left_missing = album_sort_value_is_missing(left, left_track, field);
            let right_missing = album_sort_value_is_missing(right, right_track, field);
            if left_missing || right_missing {
                ordering
            } else {
                ordering.reverse()
            }
        }
    }
}

fn album_sort_value_is_missing(
    album: &QueryAlbum<'_>,
    representative: &IndexedTrack,
    field: LibrarySortField,
) -> bool {
    match field {
        LibrarySortField::Album => representative.track.album.is_none(),
        LibrarySortField::AlbumArtist => {
            representative.track.album_artist.is_none() && representative.track.artist.is_none()
        }
        LibrarySortField::Year => album.album.year.is_none(),
        _ => false,
    }
}

fn default_album_track_order(left_track: &IndexedTrack, right_track: &IndexedTrack) -> Ordering {
    optional_search_order(
        left_track
            .search
            .album_artist
            .as_ref()
            .or(left_track.search.artist.as_ref()),
        right_track
            .search
            .album_artist
            .as_ref()
            .or(right_track.search.artist.as_ref()),
    )
    .then_with(|| {
        optional_search_order(
            left_track.search.album.as_ref(),
            right_track.search.album.as_ref(),
        )
    })
}

fn field_order(
    left: &IndexedTrack,
    right: &IndexedTrack,
    field: LibrarySortField,
    direction: LibrarySortDirection,
) -> Ordering {
    let ordering = match field {
        LibrarySortField::Title => {
            optional_search_order(Some(&left.search.title), Some(&right.search.title))
        }
        LibrarySortField::Artist => {
            optional_search_order(left.search.artist.as_ref(), right.search.artist.as_ref())
        }
        LibrarySortField::Album => {
            optional_search_order(left.search.album.as_ref(), right.search.album.as_ref())
        }
        LibrarySortField::AlbumArtist => optional_search_order(
            left.search.album_artist.as_ref(),
            right.search.album_artist.as_ref(),
        ),
        LibrarySortField::Genre => {
            optional_search_order(left.search.genre.as_ref(), right.search.genre.as_ref())
        }
        LibrarySortField::Codec => {
            optional_search_order(left.search.codec.as_ref(), right.search.codec.as_ref())
        }
        LibrarySortField::FileName => {
            optional_search_order(Some(&left.search.file_name), Some(&right.search.file_name))
        }
        LibrarySortField::FilePath => {
            optional_search_order(Some(&left.search.file_path), Some(&right.search.file_path))
        }
        LibrarySortField::Year => optional_value_order(left.track.year, right.track.year),
        LibrarySortField::BitrateKbps => {
            optional_value_order(left.track.bitrate_kbps, right.track.bitrate_kbps)
        }
        LibrarySortField::SampleRateHz => {
            optional_value_order(left.track.sample_rate_hz, right.track.sample_rate_hz)
        }
        LibrarySortField::DurationSeconds => {
            optional_value_order(left.track.duration_seconds, right.track.duration_seconds)
        }
        LibrarySortField::TrackNumber => {
            optional_value_order(left.track.track_number, right.track.track_number)
        }
        LibrarySortField::DiscNumber => {
            optional_value_order(left.track.disc_number, right.track.disc_number)
        }
        LibrarySortField::ModifiedAt => {
            optional_value_order(left.track.modified_at, right.track.modified_at)
        }
        LibrarySortField::FileSizeBytes => {
            left.track.file_size_bytes.cmp(&right.track.file_size_bytes)
        }
        LibrarySortField::IndexedAt => left.track.indexed_at.cmp(&right.track.indexed_at),
        LibrarySortField::PlayCount => left.track.play_count.cmp(&right.track.play_count),
        LibrarySortField::Relevance => Ordering::Equal,
    };
    match direction {
        LibrarySortDirection::Ascending => ordering,
        LibrarySortDirection::Descending => reverse_present_order(ordering, left, right, field),
    }
}

fn reverse_present_order(
    ordering: Ordering,
    left: &IndexedTrack,
    right: &IndexedTrack,
    field: LibrarySortField,
) -> Ordering {
    let (left_missing, right_missing) = missing_values(left, right, field);
    if left_missing || right_missing {
        ordering
    } else {
        ordering.reverse()
    }
}

fn missing_values(
    left: &IndexedTrack,
    right: &IndexedTrack,
    field: LibrarySortField,
) -> (bool, bool) {
    match field {
        LibrarySortField::Artist => (left.track.artist.is_none(), right.track.artist.is_none()),
        LibrarySortField::Album => (left.track.album.is_none(), right.track.album.is_none()),
        LibrarySortField::AlbumArtist => (
            left.track.album_artist.is_none(),
            right.track.album_artist.is_none(),
        ),
        LibrarySortField::Genre => (left.track.genre.is_none(), right.track.genre.is_none()),
        LibrarySortField::Year => (left.track.year.is_none(), right.track.year.is_none()),
        LibrarySortField::Codec => (left.track.codec.is_none(), right.track.codec.is_none()),
        LibrarySortField::BitrateKbps => (
            left.track.bitrate_kbps.is_none(),
            right.track.bitrate_kbps.is_none(),
        ),
        LibrarySortField::SampleRateHz => (
            left.track.sample_rate_hz.is_none(),
            right.track.sample_rate_hz.is_none(),
        ),
        LibrarySortField::DurationSeconds => (
            left.track.duration_seconds.is_none(),
            right.track.duration_seconds.is_none(),
        ),
        LibrarySortField::TrackNumber => (
            left.track.track_number.is_none(),
            right.track.track_number.is_none(),
        ),
        LibrarySortField::DiscNumber => (
            left.track.disc_number.is_none(),
            right.track.disc_number.is_none(),
        ),
        LibrarySortField::ModifiedAt => (
            left.track.modified_at.is_none(),
            right.track.modified_at.is_none(),
        ),
        _ => (false, false),
    }
}

fn default_track_order(left: &IndexedTrack, right: &IndexedTrack) -> Ordering {
    optional_search_order(
        left.search
            .album_artist
            .as_ref()
            .or(left.search.artist.as_ref()),
        right
            .search
            .album_artist
            .as_ref()
            .or(right.search.artist.as_ref()),
    )
    .then_with(|| optional_search_order(left.search.album.as_ref(), right.search.album.as_ref()))
    .then_with(|| optional_value_order(left.track.disc_number, right.track.disc_number))
    .then_with(|| optional_value_order(left.track.track_number, right.track.track_number))
    .then_with(|| optional_search_order(Some(&left.search.title), Some(&right.search.title)))
}

fn optional_search_order(left: Option<&SearchText>, right: Option<&SearchText>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => natural_compare(&left.sort_key, &right.sort_key),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn optional_value_order<T: Ord>(left: Option<T>, right: Option<T>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn build_snapshot_view(
    groups: &[LibraryAlbumGroup],
    collapsed_group_ids: &HashSet<String>,
) -> Vec<SnapshotViewItem> {
    let track_count = groups
        .iter()
        .filter(|group| !collapsed_group_ids.contains(&group.id))
        .map(|group| group.matched_tracks)
        .sum::<usize>();
    let mut view = Vec::with_capacity(track_count + groups.len() * ALBUM_HEADER_SLOTS);
    for (group_index, group) in groups.iter().enumerate() {
        view.push(SnapshotViewItem::AlbumHeader(group_index));
        view.push(SnapshotViewItem::AlbumContinuation(group_index));
        if !collapsed_group_ids.contains(&group.id) {
            view.extend((group.start_index..=group.end_index).map(SnapshotViewItem::Track));
        }
    }
    view
}

fn view_items_for_range(
    tracks: &[IndexedTrack],
    snapshot: &QuerySnapshot,
    offset: usize,
    limit: usize,
) -> Vec<LibraryViewItem> {
    snapshot
        .view
        .iter()
        .enumerate()
        .skip(offset)
        .take(limit)
        .map(|(index, item)| match *item {
            SnapshotViewItem::AlbumHeader(group_index) => LibraryViewItem {
                index,
                kind: LibraryViewItemKind::AlbumHeader,
                group: snapshot.groups.get(group_index).cloned(),
                track: None,
                track_index: None,
            },
            SnapshotViewItem::AlbumContinuation(group_index) => LibraryViewItem {
                index,
                kind: LibraryViewItemKind::AlbumContinuation,
                group: snapshot.groups.get(group_index).cloned(),
                track: None,
                track_index: None,
            },
            SnapshotViewItem::Track(track_position) => {
                let track = snapshot
                    .order
                    .get(track_position)
                    .and_then(|track_index| tracks.get(*track_index))
                    .map(|track| track.track.clone());
                LibraryViewItem {
                    index,
                    kind: LibraryViewItemKind::Track,
                    group: None,
                    track,
                    track_index: Some(track_position),
                }
            }
        })
        .collect()
}

fn album_identity(track: &LocalTrack) -> AlbumIdentity {
    let title = non_empty_trimmed(track.album.as_deref());
    let album_artist = non_empty_trimmed(track.album_artist.as_deref())
        .or_else(|| non_empty_trimmed(track.artist.as_deref()));
    let Some(title_value) = title.as_deref() else {
        return AlbumIdentity {
            id: UNGROUPED_ID.to_owned(),
            title: None,
            album_artist: None,
            is_ungrouped: true,
        };
    };
    let normalized_artist = normalize_group_component(album_artist.as_deref().unwrap_or(""));
    let normalized_title = normalize_group_component(title_value);
    let mut digest = Sha256::new();
    digest.update(normalized_artist.as_bytes());
    digest.update([0x1f]);
    digest.update(normalized_title.as_bytes());
    let digest = digest.finalize();
    AlbumIdentity {
        id: format!("album:{}", URL_SAFE_NO_PAD.encode(&digest[..16])),
        title,
        album_artist,
        is_ungrouped: false,
    }
}

fn non_empty_trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn normalize_group_component(value: &str) -> String {
    normalize_text(&value.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn representative_year(years: impl Iterator<Item = i64>) -> Option<i64> {
    let mut counts = HashMap::<i64, usize>::new();
    for year in years {
        *counts.entry(year).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|(left_year, left_count), (right_year, right_count)| {
            left_count
                .cmp(right_count)
                .then_with(|| right_year.cmp(left_year))
        })
        .map(|(year, _)| year)
}

fn range_list_contains(ranges: &[LibrarySelectionRange], index: usize) -> bool {
    ranges.iter().any(|range| {
        let start = range.start.min(range.end);
        let end = range.start.max(range.end);
        index >= start && index <= end
    })
}

pub(crate) fn normalize_text(value: &str) -> String {
    value
        .nfkd()
        .filter(|character| !is_combining_mark(*character))
        .flat_map(char::to_lowercase)
        .collect()
}

fn pinyin_keys(value: &str) -> (String, String) {
    let mut full = String::with_capacity(value.len());
    let mut initials = String::with_capacity(value.chars().count());
    for character in value.chars() {
        if let Some(pinyin) = character.to_pinyin() {
            let plain = pinyin.plain();
            full.push_str(plain);
            if let Some(initial) = plain.chars().next() {
                initials.push(initial);
            }
        } else if character.is_alphanumeric() {
            for normalized in character.to_lowercase() {
                full.push(normalized);
            }
        } else if character.is_whitespace() {
            full.push(' ');
        }
    }
    (full, initials)
}

fn default_search_fields() -> Vec<LibraryTextField> {
    vec![
        LibraryTextField::Title,
        LibraryTextField::Artist,
        LibraryTextField::Album,
    ]
}

fn text_field_rank(field: LibraryTextField) -> u8 {
    match field {
        LibraryTextField::Title => 0,
        LibraryTextField::Artist => 1,
        LibraryTextField::Album => 2,
        LibraryTextField::AlbumArtist => 3,
        LibraryTextField::Genre => 4,
        LibraryTextField::Codec => 5,
        LibraryTextField::FileName => 6,
        LibraryTextField::FilePath => 7,
    }
}

fn trim_front<T>(items: &mut VecDeque<T>, limit: usize) {
    while items.len() > limit {
        items.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn track(id: i64, title: &str, artist: &str, album: &str) -> LocalTrack {
        LocalTrack {
            id,
            file_path: format!("/music/{id}.flac"),
            file_name: format!("{id}.flac"),
            title: title.to_owned(),
            artist: Some(artist.to_owned()),
            album: Some(album.to_owned()),
            album_artist: Some(artist.to_owned()),
            genre: Some("Pop".to_owned()),
            year: Some(2024),
            codec: Some("FLAC".to_owned()),
            bitrate_kbps: Some(900),
            sample_rate_hz: Some(44_100),
            duration_seconds: Some(180),
            track_number: Some(id),
            disc_number: Some(1),
            file_size_bytes: 10_000,
            modified_at: Some(1),
            indexed_at: 1,
            play_count: 0,
        }
    }

    fn service(tracks: Vec<LocalTrack>) -> LibraryService {
        let mut service = LibraryService::default();
        service.replace_tracks(tracks);
        service
    }

    fn request(search: &str) -> LibraryQueryRequest {
        LibraryQueryRequest {
            search: search.to_owned(),
            search_fields: default_search_fields(),
            sort_field: LibrarySortField::Relevance,
            sort_direction: LibrarySortDirection::Descending,
            collapsed_group_ids: Vec::new(),
        }
    }

    fn page_track_ids(page: &LibraryQueryPage) -> Vec<i64> {
        page.items
            .iter()
            .filter_map(|item| item.track.as_ref().map(|track| track.id))
            .collect()
    }

    #[test]
    fn query_should_match_chinese_artist_by_full_pinyin() {
        let mut service = service(vec![track(1, "Qing Tian", "周杰伦", "叶惠美")]);

        let page = service.query(request("zhoujielun"));

        assert_eq!(page.total, 1);
    }

    #[test]
    fn query_should_match_chinese_artist_by_pinyin_initials() {
        let mut service = service(vec![track(1, "Qing Tian", "周杰伦", "叶惠美")]);

        let page = service.query(request("zjl"));

        assert_eq!(page.total, 1);
    }

    #[test]
    fn query_should_require_every_word_across_different_fields() {
        let mut service = service(vec![
            track(1, "Qing Tian", "周杰伦", "叶惠美"),
            track(2, "Qing Tian", "Other", "Other"),
        ]);

        let page = service.query(request("zjl yhm"));

        assert_eq!(page_track_ids(&page), vec![1]);
    }

    #[test]
    fn query_should_search_codec_when_it_is_in_scope() {
        let mut flac = track(1, "Track", "Artist", "Album");
        flac.codec = Some("FLAC".to_owned());
        let mut aac = track(2, "Track", "Artist", "Album");
        aac.codec = Some("AAC".to_owned());
        let mut service = service(vec![flac, aac]);
        let mut query = request("flac");
        query.search_fields = vec![LibraryTextField::Codec];

        let page = service.query(query);

        assert_eq!(page.total, 1);
        assert_eq!(page_track_ids(&page), vec![1]);
    }

    #[test]
    fn query_should_group_normalized_album_artist_and_album_title() {
        let mut first = track(1, "First", "Beyonce", " Renaissance ");
        first.album_artist = Some("Beyonce".to_owned());
        let mut second = track(2, "Second", "Beyonce", "renaissance");
        second.album_artist = Some("Beyonce\u{301}".to_owned());
        let mut third = track(3, "Third", "Beyonce", "Renaissance");
        third.album_artist = Some("  ".to_owned());
        let mut service = service(vec![first, second, third]);

        let page = service.query(request(""));

        assert_eq!(page.group_total, 1);
    }

    #[test]
    fn title_sort_should_keep_albums_together_and_sort_tracks_inside_each_group() {
        let mut service = service(vec![
            track(1, "Zulu", "Artist", "Album A"),
            track(2, "Alpha", "Artist", "Album A"),
            track(3, "Beta", "Artist", "Album B"),
            track(4, "Able", "Artist", "Album B"),
        ]);
        let mut query = request("");
        query.sort_field = LibrarySortField::Title;
        query.sort_direction = LibrarySortDirection::Ascending;

        let page = service.query(query);

        assert_eq!(page_track_ids(&page), vec![2, 1, 4, 3]);
    }

    #[test]
    fn search_should_only_return_matching_tracks_but_keep_the_album_total() {
        let mut service = service(vec![
            track(1, "Needle", "Artist", "Album"),
            track(2, "Other", "Artist", "Album"),
        ]);

        let page = service.query(request("needle"));
        let group = page.items[0]
            .group
            .as_ref()
            .expect("first item should be an album header");

        assert_eq!(
            (page.total, group.matched_tracks, group.total_tracks),
            (1, 1, 2)
        );
    }

    #[test]
    fn collapsing_an_album_should_only_remove_its_track_slots() {
        let mut service = service(vec![
            track(1, "First", "Artist", "Album"),
            track(2, "Second", "Artist", "Album"),
        ]);
        let page = service.query(request(""));
        let group_id = page.items[0]
            .group
            .as_ref()
            .expect("first item should be an album header")
            .id
            .clone();

        let result = service
            .set_group_collapsed(&page.snapshot_id, &group_id, true)
            .expect("album should collapse");

        assert_eq!((page.virtual_total, result.virtual_total), (4, 2));
    }

    #[test]
    fn ungrouped_tracks_should_share_the_final_group() {
        let mut ungrouped = track(1, "Loose", "Artist", "Ignored");
        ungrouped.album = None;
        let mut service = service(vec![ungrouped, track(2, "Grouped", "Artist", "Album")]);

        let page = service.query(request(""));
        let group_titles = page
            .items
            .iter()
            .filter(|item| item.kind == LibraryViewItemKind::AlbumHeader)
            .map(|item| item.group.as_ref().map(|group| group.title.as_deref()))
            .collect::<Vec<_>>();

        assert_eq!(group_titles, vec![Some(Some("Album")), Some(None)]);
    }

    #[test]
    fn query_should_bound_the_first_ipc_page_for_large_libraries() {
        let tracks = (0..100_000)
            .map(|id| track(id, &format!("Track {id}"), "Artist", "Album"))
            .collect();
        let mut service = service(tracks);

        let page = service.query(request(""));

        assert_eq!(page.items.len(), MAX_PAGE_SIZE);
    }

    #[test]
    fn selection_should_include_unloaded_rows_in_a_shift_range() {
        let mut service = service(
            (0..500)
                .map(|id| track(id, "Track", "Artist", "Album"))
                .collect(),
        );
        let page = service.query(request(""));
        let selection = LibrarySelectionRequest {
            select_all: false,
            ranges: vec![LibrarySelectionRange {
                start: 10,
                end: 300,
            }],
            excluded_ranges: Vec::new(),
        };

        let queue = service
            .create_playback_queue(&page.snapshot_id, 10, Some(&selection))
            .expect("selection should create a queue");

        assert_eq!(queue.total, 291);
    }

    #[test]
    fn playback_queue_should_survive_a_library_reload_by_track_id() {
        let mut service = service(vec![
            track(1, "Track 1", "Artist", "Album"),
            track(2, "Track 2", "Artist", "Album"),
            track(3, "Track 3", "Artist", "Album"),
        ]);
        let page = service.query(request(""));
        let queue = service
            .create_playback_queue(&page.snapshot_id, 0, None)
            .expect("query should create a playback queue");

        service.replace_tracks(vec![
            track(3, "Updated 3", "Artist", "Album"),
            track(1, "Updated 1", "Artist", "Album"),
            track(2, "Updated 2", "Artist", "Album"),
        ]);

        let queued_track = service
            .queue_track(&queue.queue_id, 1)
            .expect("queue should resolve through the reloaded id index");
        assert_eq!(queued_track.track.id, 2);
        assert_eq!(queued_track.track.title, "Updated 2");
    }

    #[test]
    #[ignore = "release-mode performance benchmark"]
    fn cold_library_should_return_the_first_100k_page_within_one_second() {
        let mut connection = Connection::open_in_memory().expect("database should open");
        crate::database::initialize(&mut connection).expect("database should migrate");
        let transaction = connection
            .transaction()
            .expect("fixture transaction should start");
        {
            let mut insert = transaction
                .prepare(
                    "INSERT INTO local_tracks (
                        file_path, file_name, title, artist, album, duration_seconds,
                        track_number, disc_number, file_size_bytes, indexed_at, metadata_version
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 10000, 1, ?8)",
                )
                .expect("fixture statement should prepare");
            for id in 0..100_000 {
                insert
                    .execute(rusqlite::params![
                        format!("/music/{id}.flac"),
                        format!("{id}.flac"),
                        format!("Track {id}"),
                        "周杰伦",
                        format!("Album {}", id / 12),
                        180 + id % 120,
                        id % 12 + 1,
                        LIBRARY_METADATA_VERSION,
                    ])
                    .expect("fixture row should insert");
            }
        }
        transaction.commit().expect("fixture should commit");
        let started = Instant::now();

        let mut service = LibraryService::load(&connection).expect("library should load");
        let page = service.query(request(""));
        let elapsed = started.elapsed();

        eprintln!("100k cold library: {elapsed:?}");
        assert!(
            elapsed <= Duration::from_secs(1),
            "cold library took {elapsed:?}"
        );
        assert_eq!(page.items.len(), MAX_PAGE_SIZE);
    }

    #[test]
    #[ignore = "release-mode performance benchmark"]
    fn fuzzy_search_should_rank_100k_tracks_within_200_milliseconds() {
        let tracks = (0..100_000)
            .map(|id| track(id, &format!("Track {id}"), "周杰伦", "Album"))
            .collect();
        let mut service = service(tracks);
        let started = Instant::now();

        let page = service.query(request("zjl"));
        let elapsed = started.elapsed();

        eprintln!("100k fuzzy search: {elapsed:?}");
        assert!(
            elapsed <= Duration::from_millis(200),
            "fuzzy search took {elapsed:?}"
        );
        assert_eq!(page.total, 100_000);
    }

    #[test]
    #[ignore = "release-mode performance benchmark"]
    fn fuzzy_search_should_group_100k_single_track_albums_within_200_milliseconds() {
        let tracks = (0..100_000)
            .map(|id| track(id, &format!("Track {id}"), "周杰伦", &format!("Album {id}")))
            .collect();
        let mut service = service(tracks);
        let started = Instant::now();

        let page = service.query(request("zjl"));
        let elapsed = started.elapsed();

        eprintln!("100k single-album fuzzy search: {elapsed:?}");
        assert!(
            elapsed <= Duration::from_millis(200),
            "grouped fuzzy search took {elapsed:?}"
        );
        assert_eq!((page.group_total, page.virtual_total), (100_000, 300_000));
    }
}
