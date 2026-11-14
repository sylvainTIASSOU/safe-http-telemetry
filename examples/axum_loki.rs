use std::{env, net::SocketAddr, time::Duration};

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
    loki::{config::LokiConfig, exporter::LokiExporter},
    policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet},
    redaction::Redactor,
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::config::MiddlewareConfig,
    tracing_integration::business::{BusinessLogConfig, BusinessLogLayer},
};
use serde::Deserialize;
use tracing_subscriber::prelude::*;

#[derive(Debug, Deserialize)]
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
        name: env::var("SERVICE_NAME").unwrap_or_else(|_| "safe-http-telemetry-example".to_owned()),
        environment: env::var("SERVICE_ENVIRONMENT").unwrap_or_else(|_| "development".to_owned()),
        version: env::var("SERVICE_VERSION")
            .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_owned()),
        region: env::var("SERVICE_REGION").ok(),
    };

    let mut loki_config = LokiConfig::new(push_url)?;
    if let Ok(token) = env::var("LOKI_BEARER_TOKEN") {
        loki_config = loki_config.with_bearer_token(token);
    }
    if let Ok(tenant_id) = env::var("LOKI_TENANT_ID") {
        loki_config = loki_config.with_tenant_id(tenant_id);
    }
    loki_config.batch.max_events = 100;
    loki_config.batch.flush_interval = Duration::from_millis(250);
    loki_config.request_timeout = Duration::from_secs(2);
    loki_config.retry.max_retries = 2;
    loki_config.shutdown.drain = true;
    loki_config.shutdown.deadline = Duration::from_secs(5);

    let exporter = LokiExporter::new(loki_config, service, queue.clone(), 1)?.start();

    // Keep HTTP telemetry minimal by default; opt in to detailed routes.
    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);

    // Health checks do not produce events.
    policies.add_policy(
        RoutePolicy::new(RoutePattern::exact("/health"), CaptureProfile::Minimal).exclude(),
    )?;

    // Demonstrate bounded, redacted body capture when body-capture is enabled.
    policies.add_policy(RoutePolicy::new(
        RoutePattern::exact("/items"),
        CaptureProfile::Detailed,
    ))?;
    policies.add_policy(RoutePolicy::new(
        RoutePattern::exact("/form"),
        CaptureProfile::Detailed,
    ))?;
    policies.add_policy(RoutePolicy::new(
        RoutePattern::exact("/text"),
        CaptureProfile::Detailed,
    ))?;

    let mut middleware_config = MiddlewareConfig {
        #[cfg(feature = "geolocation")]
        geolocation: env::var_os("SAFE_HTTP_MMDB_PATH")
            .map(safe_http_telemetry::geolocation::GeolocationConfig::new),
        ..MiddlewareConfig::default()
    };
    if let Ok(cidrs) = env::var("TRUSTED_PROXY_CIDRS") {
        middleware_config.client_ip.trusted_proxies = cidrs
            .split(',')
            .map(str::trim)
            .map(str::parse)
            .collect::<Result<_, _>>()?;
    }

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
        .route("/items", post(create_item))
        .route("/form", post(create_form))
        .route("/text", post(create_text))
        .layer(telemetry_layer);

    let bind_address: SocketAddr = env::var("APP_BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3030".to_owned())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(bind_address).await?;
    println!("Axum listening on http://{bind_address}");
    println!("Press Ctrl+C to drain telemetry and stop.");

    let server_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;

    let tracer_result = tracer_provider.shutdown();
    let shutdown_report = exporter.shutdown().await;
    server_result?;
    tracer_result?;
    println!("Loki shutdown: {shutdown_report:?}");
    if shutdown_report.abandoned_events != 0 {
        return Err("some telemetry events were abandoned during shutdown".into());
    }
    Ok(())
}

async fn create_item(Json(payload): Json<ItemPayload>) -> axum::http::StatusCode {
    // Avoid logging or returning submitted personal data and credentials.
    let _ = (payload.name, payload.email, payload.password, payload.token);
    tracing::info!("Item accepted");
    axum::http::StatusCode::CREATED
}

async fn create_form(Form(payload): Form<FormPayload>) -> &'static str {
    let _ = (payload.username, payload.password);
    tracing::info!("Form accepted");
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
