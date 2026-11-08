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
