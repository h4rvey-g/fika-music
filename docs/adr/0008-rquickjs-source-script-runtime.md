# rquickjs source script runtime

Status: accepted

Fika Music will use `rquickjs` as the JavaScript engine for running LX Music-style Source Scripts. This keeps the plugin runtime inside the Rust application architecture while allowing Fika to expose an LX-compatible `globalThis.lx` host API and Fika-specific extension APIs through controlled Rust bindings.

**Consequences**

- Source Scripts run against a curated host API, not a full Node.js environment.
- Compatibility work must provide the LX-style event surface, request API, and utility shims required by supported scripts.
- Timeouts, cancellation, memory limits, logging, and panic/error isolation are mandatory parts of the runtime design.
- `NeteaseCloudMusicApiEnhanced/api-enhanced` cannot be assumed to run unmodified inside `rquickjs`; it is Node-oriented and must be handled as a separate Service Bridge implementation concern.
