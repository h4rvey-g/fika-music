use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use fika_music_lib::lyrics::{LocalTrackPlaybackDetails, TrackLyricsQuery};
use fika_music_lib::netease::{
    NeteaseAccount, NeteaseMutationAudit, NeteaseQrLoginPoll, NeteaseQrLoginStart,
};
use fika_music_lib::plugin_system::PluginRecord;
use fika_music_lib::source_runtime::{SourceRequest, SourceRequestOutcome};
use fika_music_lib::{
    LocalTrack, MediaSource, NeteaseCommandError, PluginCommandError, RemoteCommandError,
    RemoteMediaSource, RemoteSearchResults, ScanProgressEvent, ScanStatus, TAURI_COMMAND_NAMES,
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
    export_all::<MediaSource>(&config)?;
    export_all::<LocalTrackPlaybackDetails>(&config)?;
    export_all::<TrackLyricsQuery>(&config)?;
    export_all::<RemoteMediaSource>(&config)?;
    export_all::<RemoteSearchResults>(&config)?;
    export_all::<RemoteCommandError>(&config)?;
    export_all::<ScanStatus>(&config)?;
    export_all::<ScanProgressEvent>(&config)?;
    export_all::<PluginCommandError>(&config)?;
    export_all::<NeteaseCommandError>(&config)?;
    export_all::<PluginRecord>(&config)?;
    export_all::<SourceRequest>(&config)?;
    export_all::<SourceRequestOutcome>(&config)?;
    export_all::<NeteaseAccount>(&config)?;
    export_all::<NeteaseQrLoginStart>(&config)?;
    export_all::<NeteaseQrLoginPoll>(&config)?;
    export_all::<NeteaseMutationAudit>(&config)?;
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
