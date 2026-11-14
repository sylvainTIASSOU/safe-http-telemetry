# Architecture

The request path observes HTTP events and submits already-filtered data to a bounded queue. It does not wait for Loki network requests. If the queue is full, the newest event is dropped and counters record the loss.

```text
Axum/Tower request
       |
       v
route policy -> bounded capture -> redaction -> bounded queue
                                                |
                           +--------------------+-------------------+
                           v                                        v
                  Loki exporter workers                   application consumer
```

The crate also accepts `tracing` business events through the same queue. OpenTelemetry integration correlates IDs; applications configure span export separately.

Events use independent schema version `1.0` for HTTP and business records. See the [schema guide](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/schemas/README.md). Loki labels use controlled service metadata and event type; high-cardinality IDs, paths, IPs, and bodies are not labels.

Optional geolocation reads an explicitly supplied local MMDB file at startup. The crate does not download or bundle a production database.
