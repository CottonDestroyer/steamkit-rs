//! A function that runs on a fixed schedule (immediately, then every `delay`),
//! on a tokio task. Stopping it cancels the task.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::util::debug_log::DebugLog;

/// The mutable state of a scheduled function.
#[derive(Debug)]
struct ScheduledFunctionState {
    /// The period; `None` means "run once".
    delay: RwLock<Option<Duration>>,
    /// Whether it's running.
    running: AtomicBool,
    /// Cancels the running task.
    cancel: Mutex<Option<CancellationToken>>,
}

/// A function that runs on a fixed schedule.
pub(crate) struct ScheduledFunction {
    /// The function to run.
    func: Arc<dyn Fn() + Send + Sync>,
    /// The mutable state.
    state: Arc<ScheduledFunctionState>,
}

impl std::fmt::Debug for ScheduledFunction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScheduledFunction")
            .field("delay", &self.delay())
            .field("running", &self.state.running.load(Ordering::SeqCst))
            .finish()
    }
}

impl ScheduledFunction {
    /// Creates a scheduled function that runs once when started.
    pub(crate) fn new(func: impl Fn() + Send + Sync + 'static) -> ScheduledFunction {
        ScheduledFunction::with_delay(func, None)
    }

    /// Creates a scheduled function that runs immediately and then every
    /// `delay`. `None` means it runs once.
    pub(crate) fn with_delay(
        func: impl Fn() + Send + Sync + 'static,
        delay: Option<Duration>,
    ) -> ScheduledFunction {
        ScheduledFunction {
            func: Arc::new(func),
            state: Arc::new(ScheduledFunctionState {
                delay: RwLock::new(delay),
                running: AtomicBool::new(false),
                cancel: Mutex::new(None),
            }),
        }
    }

    /// The period; `None` means "run once".
    pub(crate) fn delay(&self) -> Option<Duration> {
        *self.state.delay.read().unwrap_or_else(|error| error.into_inner())
    }

    /// Sets the period. Takes effect the next time it's started.
    pub(crate) fn set_delay(&self, delay: Option<Duration>) {
        *self.state.delay.write().unwrap_or_else(|error| error.into_inner()) = delay;
    }

    /// Starts it: runs the function now and then every [`Self::delay`]. Does
    /// nothing if it's already running.
    pub(crate) fn start(&self) {
        if self.state.running.swap(true, Ordering::SeqCst) {
            return;
        }

        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            self.state.running.store(false, Ordering::SeqCst);
            DebugLog::write_line(
                "ScheduledFunction",
                "Unable to start a scheduled function outside of a tokio runtime.",
            );
            return;
        };

        let token = CancellationToken::new();

        {
            let mut cancel = self.state.cancel.lock().unwrap_or_else(|error| error.into_inner());
            *cancel = Some(token.clone());
        }

        let delay = self.delay();
        let func = self.func.clone();

        handle.spawn(async move {
            match delay {
                Some(period) if !period.is_zero() => {
                    let mut interval = tokio::time::interval(period);
                    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                    loop {
                        tokio::select! {
                            _ = token.cancelled() => break,
                            _ = interval.tick() => func(),
                        }
                    }
                }
                _ => {
                    tokio::select! {
                        biased;
                        _ = token.cancelled() => {}
                        _ = std::future::ready(()) => func(),
                    }
                }
            }
        });
    }

    /// Stops it. Does nothing if it isn't running.
    pub(crate) fn stop(&self) {
        if !self.state.running.swap(false, Ordering::SeqCst) {
            return;
        }

        let token = {
            let mut cancel = self.state.cancel.lock().unwrap_or_else(|error| error.into_inner());
            cancel.take()
        };

        if let Some(token) = token {
            token.cancel();
        }
    }
}

impl Drop for ScheduledFunction {
    fn drop(&mut self) {
        self.stop();
    }
}
