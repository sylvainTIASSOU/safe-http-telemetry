# Capture profiles and route policies

## Profiles

### Minimal

Captures correlation identifiers, the HTTP method, normalized route, response
status and timing. Query parameters, request bodies, response bodies and
geolocation are disabled.

### Standard

Adds filtered query parameters and useful filtered headers. Geolocation may be
enabled when a local database is configured. Request and response bodies remain
disabled.

### Detailed

Allows bounded request and response body capture. Body capture still requires
a compatible content type, an applicable route policy, size limits and
redaction before queueing or export.

Detailed mode is explicit and must never be interpreted as unlimited capture.

## Route selectors

A route selector can contain:

- an exact path or a path-segment prefix;
- an optional HTTP method;
- a capture profile;
- an exclusion flag;
- optional request-body and response-body restrictions.

Except for the root prefix `/`, prefixes must not end with `/`.

The prefix `/api` matches:

- `/api`;
- `/api/users`;
- `/api/users/42`.

It does not match `/apiculture`.

## Resolution order

The most specific matching rule wins:

1. exact path before prefix;
2. longer prefix before shorter prefix;
3. method-specific rule before an any-method rule.

Duplicate selectors are rejected. If equally specific rules ever match, their
restrictions are combined: exclusion wins, the lowest profile wins, and an
explicit body denial wins.

## Body permissions

`Some(false)` explicitly disables capture. `Some(true)` permits capture only
when the effective profile is `Detailed`. A route rule cannot elevate a lower
profile into body capture.

## Sensitive routes

Authentication, password-reset, credential-management and payment routes
should normally be excluded or assigned `Minimal`. Do not enable `Detailed`
for these routes without a documented, domain-specific security review.