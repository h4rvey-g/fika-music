use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fika_music_lib::bundled_plugins;
use fika_music_lib::plugin_system::validate_plugin_package;

fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<OsString>) -> Result<(), String> {
    let packages = if arguments.is_empty() {
        bundled_package_paths()?
    } else {
        arguments.into_iter().map(PathBuf::from).collect()
    };
    if packages.is_empty() {
        return Err("no Plugin packages were found".to_owned());
    }

    let catalog = bundled_plugins::contract_catalog().map_err(|error| error.to_string())?;
    let mut failures = Vec::new();
    for package in packages {
        match validate_plugin_package(&package, &catalog) {
            Ok(manifest) => println!(
                "valid: {} {} ({} Provider{})",
                manifest.id,
                manifest.version,
                manifest.provider_entrypoints.len(),
                if manifest.provider_entrypoints.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ),
            Err(error) => failures.push(format!("{}: {error}", package.display())),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn bundled_package_paths() -> Result<Vec<PathBuf>, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins");
    let mut packages = fs::read_dir(&root)
        .map_err(|error| format!("could not read {}: {error}", root.display()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|file_type| file_type.is_dir())
                .map(|_| entry.path())
        })
        .filter(|path| path.join("plugin.json").is_file())
        .collect::<Vec<_>>();
    packages.sort();
    Ok(packages)
}
