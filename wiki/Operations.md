# Operations

- Set queue, capture, batch, retry, and shutdown limits for the workload. Monitor drops, queue occupancy, failed batches, retries, pending exports, and abandoned events.
- Keep `Minimal` as the default where possible. Use `Detailed` only on reviewed routes and enable `body-capture` explicitly.
- Extend redaction rules for application-specific fields. Do not put secrets or personal data in free-form tracing messages.
- Forwarded client-IP headers are ignored unless the immediate peer is configured as a trusted proxy. Configure only proxy CIDRs you control.
- Supply Loki credentials through deployment secrets, never in the URL.
- Install geolocation data separately, retain provider attribution and license, mount it read-only, and restart after updates.
- Stop HTTP serving, shut down the app's tracer provider if used, then drain the Loki exporter and inspect its report.

Use the repository's [production guide](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/docs/production.md), [security policy](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/SECURITY.md), and [geolocation guide](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/docs/geolocation.md) for full details.
