//! Bounded multi-producer, multi-consumer queue for redacted HTTP events.
//!
//! Producers use [`BoundedEventQueue::try_push`], which never waits for queue
//! space. When the queue is full, the incoming event is dropped according to
//! the explicit `drop_newest` policy.

use std::{error::Error, fmt};

use async_channel::{bounded, Receiver, Sender, TrySendError};

use crate::core::{event::TelemetryEvent, telemetry::InternalTelemetry};

/// Behavior applied when the bounded queue has no remaining capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaturationPolicy {
    /// Reject and drop the incoming event while retaining queued events.
    DropNewest,
}

/// Configuration for a bounded event queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueConfig {
    /// Maximum number of events waiting for workers.
    pub capacity: usize,
    pub saturation_policy: SaturationPolicy,
}

impl QueueConfig {
    pub const fn new(capacity: usize) -> Self {
        Self {
            capacity,
            saturation_policy: SaturationPolicy::DropNewest,
        }
    }
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self::new(1_024)
    }
}

/// Invalid bounded-queue configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueConfigError {
    ZeroCapacity,
}

impl fmt::Display for QueueConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => formatter.write_str("queue capacity must be greater than zero"),
        }
    }
}

impl Error for QueueConfigError {}

/// Non-blocking push failure that deliberately contains no event data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TryPushError {
    /// The queue is full and the incoming event was dropped.
    Full,
    /// The queue is closed and no further events can be accepted
    Closed,
}

impl fmt::Display for TryPushError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => formatter.write_str("the event queue is full; incoming event dropped"),
            Self::Closed => formatter.write_str("the event queue is closed"),
        }
    }
}

impl Error for TryPushError {}

/// Operational queue statistics containing no HTTP event data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueStats {
    pub accepted_events: u64,
    pub rejected_events: u64,
    pub dropped_events: u64,
    pub occupancy: usize,
    pub capacity: usize,
    pub closed: bool,
    pub saturation_policy: SaturationPolicy,
}

/// A cloneable producer handle for a bounded asynchronous event queue.
///
/// Cloned handles share the queue and telemetry counters. This type does not
/// implement `Debug`, because its channel contains HTTP events.
#[derive(Clone)]
pub struct BoundedEventQueue {
    sender: Sender<TelemetryEvent>,
    receiver: Receiver<TelemetryEvent>,
    config: QueueConfig,
    telemetry: InternalTelemetry,
}

impl BoundedEventQueue {
    /// Creates a bounded queue. No network worker is started by this method.
    pub fn new(
        config: QueueConfig,
        telemetry: InternalTelemetry,
    ) -> Result<Self, QueueConfigError> {
        if config.capacity == 0 {
            return Err(QueueConfigError::ZeroCapacity);
        }

        let (sender, receiver) = bounded(config.capacity);

        Ok(Self {
            sender,
            receiver,
            config,
            telemetry,
        })
    }

    /// Attempts to enqueue an already-redacted event without blocking.
    ///
    /// Under `drop_newest`, a full queue rejects the incoming event. Closing
    /// the queue rejects subsequent pushes but does not discard queued events.
    pub fn try_push(&self, event: impl Into<TelemetryEvent>) -> Result<(), TryPushError> {
        let event = event.into();

        match self.config.saturation_policy {
            SaturationPolicy::DropNewest => match self.sender.try_send(event) {
                Ok(()) => {
                    self.telemetry.record_event_accepted();
                    Ok(())
                }
                Err(TrySendError::Full(event)) => {
                    drop(event);
                    self.telemetry.record_event_rejected();
                    self.telemetry.record_event_dropped();
                    Err(TryPushError::Full)
                }
                Err(TrySendError::Closed(event)) => {
                    drop(event);
                    self.telemetry.record_event_rejected();
                    Err(TryPushError::Closed)
                }
            },
        }
    }

    /// Returns a cloneable consumer handle suitable for one asynchronous worker.
    pub fn receiver(&self) -> EventQueueReceiver {
        EventQueueReceiver {
            receiver: self.receiver.clone(),
        }
    }

    /// Closes the queue for new events while preserving events already queued.
    ///
    /// Returns `true` only for the call that changes the queue from open to
    /// closed. Once drained, all receivers return `None`.
    pub fn close(&self) -> bool {
        self.sender.close()
    }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }

    /// Returns the shared low-cardinality telemetry handle.
    pub fn telemetry(&self) -> InternalTelemetry {
        self.telemetry.clone()
    }

    /// Returns an operational snapshot with no HTTP request or response data.
    pub fn stats(&self) -> QueueStats {
        let telemetry = self.telemetry.snapshot();

        QueueStats {
            accepted_events: telemetry.accepted_events,
            rejected_events: telemetry.rejected_events,
            dropped_events: telemetry.dropped_events,
            occupancy: self.sender.len(),
            capacity: self.config.capacity,
            closed: self.sender.is_closed(),
            saturation_policy: self.config.saturation_policy,
        }
    }
}

/// A cloneable asynchronous consumer handle.
///
/// Each event is delivered to exactly one receiver. This type intentionally
/// does not implement `Debug`, because received values contain HTTP data.
#[derive(Clone)]
pub struct EventQueueReceiver {
    receiver: Receiver<TelemetryEvent>,
}

impl EventQueueReceiver {
    /// Waits for the next event without blocking an executor thread.
    ///
    /// Returns `None` after the queue is closed and all queued events have
    /// been consumed.
    pub async fn recv(&self) -> Option<TelemetryEvent> {
        self.receiver.recv().await.ok()
    }

    /// Returns one immediately available event, or `None` when empty or closed.
    pub fn try_recv(&self) -> Option<TelemetryEvent> {
        self.receiver.try_recv().ok()
    }
}
