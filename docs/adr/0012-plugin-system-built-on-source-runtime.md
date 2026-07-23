# Plugin System built on the Source Runtime

Status: accepted; package boundary amended by ADR 0013

Fika Music will implement the Plugin System as a package and lifecycle layer built on top of the core LX-compatible Source Runtime. The Plugin System does not implement LX Compatibility itself; it installs, validates, enables/disables, permission-reviews, updates, and diagnoses packages that contain Source Providers and related assets.

## Context

LX Compatibility is a core product capability. Separately, Fika needs a way to ship bundled content integrations and manage their lifecycle. These are related but distinct concerns:

- The **Source Runtime** runs LX-compatible Rust Source Providers safely.
- The **Plugin System** manages packaged units that contain Source Providers.
- The **Audio Source Registry** independently manages imported playback-only sources, as decided in ADR 0013.

Keeping these separate avoids treating LX Compatibility as optional plugin behavior and lets runtime safety be tested before package lifecycle and UI are added.

## Decision

Slice 3 will implement the Plugin System on top of the Source Runtime from Slice 2.

The Plugin System owns:

- Plugin package manifest schema.
- Bundled and user-installed Plugin discovery.
- Install/remove/enable/disable lifecycle state.
- Capability review UI and persisted approvals.
- Compatibility-target checks against the Source Runtime.
- Plugin diagnostics assembled from Source Runtime logs, security denials, load errors, and bridge errors.

The Plugin System does not own:

- The Rust Source Provider dispatcher.
- LX-style source action and response contracts.
- Host-mediated request and diagnostics bindings.
- NetEase-specific endpoint behavior.
- LX JavaScript Audio Source import or Audio Source lifecycle state.

## Consequences

- Source Runtime APIs must expose enough lifecycle hooks for the Plugin System to load and unload Source Providers safely.
- Plugin manifests describe capabilities and compatibility targets, but capability enforcement still happens inside host-mediated Source Runtime bindings.
- The bundled NetEase integration can be implemented later as the first production Plugin without changing the Source Runtime boundary.
- Imported Audio Sources can reuse the Source Runtime without becoming Plugins.
