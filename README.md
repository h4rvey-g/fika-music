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
  resource-limited QuickJS runtime with host-mediated network access. No Audio
  Sources are built in.
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
npm run bindings:check
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

The bundled NetEase Plugin starts with capabilities ungranted. Review its
permissions in the Plugin view before connecting an account.
