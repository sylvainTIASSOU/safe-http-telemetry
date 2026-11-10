use axum::{routing::get, Router};
use safe_http_telemetry::{
    axum_adapter::axum_telemetry_layer,
    core::telemetry::InternalTelemetry,
    policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::config::MiddlewareConfig,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let queue = BoundedEventQueue::new(QueueConfig::new(128), InternalTelemetry::new())?;

    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);
    policies.add_policy(
        RoutePolicy::new(RoutePattern::exact("/health"), CaptureProfile::Minimal).exclude(),
    )?;

    let telemetry_layer = axum_telemetry_layer(queue, policies, MiddlewareConfig::default())?;

    // Register routes before applying telemetry so MatchedPath is available.
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

    axum::serve(listener, app).await?;
    Ok(())
}
