use safe_http_telemetry::core::event::{HttpEvent, TelemetryEvent};

pub fn expect_http(event: TelemetryEvent) -> HttpEvent {
    match event {
        TelemetryEvent::Http(event) => *event,
        TelemetryEvent::Business(_) => {
            panic!("expected an HTTP event")
        }
    }
}
