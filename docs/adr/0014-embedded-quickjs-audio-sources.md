# Embedded QuickJS for imported Audio Sources

Status: accepted

## Context

ADR 0008 required every imported LX JavaScript source to map to reviewed Rust
behavior or statically extracted URL templates. That prevented arbitrary script
execution, but it also discarded the source's request logic. Dynamic sources
such as Xinghai could not be imported, and the Nianxin and Changqing adapters
substituted endpoint templates from a separate aggregate fixture. When those
copied endpoints changed, an imported source continued using URLs that were not
present in its own file.

Audio Sources already have a separate lifecycle under ADR 0013. They are
playback-only, integrity-checked packages with explicit `network:any` review,
which provides a narrower place to support LX JavaScript than the Plugin
System.

## Decision

Fika will execute imported Audio Source JavaScript in an embedded QuickJS
runtime behind a Rust `SourceProvider` adapter.

- Static Oxc analysis remains an import gate for the LX `musicUrl` contract and
  source catalog.
- Each initialization and playback request receives a fresh QuickJS runtime and
  context.
- Scripts receive the documented LX event interface, host-mediated HTTP,
  cancellable microtask timers, buffer helpers, MD5, random bytes,
  AES-128-CBC/ECB, and RSA encryption.
- Scripts do not receive Node.js, Tauri, DOM, direct filesystem, app database,
  account-secret, or raw credential interfaces.
- HTTP still crosses `SourceRuntimeContext`, so capability checks,
  cancellation, timeout/response limits, and diagnostics remain authoritative.
- QuickJS memory, stack, execution time, pending jobs, request count, request
  body, header, and random-byte allocations are bounded.
- Managed source fingerprints and permission review remain unchanged.
- Existing `nianxin`, `changqing`, and `static-templates` manifests are
  migrated to `quickjs`. The manifest fingerprint change revokes prior
  enablement and requires a new permission review before their own
  integrity-checked `source.js` can execute. Fika no longer substitutes
  templates from another fixture.
- Bundled and Plugin Source Providers remain Rust-native. This decision applies
  only to user-imported, playback-only Audio Sources.

This amends ADR 0008 for imported Audio Sources. It does not change ADR 0009's
decision that the NetEase bridge has no Node or QuickJS implementation.

## Consequences

- Dynamic third-party LX playback sources can run without a per-source Rust
  port.
- Source behavior can change when the user imports a new script version, so the
  existing fingerprint-based permission review remains a required trust step.
- Embedded QuickJS provides in-process isolation and resource limits, not an
  operating-system process sandbox. A future process boundary can replace the
  adapter without changing the Audio Source Registry interface.
- Runtime compatibility tests must cover the host contract, denied globals,
  resource limits, script-defined endpoint behavior, and representative live
  sources.
