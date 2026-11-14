use proptest::prelude::*;
use safe_http_telemetry::{
    core::telemetry::InternalTelemetry,
    runtime::queue::{BoundedEventQueue, QueueConfig},
};

fn arb_capacity() -> impl Strategy<Value = usize> {
    1usize..256
}

fn arb_num_events() -> impl Strategy<Value = usize> {
    0usize..512
}

proptest! {
    /// Invariant: queue occupancy never exceeds capacity, and accepted plus
    /// rejected events equals the total number of attempts.
    #[test]
    fn queue_never_exceeds_capacity(
        capacity in arb_capacity(),
        num_events in arb_num_events(),
    ) {
        let telemetry = InternalTelemetry::new();
        let queue = BoundedEventQueue::new(
            QueueConfig::new(capacity),
            telemetry.clone(),
        ).unwrap();

        let mut accepted = 0usize;
        let mut rejected = 0usize;

        for _ in 0..num_events {
            // Use the test-event constructor below to create a TelemetryEvent.
            let event = create_test_event();

            match queue.try_push(event) {
                Ok(()) => accepted += 1,
                Err(_) => rejected += 1,
            }
        }

       let snap = telemetry.snapshot();
let occupancy = queue.stats().occupancy;
prop_assert!(
    occupancy <= capacity,
    "occupancy {} exceeded capacity {}",
    occupancy,
    capacity
);
prop_assert_eq!(accepted, snap.accepted_events as usize);
prop_assert_eq!(rejected, snap.rejected_events as usize);
    }
}

// IMPORTANT: See tests/runtime_queue.rs for the test-event construction pattern.
// Reuse that pattern here so the test can generate a TelemetryEvent.
fn create_test_event() -> safe_http_telemetry::core::event::TelemetryEvent {
    use safe_http_telemetry::core::event::{
        CaptureInfo, HttpEvent, RequestInfo, ResponseInfo, TimingInfo,
    };

    HttpEvent::new(
        "prop-queue-event",
        RequestInfo::new("GET", "/test", "/test"),
        ResponseInfo::new(200),
        TimingInfo::new(1.0),
        CaptureInfo::default(),
    )
    .into()
}
