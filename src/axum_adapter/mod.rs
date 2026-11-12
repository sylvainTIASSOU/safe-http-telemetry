//! Ergonomic Axum integration built on top of the Tower middleware.
//!
//! Apply the telemetry layer to an Axum `Router` after registering routes so
//! `MatchedPath` can provide normalized routes such as `/users/{id}`.

use std::convert::Infallible;

#[cfg(feature = "body-capture")]
use std::task::{Context, Poll};

#[cfg(feature = "body-capture")]
use axum::body::Body;
use axum::{extract::MatchedPath, BoxError};
use http::Request;
use tower::Layer;

#[cfg(feature = "body-capture")]
use tower::Service;

use crate::{
    config::ConfigError,
    policy::RoutePolicySet,
    runtime::queue::BoundedEventQueue,
    tower_integration::{
        ErrorClassifier, HttpTelemetryLayer, MiddlewareConfig, RequestFailure, RouteResolver,
    },
};

#[cfg(feature = "body-capture")]
use crate::{
    core::telemetry::InternalTelemetry,
    tower_integration::{ObservedRequestBody, RequestBodyCaptureState},
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
#[derive(Clone)]
pub struct AxumTelemetryLayer {
    telemetry: HttpTelemetryLayer<AxumRouteResolver, AxumErrorClassifier>,

    #[cfg(feature = "body-capture")]
    policies: RoutePolicySet,

    #[cfg(feature = "body-capture")]
    request_body_limit: usize,

    #[cfg(feature = "body-capture")]
    header_value_limit: usize,

    #[cfg(feature = "body-capture")]
    internal_telemetry: InternalTelemetry,
}

impl<S> Layer<S> for AxumTelemetryLayer {
    #[cfg(not(feature = "body-capture"))]
    type Service =
        crate::tower_integration::HttpTelemetry<S, AxumRouteResolver, AxumErrorClassifier>;

    #[cfg(feature = "body-capture")]
    type Service = AxumTelemetry<
        crate::tower_integration::HttpTelemetry<S, AxumRouteResolver, AxumErrorClassifier>,
    >;

    fn layer(&self, inner: S) -> Self::Service {
        let telemetry = self.telemetry.layer(inner);

        #[cfg(not(feature = "body-capture"))]
        return telemetry;

        #[cfg(feature = "body-capture")]
        AxumTelemetry {
            inner: telemetry,
            policies: self.policies.clone(),
            request_body_limit: self.request_body_limit,
            header_value_limit: self.header_value_limit,
            internal_telemetry: self.internal_telemetry.clone(),
        }
    }
}

/// Axum request-body adapter. It preserves `axum::body::Body` at the service
/// boundary while observing frames through a bounded wrapper.
#[cfg(feature = "body-capture")]
#[derive(Clone)]
pub struct AxumTelemetry<S> {
    inner: S,
    policies: RoutePolicySet,
    request_body_limit: usize,
    header_value_limit: usize,
    internal_telemetry: InternalTelemetry,
}

#[cfg(feature = "body-capture")]
impl<S> Service<Request<Body>> for AxumTelemetry<S>
where
    S: Service<Request<Body>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let policy = self
            .policies
            .resolve(request.method().as_str(), request.uri().path());

        if !policy.capture_request_body {
            // No state, allocation or body wrapping when policy denies capture.
            return self.inner.call(request);
        }

        let capture = RequestBodyCaptureState::new(
            request.headers(),
            self.header_value_limit,
            &self.internal_telemetry,
        );

        let (parts, body) = request.into_parts();
        let body = ObservedRequestBody::new(body, capture.clone(), self.request_body_limit);
        let mut request = Request::from_parts(parts, Body::new(body));
        request.extensions_mut().insert(capture);

        self.inner.call(request)
    }
}

/// Builds an Axum telemetry layer over an existing bounded queue.
pub fn axum_telemetry_layer(
    queue: BoundedEventQueue,
    policies: RoutePolicySet,
    config: MiddlewareConfig,
) -> Result<AxumTelemetryLayer, ConfigError> {
    #[cfg(feature = "body-capture")]
    let internal_telemetry = queue.telemetry();

    let telemetry = HttpTelemetryLayer::with_route_resolver(
        queue,
        policies.clone(),
        config.clone(),
        AxumRouteResolver,
    )?
    .with_error_classifier(AxumErrorClassifier);

    Ok(AxumTelemetryLayer {
        telemetry,

        #[cfg(feature = "body-capture")]
        policies,

        #[cfg(feature = "body-capture")]
        request_body_limit: config.limits.max_request_body_bytes,

        #[cfg(feature = "body-capture")]
        header_value_limit: config.limits.max_header_value_bytes,

        #[cfg(feature = "body-capture")]
        internal_telemetry,
    })
}
