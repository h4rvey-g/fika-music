# LX Music-style source model compatibility

Status: accepted

Fika Music will target LX Music-style source model compatibility as a core Source Runtime capability. Fika will reuse the LX concepts that matter to the app—source keys, actions, qualities, request payloads, and response shapes—but will rewrite source behavior as Rust-native Source Providers instead of executing original LX JavaScript Source Scripts.

This keeps compatibility focused on user-visible behavior while reducing the risk of arbitrary script execution. Source Providers must still run behind explicit capabilities and host-mediated access boundaries.

**Considered Options**

- Fika-specific source API first.
- Execute original LX Music-style JavaScript Source Scripts.
- Rust-native LX Music-style source model compatibility.
- Built-in connectors only for v0.1.

**Consequences**

- The Source Runtime must prioritize typed LX-compatible request/response models and capability enforcement early.
- Existing LX JavaScript can be used as behavioral reference material, but must be ported to Rust before shipping.
- NetEase integration can be shaped as a Source Provider compatibility test, but credentials and playlist writes still require explicit host capabilities.
