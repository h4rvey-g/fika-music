# Arbitrary network access for source providers

Status: accepted

Fika Music will allow Source Providers to make arbitrary network requests through the host-mediated network API rather than limiting them to declared hosts. This favors LX Music-style compatibility and reduces early provider breakage, but it weakens network permission auditability and increases account-safety and data-exfiltration risk.

**Considered Options**

- Declared hosts only, with explicit wildcards.
- Arbitrary network requests.
- Bundled providers arbitrary, user providers declared hosts only.
- Declared hosts for writes, arbitrary for reads.

**Consequences**

- Network access must still be timeout-bound, logged, cancellable, and visible in diagnostics.
- The v0.1 host applies a bounded request timeout and checks a cooperative cancellation token before requests and while consuming response bodies; cancellation is therefore bounded by the network timeout rather than an unsafe thread abort.
- The app cannot honestly present host-level network allowlists as a security guarantee for Source Providers.
- Future versions may add optional host restrictions, but v0.1 compatibility should not depend on them.
