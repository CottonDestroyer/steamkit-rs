//! Tracks pending requests and completes them when their response arrives.
//!
//! The awaitable job types live in [`crate::types::job_id`]; this module owns
//! the registry that completes, heartbeats, times out and cancels in-flight
//! jobs, and re-exports the job types.
//!
//! A posted callback goes both to the callback queue and to the job waiting for
//! it. A `Box` can't have two owners, so the manager gives the job a clone of
//! the concrete callback and the queue keeps the original.

use std::any::TypeId;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::Mutex;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::error::SteamError;
use crate::types::job_id::JobID;

use super::callback_mgr::callback_msg::CallbackMsg;

pub use crate::types::job_id::{
    AsyncJob, AsyncJobMultiple, JobHandle, JobReceiver, MultiJobReceiver, ResultSet,
    TypedJobReceiver,
};

/// How long a job waits for its response by default.
pub const DEFAULT_JOB_TIMEOUT: Duration = Duration::from_secs(10);

/// How much a heartbeat from Steam extends a job's timeout.
pub const JOB_HEARTBEAT_EXTENSION: Duration = Duration::from_secs(10);

/// How often the manager checks for timed out jobs.
pub const JOB_TIMEOUT_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// The awaiting side of a multi-result job.
pub type MultiJobHandle<T> = AsyncJobMultiple<T>;

/// The set of callback results of a multi-result job.
pub type JobResultSet<T> = ResultSet<T>;

/// The outcome a multi-result job delivers when it completes.
enum MultiOutcome {
    /// The job completed with the accumulated results.
    Results {
        /// The accumulated callbacks.
        results: Vec<Box<dyn CallbackMsg>>,
        /// Whether the finish condition of the job was met.
        complete: bool,
        /// Whether the job failed because Steam reported a remote failure.
        failed: bool,
    },
    /// The job failed.
    Error(SteamError),
}

/// The completion channel of a multi-result job.
type MultiCompleter = Box<dyn FnOnce(MultiOutcome) + Send + Sync>;

/// The completion channel of a tracked job.
enum Completer {
    /// A job that completes with a single callback.
    Single(Mutex<Option<oneshot::Sender<Result<Box<dyn CallbackMsg>, SteamError>>>>),
    /// A job that completes with a set of callbacks.
    Multiple(Mutex<Option<MultiCompleter>>),
}

impl Completer {
    /// Delivers the callback of a single-result job.
    fn complete_single(&self, callback: Box<dyn CallbackMsg>) {
        let Self::Single(sender) = self else {
            return;
        };

        if let Some(sender) = sender.lock().take() {
            let _ = sender.send(Ok(callback));
        }
    }

    /// Delivers the results of a multi-result job.
    fn complete_multiple(&self, results: Vec<Box<dyn CallbackMsg>>, complete: bool, failed: bool) {
        let Self::Multiple(completer) = self else {
            return;
        };

        if let Some(completer) = completer.lock().take() {
            completer(MultiOutcome::Results {
                results,
                complete,
                failed,
            });
        }
    }

    /// Fails the job with the given error.
    fn fail(&self, error: SteamError) {
        match self {
            Self::Single(sender) => {
                if let Some(sender) = sender.lock().take() {
                    let _ = sender.send(Err(error));
                }
            }
            Self::Multiple(completer) => {
                if let Some(completer) = completer.lock().take() {
                    completer(MultiOutcome::Error(error));
                }
            }
        }
    }
}

/// The state of a single tracked job.
struct JobEntry {
    type_id: TypeId,
    clone_callback: fn(&dyn CallbackMsg) -> Option<Box<dyn CallbackMsg>>,
    completer: Completer,
    timeout: Mutex<Duration>,
    started: Instant,
    finish_condition: Mutex<Option<Arc<dyn Fn(&dyn CallbackMsg) -> bool + Send + Sync>>>,
    results: Mutex<Vec<Box<dyn CallbackMsg>>>,
}

impl JobEntry {
    fn new(
        type_id: TypeId,
        clone_callback: fn(&dyn CallbackMsg) -> Option<Box<dyn CallbackMsg>>,
        completer: Completer,
    ) -> Self {
        Self {
            type_id,
            clone_callback,
            completer,
            timeout: Mutex::new(DEFAULT_JOB_TIMEOUT),
            started: Instant::now(),
            finish_condition: Mutex::new(None),
            results: Mutex::new(Vec::new()),
        }
    }

    fn heartbeat(&self) {
        let mut timeout = self.timeout.lock();
        *timeout += JOB_HEARTBEAT_EXTENSION;
    }

    fn is_timedout(&self) -> bool {
        self.started.elapsed() >= *self.timeout.lock()
    }

    fn timeout(&self) -> Duration {
        *self.timeout.lock()
    }
}

impl fmt::Debug for JobEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobEntry")
            .field("type_id", &self.type_id)
            .field("timeout", &self.timeout())
            .field("elapsed", &self.started.elapsed())
            .finish()
    }
}

/// The state shared by an [`AsyncJobManager`] and its timeout task.
struct AsyncJobManagerInner {
    jobs: DashMap<JobID, Arc<JobEntry>>,
}

impl fmt::Debug for AsyncJobManagerInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncJobManagerInner")
            .field("jobs", &self.jobs.len())
            .finish()
    }
}

/// Tracks the jobs waiting for a response from Steam. Handlers use it to await
/// their requests.
pub struct AsyncJobManager {
    inner: Arc<AsyncJobManagerInner>,
    timeouts_enabled: AtomicBool,
    timeout_task: Mutex<Option<JoinHandle<()>>>,
}

impl Default for AsyncJobManager {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for AsyncJobManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncJobManager")
            .field("inner", &self.inner)
            .field("timeouts_enabled", &self.timeouts_enabled.load(Ordering::SeqCst))
            .finish()
    }
}

impl AsyncJobManager {
    /// Creates an empty job manager.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AsyncJobManagerInner {
                jobs: DashMap::new(),
            }),
            timeouts_enabled: AtomicBool::new(false),
            timeout_task: Mutex::new(None),
        }
    }

    /// The number of jobs that are currently tracked.
    pub fn tracked_job_count(&self) -> usize {
        self.inner.jobs.len()
    }

    /// Tracks a job that completes with a single callback.
    pub fn start_job<T>(&self, job_id: JobID) -> AsyncJob<T>
    where
        T: CallbackMsg + Clone + 'static,
    {
        let mut job = AsyncJob::<T>::new(job_id);
        let sender = job.take_sender();

        let entry = Arc::new(JobEntry::new(
            TypeId::of::<T>(),
            clone_callback_of::<T>,
            Completer::Single(Mutex::new(sender)),
        ));

        self.inner.jobs.insert(job_id, entry);

        job
    }

    /// Tracks a job that completes once `finish_condition` returns `true`.
    pub fn start_job_multiple<T>(
        &self,
        job_id: JobID,
        finish_condition: impl FnMut(&T) -> bool + Send + Sync + 'static,
    ) -> MultiJobHandle<T>
    where
        T: CallbackMsg + Clone + 'static,
    {
        let finish_condition = Arc::new(Mutex::new(finish_condition));

        let manager_condition = {
            let finish_condition = Arc::clone(&finish_condition);

            Arc::new(move |callback: &dyn CallbackMsg| {
                match callback.as_any().downcast_ref::<T>() {
                    Some(typed) => {
                        let mut guard = finish_condition.lock();
                        let condition = &mut *guard;
                        condition(typed)
                    }
                    None => false,
                }
            })
        };

        let job_condition = {
            let finish_condition = Arc::clone(&finish_condition);

            move |typed: &T| {
                let mut guard = finish_condition.lock();
                let condition = &mut *guard;
                condition(typed)
            }
        };

        let mut job = AsyncJobMultiple::<T>::new(job_id, job_condition);
        let sender = job.take_sender();

        let completer: MultiCompleter = Box::new(move |outcome| {
            let Some(sender) = sender else {
                return;
            };

            let result = match outcome {
                MultiOutcome::Results {
                    results,
                    complete,
                    failed,
                } => {
                    let typed: Vec<T> =
                        results.into_iter().filter_map(downcast_box::<T>).collect();

                    Ok(ResultSet {
                        complete,
                        failed,
                        results: Some(typed),
                    })
                }
                MultiOutcome::Error(error) => Err(error),
            };

            let _ = sender.send(result);
        });

        let entry = Arc::new(JobEntry::new(
            TypeId::of::<T>(),
            clone_callback_of::<T>,
            Completer::Multiple(Mutex::new(Some(completer))),
        ));

        *entry.finish_condition.lock() = Some(manager_condition);

        self.inner.jobs.insert(job_id, entry);

        job
    }

    /// Hands a callback to the job waiting for it. Ignored if no job with that
    /// id is tracked.
    pub fn try_complete_job(&self, job_id: JobID, callback: &dyn CallbackMsg) {
        let Some(entry) = self.inner.jobs.get(&job_id).map(|tracked| Arc::clone(tracked.value())) else {
            return;
        };

        let finish_condition = entry.finish_condition.lock().clone();

        match finish_condition {
            None => {
                if entry.type_id != callback.as_any().type_id() {
                    entry.completer.fail(SteamError::Malformed(format!(
                        "callback type {:?} does not match the expected type of job {}",
                        callback.as_any().type_id(),
                        job_id.value()
                    )));
                } else if let Some(copy) = (entry.clone_callback)(callback) {
                    entry.completer.complete_single(copy);
                }

                self.inner.jobs.remove(&job_id);
            }
            Some(condition) => {
                let finished = condition(callback);

                if let Some(copy) = (entry.clone_callback)(callback) {
                    entry.results.lock().push(copy);
                }

                if finished {
                    let results = std::mem::take(&mut *entry.results.lock());
                    entry.completer.complete_multiple(results, true, false);
                    self.inner.jobs.remove(&job_id);
                } else {
                    entry.heartbeat();
                }
            }
        }
    }

    /// Extends a job's timeout (Steam sent a heartbeat for it).
    pub fn heartbeat_job(&self, job_id: JobID) {
        if let Some(entry) = self.inner.jobs.get(&job_id) {
            entry.heartbeat();
        }
    }

    /// Fails a job because Steam reported an error for it.
    pub fn fail_job(&self, job_id: JobID) {
        let Some((_, entry)) = self.inner.jobs.remove(&job_id) else {
            return;
        };

        let results = std::mem::take(&mut *entry.results.lock());

        if results.is_empty() {
            entry.completer.fail(SteamError::JobFailed);
        } else {
            entry.completer.complete_multiple(results, false, true);
        }
    }

    /// Cancels and drops every tracked job.
    pub fn cancel_pending_jobs(&self) {
        let entries: Vec<Arc<JobEntry>> = self
            .inner
            .jobs
            .iter()
            .map(|tracked| Arc::clone(tracked.value()))
            .collect();
        self.inner.jobs.clear();

        for entry in entries {
            let results = std::mem::take(&mut *entry.results.lock());

            if results.is_empty() {
                entry.completer.fail(SteamError::Cancelled);
            } else {
                entry.completer.complete_multiple(results, false, false);
            }
        }
    }

    /// Turns the periodic timeout check on or off.
    pub fn set_timeouts_enabled(&self, enable: bool) {
        self.timeouts_enabled.store(enable, Ordering::SeqCst);

        if enable {
            self.start_timeout_task();
        } else {
            self.stop_timeout_task();
        }
    }

    /// Whether the periodic timeout check is running.
    pub fn timeouts_enabled(&self) -> bool {
        self.timeouts_enabled.load(Ordering::SeqCst)
    }

    /// Cancels and drops every job that has timed out.
    pub fn cancel_timed_out_jobs(&self) {
        cancel_timed_out_jobs(&self.inner);
    }

    fn start_timeout_task(&self) {
        let mut task = self.timeout_task.lock();
        if task.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return;
        }

        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };

        let inner = Arc::downgrade(&self.inner);

        *task = Some(handle.spawn(async move {
            let mut interval = tokio::time::interval(JOB_TIMEOUT_CHECK_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                interval.tick().await;

                let Some(inner) = inner.upgrade() else {
                    break;
                };

                cancel_timed_out_jobs(&inner);
            }
        }));
    }

    fn stop_timeout_task(&self) {
        let mut task = self.timeout_task.lock();
        if let Some(handle) = task.take() {
            handle.abort();
        }
    }
}

/// Cancels every job of the given manager that has timed out.
fn cancel_timed_out_jobs(inner: &AsyncJobManagerInner) {
    let timed_out: Vec<JobID> = inner
        .jobs
        .iter()
        .filter(|tracked| tracked.value().is_timedout())
        .map(|tracked| *tracked.key())
        .collect();

    for job_id in timed_out {
        let Some((_, entry)) = inner.jobs.remove(&job_id) else {
            continue;
        };

        let results = std::mem::take(&mut *entry.results.lock());

        if results.is_empty() {
            entry.completer.fail(SteamError::Timeout(entry.timeout()));
        } else {
            entry.completer.complete_multiple(results, false, false);
        }
    }
}

/// Clones the concrete callback so the job and the callback queue can each
/// own one.
fn clone_callback_of<T: CallbackMsg + Clone + 'static>(
    callback: &dyn CallbackMsg,
) -> Option<Box<dyn CallbackMsg>> {
    callback
        .as_any()
        .downcast_ref::<T>()
        .map(|typed| Box::new(typed.clone()) as Box<dyn CallbackMsg>)
}

/// Downcasts a callback message to the concrete callback type.
fn downcast_box<T: CallbackMsg + 'static>(callback: Box<dyn CallbackMsg>) -> Option<T> {
    // `CallbackMsg: Any`, so upcast to `dyn Any` and downcast from there.
    let any: Box<dyn std::any::Any> = callback;
    any.downcast::<T>().ok().map(|typed| *typed)
}
