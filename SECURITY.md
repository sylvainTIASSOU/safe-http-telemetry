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

- **Field-based redaction**: captured query parameters, headers, structured bodies and business-event fields pass through the redaction engine (`Redactor`) before queueing. HTTP raw paths are omitted from middleware events (`request.path` is `[OMITTED]`); normalized route templates remain. Custom `RouteResolver` implementations must return stable, non-sensitive route templates and never raw request paths. HTTP method, route, request/trace IDs and client IP are bounded but are not semantically redacted.
- **Bounded memory**: queues, buffers, body captures, retries and shutdown deadlines have explicit limits. No unbounded allocation is performed on the request path.
- **No synchronous export**: the request path never awaits a network call to Loki. Export happens on dedicated workers.
- **TLS for remote export**: `LokiConfig` accepts both HTTP and HTTPS URLs. Use HTTPS for remote Loki endpoints; the HTTP client uses Rustls with certificate validation enabled and does not disable verification automatically. Plain HTTP is intended for local development or a separately protected private network.
- **No credential leakage through configuration formatting**: client startup rejects Loki URLs with embedded credentials or query strings. `LokiConfig` debug formatting strips URL user information, query strings and fragments and redacts bearer tokens, including if its public URL field was mutated after construction.
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
