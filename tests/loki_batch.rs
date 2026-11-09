#![cfg(feature = "loki")]

use std::time::Duration;

use safe_http_telemetry::{
    config::ServiceConfig,
    core::event::{CaptureInfo, HttpEvent, RequestInfo, ResponseInfo, TimingInfo},
    loki::{
        batch::{BatchPush, LokiBatchBuilder, LokiBatchError},
        config::BatchConfig,
    },
};
use serde_json::json;

fn sample_service() -> ServiceConfig {
    ServiceConfig {
        name: "users-api".to_owned(),
        environment: "test".to_owned(),
        version: "0.1.0".to_owned(),
        region: Some("eu-west".to_owned()),
    }
}

fn sample_event(request_id: &str) -> HttpEvent {
    let mut request = RequestInfo::new("POST", "/api/users/{id}", "/api/users/42");
    request.body = Some(json!({
        "password": "[REDACTED]",
        "value": "bounded-batch-test"
    }));

    HttpEvent::new(
        request_id,
        request,
        ResponseInfo::new(201),
        TimingInfo::new(3.5),
        CaptureInfo::default(),
    )
}

fn batch_config() -> BatchConfig {
    BatchConfig {
        max_events: 2,
        max_bytes: 1024 * 1024,
        flush_interval: Duration::from_millis(25),
    }
}

#[test]
fn batch_enforces_event_limit_and_preserves_timestamps() {
    let mut builder = LokiBatchBuilder::new(batch_config(), sample_service()).unwrap();

    assert_eq!(
        builder.try_push(
            &sample_event("request-batch-0001"),
            1_730_000_000_000_000_001
        ),
        Ok(BatchPush::Accepted)
    );
    assert!(!builder.is_full());

    assert_eq!(
        builder.try_push(
            &sample_event("request-batch-0002"),
            1_730_000_000_000_000_002
        ),
        Ok(BatchPush::Accepted)
    );
    assert!(builder.is_full());
    assert_eq!(
        builder.try_push(
            &sample_event("request-batch-0003"),
            1_730_000_000_000_000_003
        ),
        Ok(BatchPush::Full)
    );

    let batch = builder.take().unwrap();
    assert_eq!(batch.event_count(), 2);
    assert_eq!(
        batch.size_bytes(),
        batch.payload().to_json_bytes().unwrap().len()
    );
    assert_eq!(batch.payload().streams.len(), 2);
    assert_eq!(
        batch.payload().streams[0].values[0][0],
        "1730000000000000001"
    );
    assert_eq!(
        batch.payload().streams[1].values[0][0],
        "1730000000000000002"
    );
    assert!(builder.is_empty());
    assert_eq!(builder.event_count(), 0);
    assert_eq!(builder.size_bytes(), 0);
}

#[test]
fn byte_limit_flushes_current_batch_without_losing_the_new_event() {
    let mut measuring = LokiBatchBuilder::new(batch_config(), sample_service()).unwrap();
    measuring
        .try_push(&sample_event("request-batch-0001"), 1)
        .unwrap();
    let first_event_size = measuring.size_bytes();

    let mut config = batch_config();
    config.max_bytes = first_event_size;
    let mut builder = LokiBatchBuilder::new(config, sample_service()).unwrap();

    assert_eq!(
        builder.try_push(&sample_event("request-batch-0001"), 1),
        Ok(BatchPush::Accepted)
    );
    assert_eq!(
        builder.try_push(&sample_event("request-batch-0002"), 2),
        Ok(BatchPush::Full)
    );
    assert_eq!(builder.event_count(), 1);

    let first_batch = builder.take().unwrap();
    assert_eq!(first_batch.event_count(), 1);

    assert_eq!(
        builder.try_push(&sample_event("request-batch-0002"), 2),
        Ok(BatchPush::Accepted)
    );
    assert_eq!(builder.event_count(), 1);
}

#[test]
fn event_larger_than_an_empty_batch_is_oversized() {
    let mut config = batch_config();
    config.max_bytes = 32;
    let mut builder = LokiBatchBuilder::new(config, sample_service()).unwrap();

    assert_eq!(
        builder.try_push(&sample_event("request-batch-large"), 1),
        Ok(BatchPush::Oversized)
    );
    assert!(builder.is_empty());
    assert!(builder.take().is_none());
}

#[test]
fn invalid_zero_limits_are_rejected() {
    let mut config = batch_config();
    config.max_events = 0;
    assert!(matches!(
        LokiBatchBuilder::new(config, sample_service()),
        Err(LokiBatchError::InvalidConfiguration("batch.max_events"))
    ));

    let mut config = batch_config();
    config.max_bytes = 0;
    assert!(matches!(
        LokiBatchBuilder::new(config, sample_service()),
        Err(LokiBatchError::InvalidConfiguration("batch.max_bytes"))
    ));

    let mut config = batch_config();
    config.flush_interval = Duration::ZERO;
    assert!(matches!(
        LokiBatchBuilder::new(config, sample_service()),
        Err(LokiBatchError::InvalidConfiguration("batch.flush_interval"))
    ));
}
