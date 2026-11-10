//! Ergonomic Axum integration built on top of the Tower middleware.
//!
//! Apply the telemetry layer to an Axum `Router` after registering routes so
//! `MatchedPath` can provide normalized routes such as `/users/{id}`.

use std::convert::Infallible;

use axum::{extract::MatchedPath, BoxError};
use http::Request;

use crate::{
    config::ConfigError,
    policy::RoutePolicySet,
    runtime::queue::BoundedEventQueue,
    tower_integration::{
        ErrorClassifier, HttpTelemetryLayer, MiddlewareConfig, RequestFailure, RouteResolver,
    },
};

/// Resolves Axum's normalized route instead of using the raw request path.
#[derive(Clone, Copy, Debug, Default)]
pub struct AxumRouteResolver;

impl RouteResolver for AxumRouteResolver {
    fn resolve<Body>(&self, request: &Request<Body>) -> Option<String> {
        request
            .extensions()
            .get::<MatchedPath>()
            .map(|path| path.as_str().to_owned())
    }
}

/// Classifies standard Axum/Tower errors without exposing their messages.
#[derive(Clone, Copy, Debug, Default)]
pub struct AxumErrorClassifier;

impl ErrorClassifier<BoxError> for AxumErrorClassifier {
    fn classify(&self, error: &BoxError) -> RequestFailure {
        if error
            .downcast_ref::<tower::timeout::error::Elapsed>()
            .is_some()
        {
            RequestFailure::Timeout
        } else {
            RequestFailure::ServerError
        }
    }
}

impl ErrorClassifier<Infallible> for AxumErrorClassifier {
    fn classify(&self, error: &Infallible) -> RequestFailure {
        match *error {}
    }
}

/// Axum-specific Tower layer with route and error classification installed.
pub type AxumTelemetryLayer = HttpTelemetryLayer<AxumRouteResolver, AxumErrorClassifier>;

/// Builds an Axum telemetry layer over an existing bounded queue.
pub fn axum_telemetry_layer(
    queue: BoundedEventQueue,
    policies: RoutePolicySet,
    config: MiddlewareConfig,
) -> Result<AxumTelemetryLayer, ConfigError> {
    Ok(
        HttpTelemetryLayer::with_route_resolver(queue, policies, config, AxumRouteResolver)?
            .with_error_classifier(AxumErrorClassifier),
    )
}
