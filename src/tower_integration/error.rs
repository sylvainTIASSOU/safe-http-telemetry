//! Stable service-error classification without exposing error messages.

/// Failure category used when no usable HTTP response was produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestFailure {
    ServerError,
    Timeout,
}

/// Classifies a Tower service error without serializing its contents.
pub trait ErrorClassifier<Error>: Clone + Send + Sync + 'static {
    fn classify(&self, error: &Error) -> RequestFailure;
}

/// Default classifier for services whose errors carry no timeout information.
#[derive(Clone, Copy, Debug, Default)]
pub struct ServerErrorClassifier;

impl<Error> ErrorClassifier<Error> for ServerErrorClassifier {
    fn classify(&self, _error: &Error) -> RequestFailure {
        RequestFailure::ServerError
    }
}
