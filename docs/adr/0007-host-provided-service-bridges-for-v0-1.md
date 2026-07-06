# Host-provided service bridges for v0.1

Status: accepted

Fika Music v0.1 will allow Source Scripts to use only host-provided Service Bridges, starting with a built-in `netease-api-enhanced` bridge. Source Scripts cannot bundle, install, or launch arbitrary Node/native sidecars in v0.1.

**Considered Options**

- Host-provided Service Bridges only for v0.1.
- Plugins may declare and bundle sidecars.
- Self-contained Source Scripts only, no service bridges.
- Rust-native bridges only, no Node bridge.

**Consequences**

- The plugin model remains script-level rather than becoming an arbitrary process-launching platform.
- `NeteaseCloudMusicApiEnhanced/api-enhanced` is a host-managed bridge behind the Rust Plugin Host, not the plugin itself.
- User-installed Source Scripts can call approved host bridge APIs, but service bridge installation and lifecycle remain controlled by the app.
- Future versions may add third-party bridge support only after a stronger signing, review, packaging, and resource-control model exists.
