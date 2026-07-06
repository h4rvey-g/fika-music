# Direct LX Music-style source compatibility

Status: accepted

Fika Music will target direct LX Music-style JavaScript source compatibility for the first plugin platform because reuse of existing source behavior is more important to the product direction than starting with a safer Fika-specific plugin API. This increases sandboxing, API drift, maintenance, and account-safety risk, so source scripts must still run behind explicit capabilities and host-mediated access boundaries.

**Considered Options**

- Fika-specific sandboxed JavaScript API first.
- Direct LX Music-style source compatibility.
- Built-in connectors only for v0.1.

**Consequences**

- The plugin runtime must prioritize compatibility shims and sandbox enforcement early.
- The roadmap should validate source-script behavior before assuming a stable long-term plugin API.
- NetEase integration can be shaped as a source-script compatibility test, but credentials and playlist writes still require explicit host capabilities.
