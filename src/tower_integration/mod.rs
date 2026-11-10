//! Tower middleware for the HTTP request lifecycle.
//!
//! This increment observes minimal request metadata and emits the event when
//! response headers arrive. Response-body completion and cancellation are
//! implemented by the subsequent middleware increments.

mod body;
mod config;
mod request_id;

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};

use http::{Request, Response, StatusCode};
use pin_project_lite::pin_project;
use tower::{Layer, Service};
use tracing::{field, Level as TracingLevel, Span};

use crate::{
    config::ConfigError,
    core::{
        event::{CaptureInfo, HttpEvent, LogLevel, RequestInfo, ResponseInfo, TimingInfo},
        telemetry::InternalTelemetry,
    },
    policy::RoutePolicySet,
    redaction::TRUNCATED,
    runtime::queue::BoundedEventQueue,
};

pub use body::ObservedBody;
pub use config::MiddlewareConfig;

/// Stable route value used when the HTTP framework exposes no matched route.
pub const UNMATCHED_ROUTE: &str = "unmatched";

/// Extracts a normalized route without coupling Tower to a specific router.
pub trait RouteResolver: Clone + Send + Sync + 'static {
    fn resolve<Body>(&self, request: &Request<Body>) -> Option<String>;
}

/// Route resolver for Tower services that have no router integration.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnmatchedRoute;

impl RouteResolver for UnmatchedRoute {
    fn resolve<Body>(&self, _request: &Request<Body>) -> Option<String> {
        None
    }
}

/// Configures minimal HTTP telemetry for a Tower service.
#[derive(Clone)]
pub struct HttpTelemetryLayer<R = UnmatchedRoute> {
    queue: BoundedEventQueue,
    policies: RoutePolicySet,
    config: MiddlewareConfig,
    route_resolver: R,
}

impl HttpTelemetryLayer<UnmatchedRoute> {
    pub fn new(
        queue: BoundedEventQueue,
        policies: RoutePolicySet,
        config: MiddlewareConfig,
    ) -> Result<Self, ConfigError> {
        Self::with_route_resolver(queue, policies, config, UnmatchedRoute)
    }
}

impl<R: RouteResolver> HttpTelemetryLayer<R> {
    pub fn with_route_resolver(
        queue: BoundedEventQueue,
        policies: RoutePolicySet,
        config: MiddlewareConfig,
        route_resolver: R,
    ) -> Result<Self, ConfigError> {
        config.limits.validate()?;

        Ok(Self {
            queue,
            policies,
            config,
            route_resolver,
        })
    }
}

impl<S, R: Clone> Layer<S> for HttpTelemetryLayer<R> {
    type Service = HttpTelemetry<S, R>;

    fn layer(&self, inner: S) -> Self::Service {
        HttpTelemetry {
            inner,
            queue: self.queue.clone(),
            policies: self.policies.clone(),
            config: self.config.clone(),
            route_resolver: self.route_resolver.clone(),
            telemetry: self.queue.telemetry(),
        }
    }
}

/// Tower service produced by [`HttpTelemetryLayer`].
#[derive(Clone)]
pub struct HttpTelemetry<S, R = UnmatchedRoute> {
    inner: S,
    queue: BoundedEventQueue,
    policies: RoutePolicySet,
    config: MiddlewareConfig,
    route_resolver: R,
    telemetry: InternalTelemetry,
}

impl<S, R, RequestBody, ResponseBody> Service<Request<RequestBody>> for HttpTelemetry<S, R>
where
    S: Service<Request<RequestBody>, Response = Response<ResponseBody>>,
    R: RouteResolver,
    ResponseBody: http_body::Body,
{
    type Response = Response<ObservedBody<ResponseBody>>;
    type Error = S::Error;
    type Future = HttpTelemetryResponseFuture<S::Future>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, request: Request<RequestBody>) -> Self::Future {
        let policy = self
            .policies
            .resolve(request.method().as_str(), request.uri().path());

        if policy.excluded {
            self.telemetry.record_request_excluded();
            return HttpTelemetryResponseFuture::excluded(self.inner.call(request));
        }

        self.telemetry.record_request_started();

        let max_bytes = self.config.limits.max_string_bytes;
        let method = bounded_value(request.method().as_str(), max_bytes, &self.telemetry);
        let path = bounded_value(request.uri().path(), max_bytes, &self.telemetry);
        let route = self
            .route_resolver
            .resolve(&request)
            .map(|route| bounded_value(&route, max_bytes, &self.telemetry))
            .unwrap_or_else(|| UNMATCHED_ROUTE.to_owned());
        let request_id =
            request_id::resolve(request.headers(), self.config.trust_incoming_request_id);

        let span = tracing::info_span!(
            "http.request",
            request_id = %request_id,
            method = %method,
            route = %route,
            status_code = field::Empty,
            duration_ms = field::Empty,
            outcome = field::Empty,
        );

        let started_at = Instant::now();
        let inner = span.in_scope(|| self.inner.call(request));
        let context = RequestContext {
            request_id,
            method,
            route,
            path,
            started_at,
            queue: self.queue.clone(),
            telemetry: self.telemetry.clone(),
        };

        HttpTelemetryResponseFuture::observed(inner, context, span)
    }
}

pin_project! {
    /// Response future carrying no public HTTP-data `Debug` implementation.
    pub struct HttpTelemetryResponseFuture<F> {
        #[pin]
        inner: F,
        context: Option<RequestContext>,
        span: Span,
    }
}

impl<F> HttpTelemetryResponseFuture<F> {
    fn observed(inner: F, context: RequestContext, span: Span) -> Self {
        Self {
            inner,
            context: Some(context),
            span,
        }
    }

    fn excluded(inner: F) -> Self {
        Self {
            inner,
            context: None,
            span: Span::none(),
        }
    }
}

impl<F, Body, Error> Future for HttpTelemetryResponseFuture<F>
where
    F: Future<Output = Result<Response<Body>, Error>>,
    Body: http_body::Body,
{
    type Output = Result<Response<ObservedBody<Body>>, Error>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        let result = {
            let _entered = this.span.enter();
            this.inner.as_mut().poll(context)
        };

        match result {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(response)) => {
                let status = response.status();
                let (parts, body) = response.into_parts();

                let response = match this.context.take() {
                    Some(request) => {
                        let response_received_at = Instant::now();
                        let time_to_first_byte_ms =
                            request.time_to_first_byte_ms(response_received_at);

                        if body.is_end_stream() {
                            request.complete_body(&*this.span, status, time_to_first_byte_ms, 0.0);

                            Response::from_parts(parts, ObservedBody::finished(body))
                        } else {
                            let observed = ObservedBody::observed(
                                body,
                                request,
                                Span::clone(&*this.span),
                                status,
                                response_received_at,
                                time_to_first_byte_ms,
                            );

                            Response::from_parts(parts, observed)
                        }
                    }
                    None => Response::from_parts(parts, ObservedBody::excluded(body)),
                };

                Poll::Ready(Ok(response))
            }
            Poll::Ready(Err(error)) => {
                // Error-event construction is added in increment 3c.
                this.context.take();
                Poll::Ready(Err(error))
            }
        }
    }
}

pub(crate) struct RequestContext {
    request_id: String,
    method: String,
    route: String,
    path: String,
    started_at: Instant,
    queue: BoundedEventQueue,
    telemetry: InternalTelemetry,
}

impl RequestContext {
    pub(crate) fn time_to_first_byte_ms(&self, response_received_at: Instant) -> f64 {
        response_received_at
            .saturating_duration_since(self.started_at)
            .as_secs_f64()
            * 1_000.0
    }

    pub(crate) fn complete_body(
        self,
        span: &Span,
        status: StatusCode,
        time_to_first_byte_ms: f64,
        body_duration_ms: f64,
    ) {
        let duration_ms = self.started_at.elapsed().as_secs_f64() * 1_000.0;
        let request = RequestInfo::new(self.method, self.route, self.path);
        let response = ResponseInfo::new(status.as_u16());
        let mut timing = TimingInfo::new(duration_ms);
        timing.time_to_first_byte_ms = Some(time_to_first_byte_ms);
        timing.body_duration_ms = Some(body_duration_ms);

        let event = HttpEvent::new(
            self.request_id,
            request,
            response,
            timing,
            CaptureInfo::default(),
        );

        let outcome = outcome_name(&event);
        span.record("status_code", u64::from(status.as_u16()));
        span.record("duration_ms", duration_ms);
        span.record("outcome", outcome);

        match event.level {
            LogLevel::Error => {
                tracing::event!(
                    parent: span,
                    TracingLevel::ERROR,
                    request_id = %event.request_id,
                    method = %event.request.method,
                    route = %event.request.route,
                    status_code = status.as_u16(),
                    duration_ms,
                    outcome,
                    "HTTP request completed"
                );
            }
            LogLevel::Warn => {
                tracing::event!(
                    parent: span,
                    TracingLevel::WARN,
                    request_id = %event.request_id,
                    method = %event.request.method,
                    route = %event.request.route,
                    status_code = status.as_u16(),
                    duration_ms,
                    outcome,
                    "HTTP request completed"
                );
            }
            _ => {
                tracing::event!(
                    parent: span,
                    TracingLevel::INFO,
                    request_id = %event.request_id,
                    method = %event.request.method,
                    route = %event.request.route,
                    status_code = status.as_u16(),
                    duration_ms,
                    outcome,
                    "HTTP request completed"
                );
            }
        }

        self.telemetry.record_request_completed();

        // Saturation and queue closure never change the application response.
        let _ = self.queue.try_push(event);
    }
}

fn outcome_name(event: &HttpEvent) -> &'static str {
    match event.response.outcome {
        Some(crate::core::event::ResponseOutcome::Success) => "success",
        Some(crate::core::event::ResponseOutcome::ClientError) => "client_error",
        Some(crate::core::event::ResponseOutcome::ServerError) => "server_error",
        Some(crate::core::event::ResponseOutcome::Cancelled) => "cancelled",
        Some(crate::core::event::ResponseOutcome::Timeout) => "timeout",
        None => "unknown",
    }
}

fn bounded_value(value: &str, max_bytes: usize, telemetry: &InternalTelemetry) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }

    let prefix_budget = max_bytes - TRUNCATED.len();
    let mut prefix_end = prefix_budget.min(value.len());

    while !value.is_char_boundary(prefix_end) {
        prefix_end -= 1;
    }

    let mut truncated = String::with_capacity(max_bytes);
    truncated.push_str(&value[..prefix_end]);
    truncated.push_str(TRUNCATED);
    telemetry.record_truncations(1);
    truncated
}
