use axum::{routing::get, Router};
use safe_http_telemetry::{
    axum_adapter::axum_telemetry_layer,
    core::event::TelemetryEvent,
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
    let receiver = queue.receiver();
    let printer = tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            // Events are already bounded and redacted by the middleware.
            let line = match event {
                TelemetryEvent::Http(event) => serde_json::to_string(&*event),
                TelemetryEvent::Business(event) => serde_json::to_string(&*event),
            };
            match line {
                Ok(line) => println!("{line}"),
                Err(error) => eprintln!("telemetry serialization failed: {error}"),
            }
        }
    });

    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);
    policies.add_policy(
        RoutePolicy::new(RoutePattern::exact("/health"), CaptureProfile::Minimal).exclude(),
    )?;

    let telemetry_layer =
        axum_telemetry_layer(queue.clone(), policies, MiddlewareConfig::default())?;

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

    let server_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    queue.close();
    printer.await?;
    server_result?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
