# Rust-native source provider runtime

Status: accepted

Fika Music will not execute original LX Music JavaScript Source Scripts in v0.1. Instead, Fika will rewrite LX-compatible source behavior as Rust-native Source Providers that implement LX Music-style actions and data contracts.

This supersedes the earlier `rquickjs` runtime direction. `rquickjs` is not part of the Slice 2 implementation.

**Consequences**

- Source behavior is reviewed, typed, tested Rust code rather than arbitrary JavaScript loaded into the app.
- LX Compatibility means matching the useful LX source model: source keys, actions, qualities, request payloads, response shapes, diagnostics, and host-mediated capabilities.
- Capability enforcement, diagnostics, account refs, network access, cache access, and playlist mutation remain mandatory runtime boundaries.
- Existing LX JavaScript can be used as behavioral reference material, but must be ported into Rust before shipping.
- The NetEase implementation must be a Rust Source Provider and/or Rust Service Bridge, not a QuickJS-compatible script bundle.
