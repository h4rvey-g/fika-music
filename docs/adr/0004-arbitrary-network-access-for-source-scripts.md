# Arbitrary network access for source scripts

Status: accepted

Fika Music will allow Source Scripts to make arbitrary network requests through the host-mediated network API rather than limiting them to declared hosts. This favors LX Music-style compatibility and reduces early source-script breakage, but it weakens network permission auditability and increases account-safety and data-exfiltration risk.

**Considered Options**

- Declared hosts only, with explicit wildcards.
- Arbitrary network requests.
- Bundled scripts arbitrary, user scripts declared hosts only.
- Declared hosts for writes, arbitrary for reads.

**Consequences**

- Network access must still be timeout-bound, logged, cancellable, and visible in diagnostics.
- The app cannot honestly present host-level network allowlists as a security guarantee for Source Scripts.
- Future versions may add optional host restrictions, but v0.1 compatibility should not depend on them.
