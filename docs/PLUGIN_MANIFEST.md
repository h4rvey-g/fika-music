# Plugin Manifest

Slice 3 Plugin packages are directories containing a `plugin.json` file. The
manifest is metadata and permission policy; it is not permission to execute
arbitrary native code.

## Schema

```json
{
  "manifestVersion": 1,
  "id": "example.source",
  "name": "Example Source",
  "version": "1.0.0",
  "description": "Optional description",
  "author": "Example",
  "homepage": "https://example.invalid/source",
  "providerEntrypoints": [
    {
      "id": "example-provider",
      "entrypoint": "builtin:runtime-demo",
      "capabilities": ["network:any"],
      "sourceCatalog": {}
    }
  ],
  "capabilities": ["network:any"],
  "compatibilityTarget": "fika-music",
  "supportedApiVersion": {
    "major": 1,
    "minor": 0
  },
  "requiredHostBridges": []
}
```

`version` uses semantic versioning. Provider IDs must be unique within the
package and across bundled and user-installed packages; a colliding package is
shown as invalid. Package IDs must also be unique across bundled and
user-installed packages. The supported Source Runtime API version must be
compatible with the host runtime. Unknown host bridges leave a package visible
but incompatible.

Provider capabilities can be declared at package level or entrypoint level;
the effective declaration for a Provider is the union of package-level and that
entrypoint's declarations. A user grant is persisted separately from the
declaration and each Source Provider receives only its own intersection.
Capabilities are never granted implicitly by installing a package, and an
entrypoint-only capability is not exposed to sibling Providers.

The MVP accepts symbolic built-in entrypoints (`builtin:runtime-demo`,
`builtin:catalog`, and the existing `builtin:qishui` provider). User packages
cannot load a dynamic library or launch a sidecar. This keeps package
discovery, permission review, and lifecycle management in place without
turning installation into an untrusted native-code execution boundary.

## Locations and lifecycle

- Bundled packages live under the app resource `plugins` directory.
- User packages are copied into the platform app-data `plugins` directory.
- A newly installed package with declared capabilities starts in `NeedsReview`.
- Enabling requires explicit permission review. Revoking a capability updates
  the provider grant immediately and is enforced on the next runtime request.
- Bundled packages can be disabled but cannot be removed.
- Removing a user package deactivates its Providers before the package and
  persisted state are deleted.

The registry persists enabled state, review state, capability grants, and the
latest bounded diagnostic history in SQLite. Permission state is bound to a
SHA-256 digest of the normalized manifest, so changing a package manifest
requires a new review; reinstalling identical manifest content preserves the
existing review state. The migration from the earlier non-cryptographic
fingerprint intentionally requires one new review for existing local approvals.
Enabled Providers are initialized again during application startup and registry
refresh. Refresh, removal, lifecycle, permission, and diagnostic writes use
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

The request uses the serialized `SourceRequest` contract (`musicSearch`,
`musicUrl`, `lyric`, or `pic`) and returns a typed `SourceRequestOutcome` with
the response and runtime diagnostics. The request is rejected unless the
package is enabled and its Provider exposes the requested source. A request
ID can be cancelled with `cancel_source_request`; cancellation is cooperative
and bounded by the host operation timeout. Database and Plugin registry locks
are released while Provider code runs; completion diagnostics are attached only
if the same Provider instance is still active. A diagnostic persistence failure
is retained as an in-memory warning when possible and never replaces the
Provider response or runtime error returned to the caller.

Package replacement uses a non-overlapping staged copy and revalidates the
manifest before activation. A source package that contains, or is contained by,
the destination or staging workspace is rejected. The previous package and
SQLite lifecycle state are restored if replacement or registry refresh fails.
Removal first moves a user package to a same-filesystem quarantine; it deletes
that quarantine only after the SQLite transaction commits and restores it if
removal fails.
