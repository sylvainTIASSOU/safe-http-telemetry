use std::{env, time::Duration};

use axum::{
    routing::{get, post},
    Form, Json, Router,
};
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use safe_http_telemetry::{
    axum_adapter::axum_telemetry_layer,
    config::{CaptureLimits, RedactionConfig, ServiceConfig},
    core::telemetry::InternalTelemetry,
    geolocation::GeolocationConfig,
    loki::{config::LokiConfig, exporter::LokiExporter},
    policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet},
    redaction::Redactor,
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::config::MiddlewareConfig,
    tracing_integration::business::{BusinessLogConfig, BusinessLogLayer},
};
use serde::{Deserialize, Serialize};
use tracing_subscriber::prelude::*;

#[derive(Debug, Deserialize, Serialize)]
struct ItemPayload {
    name: String,
    email: String,
    password: String,
    token: String,
}

#[derive(Debug, Deserialize)]
struct FormPayload {
    username: String,
    password: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let push_url = env::var("LOKI_PUSH_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:3100/loki/api/v1/push".to_owned());

    let queue = BoundedEventQueue::new(QueueConfig::new(1_024), InternalTelemetry::new())?;

    let redactor = Redactor::new(
        &RedactionConfig::default(),
        &CaptureLimits::default(),
        queue.telemetry(),
    )?;

    let business_layer =
        BusinessLogLayer::new(queue.clone(), redactor, BusinessLogConfig::default())?;

    let tracer_provider = SdkTracerProvider::default();
    let tracer = tracer_provider.tracer("safe-http-telemetry-example");

    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::LevelFilter::INFO)
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(business_layer)
        .try_init()?;

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

    // --- Capture policy ---
    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);

    // Health check exclu
    policies.add_policy(
        RoutePolicy::new(RoutePattern::exact("/health"), CaptureProfile::Minimal).exclude(),
    )?;

    // Hello keep in minimal
    policies.add_policy(RoutePolicy::new(
        RoutePattern::exact("/hello/{name}"),
        CaptureProfile::Minimal,
    ))?;

    // Routes détaillées pour tester la capture de corps
    policies.add_policy(RoutePolicy::new(
        RoutePattern::exact("/e2e/items"),
        CaptureProfile::Detailed,
    ))?;
    policies.add_policy(RoutePolicy::new(
        RoutePattern::exact("/e2e/form"),
        CaptureProfile::Detailed,
    ))?;
    policies.add_policy(RoutePolicy::new(
        RoutePattern::exact("/e2e/text"),
        CaptureProfile::Detailed,
    ))?;

    // --- Configuration du middleware avec géolocalisation ---
    let middleware_config = MiddlewareConfig {
        geolocation: Some(GeolocationConfig::bundled()),
        ..MiddlewareConfig::default()
    };

    let telemetry_layer = axum_telemetry_layer(queue, policies, middleware_config)?;

    let app = Router::new()
        .route(
            "/hello/{name}",
            get(|| async {
                tracing::info!(order_id = "fictitious-order-001", "Greeting requested");
                "Hello from safe HTTP telemetry\n"
            }),
        )
        .route("/health", get(|| async { "OK\n" }))
        .route("/e2e/items", post(create_item))
        .route("/e2e/form", post(create_form))
        .route("/e2e/text", post(create_text))
        .layer(telemetry_layer);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3030").await?;
    println!("Axum listening on http://127.0.0.1:3030");
    println!("Try: curl http://127.0.0.1:3030/hello/fictitious-user");
    println!("Try: curl -X POST http://127.0.0.1:3030/e2e/items -H 'content-type: application/json' -d '{{\"name\":\"John\",\"email\":\"john@example.com\",\"password\":\"secret\",\"token\":\"tok_123\"}}'");
    println!("Press Ctrl+C to drain telemetry and stop.");

    let server_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;

    let shutdown_report = exporter.shutdown().await;
    tracer_provider.shutdown()?;
    server_result?;
    println!("Loki shutdown: {shutdown_report:?}");
    if shutdown_report.abandoned_events != 0 {
        return Err("some telemetry events were abandoned during shutdown".into());
    }
    Ok(())
}

async fn create_item(Json(payload): Json<ItemPayload>) -> Json<ItemPayload> {
    tracing::info!(item_name = %payload.name, "Item created via detailed profile");
    // On renvoie le payload tel quel pour tester la capture du corps de réponse.
    Json(payload)
}

async fn create_form(Form(payload): Form<FormPayload>) -> &'static str {
    tracing::info!(
        username = %payload.username,
        has_password = !payload.password.is_empty(),
        "Form submitted via detailed profile"
    );
    "Form received\n"
}

async fn create_text(body: String) -> &'static str {
    tracing::info!(
        body_len = body.len(),
        "Text body received via detailed profile"
    );
    "Text received\n"
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
