# Security Policy

## Scope

This crate will process potentially sensitive HTTP data.

Security issues include:

- sensitive-data redaction bypasses;
- credential disclosure;
- unbounded memory allocation;
- resource exhaustion;
- telemetry export affecting application availability.

## Reporting

Do not publish real secrets, personal data or sensitive reproduction payloads in public issues.

Use the private security-reporting channel of the repository when available.

Reports should include:

- the affected version or commit;
- enabled Cargo features;
- the expected impact;
- a reproduction using fake data only;
- relevant configuration without credentials.

## Regression tests

Security fixes involving redaction must include tests that search for the fake secret in every exported or diagnostic output.

## Security guarantees

`safe-http-telemetry` applies the following guarantees on every normal path:

- **Centralized redaction**: no captured data reaches the export queue without passing through the redaction engine (`Redactor`).
- **Bounded memory**: queues, buffers, body captures, retries and shutdown deadlines have explicit limits. No unbounded allocation is performed on the request path.
- **No synchronous export**: the request path never awaits a network call to Loki. Export happens on dedicated workers.
- **TLS for remote export**: `LokiConfig` rejects plain HTTP URLs unless explicitly allowed. Certificate validation is never bypassed automatically.
- **No credential leakage**: `LokiConfig` rejects URLs with embedded credentials. Its `Debug` implementation does not expose bearer tokens.
- **Recursive logging prevention**: exporter errors are never fed back into the same Loki queue.

## Shared responsibility

The crate applies the configured policies and provides safe defaults. The operator remains responsible for:

- Classifying sensitive routes and completing the `RedactionConfig` with domain-specific fields.
- Configuring access control and retention on the Loki side.
- Verifying compliance with applicable data-protection regulations.
- Testing redaction rules against representative payloads before deployment.
- Keeping the geolocation database up to date and verifying its checksum.

## Reporting a vulnerability

Include in your report:

- Affected version or commit.
- Enabled Cargo features.
- Expected impact.
- Reproduction using **fake data only**.
- Relevant configuration without credentials.

Do not publish real secrets, personal data, or sensitive reproduction payloads in public issues.
