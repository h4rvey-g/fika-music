# Fika Music

Fika Music is a local-first Tauri music player for indexed local audio and
LX-compatible Rust Source Providers and constrained imported Audio Sources.

The current v0.1 implementation includes:

- Local MP3, FLAC, M4A, and AAC indexing and playback.
- A capability-enforced Rust Source Runtime.
- Bundled and user Plugin package lifecycle through the Plugin System.
- A separate Audio Source Registry and configuration view for local-file and
  HTTP(S) URL import of LX Music JavaScript sources. Imported scripts are
  statically checked, fingerprinted, permission-reviewed, and executed in a
  resource-limited QuickJS runtime with host-mediated network access. The
  registry also supports permission-reviewed, host-registered Rust sources.
- A bundled NetEase Cloud Music Plugin with QR account connection,
  recommendations, Remote Track playback, Playlist list/read, confirmed
  add/remove mutations, and audit history.
- A bundled public YouTube Music catalog Plugin for search, artists, albums,
  playlists, artwork, and lyrics, paired with a separate bundled Rust Audio
  Source that uses an integrity-checked official `yt-dlp` sidecar for playback
  and download.

NetEase behavior is implemented by a Node-free Rust Service Bridge pinned to
the selected `NeteaseCloudMusicApiEnhanced/api-enhanced` v4.32.1 contract.
NetEase and KuGou sessions are stored in the application-private SQLite
database so runtime requests do not trigger operating-system credential
prompts. Source Providers and the frontend receive only opaque Account Refs.

## Development

Prerequisites are Node.js, npm, Rust, and the platform requirements for Tauri 2.

```sh
npm install
npm run tauri dev
```

Frontend-only checks:

```sh
npm run bindings:check
npm run plugins:check
npm test
npm run build
```

Regenerate `src/generated/bindings.ts` after changing a Tauri command or an
IPC DTO with `npm run bindings:generate`.

Rust checks:

```sh
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --all-targets --all-features --locked --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

## Releases

A push to `main` that changes the application version runs the multi-platform
release workflow. Keep the version synchronized in `package.json`,
`package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and
`src-tauri/tauri.conf.json`. The workflow publishes Windows and Linux x86_64
bundles plus Intel and Apple silicon macOS bundles after every build succeeds.
It can also be started manually from GitHub Actions to retry an unpublished
version.

Bundled Plugins start disabled. Enabling one automatically grants the
capabilities declared by its current manifest.

## Documentation

- [Documentation index](./docs/README.md)
- [Writing Plugins](./docs/PLUGINS.md)
- [Plugin manifest reference](./docs/PLUGIN_MANIFEST.md)
