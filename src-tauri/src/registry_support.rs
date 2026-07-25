use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};

pub(crate) const MAX_MANAGED_IDENTIFIER_BYTES: usize = 128;

pub(crate) fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_MANAGED_IDENTIFIER_BYTES
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

pub(crate) fn manifest_fingerprint(manifest: &impl Serialize) -> Result<String, serde_json::Error> {
    Ok(sha256_hex(&serde_json::to_vec(manifest)?))
}

pub(crate) fn sha256_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

pub(crate) fn operation_nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

pub(crate) fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or_default()
}

pub(crate) fn remove_path(path: &Path) -> Result<(), std::io::Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_rejects_values_over_the_managed_path_limit() {
        assert!(!valid_identifier(
            &"a".repeat(MAX_MANAGED_IDENTIFIER_BYTES + 1)
        ));
    }

    #[test]
    fn remove_path_accepts_a_missing_path() {
        let root = tempfile::tempdir().expect("test directory should exist");

        assert!(remove_path(&root.path().join("missing")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn remove_path_does_not_follow_directory_symlinks() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("test directory should exist");
        let target = root.path().join("target");
        let link = root.path().join("link");
        fs::create_dir(&target).expect("target directory should exist");
        fs::write(target.join("keep.txt"), "keep").expect("target file should exist");
        symlink(&target, &link).expect("directory symlink should exist");

        remove_path(&link).expect("symlink should be removed");

        assert!(target.join("keep.txt").is_file());
    }
}
