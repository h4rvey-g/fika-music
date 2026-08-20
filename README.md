<p align="center">
  <img src="./src-tauri/icons/fika.svg" alt="Fika Music logo" width="160">
</p>

<h1 align="center">Fika Music</h1>

<p align="center">
  A local-first desktop music player built with Tauri, Rust, and Vue.
</p>

<p align="center">
  <a href="https://github.com/h4rvey-g/fika-music/actions/workflows/ci.yml"><img src="https://github.com/h4rvey-g/fika-music/actions/workflows/ci.yml/badge.svg" alt="CI status"></a>
  <a href="https://github.com/h4rvey-g/fika-music/actions/workflows/release.yml"><img src="https://github.com/h4rvey-g/fika-music/actions/workflows/release.yml/badge.svg" alt="Release status"></a>
  <a href="https://github.com/h4rvey-g/fika-music/releases"><img src="https://img.shields.io/github/v/release/h4rvey-g/fika-music?display_name=tag&sort=semver" alt="Latest release"></a>
</p>

Fika Music combines an indexed local library with capability-enforced Source
Providers and separately managed Audio Sources. It supports local playback,
remote catalogs, account-backed services, and constrained LX Music-compatible
JavaScript sources without exposing service credentials to plugins or the
frontend.

## Highlights

- **Local library:** Index and play MP3, FLAC, M4A, and AAC files.
- **Constrained extensions:** Run Rust Source Providers through a
  capability-enforced Source Runtime and manage bundled or user Plugin
  packages through a dedicated lifecycle.
- **Flexible playback sources:** Import LX Music JavaScript sources from local
  files or HTTP(S) URLs. Scripts are statically checked, fingerprinted,
  permission-reviewed, and executed in a resource-limited QuickJS runtime with
  host-mediated network access. The registry also supports permission-reviewed,
  host-registered Rust sources.
- **NetEase Cloud Music:** Connect by QR code, browse recommendations and
  playlists, play remote tracks, confirm playlist mutations, and inspect audit
  history through a Node-free Rust Service Bridge pinned to the
  `NeteaseCloudMusicApiEnhanced/api-enhanced` v4.32.1 contract.
- **YouTube Music:** Search and browse public artists, albums, playlists,
  artwork, and lyrics. Playback and downloads use a separate Rust Audio Source
  with an integrity-checked official `yt-dlp` sidecar.
- **Private account state:** NetEase and KuGou sessions stay in the
  application-private SQLite database. Plugins and the frontend receive only
  opaque Account Refs.

## Getting started

Install Node.js, npm, Rust, and the platform prerequisites for
[Tauri 2](https://v2.tauri.app/start/prerequisites/), then run:

```bash
npm install
npm run tauri dev
```

Bundled Plugins start disabled. Enabling one automatically grants the
capabilities declared by its current manifest.

## Development

Run the frontend and contract checks:

```bash
npm run bindings:check
npm run plugins:check
npm test
npm run build
```

Run the Rust checks:

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --all-targets --all-features --locked --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

After changing a Tauri command or IPC DTO, regenerate the TypeScript bindings:

```bash
npm run bindings:generate
```

## Releases

A push to `main` that changes the application version runs the multi-platform
release workflow. Keep the version synchronized in:

- `package.json`
- `package-lock.json`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/tauri.conf.json`

The workflow publishes Windows and Linux x86_64 bundles, Intel and Apple
silicon macOS bundles, and signed universal Android APK and AAB bundles after
every build succeeds. It can also be started manually from GitHub Actions to
retry an unpublished version.

### Android signing

Android releases use the stable application ID `com.hvg.fikamusic`, configured
in `src-tauri/tauri.android.conf.json`. Configure these GitHub Actions secrets
before running a release:

| Secret | Value |
| --- | --- |
| `ANDROID_KEY_BASE64` | Base64-encoded PKCS#12 Android upload keystore |
| `ANDROID_KEY_ALIAS` | Alias of the key in the keystore |
| `ANDROID_KEY_PASSWORD` | Password shared by the keystore and key |

The workflow generates the Android project from the locked Tauri CLI, signs the
APK and AAB, verifies both signatures, and uploads them to the draft GitHub
Release. Keep the keystore backed up: future Android releases must use the same
key so installed copies can be upgraded.

### macOS ad-hoc signing

The release workflow uses Tauri's ad-hoc signing identity for both macOS
architectures. This does not require an Apple Developer membership or Apple
credentials, and the workflow verifies the app's ad-hoc signature and hardened
runtime after mounting each DMG.

Ad-hoc signatures are not Apple notarization. Gatekeeper can require users to
manually approve the first launch of a downloaded app from an unidentified
developer. A built DMG can be checked locally with:

```bash
bash scripts/verify-macos-dmg.sh path/to/Fika.Music_version_arch.dmg
```

See the [Tauri macOS signing guide](https://v2.tauri.app/distribute/sign/macos/)
for ad-hoc signing details.

### In-app updates

Fika Music checks the latest published GitHub Release on startup and selects the
signed updater artifact for the current operating system and architecture. The
release workflow generates and uploads `latest.json`, updater bundles, and their
signatures when these GitHub Actions secrets are configured:

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Contents of the Tauri updater private key |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password for the updater private key |

The updater public key is committed in `src-tauri/tauri.conf.json`. The private
key must remain backed up outside the repository; losing it prevents installed
copies from trusting future releases. Never commit the private key or its
password.

## Documentation

- [Documentation index](./docs/README.md)
- [Writing Plugins](./docs/PLUGINS.md)
- [Plugin manifest reference](./docs/PLUGIN_MANIFEST.md)
- [Audio Sources](./docs/AUDIO_SOURCES.md)
- [Architecture decisions](./docs/adr/)
