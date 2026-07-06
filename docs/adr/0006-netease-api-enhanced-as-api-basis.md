# NetEase API Enhanced as API basis

Status: accepted

Fika Music will use `NeteaseCloudMusicApiEnhanced/api-enhanced` as the API basis for NetEase Cloud Music integration. The project is MIT-licensed, Node.js-based, and covers the needed NetEase areas including login, recommendations, user playlists, search, song metadata, and playlist-related operations.

**Consequences**

- The NetEase integration should track a pinned upstream version and document endpoint behavior used by Fika.
- Bridge mode was later resolved by ADR 0009: no Node runtime; use a build-time bundled QuickJS-compatible endpoint subset with Rust-owned host polyfills.
- Credential handling was later resolved by ADR 0010: Source Scripts receive opaque account refs, and the trusted Service Bridge attaches secrets internally.
