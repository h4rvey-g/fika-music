# Fika Music

Fika Music is a local-first Tauri music player for indexed local audio and
LX-compatible Rust Source Providers.

The current v0.1 implementation includes:

- Local MP3, FLAC, M4A, and AAC indexing and playback.
- A capability-enforced Rust Source Runtime.
- Bundled and user package lifecycle through the Plugin System.
- A bundled NetEase Cloud Music Plugin with QR account connection,
  recommendations, Remote Track playback, Playlist list/read, confirmed
  add/remove mutations, and audit history.

NetEase behavior is implemented by a Node-free Rust Service Bridge pinned to
the selected `NeteaseCloudMusicApiEnhanced/api-enhanced` v4.32.1 contract.
Sessions are stored in the operating-system credential store. Source Providers
and the frontend receive only opaque Account Refs.

## Development

Prerequisites are Node.js, npm, Rust, and the platform requirements for Tauri 2.

```sh
npm install
npm run tauri dev
```

Frontend-only checks:

```sh
npm test
npm run build
```

Rust checks:

```sh
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --all-targets --all-features --locked --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

The bundled NetEase Plugin starts with capabilities ungranted. Review its
permissions in the Plugin view before connecting an account.
