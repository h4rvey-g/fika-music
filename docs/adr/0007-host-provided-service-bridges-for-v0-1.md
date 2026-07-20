# Host-provided service bridges for v0.1

Status: accepted

Fika Music v0.1 will allow Source Providers to use only host-provided Service Bridges, starting with built-in NetEase behavior based on `netease-api-enhanced`. Source Providers cannot bundle, install, or launch arbitrary Node/native sidecars in v0.1.

**Considered Options**

- Host-provided Service Bridges only for v0.1.
- Plugins may declare and bundle sidecars.
- Self-contained Source Providers only, no service bridges.
- Rust-native bridges only, no Node bridge.

**Consequences**

- The Source Runtime model remains provider-level rather than becoming an arbitrary process-launching platform.
- `NeteaseCloudMusicApiEnhanced/api-enhanced` is a behavioral reference for host-managed NetEase code behind the Source Runtime host, not the Source Provider itself.
- User-installed Source Providers can call approved host bridge APIs, but service bridge installation and lifecycle remain controlled by the app.
- Future versions may add third-party bridge support only after a stronger signing, review, packaging, and resource-control model exists.
