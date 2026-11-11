//! Native `tracing` application-event layer.
//!
//! Events are converted to bounded JSON fields, redacted and submitted to the
//! existing non-blocking queue. No network operation is performed here.

use std::{collections::BTreeMap, fmt};

use serde_json::{Number, Value};
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id},
    Event, Subscriber,
};

#[cfg(feature = "opentelemetry")]
use tracing::Span;

use tracing_subscriber::{
    layer::{Context, Layer},
    registry::LookupSpan,
};

use crate::{
    config::ConfigError,
    core::event::{BusinessEvent, LogLevel},
    redaction::{Redactor, UNSUPPORTED},
    runtime::queue::BoundedEventQueue,
};

#[cfg(feature = "opentelemetry")]
use crate::tracing_integration::propagation::TraceCorrelation;

const INTERNAL_TARGET: &str = "safe_http_telemetry";

/// Limits and source metadata options for application events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusinessLogConfig {
    /// Maximum number of custom JSON fields retained per event.
    pub max_fields: usize,

    /// Includes the Rust module path when available.
    pub include_module_path: bool,
}

impl Default for BusinessLogConfig {
    fn default() -> Self {
        Self {
            max_fields: 64,
            include_module_path: false,
        }
    }
}

/// A native `tracing` layer writing redacted business events to the queue.
///
/// This type contains application-data producers and does not implement
/// `Debug`. Apply a `tracing_subscriber` filter to control enabled targets and
/// levels before events reach this layer.
pub struct BusinessLogLayer {
    queue: BoundedEventQueue,
    redactor: Redactor,
    config: BusinessLogConfig,
}

impl BusinessLogLayer {
    pub fn new(
        queue: BoundedEventQueue,
        redactor: Redactor,
        config: BusinessLogConfig,
    ) -> Result<Self, ConfigError> {
        if config.max_fields == 0 {
            return Err(ConfigError::ZeroLimit("business_log.max_fields"));
        }

        Ok(Self {
            queue,
            redactor,
            config,
        })
    }
}

impl<S> Layer<S> for BusinessLogLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        let mut visitor = RequestIdVisitor::default();
        attributes.record(&mut visitor);

        let Some(request_id) = visitor.request_id else {
            return;
        };

        if let Some(span) = context.span(id) {
            span.extensions_mut().insert(SpanRequestId(request_id));
        }
    }

    #[cfg(feature = "opentelemetry")]
    fn on_enter(&self, id: &Id, context: Context<'_, S>) {
        let Some(correlation) = TraceCorrelation::from_span(&Span::current()) else {
            return;
        };

        if let Some(span) = context.span(id) {
            let _ = span.extensions_mut().replace(SpanCorrelation(correlation));
        }
    }

    fn on_event(&self, event: &Event<'_>, context: Context<'_, S>) {
        let metadata = event.metadata();

        if is_internal_target(metadata.target()) {
            return;
        }

        let mut visitor = BusinessEventVisitor::new(self.config.max_fields);
        event.record(&mut visitor);

        let mut business_event = BusinessEvent::new(
            convert_level(metadata.level()),
            visitor.message,
            metadata.target(),
        );

        // The span value wins over an event field so that one HTTP request
        // cannot accidentally produce conflicting request IDs.
        business_event.request_id = request_id_from_scope(&context, event).or(visitor.request_id);
        business_event.module_path = if self.config.include_module_path {
            metadata.module_path().map(str::to_owned)
        } else {
            None
        };
        business_event.fields = visitor.fields;
        business_event.fields_truncated = visitor.fields_truncated;

        #[cfg(feature = "opentelemetry")]
        if let Some(correlation) = correlation_from_scope(&context, event)
            .or_else(|| TraceCorrelation::from_span(&Span::current()))
        {
            business_event.trace_id = Some(correlation.trace_id);
            business_event.span_id = Some(correlation.span_id);
        }

        self.redactor.redact_business_event(&mut business_event);

        // Queue failures are accounted for by its telemetry counters. They
        // must not be logged back into this layer recursively.
        let _ = self.queue.try_push(business_event);
    }
}

#[derive(Clone)]
struct SpanRequestId(String);

#[cfg(feature = "opentelemetry")]
#[derive(Clone)]
struct SpanCorrelation(TraceCorrelation);

#[derive(Default)]
struct RequestIdVisitor {
    request_id: Option<String>,
}

impl Visit for RequestIdVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "request_id" {
            self.request_id = Some(value.to_owned());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "request_id" {
            self.request_id = Some(format!("{value:?}"));
        }
    }
}

struct BusinessEventVisitor {
    message: Option<String>,
    request_id: Option<String>,
    fields: BTreeMap<String, Value>,
    max_fields: usize,
    fields_truncated: bool,
}

impl BusinessEventVisitor {
    fn new(max_fields: usize) -> Self {
        Self {
            message: None,
            request_id: None,
            fields: BTreeMap::new(),
            max_fields,
            fields_truncated: false,
        }
    }

    fn insert(&mut self, name: &str, value: Value) {
        match name {
            "trace_id" | "span_id" => {
                // Correlation is obtained from OpenTelemetry, not trusted
                // from arbitrary application fields.
            }
            _ if self.fields.contains_key(name) || self.fields.len() < self.max_fields => {
                self.fields.insert(name.to_owned(), value);
            }
            _ => {
                self.fields_truncated = true;
            }
        }
    }

    fn record_text(&mut self, field: &Field, value: String) {
        match field.name() {
            "message" => self.message = Some(value),
            "request_id" => self.request_id = Some(value),
            name => self.insert(name, Value::String(value)),
        }
    }
}

impl Visit for BusinessEventVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        let value = Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null);
        self.insert(field.name(), value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field.name(), Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field.name(), Value::from(value));
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        match i64::try_from(value) {
            Ok(value) => self.insert(field.name(), Value::from(value)),
            Err(_) => self.insert(field.name(), Value::String(value.to_string())),
        }
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        match u64::try_from(value) {
            Ok(value) => self.insert(field.name(), Value::from(value)),
            Err(_) => self.insert(field.name(), Value::String(value.to_string())),
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field.name(), Value::from(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_text(field, value.to_owned());
    }

    fn record_bytes(&mut self, field: &Field, _value: &[u8]) {
        self.insert(field.name(), Value::String(UNSUPPORTED.to_owned()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record_text(field, format!("{value:?}"));
    }
}

fn request_id_from_scope<S>(context: &Context<'_, S>, event: &Event<'_>) -> Option<String>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    let scope = context.event_scope(event)?;

    for span in scope {
        let extensions = span.extensions();

        if let Some(request_id) = extensions.get::<SpanRequestId>() {
            return Some(request_id.0.clone());
        }
    }

    None
}

#[cfg(feature = "opentelemetry")]
fn correlation_from_scope<S>(
    context: &Context<'_, S>,
    event: &Event<'_>,
) -> Option<TraceCorrelation>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    let scope = context.event_scope(event)?;

    for span in scope {
        let extensions = span.extensions();

        if let Some(correlation) = extensions.get::<SpanCorrelation>() {
            return Some(correlation.0.clone());
        }
    }

    None
}

fn convert_level(level: &tracing::Level) -> LogLevel {
    match *level {
        tracing::Level::TRACE => LogLevel::Trace,
        tracing::Level::DEBUG => LogLevel::Debug,
        tracing::Level::INFO => LogLevel::Info,
        tracing::Level::WARN => LogLevel::Warn,
        tracing::Level::ERROR => LogLevel::Error,
    }
}

fn is_internal_target(target: &str) -> bool {
    target == INTERNAL_TARGET
        || target
            .strip_prefix(INTERNAL_TARGET)
            .is_some_and(|suffix| suffix.starts_with("::"))
}
