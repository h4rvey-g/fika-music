# Fika Music Development Roadmap

Status: draft, to be refined through grilling decisions.
Last updated: 2026-07-06

## Product vision

Fika Music is a cross-platform desktop music player built with a Rust backend and a Tauri + Vue + Tailwind CSS + daisyUI frontend. It aims to combine:

- high-performance local music management,
- extensible online music-source integrations,
- user-controlled customization,
- low CPU, memory, network, and disk overhead.

## Current repository state

The project currently starts from the Tauri Vue TypeScript template:

- Frontend: Vue 3, TypeScript, Vite, Tailwind CSS v4, daisyUI 5.
- Backend: Rust, Tauri 2, minimal generated `greet` command.
- No product domain model, plugin API, local music database, or playback engine exists yet.

## Non-negotiable engineering goals

1. **Performance first**
   - Rust owns filesystem scanning, metadata parsing, indexing, playback coordination, plugin process supervision, and persistence.
   - The frontend should render state, dispatch commands, and avoid heavy long-running work.
   - Large collections must use incremental indexing, pagination, lazy loading, and background jobs.

2. **Low resource usage**
   - Avoid Electron-style always-heavy architecture; use Tauri for a small native shell.
   - Avoid loading every track, artwork, lyric, and plugin result into the UI at once.
   - Cache intentionally with bounded size and eviction.

3. **Safe extensibility**
   - Plugins should not be trusted by default.
   - Any JavaScript-source compatibility must be sandboxed, permissioned, versioned, and observable.
   - Network, filesystem, credential, and playlist-write capabilities should be explicitly granted.

4. **Cross-platform behavior**
   - v0.1 release blocker: current dev platform first only.
   - Keep OS-specific media-key, tray, filesystem watching, and audio-backend behavior behind Rust traits/adapters so macOS, Windows, and Linux can be promoted later without rewriting the core.

5. **Legal and account-safety boundary**
   - Integrations should prefer official APIs, user-owned accounts, user-owned local files, and source plugins that respect service terms.
   - Do not design around DRM bypass, credential theft, or unauthorized redistribution.

## Proposed architecture

```text
+---------------------------------------------------------------+
| Frontend: Tauri WebView                                       |
| Vue 3 + TypeScript + Tailwind CSS + daisyUI                   |
|                                                               |
| - Library views                                               |
| - Playback queue UI                                           |
| - Plugin management UI                                        |
| - NetEase recommendation and playlist management UI           |
| - Settings, permissions, theming, diagnostics                 |
+-----------------------------+---------------------------------+
                              |
                              | typed Tauri commands/events
                              v
+---------------------------------------------------------------+
| Rust application core                                         |
|                                                               |
| Domain services:                                              |
| - Library indexing                                            |
| - Metadata extraction                                         |
| - Playback orchestration                                      |
| - Queue/session state                                         |
| - Playlist model                                              |
| - Plugin registry and capability enforcement                  |
| - Sync jobs                                                   |
|                                                               |
| Infrastructure:                                               |
| - SQLite database                                             |
| - Filesystem watcher                                          |
| - Artwork/cache store                                         |
| - Audio engine adapter                                        |
| - Plugin runtime/sandbox adapter                              |
| - Service Bridge registry                                     |
| - Built-in netease-api-enhanced bridge                        |
| - Bundled NetEase Source Script                               |
+-----------------------------+---------------------------------+
                              |
                              | controlled capabilities
                              v
+---------------------------------------------------------------+
| External/local sources                                        |
|                                                               |
| - Local filesystem music folders                              |
| - NetEase Cloud Music account/API                             |
| - User-installed source plugins                               |
+---------------------------------------------------------------+
```

## Confirmed domain terms

The canonical glossary now lives in [`../CONTEXT.md`](../CONTEXT.md). The most important resolved term is:

- **Source Script**: a JavaScript integration module compatible with the LX Music-style source model.

Earlier provisional terms such as **Track**, **Library**, **Playlist**, **Connector**, and **Capability** have also been recorded there and should be refined as the product model gets sharper.

## Decision log

- ADR 0001: [Direct LX Music-style source compatibility](./adr/0001-direct-lx-music-style-source-compatibility.md).
- ADR 0002: [Bundled NetEase source script](./adr/0002-bundled-netease-source-script.md).
- ADR 0003: [Constrained source script runtime](./adr/0003-constrained-source-script-runtime.md).
- ADR 0004: [Arbitrary network access for source scripts](./adr/0004-arbitrary-network-access-for-source-scripts.md).
- ADR 0005: [No direct local file access for source scripts](./adr/0005-no-direct-local-file-access-for-source-scripts.md).
- ADR 0006: [NetEase API Enhanced as API basis](./adr/0006-netease-api-enhanced-as-api-basis.md).
- ADR 0007: [Host-provided service bridges for v0.1](./adr/0007-host-provided-service-bridges-for-v0-1.md).
- ADR 0008: [rquickjs source script runtime](./adr/0008-rquickjs-source-script-runtime.md).
- ADR 0009: [No Node runtime for the NetEase bridge](./adr/0009-no-node-runtime-for-netease-bridge.md).
- ADR 0010: [Opaque account refs for source scripts](./adr/0010-opaque-account-refs-for-source-scripts.md).

## Earlier provisional domain terms

These were the first working definitions before the glossary was created:

- **Track**: a playable audio item, whether local or provided by an integration.
- **Local Track**: a track backed by a file on the user's device.
- **Remote Track**: a track represented by an online service or plugin.
- **Library**: the user's indexed collection of known tracks and playlists.
- **Playlist**: an ordered track collection that can be local-only or synced to an external service.
- **Source Plugin**: a user-installed integration module that can search, resolve metadata, and optionally perform authorized account actions.
- **Connector**: a built-in or first-party integration implemented in Rust or tightly controlled code.
- **Capability**: a permission granted to a plugin or connector, such as network access, playlist write, credential access, or local file read.

Future changes should update `CONTEXT.md` first, then this roadmap only when delivery plans change.

## Initial technology choices

| Area | Recommendation | Why |
|---|---|---|
| Desktop shell | Tauri 2 | Low resource usage and Rust-native backend. |
| Backend language | Rust | Performance, safety, cross-platform system integration. |
| Frontend | Vue 3 + TypeScript | Already scaffolded; strong reactive UI model. |
| UI library | Tailwind CSS v4 + daisyUI 5 | Already installed; fast themed UI composition. |
| Persistence | SQLite via Rust | Local-first, portable, reliable, low operational overhead. |
| Async runtime | Tokio | Needed for network, indexing, plugin supervision, filesystem jobs. |
| Metadata parsing | Rust crates behind an adapter | Keeps parser choice replaceable. |
| Source runtime | rquickjs | Runs LX Music-style Source Scripts inside the Rust architecture without a full Node.js environment. |
| Plugin manifests | JSON/TOML manifest with semver | Supports permission review and compatibility checks. |
| API contracts | TypeScript types generated from Rust models or shared schemas | Reduces Tauri command drift. |

## Major architectural decisions

1. **JavaScript source compatibility model — decided**
   - Decision: direct compatibility with LX Music-style source scripts.
   - Rationale: reuse of existing source behavior is central to the product direction.
   - Constraint: compatibility does not imply unconstrained execution; source scripts still need sandboxing, timeouts, declared capabilities, host-mediated network access, and host-mediated credential/playlist access.

2. **Plugin runtime — boundary decided**
   - Decision: constrained JavaScript with host-mediated APIs.
   - Sensitive access must go through host capabilities: network, filesystem, credentials, cache, and playlist mutation.
   - Network decision: Source Scripts may make arbitrary network requests through the host-mediated network API; requests must still be timeout-bound, logged, cancellable, and visible in diagnostics.
   - Filesystem decision: Source Scripts have no direct local file access; Rust owns local library scanning and file IO, while scripts use host-mediated cache APIs and app-provided metadata.
   - Service Bridge decision: v0.1 Source Scripts can only use host-provided Service Bridges; they cannot bundle, install, or launch arbitrary Node/native sidecars.
   - Source runtime decision: use `rquickjs` for Source Scripts.
   - Still open: exact process/isolation boundary around `rquickjs` execution.

3. **NetEase delivery model — decided**
   - Decision: NetEase ships as a bundled LX Music-style compatible Source Script.
   - Rationale: this proves the compatibility runtime while keeping the first integration pinned, reviewed, tested, and permissioned as first-party code.
   - API basis decided: use `NeteaseCloudMusicApiEnhanced/api-enhanced` as the NetEase API basis.
   - Service Bridge decision: `api-enhanced` is a host-managed built-in bridge behind the Rust Plugin Host, not the plugin itself.
   - Bridge implementation decided: no Node runtime for v0.1.
   - The built-in NetEase Service Bridge uses a build-time bundled, QuickJS-compatible subset of `api-enhanced` endpoint behavior.
   - Rust owns host polyfills for HTTP, credentials/cookies, selected Buffer/crypto/zlib behavior, logging, and mutation audit.
   - Credential decision: Source Scripts receive opaque account/session references only; the trusted NetEase Service Bridge attaches stored secrets internally for approved operations.

4. **Playback engine scope — decided for v0.1**
   - Required formats: MP3, FLAC, and M4A/AAC playback and metadata.
   - Required controls: play, pause, seek, next, previous, and volume.
   - Deferred: gapless playback, replaygain, exclusive output mode, DSP/effects, and broad codec coverage.
   - Playback engine choice still needs validation against resource use and current-platform reliability.

5. **Local library database schema**
   - Must support large libraries, duplicate detection, file moves, album grouping, artwork, lyrics, and external identifiers.

6. **Cloud playlist modification semantics — decided for v0.1**
   - Decision: manual remote playlist operations only.
   - v0.1 supports listing/reading NetEase playlists, adding selected supported tracks, and removing selected tracks with explicit confirmation and audit.
   - Deferred: automatic one-way sync, two-way sync, bulk destructive operations, and reorder unless needed after add/remove works.

7. **Customization boundary — decided for v0.1**
   - v0.1 includes themes, layout density, configurable sidebar sections, keyboard shortcuts, and source-script management.
   - Deferred: arbitrary plugin UI, full UI extension panels, playback DSP plugins, and plugin-defined settings pages beyond standard manifest/capability controls.

## Roadmap phases

### Phase 0 — Product and risk alignment

Goal: answer the high-risk questions before building irreversible foundations.

Current v0.1 sequence decision: **hybrid: local playback foundation, then NetEase read through the bundled source script, then NetEase playlist write**.

Deliverables:

- Confirm what "JS sources similar to LX Music" means.
- Confirm plugin threat model and allowed capabilities.
- Confirm NetEase account/API strategy and acceptable maintenance/legal risk.
- Confirm MVP playback scope and codec expectations.
- Maintain `CONTEXT.md` glossary as terms are settled.
- Create ADRs only for hard-to-reverse decisions.

Exit criteria:

- The project has a clear MVP boundary.
- Plugin and NetEase integration risks are explicit.
- The first implementation slice can be built without guessing.

### Phase 1 — App foundation

Goal: replace the starter template with a real application shell.

Deliverables:

- App layout with daisyUI-based navigation, library area, queue/player area, plugin/settings area.
- Typed Tauri command/event layer.
- Rust module structure for domain, infrastructure, application commands, and adapters.
- Logging/tracing setup with a user-accessible diagnostics view.
- Basic settings store.
- CI checks for frontend build, Rust build, formatting, linting, and tests.

Suggested Rust modules:

```text
src-tauri/src/
  app.rs
  commands/
  domain/
    track.rs
    playlist.rs
    library.rs
    plugin.rs
  infrastructure/
    db/
    filesystem/
    audio/
    plugins/
    netease/
  jobs/
  events.rs
  errors.rs
```

Exit criteria:

- The app opens to a real shell instead of template content.
- Frontend and Rust communicate through stable typed commands.
- Basic telemetry/logging exists for debugging performance issues.

### Phase 2 — Local music MVP

Goal: deliver a fast local-library music player before relying on online integrations.

Deliverables:

- Add/remove watched music folders.
- Background scanner for common audio files.
- Metadata extraction for title, artist, album, duration, track number, disc number, year, genre, and embedded artwork pointer.
- SQLite schema and migrations.
- Incremental re-scan and filesystem watch support.
- Search and filters with pagination/virtualized rendering.
- Basic playback queue.
- MP3, FLAC, and M4A/AAC playback.
- Play/pause/seek/next/previous/volume.
- Basic local playlists.

Performance targets to validate:

- v0.1 benchmark library size: 50,000 tracks.
- App cold start remains fast with a large existing database.
- UI remains responsive during indexing.
- Indexing can be paused/cancelled.
- Memory use is bounded while scanning and browsing the benchmark library.
- Track lists, search results, and playlists use pagination or virtualization rather than loading all rows into the UI.

Exit criteria:

- A user can manage and play a local music collection without plugins.

### Phase 3 — Plugin platform MVP

Goal: define the smallest safe plugin system that can run LX Music-style source scripts without giving them unrestricted app access.

Deliverables:

- LX Music-style source compatibility surface inventory, including `globalThis.lx`, `EVENT_NAMES`, `on`, `send`, `request`, and required `utils`.
- Fika extension surface for behavior LX does not model, including recommendations, account connection, playlist list/read, and playlist mutation.
- Compatibility adapter for the subset needed by the first NetEase source script.
- Host-provided Service Bridge registry.
- Built-in `netease-api-enhanced` Service Bridge using no Node runtime.
- Build-time bundling pipeline for selected `api-enhanced` endpoint modules.
- Host-owned polyfills for the bridge: HTTP client, cookie jar, credential references, Buffer subset, crypto subset, zlib subset where needed, timers/cancellation, and logging.
- Removal or replacement of Node-only assumptions from the bridge bundle: Express server code, dynamic `require` scanning, arbitrary filesystem access, process globals, and Node HTTP agents.
- Plugin/source manifest format: id, name, version, author, entrypoint, compatibility target, required capabilities, supported API version, required host bridges.
- Plugin install/remove/enable/disable flow.
- Capability review UI.
- Sandboxed runtime prototype.
- Host APIs, Service Bridge calls, and compatibility shims for search, metadata lookup, recommendation fetch, and optional playlist-write operations.
- Source-script error isolation: crashes and timeouts do not crash the app.
- Source-script logs visible to the user.
- Plugin API versioning and compatibility checks.

Initial capabilities:

- `network:any`
- `account:ref`
- `playlist:read`
- `playlist:write`
- `metadata:read`
- `cache:read-write`
- `bridge:netease-api-enhanced`
- No `filesystem:*` capability for Source Scripts in v0.1.
- No `sidecar:*` capability for Source Scripts in v0.1.

Exit criteria:

- A compatible source script can search or recommend tracks through controlled host APIs.
- Source-script permissions are inspectable and revocable.

### Phase 4 — NetEase Cloud Music integration

Goal: ship the first real integration around recommendations and playlist operations.

Deliverables:

- NetEase account connection flow.
- Token/session storage strategy using OS credential storage where feasible, with encrypted app storage only as a fallback.
- Opaque Account Ref model for Source Script and Service Bridge calls.
- Recommendation fetch.
- Playlist list/read.
- Save selected supported track to a selected NetEase playlist.
- Remove selected track from a NetEase playlist with confirmation and audit.
- Defer automatic sync and bulk destructive changes.
- Rate-limit handling and retry policy.
- Conflict/error reporting when a local/remote track cannot be matched.
- Integration tests around API contract boundaries, using mocks where needed.

Important constraints:

- Delivery model decided: bundled compatible Source Script.
- API basis decided: `NeteaseCloudMusicApiEnhanced/api-enhanced`.
- Service Bridge model decided: `api-enhanced` is host-managed and not bundled/launched by the Source Script.
- Bridge implementation decided: no Node runtime; build-time bundled QuickJS-compatible endpoint subset with Rust-owned host polyfills.
- The permitted v0.1 API surface should stay limited to recommendations, playlist list/read, add selected track, and remove selected track.
- Account-safety policy still needs operational detail for NetEase login/session expiry and upstream API breakage.

Exit criteria:

- A user can view recommendations and save supported tracks to selected NetEase playlists.

### Phase 5 — Advanced library and sync

Goal: make local and remote music management feel coherent.

Deliverables:

- Better duplicate detection and track matching.
- Album artist / composer / multi-artist handling.
- Lyrics support.
- Artwork cache and refresh policy.
- Smart playlists.
- Import/export playlists.
- Optional one-way or two-way playlist sync, depending on earlier decisions.
- Conflict resolution UI.

Exit criteria:

- Users can organize local and NetEase-backed collections without losing track of source ownership or sync state.

### Phase 6 — Customization and polish

Goal: make the app highly customizable without compromising performance or safety.

Deliverables:

- Theme selection using daisyUI themes.
- Layout density settings.
- Keyboard shortcut editor.
- Command palette.
- Configurable sidebar sections.
- Source-script management through standard app UI only.
- Deferred plugin-provided commands or views until the plugin security model supports them.
- Accessibility pass.
- Localization infrastructure.

Exit criteria:

- Users can personalize the app while the core app remains stable and resource-efficient.

### Phase 7 — Packaging, updates, and hardening

Goal: prepare for real distribution.

Deliverables:

- Signed build for the current dev platform first.
- Promote macOS, Windows, and Linux to release-blocking platforms when playback, filesystem watching, and packaging are proven on each target.
- Auto-update strategy.
- Crash/error reporting strategy, opt-in if telemetry is used.
- Backup/restore of local database and settings.
- Plugin trust model documentation.
- Security review of plugin sandbox and credential handling.
- Performance benchmarks and regression tests.

Exit criteria:

- The app is safe enough to distribute to early users.

## Suggested v0.1 implementation sequence

Decision: build the hybrid sequence.

### Slice 1 — Local playback foundation

1. Replace starter UI with app shell.
2. Add Rust command to choose a music folder.
3. Scan a few audio files in a background job.
4. Persist track rows in SQLite.
5. Display indexed tracks in the frontend.
6. Play one local track.
7. Show indexing progress and errors.

This validates Tauri commands, Rust jobs, database, UI state, and performance assumptions before investing in plugin complexity.

### Slice 2 — NetEase read path

1. Add the minimal source-script runtime needed by the bundled NetEase Source Script.
2. Load the bundled script with a pinned compatibility target.
3. Grant only recommendation-read and required network capabilities.
4. Fetch recommendations and normalize them into **Remote Track** results.
5. Display recommendations without writing to NetEase playlists.

This validates LX Music-style compatibility without exposing playlist mutation yet.

### Slice 3 — NetEase playlist write path

1. Add account connection and credential storage.
2. Add host-mediated playlist list/read capabilities.
3. Add explicit playlist-write capability review.
4. Save a selected supported track to a selected NetEase playlist.
5. Add mutation audit logs and clear error states for unsupported tracks or API failures.

This validates the highest-risk account action only after local playback and read-only source compatibility work.

## Grill backlog

These should be answered one at a time:

1. What exactly should "JS sources similar to LX Music" mean for compatibility, permissions, and legal boundaries?
2. Should NetEase be a built-in connector or the first plugin? — resolved: bundled compatible Source Script.
3. What is the MVP: local playback first, NetEase recommendations first, or plugin runtime first? — resolved: hybrid sequence, local playback → NetEase read → playlist write.
4. Which platforms are release blockers: macOS, Windows, Linux, or all three? — resolved: current dev platform first only for v0.1.
5. What audio formats and playback features are mandatory for v0.1? — resolved: MP3, FLAC, M4A/AAC plus play/pause/seek/next/previous/volume.
6. How large should the target local library be for performance testing? — resolved: 50,000 tracks for v0.1 benchmarks.
7. Are plugins allowed to run arbitrary JavaScript, or only a constrained API? — resolved: constrained JavaScript with host-mediated APIs.
8. Can plugins make arbitrary network requests, or only declared hosts? — resolved: arbitrary network requests through the host-mediated network API.
9. Can plugins access local files directly, or only through host-mediated APIs? — resolved: no direct local file access; host-mediated cache and app-provided metadata only.
10. How should credentials be stored and exposed to plugins/connectors? — resolved: Source Scripts receive opaque Account Refs only; the trusted Service Bridge attaches secrets internally.
11. Should external playlist modification be one-way, two-way, or manual only? — resolved: manual remote playlist operations only for v0.1.
12. What customization is required for v0.1 versus later? — resolved: core customization only for v0.1.

## Immediate next step

Next step after this roadmap: start Slice 1, the local playback foundation, after validating dependency choices for SQLite, metadata parsing, and audio playback.
