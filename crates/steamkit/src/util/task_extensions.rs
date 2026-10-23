//! Ignoring cancellation errors once a token is cancelled.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio_util::sync::CancellationToken;

use crate::error::SteamError;

/// A future that swallows its cancellation error if `token` was cancelled.
pub struct IgnoringCancellation<F> {
    /// The wrapped future.
    inner: Pin<Box<F>>,
    /// Decides whether a cancellation error is swallowed.
    token: CancellationToken,
}

impl<F> IgnoringCancellation<F> {
    /// Wraps `inner` so its cancellation error is ignored once `token` is
    /// cancelled.
    pub fn new(inner: F, token: CancellationToken) -> IgnoringCancellation<F> {
        IgnoringCancellation {
            inner: Box::pin(inner),
            token,
        }
    }
}

impl<F> fmt::Debug for IgnoringCancellation<F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IgnoringCancellation")
            .field("cancelled", &self.token.is_cancelled())
            .finish()
    }
}

impl<F> Future for IgnoringCancellation<F>
where
    F: Future<Output = Result<(), SteamError>>,
{
    type Output = Result<(), SteamError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        match this.inner.as_mut().poll(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(error)) => {
                if matches!(error, SteamError::Cancelled) && this.token.is_cancelled() {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Ready(Err(error))
                }
            }
        }
    }
}

/// Awaits `task`, ignoring its cancellation error once `token` is cancelled.
/// The trait version is [`TaskExtensions::ignoring_cancellation`].
pub fn ignoring_cancellation<F>(task: F, token: CancellationToken) -> IgnoringCancellation<F>
where
    F: Future<Output = Result<(), SteamError>>,
{
    IgnoringCancellation::new(task, token)
}

/// Extension methods for futures.
pub trait TaskExtensions: Future<Output = Result<(), SteamError>> + Sized {
    /// Awaits this future, ignoring its cancellation error once `token` is
    /// cancelled. There's also the free function [`ignoring_cancellation`].
    fn ignoring_cancellation(self, token: CancellationToken) -> IgnoringCancellation<Self> {
        IgnoringCancellation::new(self, token)
    }
}

impl<F> TaskExtensions for F where F: Future<Output = Result<(), SteamError>> + Sized {}
