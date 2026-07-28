# Documentation

## Plugin development

- [Writing Plugins](./PLUGINS.md): supported extension model and end-to-end
  workflow for adding a bundled Rust Provider.
- [Plugin manifest reference](./PLUGIN_MANIFEST.md): exact package fields,
  validation rules, capabilities, Runtime versions, and lifecycle behavior.
- [Audio Sources](./AUDIO_SOURCES.md): separate imported LX JavaScript format
  and lifecycle.

## Architecture decisions

- [Plugin System built on the Source Runtime](./adr/0012-plugin-system-built-on-source-runtime.md)
- [Separate imported Audio Sources from Plugins](./adr/0013-separate-audio-source-lifecycle.md)
- [Registered Plugin Provider contracts](./adr/0015-registered-plugin-provider-contracts.md)

The complete decision history is under [`adr/`](./adr/).
