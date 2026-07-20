# Core LX-compatible Source Runtime MVP structure

Status: accepted

Fika Music will implement LX Music-style Source Provider compatibility as a core Source Runtime capability, independent of any single online music service and independent of the later Plugin System. This ensures capability enforcement, diagnostics, and host APIs are built as first-class application infrastructure before adding installable Plugin lifecycle or production service bridges.

## Context

We previously planned to build the Source Runtime incrementally alongside the NetEase integration, and some planning language called this work a "plugin system". That framing is misleading: LX compatibility is central product functionality for Fika Music, not an optional plugin layer. The Plugin System will come after the Source Runtime and will package, install, enable, permission-review, and diagnose Source Providers. The runtime itself belongs to the app core.

Coupling the runtime to NetEase-specific behavior or Plugin package lifecycle would make the compatibility layer harder to test and harder to reason about. The runtime should be validated with in-repo mock Rust Source Providers before the Plugin System, bundled NetEase Plugin, and NetEase Service Bridge are added.

## Decision

We will design a standalone Rust/Tauri core subsystem to run LX-compatible Rust Source Providers with the following components:

### 1. Minimal Rust Source Provider Model for Runtime Tests

Slice 2 uses in-repo mock Rust Source Providers rather than JavaScript fixtures. Full Plugin manifests, user-installed package scanning, install/remove/enable/disable state, and capability review UI belong to Slice 3, the Plugin System MVP.

### 2. Rust-native Source Provider Dispatcher

Each Source Provider implements a Rust trait. The Source Runtime host:

- Initializes providers and collects their LX-compatible source catalog.
- Dispatches source requests using typed Rust request/response models.
- Requires a compatible Source Runtime API version before initialization.
- Registers initialized catalogs and rejects requests for unknown sources or actions.
- Catches provider errors and panics at runtime boundaries and records diagnostics.
- Supports bounded, cooperative cancellation for provider initialization and requests.

### 3. LX-style Source Model

The runtime models LX concepts directly in Rust:

- Source keys such as `kw`, `kg`, `tx`, `wy`, `mg`, and `local`.
- Source type `music`.
- Actions such as `musicUrl`, `lyric`, and `pic`.
- Qualities such as `128k`, `320k`, `flac`, and `flac24bit`.
- Request payloads and response shapes compatible with the LX behavior Fika chooses to support.

Existing LX JavaScript may be used as reference material, but Fika does not execute it in Slice 2.

### 4. Capabilities Enforcement

Source Providers declare required capabilities, but declarations do not grant access. The host supplies separate, optionally Provider-scoped grants and the Runtime gives each Provider only the intersection of its declaration and its grant. Grants can be replaced or revoked for subsequent requests.

Network and cache operations are exposed through host-owned bindings on `SourceRuntimeContext`. Opaque account refs are validated through the `account:ref` boundary and scoped to the requesting Provider; secrets are never passed to Providers. Network diagnostics expose only a sanitized target origin, while unprivileged calls are blocked and logged as security exceptions. There is no direct filesystem capability for Providers.

The v0.1 Rust-native Provider model is for reviewed, in-process code. The capability API is a runtime boundary for Provider behavior, not an operating-system sandbox for arbitrary native code. User-installed native binaries must not be loaded as trusted Providers without a separate isolation design.

### 5. Verification Framework

The test suite will run mock Rust Source Providers:

- A provider that publishes the LX-compatible source catalog.
- A provider request that requires `network:any` to test permission enforcement.
- A provider request that returns mock lyric/musicUrl/pic responses to verify Source Runtime dispatch.
- Provider API compatibility rejection and initialized-catalog validation.
- Provider error and panic isolation, structured diagnostics, and cancellation behavior.
- Host-mediated network, cache, and opaque account-ref access.

## Consequences

- LX compatibility is treated as core app infrastructure rather than a plugin feature.
- The Plugin System becomes the package/lifecycle layer that is built on top of the Source Runtime in Slice 3.
- The NetEase integration becomes a bundled Plugin built on top of both the Source Runtime and Plugin System in Slice 4.
- Integration tests can run entirely in Rust with local mock Source Providers, reducing dependence on live remote API endpoints during runtime development.
