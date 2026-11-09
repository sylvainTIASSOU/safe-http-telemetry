//! Bounded Loki batch construction.
//!
//! Both the event count and the serialized JSON payload size are checked before
//! an event is accepted into a batch.

use std::{error::Error, fmt};

use crate::{config::ServiceConfig, core::event::HttpEvent};

use super::{
    config::BatchConfig,
    payload::{LokiPushRequest, LokiStream},
};

/// Result of attempting to add one event to a batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchPush {
    /// The event was added to the current batch.
    Accepted,
    /// The event did not fit and must be retried against an empty batch after
    /// the current batch is flushed.
    Full,
    /// One event cannot fit in an otherwise empty batch.
    Oversized,
}

/// A fully bounded batch ready for one Loki push operation.
#[derive(Clone)]
pub struct LokiBatch {
    payload: LokiPushRequest,
    event_count: usize,
    size_bytes: usize,
}

impl LokiBatch {
    pub fn payload(&self) -> &LokiPushRequest {
        &self.payload
    }

    pub const fn event_count(&self) -> usize {
        self.event_count
    }

    pub const fn size_bytes(&self) -> usize {
        self.size_bytes
    }
}

/// Incremental builder for one bounded Loki payload.
pub struct LokiBatchBuilder {
    config: BatchConfig,
    service: ServiceConfig,
    streams: Vec<LokiStream>,
    event_count: usize,
    size_bytes: usize,
}

impl LokiBatchBuilder {
    pub fn new(config: BatchConfig, service: ServiceConfig) -> Result<Self, LokiBatchError> {
        if config.max_events == 0 {
            return Err(LokiBatchError::InvalidConfiguration("batch.max_events"));
        }
        if config.max_bytes == 0 {
            return Err(LokiBatchError::InvalidConfiguration("batch.max_bytes"));
        }
        if config.flush_interval.is_zero() {
            return Err(LokiBatchError::InvalidConfiguration("batch.flush_interval"));
        }

        Ok(Self {
            config,
            service,
            streams: Vec::new(),
            event_count: 0,
            size_bytes: 0,
        })
    }

    /// Attempts to add an already-redacted event without exceeding either limit.
    pub fn try_push(
        &mut self,
        event: &HttpEvent,
        timestamp_ns: u128,
    ) -> Result<BatchPush, LokiBatchError> {
        if self.event_count >= self.config.max_events {
            return Ok(BatchPush::Full);
        }

        let stream = LokiStream::from_event(event, &self.service, timestamp_ns)
            .map_err(|_| LokiBatchError::Serialization)?;

        let mut candidate_streams = self.streams.clone();
        candidate_streams.push(stream);
        let candidate = LokiPushRequest::new(candidate_streams);
        let candidate_size = candidate
            .to_json_bytes()
            .map_err(|_| LokiBatchError::Serialization)?
            .len();

        if candidate_size > self.config.max_bytes {
            return if self.streams.is_empty() {
                Ok(BatchPush::Oversized)
            } else {
                Ok(BatchPush::Full)
            };
        }

        self.streams = candidate.streams;
        self.event_count += 1;
        self.size_bytes = candidate_size;

        Ok(BatchPush::Accepted)
    }

    pub fn is_empty(&self) -> bool {
        self.streams.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.event_count >= self.config.max_events || self.size_bytes >= self.config.max_bytes
    }

    pub const fn event_count(&self) -> usize {
        self.event_count
    }

    pub const fn size_bytes(&self) -> usize {
        self.size_bytes
    }

    /// Removes and returns the current batch, leaving this builder empty.
    pub fn take(&mut self) -> Option<LokiBatch> {
        if self.streams.is_empty() {
            return None;
        }

        let payload = LokiPushRequest::new(std::mem::take(&mut self.streams));
        let event_count = self.event_count;
        let size_bytes = self.size_bytes;
        self.event_count = 0;
        self.size_bytes = 0;

        Some(LokiBatch {
            payload,
            event_count,
            size_bytes,
        })
    }
}

/// A batch construction failure containing no HTTP event data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LokiBatchError {
    InvalidConfiguration(&'static str),
    Serialization,
}

impl fmt::Display for LokiBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(field) => {
                write!(formatter, "Loki batch setting `{field}` is invalid")
            }
            Self::Serialization => formatter.write_str("failed to serialize a Loki batch"),
        }
    }
}

impl Error for LokiBatchError {}
