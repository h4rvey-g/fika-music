# Plugin manifest reference

`plugin.json` is the versioned package and permission contract consumed by the
Plugin System. It does not load code from the package. A Provider can run only
when its symbolic entrypoint is registered by the host.

> Production currently has no unreserved third-party Provider entrypoint.
> Installing a package cannot add Rust code, a dynamic library, JavaScript, or
> a sidecar. See [Writing Plugins](./PLUGINS.md) for the supported extension
> workflow.

## Validate a package

Validate all bundled packages:

```sh
npm run plugins:check
```

Validate one package directory or its `plugin.json`:

```sh
npm run plugins:validate -- path/to/package
```

The command uses the same Rust deserializer, structural checks, and registered
Provider contracts as the application. Unknown JSON fields are rejected, so a
misspelled field cannot silently fall back to a default.

## Canonical example

This is the complete bundled KuGou manifest:

```json
{
  "manifestVersion": 1,
  "id": "fika.kugou",
  "name": "KuGou Music",
  "version": "0.1.0",
  "description": "Bundled KuGou search, QR login, playback, recommendations, and Playlist integration.",
  "author": "Fika Music",
  "homepage": "https://github.com/MakcRe/KuGouMusicApi",
  "providerEntrypoints": [
    {
      "id": "fika-kugou",
      "entrypoint": "builtin:kugou",
      "capabilities": [
        "account:ref",
        "playlist:read",
        "playlist:write",
        "bridge:kugou-music-api"
      ]
    }
  ],
  "capabilities": [],
  "compatibilityTarget": "fika-music",
  "supportedApiVersion": {
    "major": 1,
    "minor": 4
  },
  "requiredHostBridges": ["kugou-music-api"]
}
```

## Package fields

| Field | Type | Required | Rule |
| --- | --- | --- | --- |
| `manifestVersion` | integer | Yes | Must equal `1`. |
| `id` | string | Yes | Globally unique Plugin ID. Maximum 128 bytes; starts with an ASCII letter or digit; remaining characters are ASCII letters, digits, `.`, `_`, or `-`. |
| `name` | string | Yes | Non-empty after trimming. |
| `version` | string | Yes | Valid semantic version, including SemVer prerelease rules. |
| `description` | string or `null` | No | Display text. Defaults to `null`. |
| `author` | string or `null` | No | Display text. Defaults to `null`. |
| `homepage` | string or `null` | No | Display text; the current validator does not parse it as a URL. |
| `providerEntrypoints` | array | Yes | At least one Provider entry. Provider IDs must be globally unique across discovered packages. |
| `capabilities` | capability array | No | Capabilities inherited by every Provider in this package. Defaults to `[]`. |
| `compatibilityTarget` | string | Yes | `fika-music` or `*` is compatible with this host. Any other non-empty value makes the package incompatible. |
| `supportedApiVersion` | object | Yes | Source Runtime contract required by the package. The registered entrypoint contract must declare the same version. |
| `requiredHostBridges` | string array | No | Host bridge IDs required before activation. Uses the same identifier grammar as `id`. Defaults to `[]`. |

Canonical manifests use exactly the field names above. The deserializer still
accepts older aliases (`providers`, `sourceProviders`,
`sourceProviderEntrypoints`, `sourceRuntimeApiVersion`, and `hostBridges`) for
migration, but new packages must not use them. A legacy `manifest.json` file is
also readable; `plugin.json` is the only current package filename.

## Provider fields

| Field | Type | Required | Rule |
| --- | --- | --- | --- |
| `id` | string | Yes | Globally unique Provider ID using the Plugin ID grammar. |
| `entrypoint` | string | Yes | Symbolic host entrypoint with no `/`, `\`, or control characters. It must exist in `PluginProviderCatalog`. |
| `capabilities` | capability array | No | Provider-specific declarations. Defaults to `[]`. |
| `sourceCatalog` | object keyed by source ID | No | Optional pre-activation metadata. Built-in Rust Providers normally omit it and return the authoritative catalog from `initialize`. |

The effective capability set for one Provider is:

```text
package capabilities union Provider capabilities
```

The Provider factory must return exactly that set from
`SourceProvider::required_capabilities`. Entry-point capabilities never leak to
a sibling Provider.

If `sourceCatalog` is present, every object key must equal its source `id`, the
source name must be non-empty, and `actions` must be non-empty and contain no
duplicates. `qualities` must not contain duplicates. Routes are unique by
`(source ID, action)` across Providers in one package. Runtime initialization
also rejects empty catalogs, whitespace in source IDs, key/ID mismatches, and
duplicate actions or qualities.

## Registered entrypoints

| Entrypoint | Plugin ID | Provider ID | Runtime API | Required host bridge |
| --- | --- | --- | --- | --- |
| `builtin:netease` | `fika.netease` | `fika-netease` | `1.4` | `netease-api-enhanced` |
| `builtin:kugou` | `fika.kugou` | `fika-kugou` | `1.4` | `kugou-music-api` |

Both production entrypoints are reserved for their listed Plugin and Provider
IDs. `builtin:runtime-demo` exists only in Rust tests. Legacy
`builtin:lx-js:*` entries are recognized only for migration to the separate
Audio Source Registry and are rejected for Plugin installation.

## Capabilities

| Value | Host operation it gates |
| --- | --- |
| `network:any` | General host-mediated network requests. |
| `account:ref` | Resolution of an opaque, Provider-scoped Account Ref. |
| `playlist:read` | Account Playlist listing and detail access. |
| `playlist:write` | Playlist mutation; mutation paths also require read access. |
| `metadata:read` | Reserved for host-mediated metadata reads; no current production Provider uses it. |
| `cache:read-write` | Provider-scoped runtime cache reads and writes. |
| `bridge:netease-api-enhanced` | Calls through the NetEase host bridge. |
| `bridge:kugou-music-api` | Calls through the KuGou host bridge. |

A bridge has two independent declarations:

- `requiredHostBridges` makes host availability part of package compatibility.
- The matching `bridge:*` capability authorizes Provider calls at runtime.

Registered Provider contracts require both declarations where applicable.
Enabling a Plugin grants its complete current manifest declaration. Changing
the normalized manifest changes its SHA-256 fingerprint, disables the Plugin,
and clears the previous grants until it is enabled again.

## Runtime versions

Compatibility requires the same major version and a package minor version less
than or equal to the host minor version. The current host is `1.4`.

| Version | Contract introduced |
| --- | --- |
| `1.0` | `musicSearch`, `musicUrl`, `lyric`, `pic` |
| `1.1` | `musicRecommendations`, `playlistList`, `playlistRead`, `playlistAddTrack`, `playlistRemoveTrack` |
| `1.2` | `artistSearch`, `albumSearch`, `playlistSearch`, `searchSuggestions`, `artistTopTracks`, `albumRead`, `playlistReadPublic` |
| `1.3` | Recommendation kinds: `daily`, `roaming`, and `radar`; omitted `kind` defaults to `daily`. |
| `1.4` | `artistAlbums`, `artistBiography` |

The serialized request and response DTOs are generated from Rust into
`src/generated/bindings.ts`. Do not maintain a second handwritten type list.

## Validation and lifecycle

Validation occurs in this order:

1. JSON decoding rejects missing required fields, unknown fields, unknown enum
   values, and wrong JSON types.
2. `PluginManifest::validate` checks identifiers, SemVer, Provider/source
   structure, bridge IDs, and declared route collisions.
3. `PluginProviderCatalog::validate_manifest` checks that every entrypoint is
   registered and matches its fixed Plugin ID, Provider ID, Runtime API,
   required capabilities, and required host bridges.
4. Compatibility checks compare the target, Runtime API, and available host
   bridges.
5. Activation builds the Provider, verifies its ID, Runtime API, and
   capabilities, then lets the Source Runtime validate the returned catalog
   and every request/response.

Malformed or contract-invalid discovered packages are visible as `invalid`.
Target, Runtime API, or host availability failures are `incompatible`.
Provider factory or initialization failures are `error`. Valid packages start
`disabled` and become `enabled` only after all Providers initialize.

Bundled packages cannot be removed. User packages are copied into the platform
app-data `plugins` directory and can be removed. Installation rejects symbolic
links and overlapping source, destination, or staging paths. Refresh,
replacement, removal, capability changes, and activation use database and
runtime compensation so a failed operation restores the previous package and
Provider state where possible.

Imported LX JavaScript is not a Plugin. Its format and lifecycle are documented
in [Audio Sources](./AUDIO_SOURCES.md).
