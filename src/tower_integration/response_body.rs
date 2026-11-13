//! Bounded response-body prefix capture shared with the terminal event.

use std::sync::{Arc, Mutex, MutexGuard};

use bytes::Buf;
use http::{header, HeaderMap};

use crate::core::telemetry::InternalTelemetry;

use super::bounded_value;

#[derive(Clone)]
pub(crate) struct ResponseBodyCaptureState {
    inner: Arc<Mutex<CaptureState>>,
    limit: usize,
}

struct CaptureState {
    bytes: Vec<u8>,
    observed_bytes: u64,
    truncated: bool,
    ended: bool,
    failed: bool,
    content_type: Option<String>,
    content_encoding: Option<String>,
    content_length: Option<u64>,
}

pub(super) struct CapturedResponseBody {
    pub(super) bytes: Vec<u8>,
    pub(super) truncated: bool,
    pub(super) ended: bool,
    pub(super) failed: bool,
    pub(super) content_type: Option<String>,
    pub(super) content_encoding: Option<String>,
    pub(super) content_length: Option<u64>,
}

impl ResponseBodyCaptureState {
    pub(super) fn new(
        headers: &HeaderMap,
        header_limit: usize,
        body_limit: usize,
        telemetry: &InternalTelemetry,
    ) -> Self {
        let header_value = |name| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(|value| bounded_value(value, header_limit, telemetry))
        };

        Self {
            limit: body_limit,
            inner: Arc::new(Mutex::new(CaptureState {
                bytes: Vec::new(),
                observed_bytes: 0,
                truncated: false,
                ended: false,
                failed: false,
                content_type: header_value(header::CONTENT_TYPE),
                content_encoding: header_value(header::CONTENT_ENCODING),
                content_length: headers
                    .get(header::CONTENT_LENGTH)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok()),
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, CaptureState> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn observe<Data>(&self, data: &Data)
    where
        Data: Buf + Clone,
    {
        let remaining = data.remaining();
        let mut state = self.lock();
        state.observed_bytes = state.observed_bytes.saturating_add(remaining as u64);

        let available = self.limit.saturating_sub(state.bytes.len());
        let copied = available.min(remaining);

        if copied > 0 {
            let mut copy = data.clone();
            let prefix = copy.copy_to_bytes(copied);
            state.bytes.extend_from_slice(&prefix);
        }

        if copied < remaining {
            state.truncated = true;
        }
    }

    pub(super) fn end(&self) {
        self.lock().ended = true;
    }

    #[allow(dead_code)]
    pub(super) fn fail(&self) {
        let mut state = self.lock();
        state.failed = true;
        state.ended = true;
    }

    pub(super) fn finish(&self) -> CapturedResponseBody {
        let mut state = self.lock();

        CapturedResponseBody {
            bytes: std::mem::take(&mut state.bytes),
            truncated: state.truncated,
            ended: state.ended,
            failed: state.failed,
            content_type: state.content_type.clone(),
            content_encoding: state.content_encoding.clone(),
            content_length: state.content_length,
        }
    }
}
