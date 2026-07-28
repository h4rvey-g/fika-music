# Separate YouTube Music catalog and playback providers

Status: accepted

## Context

YouTube Music exposes public catalog data through the WEB_REMIX InnerTube
client, but media playback has a separate and more volatile contract. Player
responses can contain signature ciphers, `n` transforms, client-specific bot
checks, and Proof-of-Origin token requirements. Treating catalog availability
as proof of media availability would publish broken playback routes.

Fika already separates content Plugins from playback-only Audio Sources. ADR
0013 initially stated that Fika shipped no built-in Audio Source because the
only supported Audio Sources were user-imported LX scripts. A native YouTube
extractor cannot be represented honestly as an imported script.

## Decision

Fika ships two independent Rust providers joined only by the `yt` source ID and
the canonical YouTube video ID stored as `videoId`.

- `fika.youtube-music` is a bundled Plugin Provider. It dynamically reads the
  current InnerTube API key, WEB_REMIX client version, and visitor data from
  `music.youtube.com`, then uses host-mediated HTTP for public search,
  suggestions, artists, albums, playlists, artwork, and lyrics.
- The catalog Provider does not declare `musicUrl`, account capabilities, or
  playlist mutation actions. It does not require Python or an external
  process.
- `fika.youtube-music-playback` is a bundled Rust Audio Source. It declares only
  `musicUrl` for `yt`, accepts only canonical video IDs, and selects an
  audio-only media format.
- The playback Provider is a Rust adapter around the official `yt-dlp`
  executable, pinned to release `2026.07.04`. Fika downloads the appropriate
  standalone asset to app-data on first use and verifies its expected byte size
  and SHA-256 digest before execution. `FIKA_YT_DLP_PATH` is an explicit
  development and managed-deployment override. Fika does not statically link
  the GPL-3.0-only `yt-dlp` Rust wrapper crate.
- Sidecar commands use `--ignore-config`, fixed extraction and download
  arguments, a canonical watch URL assembled from the validated video ID,
  bounded captured output, cancellation, and operation timeouts. Playback uses
  `bestaudio[ext=m4a]/bestaudio`; downloads let `yt-dlp` write that audio-only
  format directly to Fika's registered temporary path.
- yt-dlp metadata resolution may use the remaining overall playback deadline
  instead of the shorter generic Audio Source layer timeout. Background
  download resolution has a two-minute cap. Other Audio Sources retain their
  configured layer timeout, and all user cancellation remains immediate.
- Resolved `googlevideo.com` media URLs are not loaded directly by the WebView.
  Fika converts them to its `fika-media` custom protocol and proxies bounded
  byte-range requests through Rust. The protocol accepts only HTTPS
  `*.googlevideo.com/videoplayback` targets, caps each response at 1 MiB, and
  forwards an allowlisted subset of the request headers supplied by `yt-dlp`.
  It also supplies explicit audio, range, CORS, and cross-origin resource
  headers. It is not a general-purpose URL proxy.
- The Audio Source Registry accepts host-registered bundled factories in
  addition to imported QuickJS packages. Bundled registrations are validated
  against their Provider ID, Runtime API, capabilities, and source catalog.
  They use the normal permission review and enable/disable lifecycle but cannot
  be removed.
- Live contract tests are ignored by default. They separately verify public
  catalog routes, a byte-range request against the resolved audio URL, and a
  complete sidecar audio download.

This decision amends only ADR 0013's statement that Fika ships no built-in
Audio Source. The separate Audio Source lifecycle and the imported QuickJS
security model remain unchanged.

## Consequences

- Catalog browsing can continue when media extraction is unavailable, and
  playback failures are attributed to the Audio Source rather than the Plugin.
- WebView cross-origin media protections do not receive the signed CDN URL
  directly, while seeking and progressive playback continue to use bounded
  byte ranges.
- YouTube can change either private contract without notice. Dynamic WEB_REMIX
  bootstrap avoids hard-coded API configuration, while the pinned sidecar and
  live tests make playback breakage explicit and reproducible.
- The Rust adapter and verified sidecar are trusted application components.
  The Audio Source still requires the `network:any` grant and participates in
  cancellation checks and runtime diagnostics, but the subprocess and its
  network access are not the imported-script sandbox.
- First use requires downloading a platform asset of roughly 14-40 MiB. The
  adapter prewarms installation after an enabled source initializes and fails
  closed on unsupported platforms, unexpected size, or digest mismatch.
- Only public, anonymously accessible behavior is advertised. Account library,
  Premium media, private playlists, and write operations remain unsupported.
