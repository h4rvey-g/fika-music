# Constrained source script runtime

Status: accepted

Fika Music will run LX Music-style Source Scripts as constrained JavaScript and require network, filesystem, credentials, cache, and playlist mutation to go through host-mediated APIs. This preserves direct source-script compatibility where possible while keeping sensitive app and account access permissioned, logged, timeout-bound, and revocable.

**Considered Options**

- Constrained JavaScript with host-mediated APIs.
- Arbitrary JavaScript with broad runtime APIs.
- Bundled scripts constrained while user scripts are disabled for v0.1.
- WASM-only plugins.

**Consequences**

- The runtime must provide LX-style compatibility shims instead of exposing unrestricted platform APIs.
- Some existing LX Music-style scripts may require adaptation if they depend on unsupported globals or direct environment access.
- Capability enforcement, timeouts, script logs, and host API contracts are part of the plugin MVP rather than later hardening.
