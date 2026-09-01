# Isolated V8 fallback for opaque imported Audio Sources

Status: accepted

## Context

ADR 0014 executes imported Audio Sources in embedded QuickJS after static Oxc
analysis identifies the LX `musicUrl` contract and source catalog. Some current
sources package the contract in a private bytecode virtual machine. The file is
valid JavaScript and initializes in LX Music's V8 environment, but its action
and catalog strings are unavailable to static analysis. Some virtual machines
also depend on V8 function-binding semantics and cannot initialize in QuickJS.

Allowing such a file through the existing gate would only move the failure to
enablement. Executing it in the application WebView would expose a real DOM and
broaden the imported-script security surface.

## Decision

Fika adds an isolated V8 fallback for opaque imported Audio Sources.

- Statically recognized LX scripts continue to use the embedded QuickJS
  adapter. The V8 adapter is selected only when Oxc parses the file, the normal
  LX contract gate fails, and the obfuscation report identifies an opaque or
  minified source.
- Import does not execute the opaque script or contact its network endpoints.
  The initial manifest declares the five remote LX source IDs, `musicUrl`, and
  standard qualities. Local tracks do not use an Audio Source. The real runtime
  catalog must match this manifest when the user enables the source after
  reviewing `network:any`; any `local` entry published by a script is ignored.
- V8 runs in a fresh Deno subprocess for each initialization and playback
  request. Fika pins Deno `2.9.5`, downloads the official platform ZIP on first
  enablement, checks the archive size and SHA-256 digest, extracts only the
  expected executable, and verifies the executable SHA-256 digest before use.
  `FIKA_LX_V8_PATH` is an explicit development and managed-deployment override.
- Deno runs without any `--allow-*` permissions and with config, lockfile,
  remote imports, and npm resolution disabled. The process receives an empty
  environment apart from fixed non-secret runtime flags. V8 old-space is
  limited to 96 MiB and stack size to 512 KiB.
- The trusted runner captures its Deno and crypto references, removes ambient
  `Deno`, `process`, filesystem, direct network, worker, and storage entry
  points, then exposes the documented LX host object. The runner itself is
  integrity-checked against the application-embedded source.
- Parent and sidecar communicate through bounded JSON Lines messages carrying
  a per-execution random nonce. Script logs, initialization data, results, and
  HTTP requests cross this protocol. Unknown output and messages with another
  nonce are ignored.
- The sidecar has no direct network permission. Every LX HTTP request returns
  to `SourceRuntimeContext`, which remains responsible for capability checks,
  cancellation, timeout and response limits, diagnostics, and request count.
- The parent kills the process on cancellation, timeout, protocol overflow,
  invalid catalog, or malformed output. The sidecar receives no application
  database handles, account secrets, plugin state, or local file paths.

This amends ADR 0014 only for imported Audio Sources whose contract is opaque
to static analysis. QuickJS remains the default imported-source runtime.

## Consequences

- V8-only LX sources can be imported without exposing them to the application
  WebView or adding Node.js APIs to the main process.
- First enablement downloads a platform asset of roughly 38-43 MiB. Unsupported
  operating-system or CPU targets fail closed.
- Opaque packages are visible in the registry before their real catalog can be
  verified. Enablement is the authoritative compatibility check and fails if
  the runtime catalog differs from the conservative manifest.
- A subprocess and Deno's permission model isolate normal script behavior from
  the application. They do not constitute a formally verified OS sandbox; a
  native V8 or Deno exploit could still run with the user's OS identity, but it
  would not share the application process or its in-memory state.
- Live contract tests remain ignored by default because they require the
  pinned Deno executable, a representative opaque source, and its third-party
  endpoint.
