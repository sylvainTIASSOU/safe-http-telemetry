use std::{env, time::Duration};

use axum::{routing::get, Router};
use safe_http_telemetry::{
    axum_adapter::axum_telemetry_layer,
    config::ServiceConfig,
    core::telemetry::InternalTelemetry,
    loki::{config::LokiConfig, exporter::LokiExporter},
    policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::config::MiddlewareConfig,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let push_url = env::var("LOKI_PUSH_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:3100/loki/api/v1/push".to_owned());

    let queue = BoundedEventQueue::new(QueueConfig::new(1_024), InternalTelemetry::new())?;

    let service = ServiceConfig {
        name: "safe-http-telemetry-example".to_owned(),
        environment: "development".to_owned(),
        version: "0.1.0".to_owned(),
        region: None,
    };

    let mut loki_config = LokiConfig::new(push_url)?;
    loki_config.batch.max_events = 100;
    loki_config.batch.flush_interval = Duration::from_millis(250);
    loki_config.request_timeout = Duration::from_secs(2);
    loki_config.retry.max_retries = 2;
    loki_config.shutdown.drain = true;
    loki_config.shutdown.deadline = Duration::from_secs(5);

    let exporter = LokiExporter::new(loki_config, service, queue.clone(), 1)?.start();

    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);
    policies.add_policy(
        RoutePolicy::new(RoutePattern::exact("/health"), CaptureProfile::Minimal).exclude(),
    )?;

    let telemetry_layer = axum_telemetry_layer(queue, policies, MiddlewareConfig::default())?;

    let app = Router::new()
        .route(
            "/hello/{name}",
            get(|| async { "Hello from safe HTTP telemetry\n" }),
        )
        .route("/health", get(|| async { "OK\n" }))
        .layer(telemetry_layer);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3030").await?;
    println!("Axum listening on http://127.0.0.1:3030");
    println!("Try: curl http://127.0.0.1:3030/hello/fictitious-user");
    println!("Press Ctrl+C to drain telemetry and stop.");

    // Stop accepting HTTP requests before closing and draining the exporter.
    let server_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;

    let shutdown_report = exporter.shutdown().await;
    server_result?;

    println!("Loki shutdown: {shutdown_report:?}");

    if shutdown_report.abandoned_events != 0 {
        return Err("some telemetry events were abandoned during shutdown".into());
    }

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
