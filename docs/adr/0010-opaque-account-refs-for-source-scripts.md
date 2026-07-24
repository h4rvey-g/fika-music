# Opaque account refs for source providers

Status: accepted

Fika Music will not expose raw NetEase cookies, tokens, passwords, or refresh credentials to Source Providers. Source Providers receive opaque account/session references, while the trusted host-managed NetEase Service Bridge attaches stored credentials internally for approved operations.

**Considered Options**

- Opaque account refs; bridge attaches secrets internally.
- Raw cookies available to the bundled NetEase provider only.
- Raw cookies available to any provider with credential capability.
- No persistent credentials; user supplies a session each run.

**Consequences**

- Credential storage belongs to the app core and uses the application-private
  SQLite database so account-backed Plugin calls never trigger an operating
  system credential prompt. On Unix platforms, the app-data directory and
  database file are restricted to the current user. This deliberately trades
  Keychain-level at-rest protection for prompt-free local persistence.
- Existing operating-system credential entries are not read or migrated because
  doing so can itself trigger an authorization prompt. Accounts must reconnect
  once after this storage change.
- Account reference lookup keys are scoped to the requesting Source Provider; the same provider-supplied key may resolve independently for different Providers.
- Playlist writes and other account mutations require explicit capabilities and mutation audit records.
- Source Providers cannot exfiltrate raw credentials even though arbitrary network requests are allowed.
- The NetEase bridge contract must support authenticated calls by account reference, not by raw cookie parameter from provider code.
