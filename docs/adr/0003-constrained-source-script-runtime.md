# Constrained source provider runtime

Status: accepted

Fika Music will run LX Music-style behavior through Rust-native Source Providers and require network, filesystem, credentials, cache, and playlist mutation to go through host-mediated APIs. This preserves the LX source model while keeping sensitive app and account access permissioned, logged, cancellable, and revocable.

**Considered Options**

- Rust-native Source Providers with host-mediated APIs.
- Constrained JavaScript with host-mediated APIs.
- Arbitrary JavaScript with broad runtime APIs.
- Bundled providers constrained while user providers are disabled for v0.1.
- WASM-only Source Providers.

**Consequences**

- The runtime must provide typed LX-style source contracts instead of exposing unrestricted platform APIs.
- Existing LX Music-style scripts require Rust ports before shipping.
- Capability enforcement, cancellation, provider diagnostics, and host API contracts are part of the core Source Runtime MVP rather than later hardening.
