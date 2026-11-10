//! Streaming response-body observation without content buffering.

use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};

use http::{HeaderMap, StatusCode};
use http_body::{Body, Frame, SizeHint};
use pin_project_lite::pin_project;
use tracing::Span;

use super::RequestContext;

pin_project! {
    /// A response body that forwards every frame without changing it.
    ///
    /// No HTTP-data `Debug` implementation is exposed.
    pub struct ObservedBody<B> {
        #[pin]
        inner: B,
        completion: Option<ResponseCompletion>,
        finished: bool,
    }
}

impl<B> ObservedBody<B> {
    pub(crate) fn excluded(inner: B) -> Self {
        Self {
            inner,
            completion: None,
            finished: false,
        }
    }

    pub(crate) fn finished(inner: B) -> Self {
        Self {
            inner,
            completion: None,
            finished: true,
        }
    }

    pub(crate) fn observed(
        inner: B,
        request: RequestContext,
        span: Span,
        status: StatusCode,
        response_received_at: Instant,
        time_to_first_byte_ms: f64,
    ) -> Self {
        Self {
            inner,
            completion: Some(ResponseCompletion {
                request: Some(request),
                span,
                status,
                response_received_at,
                time_to_first_byte_ms,
            }),
            finished: false,
        }
    }
}

impl<B> Body for ObservedBody<B>
where
    B: Body,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let mut this = self.project();

        if *this.finished {
            return Poll::Ready(None);
        }

        match this.inner.as_mut().poll_frame(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Ok(frame))) => {
                // Some bodies report end-of-stream immediately after their
                // final data or trailer frame, without another poll returning
                // `None`.
                let ended = this.inner.as_ref().get_ref().is_end_stream();
                if ended {
                    finish(this.completion, this.finished);
                }

                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(error))) => {
                *this.finished = true;

                if let Some(completion) = this.completion.take() {
                    completion.fail();
                }

                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                finish(this.completion, this.finished);
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

struct ResponseCompletion {
    request: Option<RequestContext>,
    span: Span,
    status: StatusCode,
    response_received_at: Instant,
    time_to_first_byte_ms: f64,
}

impl ResponseCompletion {
    fn complete(mut self) {
        let body_duration_ms = self.response_received_at.elapsed().as_secs_f64() * 1_000.0;

        if let Some(request) = self.request.take() {
            request.complete_body(
                &self.span,
                self.status,
                self.time_to_first_byte_ms,
                body_duration_ms,
            );
        }
    }

    fn fail(mut self) {
        let body_duration_ms = self.response_received_at.elapsed().as_secs_f64() * 1_000.0;

        if let Some(request) = self.request.take() {
            request.fail_body(
                &self.span,
                self.status,
                self.time_to_first_byte_ms,
                body_duration_ms,
            );
        }
    }
}

impl Drop for ResponseCompletion {
    fn drop(&mut self) {
        let Some(request) = self.request.take() else {
            return;
        };

        let body_duration_ms = self.response_received_at.elapsed().as_secs_f64() * 1_000.0;

        request.cancel_body(self.status, self.time_to_first_byte_ms, body_duration_ms);
    }
}

fn finish(completion: &mut Option<ResponseCompletion>, finished: &mut bool) {
    *finished = true;

    if let Some(completion) = completion.take() {
        completion.complete();
    }
}

// Compile-time assertion that HeaderMap remains available for trailer frames
// without requiring a separate body representation.
#[allow(dead_code)]
fn _trailers_use_standard_http_type(_: Option<HeaderMap>) {}
