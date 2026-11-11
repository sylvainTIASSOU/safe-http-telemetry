//! Fixed-worker asynchronous Loki exporter with bounded retries.
//!
//! Producers only interact with the bounded queue. Network operations, batch
//! flushing and retry delays are performed exclusively by export workers.

use std::{
    error::Error,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::{
    task::JoinHandle,
    time::{timeout, Instant},
};

use crate::{
    config::ServiceConfig,
    core::{event::HttpEvent, telemetry::InternalTelemetry},
    runtime::queue::{BoundedEventQueue, EventQueueReceiver},
};

use super::{
    batch::{BatchPush, LokiBatchBuilder, LokiBatchError},
    client::{LokiClient, LokiClientError},
    config::{LokiConfig, RetryConfig, ShutdownConfig},
    payload::LokiPushRequest,
};

pub const MAX_WORKER_COUNT: usize = 64;

/// Configuration for an exporter that has not started its workers yet.
pub struct LokiExporter {
    queue: BoundedEventQueue,
    client: LokiClient,
    service: ServiceConfig,
    config: LokiConfig,
    worker_count: usize,
    telemetry: InternalTelemetry,
}

impl LokiExporter {
    pub fn new(
        config: LokiConfig,
        service: ServiceConfig,
        queue: BoundedEventQueue,
        worker_count: usize,
    ) -> Result<Self, LokiExporterError> {
        if worker_count == 0 || worker_count > MAX_WORKER_COUNT {
            return Err(LokiExporterError::InvalidWorkerCount);
        }

        LokiBatchBuilder::new(config.batch.clone(), service.clone())
            .map_err(LokiExporterError::BatchConfiguration)?;
        let client = LokiClient::new(config.clone()).map_err(LokiExporterError::Client)?;
        let telemetry = queue.telemetry();

        Ok(Self {
            queue,
            client,
            service,
            config,
            worker_count,
            telemetry,
        })
    }

    /// Starts exactly the configured number of workers.
    ///
    /// Keep or clone the queue before calling this method to continue producing
    /// events. The returned handle can be joined after the queue is closed.
    pub fn start(self) -> LokiExporterHandle {
        let Self {
            queue,
            client,
            service,
            config,
            worker_count,
            telemetry,
        } = self;

        let mut workers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let receiver = queue.receiver();
            let client = client.clone();
            let service = service.clone();
            let config = config.clone();
            let telemetry = telemetry.clone();

            workers.push(tokio::spawn(async move {
                run_worker(receiver, client, service, config, telemetry).await;
            }));
        }

        LokiExporterHandle {
            workers,
            queue,
            shutdown: config.shutdown,
            telemetry,
        }
    }
}

/// Final state of a shutdown operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownOutcome {
    /// All workers drained their inputs and terminated before the deadline.
    Drained,
    /// Draining was disabled and workers were aborted.
    Aborted,
    /// The deadline expired and remaining work was abandoned.
    TimedOut,
    /// At least one worker failed while the queue was being drained.
    WorkerFailure,
}

/// Operational shutdown result containing no HTTP event data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownReport {
    pub outcome: ShutdownOutcome,
    pub abandoned_events: usize,
    pub worker_failures: usize,
}

/// Join and shutdown handle for the fixed worker group.
pub struct LokiExporterHandle {
    workers: Vec<JoinHandle<()>>,
    queue: BoundedEventQueue,
    shutdown: ShutdownConfig,
    telemetry: InternalTelemetry,
}

impl LokiExporterHandle {
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    // Waits for workers after the caller closes the queue.
    //
    // Prefer [`LokiExporterHandle::shutdown`] in production, because this
    // method has no deadline and waits indefinitely while the queue is open.
    // pub async fn join(mut self) -> Result<(), LokiExporterError> {
    //     if join_workers(&mut self.workers).await == 0 {
    //         Ok(())
    //     } else {
    //         Err(LokiExporterError::WorkerJoin)
    //     }
    // }

    /// Closes producers, drains within the configured deadline, and accounts
    /// for queued or worker-owned events that could not be exported.
    pub async fn shutdown(mut self) -> ShutdownReport {
        self.queue.close();

        let (outcome, worker_failures) = if self.shutdown.drain {
            match timeout(self.shutdown.deadline, join_workers(&mut self.workers)).await {
                Ok(0) => (ShutdownOutcome::Drained, 0),
                Ok(failures) => (ShutdownOutcome::WorkerFailure, failures),
                Err(_) => {
                    abort_workers(&mut self.workers).await;
                    (ShutdownOutcome::TimedOut, 0)
                }
            }
        } else {
            abort_workers(&mut self.workers).await;
            (ShutdownOutcome::Aborted, 0)
        };

        let mut abandoned_events = 0_usize;
        let receiver = self.queue.receiver();
        while receiver.try_recv().is_some() {
            abandoned_events = abandoned_events.saturating_add(1);
        }

        let pending = self.telemetry.snapshot().pending_export_events;
        abandoned_events =
            abandoned_events.saturating_add(usize::try_from(pending).unwrap_or(usize::MAX));

        self.telemetry.record_shutdown(
            matches!(outcome, ShutdownOutcome::TimedOut),
            abandoned_events,
        );

        ShutdownReport {
            outcome,
            abandoned_events,
            worker_failures,
        }
    }
}

async fn join_workers(workers: &mut [JoinHandle<()>]) -> usize {
    let mut failures = 0;
    for worker in workers {
        if worker.await.is_err() {
            failures += 1;
        }
    }
    failures
}

async fn abort_workers(workers: &mut [JoinHandle<()>]) {
    for worker in workers {
        if worker.is_finished() {
            continue;
        }

        worker.abort();
        let _ = worker.await;
    }
}

async fn run_worker(
    receiver: EventQueueReceiver,
    client: LokiClient,
    service: ServiceConfig,
    config: LokiConfig,
    telemetry: InternalTelemetry,
) {
    let Ok(mut batch) = LokiBatchBuilder::new(config.batch.clone(), service) else {
        return;
    };
    let mut flush_at = None;

    loop {
        if batch.is_empty() {
            match receiver.recv().await {
                Some(event) => {
                    telemetry.record_event_dequeued();
                    process_event(
                        event,
                        &client,
                        &config,
                        &telemetry,
                        &mut batch,
                        &mut flush_at,
                    )
                    .await;
                }
                None => break,
            }
            continue;
        }

        let deadline = flush_at.unwrap_or_else(|| Instant::now() + config.batch.flush_interval);
        flush_at = Some(deadline);
        let wait = deadline.saturating_duration_since(Instant::now());

        match tokio::time::timeout(wait, receiver.recv()).await {
            Ok(Some(event)) => {
                telemetry.record_event_dequeued();
                process_event(
                    event,
                    &client,
                    &config,
                    &telemetry,
                    &mut batch,
                    &mut flush_at,
                )
                .await;
            }
            Ok(None) => {
                flush_batch(&mut batch, &client, &config.retry, &telemetry).await;
                break;
            }
            Err(_) => {
                flush_batch(&mut batch, &client, &config.retry, &telemetry).await;
                flush_at = None;
            }
        }
    }

    if !batch.is_empty() {
        flush_batch(&mut batch, &client, &config.retry, &telemetry).await;
    }
}

async fn process_event(
    event: HttpEvent,
    client: &LokiClient,
    config: &LokiConfig,
    telemetry: &InternalTelemetry,
    batch: &mut LokiBatchBuilder,
    flush_at: &mut Option<Instant>,
) {
    let timestamp_ns = unix_timestamp_ns();
    let mut pending = Some(event);

    while let Some(event) = pending.take() {
        match batch.try_push(&event, timestamp_ns) {
            Ok(BatchPush::Accepted) => {
                if flush_at.is_none() {
                    *flush_at = Some(Instant::now() + config.batch.flush_interval);
                }
                if batch.is_full() {
                    flush_batch(batch, client, &config.retry, telemetry).await;
                    *flush_at = None;
                }
            }
            Ok(BatchPush::Full) => {
                flush_batch(batch, client, &config.retry, telemetry).await;
                *flush_at = None;
                pending = Some(event);
            }
            Ok(BatchPush::Oversized) => {
                telemetry.record_oversized_event();
            }
            Err(_) => {
                telemetry.record_event_processing_failure();
            }
        }
    }
}

async fn flush_batch(
    builder: &mut LokiBatchBuilder,
    client: &LokiClient,
    retry: &RetryConfig,
    telemetry: &InternalTelemetry,
) {
    let Some(batch) = builder.take() else {
        return;
    };

    let result = push_with_retries(client, batch.payload(), retry, telemetry).await;
    match result {
        Ok(()) => telemetry.record_exported_batch(batch.event_count()),
        Err(_) => telemetry.record_failed_batch(batch.event_count()),
    }
}

async fn push_with_retries(
    client: &LokiClient,
    payload: &LokiPushRequest,
    retry: &RetryConfig,
    telemetry: &InternalTelemetry,
) -> Result<(), LokiClientError> {
    let mut retries = 0;
    let mut backoff = retry.initial_backoff;

    loop {
        match client.push(payload).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                if error.is_retryable() {
                    telemetry.record_retryable_export_failure();
                } else {
                    telemetry.record_permanent_export_failure();
                }

                if !error.is_retryable() || retries >= retry.max_retries {
                    return Err(error);
                }

                retries += 1;
                telemetry.record_export_retry();
                tokio::time::sleep(backoff).await;
                backoff = backoff.saturating_mul(2).min(retry.max_backoff);
            }
        }
    }
}

fn unix_timestamp_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

/// Exporter construction or worker failure without event or credential data.
#[derive(Debug)]
pub enum LokiExporterError {
    InvalidWorkerCount,
    BatchConfiguration(LokiBatchError),
    Client(LokiClientError),
    WorkerJoin,
}

impl fmt::Display for LokiExporterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWorkerCount => {
                formatter.write_str("worker count must be between 1 and 64")
            }
            Self::BatchConfiguration(error) => {
                write!(formatter, "invalid Loki batch configuration: {error}")
            }
            Self::Client(error) => write!(formatter, "failed to create the Loki client: {error}"),
            Self::WorkerJoin => {
                formatter.write_str("a Loki export worker did not terminate cleanly")
            }
        }
    }
}

impl Error for LokiExporterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::BatchConfiguration(error) => Some(error),
            Self::Client(error) => Some(error),
            _ => None,
        }
    }
}
