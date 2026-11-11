//! W3C Trace Context propagation and OpenTelemetry span correlation.

use http::{HeaderMap, HeaderName, HeaderValue};
use opentelemetry::{
    propagation::{Extractor, Injector, TextMapPropagator},
    trace::TraceContextExt,
    Context,
};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Trace and span identifiers associated with the current OpenTelemetry span.
///
/// This type intentionally does not implement `Debug`: identifiers originate
/// from HTTP processing and must not be accidentally exposed through generic
/// diagnostics.
#[derive(Clone, PartialEq, Eq)]
pub struct TraceCorrelation {
    pub trace_id: String,
    pub span_id: String,
}

impl TraceCorrelation {
    /// Returns valid identifiers from the OpenTelemetry context of a span.
    ///
    /// Returns `None` when no OpenTelemetry tracing layer is installed or when
    /// the span has no valid OpenTelemetry span context.
    pub fn from_span(span: &Span) -> Option<Self> {
        let context = span.context();
        let span_context = context.span().span_context().clone();

        if !span_context.is_valid() {
            return None;
        }

        Some(Self {
            trace_id: span_context.trace_id().to_string(),
            span_id: span_context.span_id().to_string(),
        })
    }
}

/// Extracts a valid remote W3C Trace Context from HTTP headers.
///
/// Missing, malformed and invalid contexts are ignored. Header values and
/// parsing errors are never included in diagnostics.
pub fn extract_trace_context(headers: &HeaderMap) -> Option<Context> {
    let context = TraceContextPropagator::new().extract(&HeaderExtractor::new(headers));

    if context.span().span_context().is_valid() {
        Some(context)
    } else {
        None
    }
}

/// Injects an OpenTelemetry context into HTTP headers using W3C Trace Context.
///
/// This is intended for outgoing client requests. The middleware does not add
/// `traceparent` to application responses.
pub fn inject_trace_context(context: &Context, headers: &mut HeaderMap) {
    TraceContextPropagator::new().inject_context(context, &mut HeaderInjector { headers });
}

struct HeaderExtractor<'a> {
    headers: &'a HeaderMap,
}

impl<'a> HeaderExtractor<'a> {
    fn new(headers: &'a HeaderMap) -> Self {
        Self { headers }
    }
}

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(key)?.to_str().ok()
    }

    fn keys(&self) -> Vec<&str> {
        self.headers.keys().map(HeaderName::as_str).collect()
    }
}

struct HeaderInjector<'a> {
    headers: &'a mut HeaderMap,
}

impl Injector for HeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        let Ok(name) = HeaderName::from_bytes(key.as_bytes()) else {
            return;
        };
        let Ok(value) = HeaderValue::from_str(&value) else {
            return;
        };

        self.headers.insert(name, value);
    }
}
