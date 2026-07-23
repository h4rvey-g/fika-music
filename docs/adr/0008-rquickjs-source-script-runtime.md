# Rust-native source provider runtime

Status: accepted; imported Audio Source execution amended by ADR 0014

Fika Music will not execute original LX Music JavaScript Source Scripts in v0.1. Instead, Fika will rewrite LX-compatible source behavior as Rust-native Source Providers that implement LX Music-style actions and data contracts.

ADR 0014 later permits constrained embedded QuickJS execution for
user-imported, playback-only Audio Sources. Bundled and Plugin Source Providers
remain Rust-native under this decision.

This supersedes the earlier `rquickjs` runtime direction. `rquickjs` is not part of the Slice 2 implementation.

**Consequences**

- Bundled and Plugin Source Provider behavior is reviewed, typed, tested Rust code rather than arbitrary JavaScript loaded into the app.
- LX Compatibility means matching the useful LX source model: source keys, actions, qualities, request payloads, response shapes, diagnostics, and host-mediated capabilities.
- Capability enforcement, diagnostics, account refs, network access, cache access, and playlist mutation remain mandatory runtime boundaries.
- Existing LX JavaScript must be ported into Rust before shipping as a bundled
  or Plugin Source Provider. ADR 0014 defines the narrower execution policy for
  user-imported Audio Sources.
- The NetEase implementation must be a Rust Source Provider and/or Rust Service Bridge, not a QuickJS-compatible script bundle.
