# Opaque account refs for source scripts

Status: accepted

Fika Music will not expose raw NetEase cookies, tokens, passwords, or refresh credentials to Source Scripts. Source Scripts receive opaque account/session references, while the trusted host-managed NetEase Service Bridge attaches stored credentials internally for approved operations.

**Considered Options**

- Opaque account refs; bridge attaches secrets internally.
- Raw cookies available to the bundled NetEase script only.
- Raw cookies available to any script with credential capability.
- No persistent credentials; user supplies a session each run.

**Consequences**

- Credential storage belongs to the app core, using OS credential storage where feasible and encrypted app storage only as a fallback.
- Playlist writes and other account mutations require explicit capabilities and mutation audit records.
- Source Scripts cannot exfiltrate raw credentials even though arbitrary network requests are allowed.
- The NetEase bridge contract must support authenticated calls by account reference, not by raw cookie parameter from script code.
