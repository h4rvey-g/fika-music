use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use fika_music_lib::audio_source_system::{
    AudioSourceAvailability, AudioSourceCommandError, AudioSourceRecord,
};
use fika_music_lib::kugou::{KugouAccount, KugouQrLoginPoll, KugouQrLoginStart};
use fika_music_lib::lyrics::{LocalTrackPlaybackDetails, TrackLyricsQuery};
use fika_music_lib::netease::{
    NeteaseAccount, NeteaseMutationAudit, NeteaseQrLoginPoll, NeteaseQrLoginStart,
};
use fika_music_lib::online_download::{
    OnlineDownloadItem, OnlineDownloadItemState, OnlineDownloadProgressEvent, OnlineDownloadState,
    OnlineDownloadTask,
};
use fika_music_lib::online_music::{
    AudioSourceSelectionMode, OnlineAlbum, OnlineAlbumPage, OnlineArtist, OnlineArtistBiography,
    OnlineChannel, OnlineMusicSettings, OnlinePlaylist, OnlinePlaylistDetailError,
    OnlinePlaylistsResult, OnlineRecommendationsResult, OnlineSearchData, OnlineSearchHistoryEntry,
    OnlineSearchSection, OnlineSearchSectionEvent, OnlineSearchSectionResult,
    OnlineSuggestionsResult, OnlineTrack, OnlineTrackPage,
};
use fika_music_lib::plugin_system::PluginRecord;
use fika_music_lib::source_runtime::{SourceRequest, SourceRequestOutcome};
use fika_music_lib::{
    AlbumArtSettings, AlbumArtTaskStatus, AlbumCoverCandidate, AlbumCoverResult, AlbumCoverStatus,
    KugouCommandError, LibraryAlbumGroup, LibraryChangedEvent, LibraryGroupToggleResult,
    LibraryPlaybackQueue, LibraryQueryPage, LibraryQueryRequest, LibraryQueueTrack,
    LibrarySelectionRange, LibrarySelectionRequest, LibrarySortDirection, LibrarySortField,
    LibraryTaskState, LibraryTextField, LibraryViewItem, LibraryViewItemKind, LibraryViewRange,
    LocalTrack, MediaSource, MetadataLookupItemResult, MetadataLookupTaskStatus,
    MusicCollectionDetail, MusicCollectionItem, MusicCollectionItemKind, MusicCollectionMutation,
    MusicCollectionSummary, NeteaseCommandError, PluginCommandError, RemoteCommandError,
    ScanProgressEvent, ScanStatus, SmartCollectionField, SmartCollectionOperator,
    SmartCollectionRule, SmartCollectionRules, TAURI_COMMAND_NAMES,
};
use ts_rs::{Config, TS};

const BINDINGS_FILE: &str = "bindings.ts";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let check = std::env::args()
        .skip(1)
        .any(|argument| argument == "--check");
    let frontend_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/generated");

    if check {
        let temporary_dir =
            std::env::temp_dir().join(format!("fika-music-bindings-check-{}", std::process::id()));
        if temporary_dir.exists() {
            fs::remove_dir_all(&temporary_dir)?;
        }
        generate(&temporary_dir)?;
        let expected = fs::read(temporary_dir.join(BINDINGS_FILE))?;
        let actual = fs::read(frontend_dir.join(BINDINGS_FILE)).unwrap_or_default();
        fs::remove_dir_all(temporary_dir)?;
        if actual != expected {
            return Err("TypeScript bindings are stale; run `npm run bindings:generate`".into());
        }
    } else {
        generate(&frontend_dir)?;
    }

    Ok(())
}

fn generate(output_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(output_dir)?;
    let output_file = output_dir.join(BINDINGS_FILE);
    if output_file.exists() {
        fs::remove_file(&output_file)?;
    }

    let config = Config::new()
        .with_out_dir(output_dir)
        .with_large_int("number");
    export_all::<LocalTrack>(&config)?;
    export_all::<LibraryTextField>(&config)?;
    export_all::<LibrarySortField>(&config)?;
    export_all::<LibrarySortDirection>(&config)?;
    export_all::<LibraryQueryRequest>(&config)?;
    export_all::<LibraryViewItemKind>(&config)?;
    export_all::<LibraryAlbumGroup>(&config)?;
    export_all::<LibraryViewItem>(&config)?;
    export_all::<LibraryQueryPage>(&config)?;
    export_all::<LibraryViewRange>(&config)?;
    export_all::<LibraryGroupToggleResult>(&config)?;
    export_all::<AlbumCoverStatus>(&config)?;
    export_all::<AlbumCoverCandidate>(&config)?;
    export_all::<AlbumCoverResult>(&config)?;
    export_all::<AlbumArtSettings>(&config)?;
    export_all::<LibraryTaskState>(&config)?;
    export_all::<AlbumArtTaskStatus>(&config)?;
    export_all::<MetadataLookupItemResult>(&config)?;
    export_all::<MetadataLookupTaskStatus>(&config)?;
    export_all::<LibrarySelectionRange>(&config)?;
    export_all::<LibrarySelectionRequest>(&config)?;
    export_all::<LibraryPlaybackQueue>(&config)?;
    export_all::<LibraryQueueTrack>(&config)?;
    export_all::<MusicCollectionSummary>(&config)?;
    export_all::<MusicCollectionItemKind>(&config)?;
    export_all::<MusicCollectionItem>(&config)?;
    export_all::<MusicCollectionDetail>(&config)?;
    export_all::<MusicCollectionMutation>(&config)?;
    export_all::<SmartCollectionField>(&config)?;
    export_all::<SmartCollectionOperator>(&config)?;
    export_all::<SmartCollectionRule>(&config)?;
    export_all::<SmartCollectionRules>(&config)?;
    export_all::<MediaSource>(&config)?;
    export_all::<LocalTrackPlaybackDetails>(&config)?;
    export_all::<TrackLyricsQuery>(&config)?;
    export_all::<RemoteCommandError>(&config)?;
    export_all::<ScanStatus>(&config)?;
    export_all::<ScanProgressEvent>(&config)?;
    export_all::<LibraryChangedEvent>(&config)?;
    export_all::<PluginCommandError>(&config)?;
    export_all::<AudioSourceCommandError>(&config)?;
    export_all::<AudioSourceAvailability>(&config)?;
    export_all::<NeteaseCommandError>(&config)?;
    export_all::<KugouCommandError>(&config)?;
    export_all::<PluginRecord>(&config)?;
    export_all::<AudioSourceRecord>(&config)?;
    export_all::<SourceRequest>(&config)?;
    export_all::<SourceRequestOutcome>(&config)?;
    export_all::<NeteaseAccount>(&config)?;
    export_all::<NeteaseQrLoginStart>(&config)?;
    export_all::<NeteaseQrLoginPoll>(&config)?;
    export_all::<NeteaseMutationAudit>(&config)?;
    export_all::<KugouAccount>(&config)?;
    export_all::<KugouQrLoginStart>(&config)?;
    export_all::<KugouQrLoginPoll>(&config)?;
    export_all::<AudioSourceSelectionMode>(&config)?;
    export_all::<OnlineMusicSettings>(&config)?;
    export_all::<OnlineSearchHistoryEntry>(&config)?;
    export_all::<OnlineChannel>(&config)?;
    export_all::<OnlineSearchSection>(&config)?;
    export_all::<OnlineSearchData>(&config)?;
    export_all::<OnlineSearchSectionResult>(&config)?;
    export_all::<OnlineSearchSectionEvent>(&config)?;
    export_all::<OnlineSuggestionsResult>(&config)?;
    export_all::<OnlineTrack>(&config)?;
    export_all::<OnlineArtist>(&config)?;
    export_all::<OnlineArtistBiography>(&config)?;
    export_all::<OnlineAlbum>(&config)?;
    export_all::<OnlineAlbumPage>(&config)?;
    export_all::<OnlinePlaylist>(&config)?;
    export_all::<OnlinePlaylistDetailError>(&config)?;
    export_all::<OnlinePlaylistsResult>(&config)?;
    export_all::<OnlineTrackPage>(&config)?;
    export_all::<OnlineRecommendationsResult>(&config)?;
    export_all::<OnlineDownloadState>(&config)?;
    export_all::<OnlineDownloadItemState>(&config)?;
    export_all::<OnlineDownloadItem>(&config)?;
    export_all::<OnlineDownloadProgressEvent>(&config)?;
    export_all::<OnlineDownloadTask>(&config)?;
    append_command_names(&output_file)?;
    Ok(())
}

fn export_all<T: TS + 'static>(config: &Config) -> Result<(), ts_rs::ExportError> {
    T::export_all(config)
}

fn append_command_names(path: &Path) -> Result<(), std::io::Error> {
    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    writeln!(file, "\nexport const TAURI_COMMANDS = {{")?;
    for command in TAURI_COMMAND_NAMES {
        writeln!(file, "  {}: \"{}\",", snake_to_camel(command), command)?;
    }
    writeln!(file, "}} as const;\n")?;
    writeln!(
        file,
        "export type TauriCommand = (typeof TAURI_COMMANDS)[keyof typeof TAURI_COMMANDS];"
    )?;
    Ok(())
}

fn snake_to_camel(value: &str) -> String {
    let mut result = String::new();
    let mut uppercase_next = false;
    for character in value.chars() {
        if character == '_' {
            uppercase_next = true;
        } else if uppercase_next {
            result.push(character.to_ascii_uppercase());
            uppercase_next = false;
        } else {
            result.push(character);
        }
    }
    result
}
