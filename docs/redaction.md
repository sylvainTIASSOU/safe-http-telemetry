# Sensitive-data redaction

## Processing guarantee

Captured data must follow this order:

1. apply bounded capture;
2. parse the supported structured format;
3. recursively redact sensitive fields;
4. apply content and size limits;
5. verify the resulting telemetry representation;
6. only then queue or export it.

Raw captured values must never enter the export queue.

## Field normalization

Field matching ignores letter case and non-alphanumeric separators.

These names are equivalent:

- `access_token`;
- `access-token`;
- `accessToken`;
- `ACCESS.TOKEN`.

Matching is exact after normalization. For example, `token` is sensitive, but
`tokenizer` is not automatically treated as sensitive.

## Recursive behavior

Objects and arrays are traversed recursively. When a sensitive field contains
an object or array, the complete value is replaced by `[REDACTED]`.

Headers and query parameters use the same normalized field rules. Query scalar
and array shapes are preserved unless the complete parameter is redacted.

## Bounds

JSON processing is bounded by:

- maximum depth;
- maximum visited nodes;
- maximum string bytes.

Header values have an independent byte limit. String truncation preserves UTF-8
boundaries and includes `[TRUNCATED]` within the configured output limit.

## Markers

- `[REDACTED]`: hidden by a sensitive-field rule;
- `[TRUNCATED]`: cut by a depth, node or byte limit;
- `[OMITTED]`: disabled by policy;
- `[UNSUPPORTED]`: unsupported content format;
- `[PARSE_ERROR]`: expected structured content could not be parsed.

Invalid or incomplete JSON must not be exported as raw text under the
assumption that structural redaction occurred. Body parsing and the default
omission behavior are implemented with HTTP body capture in a later phase.

## Security limits

No generic engine can detect every possible secret. Applications must add
custom field names and restrictive route policies for domain-specific
credentials. Tests must use fake secrets only.

Access to telemetry storage remains necessary even after redaction.