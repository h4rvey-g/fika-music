# Documentation

## Plugin development

- [Writing Plugins](./PLUGINS.md): supported extension model and end-to-end
  workflow for adding a bundled Rust Provider.
- [Plugin manifest reference](./PLUGIN_MANIFEST.md): exact package fields,
  validation rules, capabilities, Runtime versions, and lifecycle behavior.
- [Audio Sources](./AUDIO_SOURCES.md): imported LX JavaScript and bundled Rust
  playback source lifecycle.

## Architecture decisions

- [Plugin System built on the Source Runtime](./adr/0012-plugin-system-built-on-source-runtime.md)
- [Separate imported Audio Sources from Plugins](./adr/0013-separate-audio-source-lifecycle.md)
- [Registered Plugin Provider contracts](./adr/0015-registered-plugin-provider-contracts.md)
- [Separate YouTube Music catalog and playback providers](./adr/0016-separate-youtube-music-catalog-and-playback.md)

The complete decision history is under [`adr/`](./adr/).
