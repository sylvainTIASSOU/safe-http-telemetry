use safe_http_telemetry::core::telemetry::InternalTelemetry;
use std::time::Duration;

#[test]
fn export_metrics_start_and_complete_are_consistent() {
    let telemetry = InternalTelemetry::new();

    assert_eq!(telemetry.snapshot().in_flight_batches, 0);
    assert_eq!(telemetry.snapshot().export_operations, 0);
    assert_eq!(telemetry.snapshot().export_duration_ms_total, 0);

    telemetry.record_export_started();
    assert_eq!(telemetry.snapshot().in_flight_batches, 1);

    telemetry.record_export_completed(Duration::from_millis(42));

    let snap = telemetry.snapshot();
    assert_eq!(snap.in_flight_batches, 0);
    assert_eq!(snap.export_operations, 1);
    assert_eq!(snap.export_duration_ms_total, 42);
}

#[test]
fn multiple_export_operations_accumulate_duration() {
    let telemetry = InternalTelemetry::new();

    telemetry.record_export_started();
    telemetry.record_export_completed(Duration::from_millis(10));

    telemetry.record_export_started();
    telemetry.record_export_completed(Duration::from_millis(25));

    let snap = telemetry.snapshot();
    assert_eq!(snap.in_flight_batches, 0);
    assert_eq!(snap.export_operations, 2);
    assert_eq!(snap.export_duration_ms_total, 35);
}
