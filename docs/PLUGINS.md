# Writing Plugins

Fika Music Plugins package one or more host-compiled Rust Source Providers.
The Plugin System owns package discovery, validation, permissions, lifecycle,
diagnostics, and dispatch; the Source Runtime owns request execution and host
capability enforcement.

> The supported extension workflow currently requires a Fika Music source
> change and a new application build. A package alone cannot introduce
> executable code. Dynamic libraries, package JavaScript, WASM, and sidecars
> are not supported Provider formats.

## Verify the current Plugins

From the repository root:

```sh
npm run plugins:check
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin_system::tests
```

The first command validates every bundled `plugin.json` against the same
registered contracts used by the application. The second exercises discovery,
activation, capability isolation, dispatch, persistence, and rollback.

## Understand the modules

| Module | Responsibility |
| --- | --- |
| `src-tauri/src/plugin_system/provider_catalog.rs` | Stable contract and factory registration interface. |
| `src-tauri/src/bundled_plugins.rs` | Single production composition root for all Provider contracts and factories. |
| `src-tauri/src/plugin_system.rs` | Package discovery, install/remove, lifecycle state, persistence, diagnostics, and dispatch routing. |
| `src-tauri/src/source_runtime.rs` | `SourceProvider`, typed requests/responses, catalog validation, capability enforcement, cancellation, and panic isolation. |
| `src-tauri/src/netease.rs`, `kugou.rs` | Existing production Provider and host bridge implementations. |
| `src-tauri/plugins/<name>/plugin.json` | Bundled package metadata and permission declaration. |
| `src/generated/bindings.ts` | Generated frontend DTOs and Tauri command names. |

At startup, `bundled_plugins::provider_catalog` registers every supported
symbolic entrypoint. `PluginRegistry` validates packages against that catalog,
then asks it to construct a Provider during activation. The registry contains
no platform-specific factory fields or `match` statement.

## Add a bundled Rust Plugin

Use the KuGou implementation as the smallest current production example.

1. Add a Provider module under `src-tauri/src/` and export it from `lib.rs`.
2. Define stable constants for the Plugin ID, Provider ID, symbolic entrypoint,
   oldest required Runtime API, and host bridge ID.
3. Implement `SourceProvider` for the Provider.
4. If external I/O is needed, expose it through a narrow host bridge trait and
   require the corresponding `bridge:*` capability in `SourceRuntimeContext`.
5. Add one contract and factory registration in `bundled_plugins.rs`.
6. Add `src-tauri/plugins/<name>/plugin.json` with exactly matching IDs,
   Runtime API, capabilities, and host bridges.
7. Add Provider unit tests and a manifest-to-contract test.
8. Run the complete verification commands below.

The registration shape is intentionally data-driven:

```rust
let registration = PluginProviderRegistration::new(contract, move |context| {
    let provider: Arc<dyn SourceProvider> = Arc::new(MySourceProvider::new(
        context.provider_id,
        context.declared_capabilities,
        Arc::clone(&host_bridge),
    ));
    Ok(provider)
});
```

`PluginProviderBuildContext` also includes `plugin_id`, `package_path`, and the
optional manifest `source_catalog`. A factory should use host-owned dependencies
captured during application composition. It must not load arbitrary executable
content from `package_path`.

## Implement SourceProvider

| Method | Required behavior |
| --- | --- |
| `id` | Return the Provider ID from the manifest. It must be stable and globally unique. |
| `api_version` | Return the oldest Source Runtime contract required by the implementation. Keep it equal to the registered contract. |
| `required_capabilities` | Return exactly the effective manifest set supplied to the factory. The catalog rejects a mismatch before initialization. |
| `initialize` | Return the complete source/action/quality catalog. Source IDs are routing keys and must remain stable. |
| `handle_request` | Handle declared actions and return the matching `SourceResponse` variant. Use `context.unsupported_action` for other actions. |

The Runtime validates requests before calling the Provider and validates the
response variant and payload afterward. It also catches Provider panics,
checks cancellation, bounds diagnostics, and prevents a Provider from using a
host operation unless that capability is both declared and granted.

Do not perform network, account, credential, cache, or filesystem operations
directly in Provider code when a `SourceRuntimeContext` operation or host bridge
owns that behavior. The host seam is where timeouts, cancellation, opaque
Account Refs, response limits, and permission checks are enforced.

## Declare the package

Read [Plugin manifest reference](./PLUGIN_MANIFEST.md) before adding the
package. The contract validator checks the facts duplicated between Rust and
JSON:

- Plugin ID and Provider ID.
- Symbolic entrypoint.
- Source Runtime API version.
- Minimum capabilities.
- Required host bridges.

This duplication is deliberate: Rust describes what the host implementation
can do, while `plugin.json` is the package's inspectable permission statement.
CI fails when they drift.

Package-level capabilities apply to every Provider. Prefer entrypoint-level
capabilities when only one Provider needs a permission. Route ownership is the
pair `(source ID, action)`; two Providers in one Plugin cannot own the same
pair.

## Integrate application behavior

The generic Plugin manager, lifecycle controls, diagnostics, sidebar item, and
`PluginWorkspace` require no platform-specific frontend changes. A new enabled
Plugin automatically appears through `PluginRecord`.

Add dedicated Vue views or online-music orchestration only when the integration
has product-specific workflows such as QR login or account Playlist mutation.
Keep generic lifecycle behavior in the Plugin System rather than branching on
Plugin IDs in new call sites.

After changing a Rust IPC DTO or Tauri command, regenerate and verify the
TypeScript bindings:

```sh
npm run bindings:generate
npm run bindings:check
```

Never edit `src/generated/bindings.ts` by hand.

## Verify a new Plugin

Run all checks from the repository root:

```sh
npm run plugins:check
npm run bindings:check
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

At minimum, tests for a new Provider must cover:

- Its bundled manifest matches the registered contract.
- Initialization returns a valid catalog with unique actions and qualities.
- Every declared request returns the matching response type.
- Missing capabilities produce a security denial.
- Host bridge failures preserve stable error codes and diagnostics.
- Cancellation is observed around host operations.
- Account and mutation paths never expose stored credentials.

## Package-only additions

A package-only addition is usable only when its entrypoint contract is already
registered and not reserved for another Plugin or Provider ID. There are no
such production entrypoints today. The installer remains useful for lifecycle
and replacement testing, but it is not a third-party code-loading mechanism.

Adding externally executable Plugins would require a separate decision covering
signing, ABI or sandbox format, dependency loading, resource limits, update
trust, and host compatibility. Those concerns are intentionally outside the
current Provider catalog interface.
