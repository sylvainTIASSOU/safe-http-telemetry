use proptest::prelude::*;
use safe_http_telemetry::config::CaptureLimits;

// Generate plausibly invalid limits.
// Bias toward zero so proptest reaches failing cases sooner.
fn arb_limits() -> impl Strategy<Value = CaptureLimits> {
    (
        0usize..1024, // max_request_body_bytes
        0usize..1024, // max_response_body_bytes
        0usize..1024, // max_header_value_bytes
        0usize..1024, // max_total_header_bytes
        0usize..1024, // max_headers
        0usize..1024, // max_query_parameters
        0usize..1024, // max_json_depth
        0usize..1024, // max_json_nodes
        0usize..1024, // max_string_bytes
        0usize..1024, // max_event_bytes
        0usize..1024, // max_loki_batch_bytes
    )
        .prop_map(
            |(
                max_request_body_bytes,
                max_response_body_bytes,
                max_header_value_bytes,
                max_total_header_bytes,
                max_headers,
                max_query_parameters,
                max_json_depth,
                max_json_nodes,
                max_string_bytes,
                max_event_bytes,
                max_loki_batch_bytes,
            )| CaptureLimits {
                max_request_body_bytes,
                max_response_body_bytes,
                max_header_value_bytes,
                max_total_header_bytes,
                max_headers,
                max_query_parameters,
                max_json_depth,
                max_json_nodes,
                max_string_bytes,
                max_event_bytes,
                max_loki_batch_bytes,
            },
        )
}

proptest! {
    /// Invariant: `validate()` must fail if any limit is zero.
    #[test]
    fn zero_limit_is_always_rejected(limits in arb_limits()) {
    let has_zero = [
            limits.max_request_body_bytes,
            limits.max_response_body_bytes,
            limits.max_header_value_bytes,
            limits.max_total_header_bytes,
            limits.max_headers,
            limits.max_query_parameters,
            limits.max_json_depth,
            limits.max_json_nodes,
            limits.max_string_bytes,
            limits.max_event_bytes,
            limits.max_loki_batch_bytes,
        ]
        .contains(&0);

        if has_zero {
            prop_assert!(limits.validate().is_err(),
                "a zero limit must be rejected, got {:?}", limits);
        }
    }

    /// Invariant: `validate()` must fail when `max_event_bytes > max_loki_batch_bytes`.
    #[test]
    fn event_larger_than_batch_is_rejected(
        event_bytes in 1usize..1024,
        batch_bytes in 1usize..1024,
    ) {
        let limits = CaptureLimits {
            max_event_bytes: event_bytes,
            max_loki_batch_bytes: batch_bytes,
            ..CaptureLimits::default()
        };
        if event_bytes > batch_bytes {
            prop_assert!(limits.validate().is_err(),
                "event limit greater than batch limit must be rejected");
        }
    }
}
