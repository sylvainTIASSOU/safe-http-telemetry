//! Bounded request-body observation for the Axum adapter.
//!
//! Frames are forwarded unchanged. Only a bounded prefix is copied into the
//! shared capture state; parsing and redaction happen during event creation.

use std::{
    sync::{Arc, Mutex, MutexGuard},
    task::{Context, Poll},
};

use bytes::Buf;
use http::{header, HeaderMap};
use http_body::{Body, Frame, SizeHint};
use pin_project_lite::pin_project;

use crate::core::telemetry::InternalTelemetry;

use super::bounded_value;

#[derive(Clone)]
pub(crate) struct RequestBodyCaptureState {
    inner: Arc<Mutex<CaptureState>>,
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

pub(crate) struct CapturedRequestBody {
    pub(crate) bytes: Vec<u8>,
    pub(crate) truncated: bool,
    pub(crate) ended: bool,
    pub(crate) failed: bool,
    pub(crate) content_type: Option<String>,
    pub(crate) content_encoding: Option<String>,
    pub(crate) content_length: Option<u64>,
}

impl RequestBodyCaptureState {
    pub(crate) fn new(headers: &HeaderMap, limit: usize, telemetry: &InternalTelemetry) -> Self {
        let header_value = |name| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(|value| bounded_value(value, limit, telemetry))
        };

        Self {
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

    fn observe<Data>(&self, data: &Data, limit: usize)
    where
        Data: Buf + Clone,
    {
        let remaining = data.remaining();
        let mut state = self.lock();
        state.observed_bytes = state.observed_bytes.saturating_add(remaining as u64);

        let available = limit.saturating_sub(state.bytes.len());
        let copied = available.min(remaining);

        if copied > 0 {
            // Axum uses Bytes here: cloning the Buf handle does not clone the
            // underlying frame allocation. The original frame remains intact.
            let mut copy = data.clone();
            let prefix = copy.copy_to_bytes(copied);
            state.bytes.extend_from_slice(&prefix);
        }

        if copied < remaining {
            state.truncated = true;
        }
    }

    fn end(&self) {
        self.lock().ended = true;
    }

    fn fail(&self) {
        let mut state = self.lock();
        state.failed = true;
        state.ended = true;
    }

    pub(crate) fn finish(&self) -> CapturedRequestBody {
        let mut state = self.lock();

        CapturedRequestBody {
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

pin_project! {
    /// Request body that forwards each frame and copies only a bounded prefix.
    pub(crate) struct ObservedRequestBody<B> {
        #[pin]
        inner: B,
        capture: RequestBodyCaptureState,
        limit: usize,
        finished: bool,
    }
}

impl<B> ObservedRequestBody<B> {
    pub(crate) fn new(inner: B, capture: RequestBodyCaptureState, limit: usize) -> Self
    where
        B: Body,
    {
        let finished = inner.is_end_stream();
        if finished {
            capture.end();
        }

        Self {
            inner,
            capture,
            limit,
            finished,
        }
    }
}

impl<B> Body for ObservedRequestBody<B>
where
    B: Body,
    B::Data: Buf + Clone,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let mut this = self.project();

        if *this.finished {
            return Poll::Ready(None);
        }

        match this.inner.as_mut().poll_frame(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    this.capture.observe(data, *this.limit);
                }

                if this.inner.as_ref().get_ref().is_end_stream() {
                    *this.finished = true;
                    this.capture.end();
                }

                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(error))) => {
                *this.finished = true;
                this.capture.fail();
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                *this.finished = true;
                this.capture.end();
                Poll::Ready(None)
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.finished || self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use http_body_util::{BodyExt, StreamBody};

    use super::*;

    #[tokio::test]
    async fn frames_and_trailers_are_unchanged_while_the_prefix_is_bounded() {
        let data = vec![
            Ok::<_, std::io::Error>(Frame::data(Bytes::from_static(b"abc"))),
            Ok(Frame::data(Bytes::from_static(b"def"))),
            Ok(Frame::trailers(HeaderMap::new())),
        ];
        let inner = StreamBody::new(futures_util::stream::iter(data));
        let capture =
            RequestBodyCaptureState::new(&HeaderMap::new(), 32, &InternalTelemetry::default());
        let mut body = ObservedRequestBody::new(inner, capture.clone(), 4);

        let first = body.frame().await.unwrap().unwrap();
        assert_eq!(first.into_data().unwrap(), Bytes::from_static(b"abc"));

        let second = body.frame().await.unwrap().unwrap();
        assert_eq!(second.into_data().unwrap(), Bytes::from_static(b"def"));

        let trailers = body.frame().await.unwrap().unwrap();
        assert!(trailers.into_trailers().is_ok());
        assert!(body.frame().await.is_none());

        let captured = capture.finish();
        assert_eq!(captured.bytes, b"abcd");
        assert!(captured.truncated);
        assert!(captured.ended);
        assert!(!captured.failed);
    }

    #[tokio::test]
    async fn body_errors_are_forwarded_and_mark_capture_as_failed() {
        let frames = vec![
            Ok::<_, std::io::Error>(Frame::data(Bytes::from_static(b"prefix"))),
            Err(std::io::Error::other("test error")),
        ];
        let inner = StreamBody::new(futures_util::stream::iter(frames));
        let capture =
            RequestBodyCaptureState::new(&HeaderMap::new(), 32, &InternalTelemetry::default());
        let mut body = ObservedRequestBody::new(inner, capture.clone(), 32);

        let frame = body.frame().await.unwrap().unwrap();
        assert_eq!(frame.into_data().unwrap(), Bytes::from_static(b"prefix"));

        let error = body.frame().await.unwrap().unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Other);

        let captured = capture.finish();
        assert_eq!(captured.bytes, b"prefix");
        assert!(captured.failed);
        assert!(captured.ended);
    }
}
