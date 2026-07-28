# Registered Plugin Provider contracts

Status: accepted

## Context

The initial Plugin System loaded Providers through a platform-specific `match`
inside `plugin_system.rs`. `PluginRegistry` stored one optional bridge field and
one builder method per integration. Adding a Provider required coordinated
changes across the registry struct, startup bridge list, loader branches, and
manifest, but no interface checked that those declarations agreed.

The package format also accepted structurally valid unknown entrypoints until
activation. That made installation appear more extensible than the production
runtime actually was and deferred deterministic configuration errors to a
later lifecycle transition.

## Decision

Fika Music uses `PluginProviderCatalog` as the sole seam between package
lifecycle and host-compiled Provider implementations.

Each production registration contains:

- One fixed Plugin ID, Provider ID, and symbolic entrypoint.
- The Provider's Source Runtime API version.
- Capabilities and host bridges the manifest must declare.
- A host-owned factory that receives normalized package context and captured
  bridge dependencies.

Manifest validation and Provider construction both use this catalog. Unknown
entrypoints and mismatched reserved IDs are invalid before activation. The
factory result must report the manifest Provider ID, the registered Runtime API
version, and exactly its effective capability set.

`bundled_plugins.rs` is the production composition root. `PluginRegistry`
depends only on the catalog and no longer contains integration-specific bridge
fields or loader branches.

JSON decoding rejects unknown manifest fields. A repository command validates
all bundled manifests against a contract-only catalog in CI:

```sh
npm run plugins:check
```

## Consequences

- Adding a host-compiled Provider requires one explicit registration plus its
  implementation, manifest, and tests.
- Rust registration metadata and inspectable package permissions cannot drift
  without failing validation.
- A package cannot claim a production entrypoint reserved for another Plugin.
- A package alone still cannot introduce executable code; dynamic extension
  formats remain a separate security and compatibility decision.
- Test code can inject an unscoped Provider contract without weakening the
  fixed identity rules used by production registrations.
