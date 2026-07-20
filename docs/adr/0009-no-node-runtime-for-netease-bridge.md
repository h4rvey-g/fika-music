# No Node runtime for the NetEase bridge

Status: accepted

Fika Music v0.1 will not ship or launch a Node.js runtime for the NetEase integration. Fika will also avoid a QuickJS-compatible JavaScript bridge bundle. The selected NetEase endpoint behavior will be rewritten in Rust, with `NeteaseCloudMusicApiEnhanced/api-enhanced` used as the behavioral/API reference.

**Considered Options**

- No Node runtime, bundled QuickJS-compatible bridge.
- Lazy local Node bridge.
- Bundled bridge plus dev-only Node comparison harness.
- Rust-only reimplementation with no JavaScript bridge bundle.

**Consequences**

- The app preserves the low-resource Rust/Tauri architecture and avoids a long-lived Node sidecar.
- The Rust bridge implementation must remove Express server code, dynamic `require` scanning, arbitrary filesystem access, and Node-specific process assumptions from the upstream reference behavior.
- The bridge should pin upstream `api-enhanced` behavior and include tests comparing selected endpoint behavior where feasible.
- Host HTTP, credential, crypto/zlib, logging, and audit support are part of the trusted app core, not supplied by user-installed Source Providers.
