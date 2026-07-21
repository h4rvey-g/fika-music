# Bundled NetEase Plugin

Status: accepted

Fika Music will deliver the first NetEase Cloud Music integration as a bundled Plugin package containing an LX Music-style compatible Rust Source Provider, rather than as a permanently built-in Connector or a fully external user-installed Plugin. This proves the Source Runtime and Plugin System together while allowing the project to pin, test, review, and permission the NetEase implementation like first-party code.

**Considered Options**

- Bundled NetEase Plugin containing a compatible Rust Source Provider.
- Built-in Rust Connector.
- External user-installed Plugin.

**Consequences**

- The v0.1 Source Runtime must support enough LX-style compatibility for the NetEase Source Provider.
- The v0.1 Plugin System must support enough bundled package lifecycle, capability review, diagnostics, and compatibility checks for the NetEase Plugin.
- Credentials and playlist-write operations still need host-mediated capabilities rather than direct provider access.
- The bundled Plugin has tests and targets
  `NeteaseCloudMusicApiEnhanced/api-enhanced` v4.32.1 at commit
  `a366983e992fe83e03bc89057144fca6b230be3b`. Updating that target requires
  contract review and explicit test changes rather than an unbounded upstream
  update.
