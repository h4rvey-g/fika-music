# Constrained source provider runtime

Status: accepted; imported Audio Source execution refined by ADR 0014

Fika Music will run LX Music-style behavior through Rust-native Source Providers and require network, filesystem, credentials, cache, and playlist mutation to go through host-mediated APIs. This preserves the LX source model while keeping sensitive app and account access permissioned, logged, cancellable, and revocable.

ADR 0014 applies the same host-mediated constraints to embedded QuickJS for the
narrower, playback-only Audio Source lifecycle.

**Considered Options**

- Rust-native Source Providers with host-mediated APIs.
- Constrained JavaScript with host-mediated APIs.
- Arbitrary JavaScript with broad runtime APIs.
- Bundled providers constrained while user providers are disabled for v0.1.
- WASM-only Source Providers.

**Consequences**

- The runtime must provide typed LX-style source contracts instead of exposing unrestricted platform APIs.
- Existing LX Music-style scripts require Rust ports before shipping as bundled
  or Plugin Source Providers. ADR 0014 permits constrained execution only for
  user-imported, playback-only Audio Sources.
- Capability enforcement, cancellation, provider diagnostics, and host API contracts are part of the core Source Runtime MVP rather than later hardening.
