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
    /// Invariant: la taille de la file ne dépasse jamais sa capacité,
    /// et le nombre d'événements acceptés + rejetés est égal au nombre d'essais.
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
            // ⚠️ IMPORTANT : Remplace cette ligne par la même création d'événement
            // que celle utilisée dans tests/runtime_queue.rs
            // Par exemple : let event = TelemetryEvent::dummy();
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

// ⚠️ IMPORTANT : Regarde dans tests/runtime_queue.rs comment tu crées un événement factice.
// Copie cette logique ici pour que le test puisse générer un TelemetryEvent.
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
