# No direct local file access for source providers

Status: accepted

Fika Music Source Providers will not receive direct local filesystem access. The Rust backend owns local library scanning and file IO, while Source Providers use host-mediated cache APIs and app-provided metadata; explicit user-selected file handles can be considered later only for concrete use cases.

**Considered Options**

- No direct local file access.
- Read-only access to selected music folders.
- Manifest-declared file path scopes.
- Arbitrary filesystem access.

**Consequences**

- Source Providers cannot inspect the user's music folders, app database, credentials, documents, or home directory directly.
- Provider APIs must avoid exposing raw filesystem APIs.
- Any future file access should be user-initiated, scoped, revocable, and separate from v0.1 source-provider compatibility.
