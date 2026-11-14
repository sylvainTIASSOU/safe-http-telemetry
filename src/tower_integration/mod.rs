//! Tower middleware for the HTTP request lifecycle.
//!
//! This middleware observes minimal request metadata through response-body
//! completion, service errors, timeouts and premature cancellation. Terminal
//! events are finalized exactly once and submitted without network activity
//! on the application request path.

mod body;
mod client_ip;
pub mod config;
mod error;
mod metadata;

#[cfg(feature = "body-capture")]
mod request_body;

#[cfg(feature = "body-capture")]
mod response_body;

mod request_id;

use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};

use http::{Request, Response, StatusCode};
use pin_project_lite::pin_project;
use tower::{Layer, Service};
use tracing::{field, Level as TracingLevel, Span};

#[cfg(feature = "body-capture")]
use crate::body_capture::BodyCaptureEngine;

#[cfg(feature = "geolocation")]
use crate::geolocation::GeolocationEngine;

use crate::{
    config::ConfigError,
    core::{
        event::{
            CaptureInfo, HttpEvent, LogLevel, RequestInfo, ResponseInfo, ResponseOutcome,
            TimingInfo,
        },
        telemetry::InternalTelemetry,
    },
    policy::RoutePolicySet,
    runtime::queue::BoundedEventQueue,
};

use metadata::{bounded_value, MetadataCapturer};

pub use body::ObservedBody;
pub use config::{ClientIpConfig, MiddlewareConfig};
pub use error::{ErrorClassifier, RequestFailure, ServerErrorClassifier};

#[cfg(feature = "opentelemetry")]
use crate::tracing_integration::propagation::{extract_trace_context, TraceCorrelation};
#[cfg(feature = "opentelemetry")]
use tracing_opentelemetry::OpenTelemetrySpanExt;

#[cfg(feature = "body-capture")]
pub(crate) use request_body::{ObservedRequestBody, RequestBodyCaptureState};

#[cfg(feature = "body-capture")]
use response_body::ResponseBodyCaptureState;

/// Stable route value used when the HTTP framework exposes no matched route.
pub const UNMATCHED_ROUTE: &str = "unmatched";

/// Extracts a normalized, non-sensitive route without coupling Tower to a
/// specific router. Implementations must return a route template or stable
/// label and must never return the raw request path.
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
pub struct HttpTelemetryLayer<R = UnmatchedRoute, C = ServerErrorClassifier> {
    queue: BoundedEventQueue,
    policies: RoutePolicySet,
    config: MiddlewareConfig,
    route_resolver: R,
    error_classifier: C,
    metadata: MetadataCapturer,
    #[cfg(feature = "body-capture")]
    body_capture: BodyCaptureEngine,
    #[cfg(feature = "geolocation")]
    geolocation: Option<GeolocationEngine>,
}

impl HttpTelemetryLayer<UnmatchedRoute, ServerErrorClassifier> {
    pub fn new(
        queue: BoundedEventQueue,
        policies: RoutePolicySet,
        config: MiddlewareConfig,
    ) -> Result<Self, ConfigError> {
        Self::with_route_resolver(queue, policies, config, UnmatchedRoute)
    }
}

impl<R: RouteResolver> HttpTelemetryLayer<R, ServerErrorClassifier> {
    pub fn with_route_resolver(
        queue: BoundedEventQueue,
        policies: RoutePolicySet,
        config: MiddlewareConfig,
        route_resolver: R,
    ) -> Result<Self, ConfigError> {
        config.limits.validate()?;
        config.client_ip.validate()?;
        let metadata = MetadataCapturer::new(&config, queue.telemetry())?;

        #[cfg(feature = "body-capture")]
        let body_capture =
            BodyCaptureEngine::new(&config.limits, &config.redaction, queue.telemetry())?;

        #[cfg(feature = "geolocation")]
        let geolocation = config
            .geolocation
            .as_ref()
            .map(|config| GeolocationEngine::open(config, queue.telemetry()))
            .transpose()?;
        Ok(Self {
            queue,
            policies,
            config,
            route_resolver,
            error_classifier: ServerErrorClassifier,
            metadata,
            #[cfg(feature = "body-capture")]
            body_capture,
            #[cfg(feature = "geolocation")]
            geolocation,
        })
    }

    /// Installs a typed error classifier, for example for a timeout layer.
    pub fn with_error_classifier<C>(self, error_classifier: C) -> HttpTelemetryLayer<R, C> {
        HttpTelemetryLayer {
            queue: self.queue,
            policies: self.policies,
            config: self.config,
            route_resolver: self.route_resolver,
            error_classifier,
            metadata: self.metadata,
            #[cfg(feature = "body-capture")]
            body_capture: self.body_capture,
            #[cfg(feature = "geolocation")]
            geolocation: self.geolocation.clone(),
        }
    }
}

impl<S, R: Clone, C: Clone> Layer<S> for HttpTelemetryLayer<R, C> {
    type Service = HttpTelemetry<S, R, C>;

    fn layer(&self, inner: S) -> Self::Service {
        HttpTelemetry {
            inner,
            queue: self.queue.clone(),
            policies: self.policies.clone(),
            config: self.config.clone(),
            route_resolver: self.route_resolver.clone(),
            error_classifier: self.error_classifier.clone(),
            metadata: self.metadata.clone(),
            #[cfg(feature = "body-capture")]
            body_capture: self.body_capture.clone(),
            telemetry: self.queue.telemetry(),
            #[cfg(feature = "geolocation")]
            geolocation: self.geolocation.clone(),
        }
    }
}

/// Tower service produced by [`HttpTelemetryLayer`].
#[derive(Clone)]
pub struct HttpTelemetry<S, R = UnmatchedRoute, C = ServerErrorClassifier> {
    inner: S,
    queue: BoundedEventQueue,
    policies: RoutePolicySet,
    config: MiddlewareConfig,
    route_resolver: R,
    error_classifier: C,
    metadata: MetadataCapturer,
    telemetry: InternalTelemetry,
    #[cfg(feature = "body-capture")]
    body_capture: BodyCaptureEngine,
    #[cfg(feature = "geolocation")]
    geolocation: Option<GeolocationEngine>,
}

impl<S, R, C, RequestBody, ResponseBody> Service<Request<RequestBody>> for HttpTelemetry<S, R, C>
where
    S: Service<Request<RequestBody>, Response = Response<ResponseBody>>,
    R: RouteResolver,
    ResponseBody: http_body::Body,
    C: ErrorClassifier<S::Error>,
{
    type Response = Response<ObservedBody<ResponseBody>>;
    type Error = S::Error;
    type Future = HttpTelemetryResponseFuture<S::Future, C>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, request: Request<RequestBody>) -> Self::Future {
        let policy = self
            .policies
            .resolve(request.method().as_str(), request.uri().path());

        if policy.excluded {
            self.telemetry.record_request_excluded();
            return HttpTelemetryResponseFuture::excluded(
                self.inner.call(request),
                self.error_classifier.clone(),
            );
        }

        self.telemetry.record_request_started();

        let max_bytes = self.metadata.limits().max_string_bytes;
        let method = bounded_value(request.method().as_str(), max_bytes, &self.telemetry);
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
            trace_id = field::Empty,
            span_id = field::Empty,
            status_code = field::Empty,
            duration_ms = field::Empty,
            outcome = field::Empty,
        );

        #[cfg(feature = "opentelemetry")]
        let (trace_id, span_id) = {
            if let Some(parent) = extract_trace_context(request.headers()) {
                // The span has just been created, so its parent can still be assigned.
                // If no OpenTelemetry layer is installed, correlation remains absent.
                let _ = span.set_parent(parent);
            }

            match TraceCorrelation::from_span(&span) {
                Some(correlation) => (Some(correlation.trace_id), Some(correlation.span_id)),
                None => (None, None),
            }
        };

        #[cfg(not(feature = "opentelemetry"))]
        let (trace_id, span_id): (Option<String>, Option<String>) = (None, None);

        if let Some(trace_id) = trace_id.as_deref() {
            span.record("trace_id", trace_id);
        }

        if let Some(span_id) = span_id.as_deref() {
            span.record("span_id", span_id);
        }

        let query = if policy.capture_query {
            self.metadata.capture_query(request.uri().query())
        } else {
            BTreeMap::new()
        };

        let headers = if policy.capture_request_headers {
            self.metadata.capture_headers(request.headers())
        } else {
            BTreeMap::new()
        };
        // Raw paths can contain credentials or reset tokens. Keep the schema
        // field but omit its value; the normalized route is recorded separately.
        let mut request_info =
            RequestInfo::new(method.clone(), route.clone(), crate::redaction::OMITTED);
        request_info.query = query;
        request_info.headers = headers;

        #[cfg(feature = "geolocation")]
        let geolocation_enabled = self.geolocation.is_some();
        #[cfg(not(feature = "geolocation"))]
        let geolocation_enabled = false;

        if policy.profile.allows_geolocation()
            && (self.config.capture_client_ip || geolocation_enabled)
        {
            let address = client_ip::resolve(&request, &self.config.client_ip);
            request_info.client_ip = address.map(|address| address.to_string());

            #[cfg(feature = "geolocation")]
            if let (Some(engine), Some(address)) = (&self.geolocation, address) {
                request_info.geolocation = engine.lookup(address);
            }
        }

        #[cfg(feature = "body-capture")]
        let request_body = request
            .extensions()
            .get::<RequestBodyCaptureState>()
            .cloned();

        let started_at = Instant::now();
        let inner = span.in_scope(|| self.inner.call(request));
        let context = RequestContext {
            request_id,
            trace_id,
            span_id,
            request: request_info,
            response_headers: BTreeMap::new(),
            capture_response_headers: policy.capture_response_headers,
            metadata: self.metadata.clone(),
            started_at,
            queue: self.queue.clone(),
            telemetry: self.telemetry.clone(),
            span: span.clone(),
            terminal: false,
            emit_cancellation_events: self.config.emit_cancellation_events,
            #[cfg(feature = "body-capture")]
            body_capture: self.body_capture.clone(),
            #[cfg(feature = "body-capture")]
            request_body,
            #[cfg(feature = "body-capture")]
            capture_response_body: policy.capture_response_body,
            #[cfg(feature = "body-capture")]
            response_body_limit: self.config.limits.max_response_body_bytes,
            #[cfg(feature = "body-capture")]
            response_header_limit: self.config.limits.max_header_value_bytes,
            #[cfg(feature = "body-capture")]
            response_body: None,
        };

        HttpTelemetryResponseFuture::observed(inner, context, span, self.error_classifier.clone())
    }
}

pin_project! {
    /// Response future carrying no public HTTP-data `Debug` implementation.
    pub struct HttpTelemetryResponseFuture<F, C> {
        #[pin]
        inner: F,
        context: Option<RequestContext>,
        span: Span,
        error_classifier: C,
    }
}

impl<F, C> HttpTelemetryResponseFuture<F, C> {
    fn observed(inner: F, context: RequestContext, span: Span, error_classifier: C) -> Self {
        Self {
            inner,
            context: Some(context),
            span,
            error_classifier,
        }
    }

    fn excluded(inner: F, error_classifier: C) -> Self {
        Self {
            inner,
            context: None,
            span: Span::none(),
            error_classifier,
        }
    }
}

impl<F, Body, Error, C> Future for HttpTelemetryResponseFuture<F, C>
where
    F: Future<Output = Result<Response<Body>, Error>>,
    Body: http_body::Body,
    C: ErrorClassifier<Error>,
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
                        let mut request = request;
                        request.capture_response_headers(&parts.headers);

                        let body_is_end_stream = body.is_end_stream();

                        #[cfg(feature = "body-capture")]
                        request.prepare_response_body(&parts.headers, body_is_end_stream);

                        let response_received_at = Instant::now();
                        let time_to_first_byte_ms =
                            request.time_to_first_byte_ms(response_received_at);

                        if body_is_end_stream {
                            request.complete_body(&*this.span, status, time_to_first_byte_ms, 0.0);

                            Response::from_parts(parts, ObservedBody::finished(body))
                        } else {
                            #[cfg(feature = "body-capture")]
                            let response_body = request.response_body.clone();

                            let observed = ObservedBody::observed(
                                body,
                                request,
                                Span::clone(&*this.span),
                                status,
                                response_received_at,
                                time_to_first_byte_ms,
                                #[cfg(feature = "body-capture")]
                                response_body,
                            );

                            Response::from_parts(parts, observed)
                        }
                    }
                    None => Response::from_parts(parts, ObservedBody::excluded(body)),
                };

                Poll::Ready(Ok(response))
            }
            Poll::Ready(Err(error)) => {
                if let Some(request) = this.context.take() {
                    let failure = this.error_classifier.classify(&error);
                    request.complete_failure(&*this.span, failure);
                }

                Poll::Ready(Err(error))
            }
        }
    }
}

pub(crate) struct RequestContext {
    request_id: String,
    trace_id: Option<String>,
    span_id: Option<String>,
    request: RequestInfo,
    response_headers: BTreeMap<String, String>,
    capture_response_headers: bool,
    metadata: MetadataCapturer,
    started_at: Instant,
    queue: BoundedEventQueue,
    telemetry: InternalTelemetry,
    span: Span,
    terminal: bool,
    emit_cancellation_events: bool,
    #[cfg(feature = "body-capture")]
    body_capture: BodyCaptureEngine,
    #[cfg(feature = "body-capture")]
    request_body: Option<RequestBodyCaptureState>,
    #[cfg(feature = "body-capture")]
    capture_response_body: bool,
    #[cfg(feature = "body-capture")]
    response_body_limit: usize,
    #[cfg(feature = "body-capture")]
    response_header_limit: usize,
    #[cfg(feature = "body-capture")]
    response_body: Option<ResponseBodyCaptureState>,
}

impl RequestContext {
    fn capture_response_headers(&mut self, headers: &http::HeaderMap) {
        if self.capture_response_headers {
            self.response_headers = self.metadata.capture_headers(headers);
        }
    }

    #[cfg(feature = "body-capture")]
    fn prepare_response_body(&mut self, headers: &http::HeaderMap, ended: bool) {
        if !self.capture_response_body {
            return;
        }

        let capture = ResponseBodyCaptureState::new(
            headers,
            self.response_header_limit,
            self.response_body_limit,
            &self.telemetry,
        );

        if ended {
            capture.end();
        }

        self.response_body = Some(capture);
    }

    pub(crate) fn time_to_first_byte_ms(&self, response_received_at: Instant) -> f64 {
        response_received_at
            .saturating_duration_since(self.started_at)
            .as_secs_f64()
            * 1_000.0
    }

    pub(crate) fn complete_body(
        mut self,
        span: &Span,
        status: StatusCode,
        time_to_first_byte_ms: f64,
        body_duration_ms: f64,
    ) {
        self.terminal = true;
        let event = self.build_event(
            Some(status.as_u16()),
            ResponseOutcome::from_status(status.as_u16()),
            Some(time_to_first_byte_ms),
            Some(body_duration_ms),
        );

        self.telemetry.record_request_completed();
        emit_terminal_event(span, event, &self.queue);
    }

    fn complete_failure(mut self, span: &Span, failure: RequestFailure) {
        self.terminal = true;
        let outcome = match failure {
            RequestFailure::ServerError => ResponseOutcome::ServerError,
            RequestFailure::Timeout => ResponseOutcome::Timeout,
        };
        let event = self.build_event(None, outcome, None, None);

        match failure {
            RequestFailure::ServerError => self.telemetry.record_request_failed(),
            RequestFailure::Timeout => self.telemetry.record_request_timed_out(),
        }

        emit_terminal_event(span, event, &self.queue);
    }

    pub(crate) fn fail_body(
        mut self,
        span: &Span,
        status: StatusCode,
        time_to_first_byte_ms: f64,
        body_duration_ms: f64,
    ) {
        self.terminal = true;
        let event = self.build_event(
            Some(status.as_u16()),
            ResponseOutcome::ServerError,
            Some(time_to_first_byte_ms),
            Some(body_duration_ms),
        );

        self.telemetry.record_request_failed();
        emit_terminal_event(span, event, &self.queue);
    }

    pub(crate) fn cancel_body(
        mut self,
        status: StatusCode,
        time_to_first_byte_ms: f64,
        body_duration_ms: f64,
    ) {
        self.terminal = true;
        self.telemetry.record_request_cancelled();

        if self.emit_cancellation_events {
            let event = self.build_event(
                Some(status.as_u16()),
                ResponseOutcome::Cancelled,
                Some(time_to_first_byte_ms),
                Some(body_duration_ms),
            );

            emit_terminal_event(&self.span, event, &self.queue);
        }
    }

    fn build_event(
        &self,
        status_code: Option<u16>,
        outcome: ResponseOutcome,
        time_to_first_byte_ms: Option<f64>,
        body_duration_ms: Option<f64>,
    ) -> HttpEvent {
        let request = self.request.clone();

        #[cfg(feature = "body-capture")]
        let mut request = request;

        let mut response = ResponseInfo::terminal(status_code, outcome);
        response.headers = self.response_headers.clone();

        let capture = CaptureInfo::default();

        #[cfg(feature = "body-capture")]
        let mut capture = capture;

        #[cfg(feature = "body-capture")]
        if let Some(state) = &self.request_body {
            let captured = state.finish();
            request.content_type = captured.content_type.clone();
            request.content_length = captured.content_length;

            if captured.failed {
                // A partial prefix from a failed stream is never exported.
                capture.request_body_status =
                    Some(crate::core::event::BodyCaptureStatus::ParseError);
                capture.request_body_truncated = true;
                self.telemetry.record_parse_error();
            } else if captured.bytes.is_empty() && !captured.ended {
                // The application did not consume the request stream.
                capture.request_body_status = Some(crate::core::event::BodyCaptureStatus::Omitted);
                self.telemetry.record_body_omission();
            } else {
                let outcome = self.body_capture.capture(
                    crate::body_capture::BodySource::Request,
                    captured.content_type.as_deref(),
                    captured.content_encoding.as_deref(),
                    &captured.bytes,
                    captured.truncated || !captured.ended,
                );

                request.body = outcome.body;
                capture.request_body_status = Some(outcome.status);
                capture.request_body_truncated = outcome.truncated;
            }
        }

        #[cfg(feature = "body-capture")]
        if let Some(state) = &self.response_body {
            let captured = state.finish();
            response.content_type = captured.content_type.clone();
            response.content_length = captured.content_length;

            if captured.failed {
                capture.response_body_status =
                    Some(crate::core::event::BodyCaptureStatus::ParseError);
                capture.response_body_truncated = true;
                self.telemetry.record_parse_error();
            } else if captured.bytes.is_empty() && !captured.ended {
                capture.response_body_status = Some(crate::core::event::BodyCaptureStatus::Omitted);
                self.telemetry.record_body_omission();
            } else {
                let outcome = self.body_capture.capture(
                    crate::body_capture::BodySource::Response,
                    captured.content_type.as_deref(),
                    captured.content_encoding.as_deref(),
                    &captured.bytes,
                    captured.truncated || !captured.ended,
                );

                response.body = outcome.body;
                capture.response_body_status = Some(outcome.status);
                capture.response_body_truncated = outcome.truncated;
            }
        }

        let mut timing = TimingInfo::new(self.started_at.elapsed().as_secs_f64() * 1_000.0);
        #[cfg(feature = "body-capture")]
        self.telemetry.record_body_capture_outcomes(&capture);
        timing.time_to_first_byte_ms = time_to_first_byte_ms;
        timing.body_duration_ms = body_duration_ms;

        let mut event = HttpEvent::new(self.request_id.clone(), request, response, timing, capture);
        event.trace_id = self.trace_id.clone();
        event.span_id = self.span_id.clone();

        event
    }
}

impl Drop for RequestContext {
    fn drop(&mut self) {
        if self.terminal {
            return;
        }

        self.telemetry.record_request_cancelled();

        if self.emit_cancellation_events {
            let event = self.build_event(None, ResponseOutcome::Cancelled, None, None);
            emit_terminal_event(&self.span, event, &self.queue);
        }
    }
}

fn emit_terminal_event(span: &Span, event: HttpEvent, queue: &BoundedEventQueue) {
    let status_code = event.response.status_code;
    let duration_ms = event.timing.duration_ms;
    let outcome = outcome_name(&event);

    if let Some(status_code) = status_code {
        span.record("status_code", u64::from(status_code));
    }
    span.record("duration_ms", duration_ms);
    span.record("outcome", outcome);

    match event.level {
        LogLevel::Error => {
            tracing::event!(
                parent: span,
                TracingLevel::ERROR,
                request_id = %event.request_id,
                trace_id = field::display(event.trace_id.as_deref().unwrap_or("")),
                span_id = field::display(event.span_id.as_deref().unwrap_or("")),
                method = %event.request.method,
                route = %event.request.route,
                status_code = field::debug(status_code),
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
                trace_id = field::display(event.trace_id.as_deref().unwrap_or("")),
                span_id = field::display(event.span_id.as_deref().unwrap_or("")),
                method = %event.request.method,
                route = %event.request.route,
                status_code = field::debug(status_code),
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
                trace_id = field::display(event.trace_id.as_deref().unwrap_or("")),
                span_id = field::display(event.span_id.as_deref().unwrap_or("")),
                method = %event.request.method,
                route = %event.request.route,
                status_code = field::debug(status_code),
                duration_ms,
                outcome,
                "HTTP request completed"
            );
        }
    }

    let _ = queue.try_push(event);
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
