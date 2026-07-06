# No Node runtime for the NetEase bridge

Status: accepted

Fika Music v0.1 will not ship or launch a Node.js runtime for the NetEase integration. Instead, the built-in NetEase Service Bridge will use a build-time bundled, QuickJS-compatible subset of `NeteaseCloudMusicApiEnhanced/api-enhanced`, while Rust provides host-owned polyfills for HTTP, credentials/cookies, selected Buffer/crypto/zlib behavior, logging, and mutation audit.

**Considered Options**

- No Node runtime, bundled QuickJS-compatible bridge.
- Lazy local Node bridge.
- Bundled bridge plus dev-only Node comparison harness.
- Rust-only reimplementation with no JavaScript bridge bundle.

**Consequences**

- The app preserves the low-resource Rust/Tauri architecture and avoids a long-lived Node sidecar.
- The bridge build must remove Express server code, dynamic `require` scanning, arbitrary filesystem access, and Node-specific process assumptions.
- The bridge should pin upstream `api-enhanced` behavior and include tests comparing selected endpoint behavior where feasible.
- Host polyfills are part of the trusted app core, not supplied by user-installed Source Scripts.
