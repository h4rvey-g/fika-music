# Separate imported Audio Sources from Plugins

Status: accepted

## Context

The first LX JavaScript importer represented each imported playback source as
a Plugin. That reused package lifecycle code, but it leaked the wrong domain
model into the API and UI: imported sources appeared in the Plugin list and
sidebar, Plugin commands performed source import, and the Audio Sources page
mixed configuration with search, Track ID resolution, playback, lyrics, and
artwork.

Plugins are content integrations that may expose broader application behavior.
An imported LX source in the current product has one narrower responsibility:
resolve `musicUrl` through a reviewed Rust adapter. The shared Source Runtime is
an implementation detail and does not require the two concepts to share an
external lifecycle model.

## Decision

Fika will manage imported LX sources through an independent Audio Source
Registry.

- Audio Sources use `AudioSourceRecord` and `audio-source.json`.
- Managed packages live under `<app-data>/audio-sources`, not `plugins`.
- SQLite uses dedicated Audio Source state and diagnostic tables.
- Tauri exposes dedicated list/import/review/enable/remove/dispatch commands.
- The Plugin API returns only Plugins and has no LX source import commands.
- The Source Runtime and reviewed Rust adapter implementation remain reusable
  internal infrastructure.
- Fika ships no built-in Audio Source record. Playback source choices come only
  from enabled user imports or migrated legacy imports.
- The Audio Sources page is configuration-only. It contains no music search,
  Track ID resolver, player, lyrics, or artwork UI.
- Legacy importer-created Plugin packages are migrated at startup with their
  permission state and diagnostics preserved.

## Consequences

- Audio Sources cannot appear in Plugin navigation or Plugin management.
- Plugin and Audio Source lifecycle changes can evolve independently without
  type tags or entrypoint-prefix checks in frontend code.
- Content integrations such as the bundled NetEase Plugin may consume an
  enabled Audio Source for playback without owning that source.
- Source Runtime capability enforcement remains the common security boundary.
- The application must handle an empty Audio Source list and disable remote
  playback until the user imports, reviews, and enables one.
- Legacy `builtin:lx-js:*` Plugin manifests remain recognizable only for
  migration and fail validation for new Plugin installation.
