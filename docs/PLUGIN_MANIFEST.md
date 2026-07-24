# Plugin Manifest

Plugin packages are directories containing a `plugin.json` file. The
manifest is metadata and permission policy; it is not permission to execute
arbitrary native code. Imported playback-only Audio Sources are not Plugins;
their separate format is documented in [`AUDIO_SOURCES.md`](./AUDIO_SOURCES.md).

## Schema

```json
{
  "manifestVersion": 1,
  "id": "fika.netease",
  "name": "NetEase Cloud Music",
  "version": "0.1.0",
  "description": "Bundled recommendations and Playlist integration",
  "author": "Fika Music",
  "providerEntrypoints": [
    {
      "id": "fika-netease",
      "entrypoint": "builtin:netease",
      "capabilities": [
        "account:ref",
        "playlist:read",
        "playlist:write",
        "bridge:netease-api-enhanced"
      ],
      "sourceCatalog": {}
    }
  ],
  "capabilities": [],
  "compatibilityTarget": "fika-music",
  "supportedApiVersion": {
    "major": 1,
    "minor": 1
  },
  "requiredHostBridges": ["netease-api-enhanced"]
}
```

`version` uses semantic versioning. Provider IDs must be unique within the
package and across bundled and user-installed packages; a colliding package is
shown as invalid. Package IDs must also be unique across bundled and
user-installed packages. Each `(source, action)` route must belong to exactly
one Provider within a package; overlapping declared routes are rejected during
manifest validation and overlapping runtime catalogs reject activation. The
supported Source Runtime API version must be compatible with the host runtime.
Unknown host bridges leave a package visible but incompatible.

Provider capabilities can be declared at package level or entrypoint level;
the effective declaration for a Provider is the union of package-level and that
entrypoint's declarations. Enabling a Plugin grants its complete declared set,
and each Source Provider receives only its own intersection. Installation does
not execute a Provider, and an entrypoint-only capability is not exposed to
sibling Providers.

The production runtime accepts reserved entrypoints for bundled integrations:
`builtin:netease` requires the `netease-api-enhanced` Service Bridge, while
`builtin:kugou` requires the `kugou-music-api` Service Bridge. User packages
cannot load a dynamic library or launch a sidecar. This keeps package
discovery, capability enforcement, and lifecycle management in place without
turning installation into an untrusted native-code execution boundary.

`builtin:runtime-demo` and `builtin:catalog` exist only in Rust test builds.
`builtin:qishui` is not a production entrypoint. Legacy
`builtin:lx-js:<adapter>:<source-fingerprint>` manifests are recognized only
for startup migration and are rejected as new Plugin packages.

`builtin:netease` is reserved for package `fika.netease` and Provider
`fika-netease`; another package cannot use that host bridge entrypoint.
`builtin:kugou` is likewise reserved for package `fika.kugou` and Provider
`fika-kugou`.

## Audio Source boundary

LX JavaScript source import is owned by the Audio Source Registry and the
Audio Sources view. It never creates a `PluginRecord`, `plugin.json`, Plugin
sidebar entry, or Plugin permission state. The Source Runtime remains shared
internally, but the two registries have separate package formats, directories,
SQLite tables, commands, diagnostics, and lifecycle records.

At startup, legacy user Plugin packages with an importer-owned
`builtin:lx-js:*` entrypoint are converted to managed Audio Source packages.
Their enabled state, permission review, grants, and diagnostics are moved before
the old Plugin rows and directory are removed.

## Locations and lifecycle

- Bundled packages live under the app resource `plugins` directory.
- User packages are copied into the platform app-data `plugins` directory.
- A newly installed package starts disabled and can be enabled immediately.
- Enabling automatically grants every capability declared by the current
  manifest. The Plugin manager presents these declarations as read-only data.
- Bundled packages can be disabled but cannot be removed.
- Removing a user package deactivates its Providers before the package and
  persisted state are deleted.

The registry persists enabled state, capability grants, and the latest bounded
diagnostic history in SQLite. Grant state is bound to a SHA-256 digest of the
normalized manifest, so changing a package manifest disables the package and
clears prior grants; the next Enable action grants the new declaration.
Reinstalling identical manifest content preserves the existing lifecycle state.
Enabled Providers are initialized again during application startup and registry
refresh. Refresh, removal, lifecycle, capability, and diagnostic writes use
SQLite transactions or savepoints. Refresh and removal also restore the prior
Provider handles, runtime grants, in-memory records, and package directory when
a database or filesystem step fails.
Runtime initialization reports, load failures, compatibility failures, and
security denials are exposed per package through the Plugin System commands
and UI.

## Runtime requests

Enabled packages can receive typed Source Runtime requests through the
`dispatch_plugin_request` Tauri command:

```json
{
  "pluginId": "example.source",
  "request": {
    "action": "musicSearch",
    "source": "wy",
    "keyword": "fika",
    "page": 1,
    "pageSize": 20
  },
  "requestId": "optional-cancellation-key"
}
```

The request uses the serialized `SourceRequest` contract. Runtime API 1.0
includes `musicSearch`, `musicUrl`, `lyric`, and `pic`. Runtime API 1.1 adds
`musicRecommendations`, `playlistList`, `playlistRead`, `playlistAddTrack`, and
`playlistRemoveTrack`, with normalized Remote Track and Playlist response
types. The request is rejected unless the package is enabled, its Provider
exposes the requested source/action, and the required capabilities are granted.
Playlist mutations require both `playlist:read` and `playlist:write` so the
bridge can verify Playlist ownership before writing; account-backed calls
resolve an opaque Account Ref through the Provider-scoped host boundary.

A request ID can be cancelled with `cancel_source_request`; cancellation is
cooperative and bounded by the host operation timeout. Database and Plugin
registry locks are released while Provider code runs; completion diagnostics
are attached only if the same Provider instance is still active. A diagnostic
persistence failure is retained as an in-memory warning when possible and never
replaces the Provider response or runtime error returned to the caller.

Package replacement uses a non-overlapping staged copy and revalidates the
manifest before activation. A source package that contains, or is contained by,
the destination or staging workspace is rejected. The previous package and
SQLite lifecycle state are restored if replacement or registry refresh fails.
Removal first moves a user package to a same-filesystem quarantine; it deletes
that quarantine only after the SQLite transaction commits. If quarantine
cleanup fails, the registry restores the package, persisted lifecycle state,
and active Provider state before reporting the failed removal.
