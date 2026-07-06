# Bundled NetEase source script

Status: accepted

Fika Music will deliver the first NetEase Cloud Music integration as a bundled LX Music-style compatible Source Script rather than as a built-in Rust connector or a fully external user-installed script. This proves the source-script compatibility layer early while allowing the project to pin, test, review, and permission the NetEase implementation like first-party code.

**Considered Options**

- Bundled compatible Source Script.
- Built-in Rust Connector.
- External user-installed Source Script.

**Consequences**

- The v0.1 plugin runtime must support enough LX-style compatibility for the bundled NetEase script.
- Credentials and playlist-write operations still need host-mediated capabilities rather than direct script access.
- The bundled script should have tests, a pinned compatibility target, and a conservative update policy.
