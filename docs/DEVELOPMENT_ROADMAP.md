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
- No product domain model, Source Runtime API, local music database, or playback engine exists yet.

## Non-negotiable engineering goals

1. **Performance first**
   - Rust owns filesystem scanning, metadata parsing, indexing, playback coordination, Source Runtime supervision, and persistence.
   - The frontend should render state, dispatch commands, and avoid heavy long-running work.
   - Large collections must use incremental indexing, pagination, lazy loading, and background jobs.

2. **Low resource usage**
   - Avoid Electron-style always-heavy architecture; use Tauri for a small native shell.
   - Avoid loading every track, artwork, lyric, and source-provider result into the UI at once.
   - Cache intentionally with bounded size and eviction.

3. **Safe LX compatibility and extensibility**
   - Source Providers should not be trusted by default, whether bundled or user-installed.
   - LX Music-style compatibility is a core app capability, and it must be permissioned, versioned, and observable.
   - Network, filesystem, credential, and playlist-write capabilities should be explicitly granted.

4. **Cross-platform behavior**
   - v0.1 release blocker: current dev platform first only.
   - Keep OS-specific media-key, tray, filesystem watching, and audio-backend behavior behind Rust traits/adapters so macOS, Windows, and Linux can be promoted later without rewriting the core.

5. **Legal and account-safety boundary**
   - Integrations should prefer official APIs, user-owned accounts, user-owned local files, and Source Providers that respect service terms.
   - Do not design around DRM bypass, credential theft, or unauthorized redistribution.

## Proposed architecture

```text
+---------------------------------------------------------------+
| Frontend: Tauri WebView                                       |
| Vue 3 + TypeScript + Tailwind CSS + daisyUI                   |
|                                                               |
| - Library views                                               |
| - Playback queue UI                                           |
| - Source Provider management UI                                |
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
| - Source Provider registry and capability enforcement          |
| - Sync jobs                                                   |
|                                                               |
| Infrastructure:                                               |
| - SQLite database                                             |
| - Filesystem watcher                                          |
| - Artwork/cache store                                         |
| - Audio engine adapter                                        |
| - Source Runtime/provider adapter                             |
| - Service Bridge registry                                     |
| - Built-in netease-api-enhanced bridge                        |
| - Bundled NetEase Source Provider                             |
+-----------------------------+---------------------------------+
                              |
                              | controlled capabilities
                              v
+---------------------------------------------------------------+
| External/local sources                                        |
|                                                               |
| - Local filesystem music folders                              |
| - NetEase Cloud Music account/API                             |
| - User-installed Source Providers                             |
+---------------------------------------------------------------+
```

## Confirmed domain terms

The canonical glossary now lives in [`../CONTEXT.md`](../CONTEXT.md). The most important resolved term is:

- **Source Provider**: a Rust-native online music source module loaded by Fika's Source Runtime.
- **LX Compatibility**: Fika's core ability to model LX Music-style source actions and data contracts through Rust-native Source Providers.
- **Plugin**: a packaged, installable or bundled unit that contains a manifest and one or more Source Providers/assets.
- **Plugin System**: the app layer that installs, validates, enables/disables, permission-reviews, updates, and diagnoses Plugins.

Earlier provisional terms such as **Track**, **Library**, **Playlist**, **Connector**, and **Capability** have also been recorded there and should be refined as the product model gets sharper.

## Decision log

- ADR 0001: [Direct LX Music-style source compatibility](./adr/0001-direct-lx-music-style-source-compatibility.md).
- ADR 0002: [Bundled NetEase Plugin](./adr/0002-bundled-netease-plugin.md).
- ADR 0003: [Constrained source script runtime](./adr/0003-constrained-source-script-runtime.md).
- ADR 0004: [Arbitrary network access for source scripts](./adr/0004-arbitrary-network-access-for-source-scripts.md).
- ADR 0005: [No direct local file access for source scripts](./adr/0005-no-direct-local-file-access-for-source-scripts.md).
- ADR 0006: [NetEase API Enhanced as API basis](./adr/0006-netease-api-enhanced-as-api-basis.md).
- ADR 0007: [Host-provided service bridges for v0.1](./adr/0007-host-provided-service-bridges-for-v0-1.md).
- ADR 0008: [Rust-native source provider runtime](./adr/0008-rquickjs-source-script-runtime.md).
- ADR 0009: [No Node runtime for the NetEase bridge](./adr/0009-no-node-runtime-for-netease-bridge.md).
- ADR 0010: [Opaque account refs for source scripts](./adr/0010-opaque-account-refs-for-source-scripts.md).
- ADR 0011: [Core LX-compatible Source Runtime MVP structure](./adr/0011-core-lx-compatible-source-runtime-mvp.md).
- ADR 0012: [Plugin System built on the Source Runtime](./adr/0012-plugin-system-built-on-source-runtime.md).

## Earlier provisional domain terms

These were the first working definitions before the glossary was created:

- **Track**: a playable audio item, whether local or provided by an integration.
- **Local Track**: a track backed by a file on the user's device.
- **Remote Track**: a track represented by an online service or Source Provider.
- **Library**: the user's indexed collection of known tracks and playlists.
- **Playlist**: an ordered track collection that can be local-only or synced to an external service.
- **Source Provider**: a Rust-native integration module that can search, resolve metadata, and optionally perform authorized account actions.
- **Plugin**: a package that can carry one or more Source Providers and supporting metadata/assets.
- **Connector**: a built-in or first-party integration implemented in Rust or tightly controlled code.
- **Capability**: a permission granted to a Source Provider or Connector, such as network access, playlist write, credential access, or local file read.

Future changes should update `CONTEXT.md` first, then this roadmap only when delivery plans change.

## Initial technology choices

| Area | Recommendation | Why |
|---|---|---|
| Desktop shell | Tauri 2 | Low resource usage and Rust-native backend. |
| Backend language | Rust | Performance, safety, cross-platform system integration. |
| Frontend | Vue 3 + TypeScript | Already scaffolded; strong reactive UI model. |
| UI library | Tailwind CSS v4 + daisyUI 5 | Already installed; fast themed UI composition. |
| Persistence | SQLite via Rust | Local-first, portable, reliable, low operational overhead. |
| Async runtime | Tokio | Needed for network, indexing, Source Runtime supervision, filesystem jobs. |
| Metadata parsing | Rust crates behind an adapter | Keeps parser choice replaceable. |
| Source runtime | Rust-native Source Provider dispatcher | Keeps LX-compatible source behavior in audited Rust code instead of executing original LX JavaScript. |
| Source Provider manifests | JSON/TOML manifest with semver | Supports permission review and compatibility checks. |
| API contracts | TypeScript types generated from Rust models or shared schemas | Reduces Tauri command drift. |

## Major architectural decisions

1. **LX source compatibility model — decided**
   - Decision: Rust-native Source Providers implement LX Music-style actions and data contracts.
   - Rationale: reuse the LX interaction model while keeping executable source behavior in audited Rust code.
   - Constraint: compatibility does not imply unconstrained execution; Source Providers still need declared capabilities, host-mediated network access, diagnostics, and host-mediated credential/playlist access.

2. **Source Runtime — boundary decided**
   - Decision: Rust-native Source Provider dispatch with host-mediated APIs.
   - Sensitive access must go through host capabilities: network, filesystem, credentials, cache, and playlist mutation.
   - Network decision: Source Providers may make arbitrary network requests through the host-mediated network API; requests must still be timeout-bound, logged, cancellable, and visible in diagnostics.
   - Filesystem decision: Source Providers have no direct local file access; Rust owns local library scanning and file IO, while providers use host-mediated cache APIs and app-provided metadata.
   - Service Bridge decision: v0.1 Source Providers can only use host-provided Service Bridges; they cannot bundle, install, or launch arbitrary Node/native sidecars.
   - Source runtime decision: do not execute original LX JavaScript; rewrite LX-compatible behavior as Rust Source Providers.
   - Plugin System decision: installable/bundled Plugin package lifecycle is built above the Source Runtime; LX Compatibility remains core runtime behavior.

3. **NetEase delivery model — decided**
   - Decision: NetEase ships as a bundled Plugin containing an LX Music-style compatible Rust Source Provider.
   - Rationale: this proves the Source Runtime and Plugin System together while keeping the first integration pinned, reviewed, tested, and permissioned as first-party code.
   - API basis decided: use `NeteaseCloudMusicApiEnhanced/api-enhanced` as the NetEase API basis.
   - Service Bridge decision: `api-enhanced` is a host-managed built-in bridge behind the Source Runtime host, not the Source Provider itself.
   - Bridge implementation decided: no Node runtime for v0.1.
   - The built-in NetEase Service Bridge rewrites the needed `api-enhanced` endpoint behavior in Rust.
   - Rust owns HTTP, credentials/cookies, selected crypto/zlib behavior, logging, and mutation audit.
   - Credential decision: Source Providers receive opaque account/session references only; the trusted NetEase Service Bridge attaches stored secrets internally for approved operations.

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
   - v0.1 includes themes, layout density, configurable sidebar sections, keyboard shortcuts, and Source Provider management.
   - Deferred: arbitrary Source Provider UI, full UI extension panels, playback DSP extensions, and provider-defined settings pages beyond standard manifest/capability controls.

## Roadmap phases

### Phase 0 — Product and risk alignment

Goal: answer the high-risk questions before building irreversible foundations.

Current v0.1 sequence decision: **local playback foundation, then LX Compatibility in the core Source Runtime, then the Plugin System, then the bundled NetEase Plugin**.

Deliverables:

- Confirm what "LX-compatible source behavior" means.
- Confirm Source Runtime threat model and allowed capabilities.
- Confirm NetEase account/API strategy and acceptable maintenance/legal risk.
- Confirm MVP playback scope and codec expectations.
- Maintain `CONTEXT.md` glossary as terms are settled.
- Create ADRs only for hard-to-reverse decisions.

Exit criteria:

- The project has a clear MVP boundary.
- Source Runtime, Plugin System, and NetEase Plugin risks are explicit.
- The first implementation slice can be built without guessing.

### Phase 1 — App foundation

Goal: replace the starter template with a real application shell.

Deliverables:

- App layout with daisyUI-based navigation, library area, queue/player area, Source Provider/settings area.
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
    source_runtime.rs
  infrastructure/
    db/
    filesystem/
    audio/
    source_runtime/
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

- A user can manage and play a local music collection without online Source Providers.

### Phase 3 — Core LX-compatible Source Runtime MVP *(Completed)*

Goal: define the smallest safe core Source Runtime that can initialize and dispatch LX Music-style Rust Source Providers without giving them unrestricted app access.

Deliverables:

- LX Music-style source compatibility surface inventory, including source keys, actions, qualities, request payloads, and response shapes.
- Rust-native Source Provider trait and dispatcher prototype.
- Host-mediated bindings for the implemented search and URL-resolution paths; metadata lookup and recommendation fetch remain follow-up contracts until their LX response shapes are finalized.
- Minimal mock Rust Source Providers compiled only for runtime tests, not full Plugin packaging.
- Provider error isolation: provider failures do not crash the app.
- Provider diagnostics visible to the user.
- Source Runtime API versioning and compatibility checks.

Deferred to the Plugin System phase:

- Install/remove/enable/disable flow.
- Plugin package manifest format and version update flow.
- Capability review UI for installed Plugins.
- User-installed provider package directories and trust prompts.

Deferred to the NetEase Plugin phase:

- Built-in `netease-api-enhanced` Service Bridge using no Node runtime.
- Build-time bundling pipeline for selected `api-enhanced` endpoint modules.
- Host-owned bridge polyfills for NetEase-specific HTTP/cookie/crypto/zlib behavior.
- Account connection, playlist list/read, and playlist mutation.

Initial capabilities:

- `network:any`
- `account:ref`
- `playlist:read`
- `playlist:write`
- `metadata:read`
- `cache:read-write`
- `bridge:netease-api-enhanced`
- No `filesystem:*` capability for Source Providers in v0.1.
- No `sidecar:*` capability for Source Providers in v0.1.

Exit criteria:

- A compatible Rust Source Provider can search or recommend tracks through controlled host APIs.
- Runtime capability checks, diagnostics, and errors are validated with mock Source Providers.

Implementation notes for the completed Slice 2 runtime:

- Source Providers declare capabilities separately from host-granted capabilities; grants can be scoped and revoked per Provider, and only the intersection is exposed through `SourceRuntimeContext`.
- Network, cache, and account-reference access go through host-owned bindings with bounded responses/cache storage, timeouts, Provider-scoped opaque refs, sanitized network targets in diagnostics, and cooperative cancellation.
- Provider API `1.0` compatibility, initialized catalogs, typed LX request/response envelopes, invalid catalog/request handling, provider errors and panics, response validation, and in-flight cancellation are covered by Rust tests.
- Tauri remote-source commands are asynchronous blocking tasks, return structured diagnostics, accept cancellable request IDs, and the frontend displays diagnostics for both successful and failed remote requests.
- The current Rust-native Providers are reviewed in-process code. Capability checks are not an operating-system sandbox for arbitrary native binaries; installable untrusted native Providers remain outside the v0.1 trust model.

### Phase 4 — Plugin System MVP

Goal: add the installable/bundled Plugin layer on top of the core Source Runtime without changing the LX compatibility core.

Deliverables:

- Plugin package manifest format: id, name, version, author, provider entrypoints, compatibility target, declared capabilities, supported API version, required host bridges.
- Plugin registry for bundled and user-installed Plugin packages.
- Plugin install/remove/enable/disable flow.
- Capability review UI and permission persistence.
- Plugin diagnostics view using Source Runtime logs and security events.
- Plugin API versioning, compatibility checks, and unsupported-plugin error states.
- User-installed Plugin storage directory and import validation.
- Clear separation between Plugin package lifecycle and Source Provider dispatch.

Exit criteria:

- A user or bundled package can install, inspect, enable, disable, and diagnose a Plugin that runs through the Source Runtime.
- Plugin permissions are inspectable and revocable.

### Phase 5 — NetEase Cloud Music Plugin

Goal: ship the first real Plugin on top of the core LX-compatible Source Runtime and Plugin System.

Deliverables:

- Bundled NetEase Plugin package.
- Bundled NetEase Rust Source Provider loaded through the Plugin System.
- Node-free Rust implementation of the selected `netease-api-enhanced` behavior.
- Rust host services for HTTP client, cookie jar, credential references, crypto/zlib behavior where needed, timers/cancellation, and logging.
- Removal or replacement of Node-only assumptions from the upstream reference: Express server code, dynamic `require` scanning, arbitrary filesystem access, process globals, and Node HTTP agents.
- NetEase account connection flow.
- Token/session storage strategy using OS credential storage where feasible, with encrypted app storage only as a fallback.
- Opaque Account Ref model for Source Provider and Service Bridge calls.
- Recommendation fetch.
- Playlist list/read.
- Save selected supported track to a selected NetEase playlist.
- Remove selected track from a NetEase playlist with confirmation and audit.
- Defer automatic sync and bulk destructive changes.
- Rate-limit handling and retry policy.
- Conflict/error reporting when a local/remote track cannot be matched.
- Integration tests around API contract boundaries, using mocks where needed.

Important constraints:

- Delivery model decided: bundled NetEase Plugin containing a compatible Rust Source Provider.
- API basis decided: `NeteaseCloudMusicApiEnhanced/api-enhanced`.
- Service Bridge model decided: `api-enhanced` is host-managed and not bundled/launched by the Source Provider.
- Bridge implementation decided: no Node runtime and no QuickJS bundle; selected endpoint behavior is rewritten in Rust.
- The permitted v0.1 API surface should stay limited to recommendations, playlist list/read, add selected track, and remove selected track.
- Account-safety policy still needs operational detail for NetEase login/session expiry and upstream API breakage.

Exit criteria:

- A user can view recommendations and save supported tracks to selected NetEase playlists.

### Phase 6 — Advanced library and sync

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

### Phase 7 — Customization and polish

Goal: make the app highly customizable without compromising performance or safety.

Deliverables:

- Theme selection using daisyUI themes.
- Layout density settings.
- Keyboard shortcut editor.
- Command palette.
- Configurable sidebar sections.
- Source Provider management through standard app UI only.
- Deferred Source Provider-provided commands or views until the Source Runtime security model supports them.
- Accessibility pass.
- Localization infrastructure.

Exit criteria:

- Users can personalize the app while the core app remains stable and resource-efficient.

### Phase 8 — Packaging, updates, and hardening

Goal: prepare for real distribution.

Deliverables:

- Signed build for the current dev platform first.
- Promote macOS, Windows, and Linux to release-blocking platforms when playback, filesystem watching, and packaging are proven on each target.
- Auto-update strategy.
- Crash/error reporting strategy, opt-in if telemetry is used.
- Backup/restore of local database and settings.
- Source Provider trust model documentation.
- Security review of Source Runtime capability enforcement and credential handling.
- Performance benchmarks and regression tests.

Exit criteria:

- The app is safe enough to distribute to early users.

## Suggested v0.1 implementation sequence

Decision: build LX compatibility first, then the Plugin System, then the NetEase Plugin.

### Slice 1 — Local playback foundation

1. Replace starter UI with app shell.
2. Add Rust command to choose a music folder.
3. Scan a few audio files in a background job.
4. Persist track rows in SQLite.
5. Display indexed tracks in the frontend.
6. Play one local track.
7. Show indexing progress and errors.

This validates Tauri commands, Rust jobs, database, UI state, and performance assumptions before investing in Source Runtime complexity. *(Completed)*

### Slice 2 — Core LX-compatible Source Runtime MVP *(Completed)*

1. **Rust Source Provider Runtime**: Define a Rust trait and dispatcher for LX-style source initialization and request handling.
2. **Capability Framework**: Implement runtime permission checking and restriction (e.g. `network:any`, `account:ref`, `cache:read-write`) to prevent unauthorized platform actions.
3. **LX Compatibility Model**: Represent LX Music-style source keys, actions, qualities, request payloads, response shapes, and diagnostics in Rust types.
4. **Mock Rust Source Providers**: Run in-repo mock providers directly through the Source Runtime without plugin package/install semantics.
5. **Runtime Verification**: Validate LX compatibility, capability enforcement, provider error handling, and diagnostic capture.

This ensures the core LX provider model and safety boundaries are validated before adding package lifecycle, installation, or NetEase-specific endpoint code.

### Slice 3 — Plugin System MVP

1. **Plugin Package Manifest**: Define package metadata, Source Provider entrypoints, capabilities, compatibility target, and required host bridges.
2. **Plugin Registry**: Scan bundled and user-installed Plugin locations, validate manifests, and expose Plugin state to the frontend.
3. **Lifecycle Management**: Add install/remove/enable/disable flows and persistence.
4. **Capability Review UI**: Let users inspect and revoke Plugin capabilities before the Plugin's Source Providers run.
5. **Diagnostics**: Surface Source Runtime logs, security denials, load errors, and compatibility failures per Plugin.

This adds a user-visible Plugin System on top of the core LX-compatible Source Runtime without redefining LX compatibility as a plugin feature.

### Slice 4 — NetEase Plugin

1. **Bundled NetEase Plugin Package**: Package the NetEase Rust Source Provider and manifest as a bundled Plugin.
2. **NetEase Service Bridge**: Build the Node-free Rust Service Bridge for the selected `netease-api-enhanced` behavior.
3. **Read Path**: Fetch recommendation feeds and normalize them into **Remote Track** results in the frontend.
4. **Account & Playlist Path**: Add account connection, Account Refs, playlist list/read, and explicit playlist mutations.
5. **Audit & Error States**: Add mutation audit logs and clear errors for unsupported tracks, credential expiry, bridge failures, and API failures.

This validates the first production Plugin after both the core LX-compatible Source Runtime and the Plugin System exist.

## Grill backlog

These should be answered one at a time:

1. What exactly should "JS sources similar to LX Music" mean for compatibility, permissions, and legal boundaries?
2. Should NetEase be a built-in connector or a bundled Plugin? — resolved: bundled NetEase Plugin containing a compatible Rust Source Provider.
3. What is the MVP: local playback first, NetEase recommendations first, Source Runtime first, or Plugin System first? — resolved: local playback → LX Compatibility → Plugin System → NetEase Plugin.
4. Which platforms are release blockers: macOS, Windows, Linux, or all three? — resolved: current dev platform first only for v0.1.
5. What audio formats and playback features are mandatory for v0.1? — resolved: MP3, FLAC, M4A/AAC plus play/pause/seek/next/previous/volume.
6. How large should the target local library be for performance testing? — resolved: 50,000 tracks for v0.1 benchmarks.
7. Are Source Providers allowed to run arbitrary JavaScript? — resolved: no; LX-compatible behavior is rewritten as Rust Source Providers.
8. Can Source Providers make arbitrary network requests, or only declared hosts? — resolved: arbitrary network requests through the host-mediated network API.
9. Can Source Providers access local files directly, or only through host-mediated APIs? — resolved: no direct local file access; host-mediated cache and app-provided metadata only.
10. How should credentials be stored and exposed to Source Providers/connectors? — resolved: Source Providers receive opaque Account Refs only; the trusted Service Bridge attaches secrets internally.
11. Should external playlist modification be one-way, two-way, or manual only? — resolved: manual remote playlist operations only for v0.1.
12. What customization is required for v0.1 versus later? — resolved: core customization only for v0.1.

## Immediate next step

Next step after this roadmap: begin Slice 3 with the Plugin Package Manifest and registry, keeping Provider execution behind the completed Source Runtime boundary.
