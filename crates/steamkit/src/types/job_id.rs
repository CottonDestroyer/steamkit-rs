//! Job ids, and the awaitable jobs built on them.
//!
//! A job owns the sending half of a `tokio::sync::oneshot` channel; the waiting
//! side holds a [`JobReceiver`]. `AsyncJob` (no type argument) is the untyped
//! job and `AsyncJob<T>` the typed one.

use std::any::Any;
use std::marker::PhantomData;
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use crate::error::SteamError;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;

/// The default job timeout.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// How much each heartbeat extends a job's timeout.
const HEARTBEAT_TIMEOUT_EXTENSION: Duration = Duration::from_secs(10);

/// The id of a network task (a "job").
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct JobID(u64);

impl JobID {
    /// An invalid job id (`u64::MAX`).
    pub const INVALID: JobID = JobID(u64::MAX);

    /// Creates an invalid job id.
    pub fn new() -> JobID {
        JobID(u64::MAX)
    }

    /// Creates a job id from its 64-bit value.
    pub fn from_value(value: u64) -> JobID {
        JobID(value)
    }

    /// The 64-bit value.
    pub fn value(&self) -> u64 {
        self.0
    }

    /// Sets the 64-bit value.
    pub fn set_value(&mut self, value: u64) {
        self.0 = value;
    }

    /// The sequential count.
    pub fn sequential_count(&self) -> u32 {
        ((self.0 >> 0) & 0xF_FFFF) as u32
    }

    /// Sets the sequential count.
    pub fn set_sequential_count(&mut self, value: u32) {
        let mask = 0xF_FFFF_u64;
        self.0 = (self.0 & !mask) | (value as u64 & mask);
    }

    /// When the server that generated this id started.
    pub fn start_time(&self) -> u64 {
        (self.0 >> 20) & 0x3_FFFF_FFFF
    }

    /// Sets when the server that generated this id started.
    pub fn set_start_time(&mut self, value: u64) {
        let mask = 0x3_FFFF_FFFF_u64 << 20;
        self.0 = (self.0 & !mask) | ((value & 0x3_FFFF_FFFF) << 20);
    }

    /// The process id of the server that generated this id.
    pub fn process_id(&self) -> u32 {
        ((self.0 >> 50) & 0xF) as u32
    }

    /// Sets the process id of the server that generated this id.
    pub fn set_process_id(&mut self, value: u32) {
        let mask = 0xF_u64 << 50;
        self.0 = (self.0 & !mask) | ((value as u64 & 0xF) << 50);
    }

    /// The box id of the server that generated this id.
    pub fn box_id(&self) -> u32 {
        ((self.0 >> 54) & 0x3_FF) as u32
    }

    /// Sets the box id of the server that generated this id.
    pub fn set_box_id(&mut self, value: u32) {
        let mask = 0x3_FF_u64 << 54;
        self.0 = (self.0 & !mask) | ((value as u64 & 0x3_FF) << 54);
    }
}

impl std::fmt::Display for JobID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

impl std::fmt::Debug for JobID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

impl From<JobID> for u64 {
    fn from(job_id: JobID) -> u64 {
        job_id.value()
    }
}

impl From<u64> for JobID {
    fn from(value: u64) -> JobID {
        JobID::from_value(value)
    }
}

/// The result an async job delivers when it completes.
type JobResult = Result<Box<dyn CallbackMsg>, SteamError>;

/// The type argument of the untyped [`AsyncJob`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct AnyCallback;

/// An awaitable [`JobID`]. `AsyncJob` is untyped, `AsyncJob<T>` typed.
pub struct AsyncJob<T = AnyCallback> {
    job_id: JobID,
    timeout: Duration,
    job_start: Instant,
    sender: Option<oneshot::Sender<JobResult>>,
    receiver: JobReceiver,
    _marker: PhantomData<fn() -> T>,
}

impl<T> AsyncJob<T> {
    /// Creates a job. Register it with the job manager by handing over its
    /// sender ([`AsyncJob::take_sender`]).
    pub fn new(job_id: JobID) -> AsyncJob<T> {
        let (sender, receiver) = oneshot::channel();
        AsyncJob {
            job_id,
            timeout: DEFAULT_TIMEOUT,
            job_start: Instant::now(),
            sender: Some(sender),
            receiver: JobReceiver { receiver },
            _marker: PhantomData,
        }
    }

    /// The [`JobID`] of this job.
    pub fn job_id(&self) -> JobID {
        self.job_id
    }

    /// How long until this job times out and is cancelled. 10 seconds by default.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Sets how long until this job times out and is cancelled.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Whether this job has timed out.
    pub fn is_timedout(&self) -> bool {
        self.job_start.elapsed() >= self.timeout
    }

    /// Records a heartbeat and extends the job's timeout.
    pub fn heartbeat(&mut self) {
        self.timeout += HEARTBEAT_TIMEOUT_EXTENSION;
    }

    /// Adds a callback to the job's results. For the untyped job, one callback
    /// always completes it.
    pub fn add_result(&mut self, callback: Box<dyn CallbackMsg>) -> bool {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Ok(callback));
        }
        true
    }

    /// Fails the job: `due_to_remote_failure` if Steam reported a failure,
    /// otherwise it timed out.
    pub fn set_failed(&mut self, due_to_remote_failure: bool) {
        if let Some(sender) = self.sender.take() {
            let error = if due_to_remote_failure {
                SteamError::JobFailed
            } else {
                SteamError::Timeout(self.timeout)
            };
            let _ = sender.send(Err(error));
        }
    }

    /// Hands the sending half of the completion channel to the job manager,
    /// which completes the job when the matching callback arrives.
    pub fn take_sender(&mut self) -> Option<oneshot::Sender<JobResult>> {
        self.sender.take()
    }

    /// Takes the receiving half of the completion channel.
    pub fn into_receiver(self) -> JobReceiver {
        self.receiver
    }
}

impl<T> From<&AsyncJob<T>> for JobID {
    fn from(job: &AsyncJob<T>) -> JobID {
        job.job_id()
    }
}

impl<T> std::fmt::Debug for AsyncJob<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncJob")
            .field("job_id", &self.job_id)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl<T: CallbackMsg> AsyncJob<T> {
    /// Converts this job into its awaitable form.
    pub fn to_task(self) -> TypedJobReceiver<T> {
        TypedJobReceiver {
            receiver: self.receiver,
            _marker: PhantomData,
        }
    }

    /// Waits for the job's result.
    pub async fn wait(self) -> Result<T, SteamError> {
        self.to_task().wait().await
    }
}

/// The receiving half of a job's completion channel.
pub struct JobReceiver {
    receiver: oneshot::Receiver<JobResult>,
}

impl JobReceiver {
    /// Waits for the job to complete.
    pub async fn wait(self) -> Result<Box<dyn CallbackMsg>, SteamError> {
        match self.receiver.await {
            Ok(result) => result,
            Err(_) => Err(SteamError::Cancelled),
        }
    }
}

impl std::fmt::Debug for JobReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JobReceiver")
    }
}

/// The waiting side of a typed job.
pub struct TypedJobReceiver<T: CallbackMsg> {
    receiver: JobReceiver,
    _marker: PhantomData<fn() -> T>,
}

impl<T: CallbackMsg> TypedJobReceiver<T> {
    /// Waits for the job to complete and downcasts the callback to `T`.
    pub async fn wait(self) -> Result<T, SteamError> {
        let message = self.receiver.wait().await?;

        // `CallbackMsg: Any`, so upcast to `dyn Any` and downcast from there.
        let any: Box<dyn Any> = message;
        match any.downcast::<T>() {
            Ok(typed) => Ok(*typed),
            Err(_) => Err(SteamError::Malformed(
                "callback message does not match the expected type".to_string(),
            )),
        }
    }
}

impl<T: CallbackMsg> std::fmt::Debug for TypedJobReceiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TypedJobReceiver")
    }
}

/// The results of an [`AsyncJobMultiple`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultSet<T> {
    /// Whether the set is complete, i.e. holds every result Steam sent.
    pub complete: bool,
    /// Whether the job got an incomplete set and then hit a remote failure.
    pub failed: bool,
    /// The results.
    pub results: Option<Vec<T>>,
}

/// An awaitable [`JobID`] that can get several callbacks.
pub struct AsyncJobMultiple<T: CallbackMsg> {
    job_id: JobID,
    timeout: Duration,
    job_start: Instant,
    sender: Option<oneshot::Sender<Result<ResultSet<T>, SteamError>>>,
    receiver: MultiJobReceiver<T>,
    finish_condition: Box<dyn FnMut(&T) -> bool + Send + Sync>,
    results: Vec<T>,
    _marker: PhantomData<fn() -> T>,
}

impl<T: CallbackMsg> AsyncJobMultiple<T> {
    /// Creates a job; it's complete once `finish_condition` returns `true` for
    /// a result.
    pub fn new(
        job_id: JobID,
        finish_condition: impl FnMut(&T) -> bool + Send + Sync + 'static,
    ) -> AsyncJobMultiple<T> {
        let (sender, receiver) = oneshot::channel();
        AsyncJobMultiple {
            job_id,
            timeout: DEFAULT_TIMEOUT,
            job_start: Instant::now(),
            sender: Some(sender),
            receiver: MultiJobReceiver {
                receiver,
                _marker: PhantomData,
            },
            finish_condition: Box::new(finish_condition),
            results: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// The [`JobID`] of this job.
    pub fn job_id(&self) -> JobID {
        self.job_id
    }

    /// The timeout of this job.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Sets the timeout of this job.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Whether this job has timed out.
    pub fn is_timedout(&self) -> bool {
        self.job_start.elapsed() >= self.timeout
    }

    /// Records a heartbeat and extends the job's timeout.
    pub fn heartbeat(&mut self) {
        self.timeout += HEARTBEAT_TIMEOUT_EXTENSION;
    }

    /// The results collected so far.
    pub fn results(&self) -> &[T] {
        &self.results
    }

    /// Adds a callback to the results. Returns `true` if it completes the set.
    pub fn add_result(&mut self, callback: Box<dyn CallbackMsg>) -> bool {
        let Some(callback) = downcast_callback::<T>(callback) else {
            return false;
        };

        let is_complete = (self.finish_condition)(&callback);

        // add this callback to our result set
        self.results.push(callback);

        if is_complete {
            // if we've passed our finish condition based on this callback
            // (for instance, if steam tells us we have no more pending messages
            // for this job) then we're complete
            let result_set = ResultSet {
                complete: true,
                failed: false,
                results: Some(std::mem::take(&mut self.results)),
            };

            if let Some(sender) = self.sender.take() {
                let _ = sender.send(Ok(result_set));
            }

            return true;
        }

        // otherwise, we're not complete and we'll wait for the next message.
        // trigger heartbeat logic to keep this job alive as it waits for the next
        // message
        self.heartbeat();

        false
    }

    /// Fails the job: `due_to_remote_failure` if Steam reported a failure,
    /// otherwise it timed out.
    pub fn set_failed(&mut self, due_to_remote_failure: bool) {
        // steamclient is informing this async job that we've either timed out or
        // failed while waiting for an additional callback. Now we have to determine
        // what to do:
        if self.results.is_empty() {
            // if we have zero callbacks in our result set, we cancel this task
            let error = if due_to_remote_failure {
                SteamError::JobFailed
            } else {
                SteamError::Timeout(self.timeout)
            };

            if let Some(sender) = self.sender.take() {
                let _ = sender.send(Err(error));
            }
        } else {
            // if we do have a partial result set, we can complete this job with the
            // results we have and let consumers work out what they want to do with
            // the incomplete set
            let result_set = ResultSet {
                // we're not a complete set
                complete: false,
                // let consumers know if we've failed because of a remote failure
                failed: due_to_remote_failure,
                results: Some(std::mem::take(&mut self.results)),
            };

            if let Some(sender) = self.sender.take() {
                let _ = sender.send(Ok(result_set));
            }
        }
    }

    /// Hands the sending half of the completion channel to the job manager.
    pub fn take_sender(&mut self) -> Option<oneshot::Sender<Result<ResultSet<T>, SteamError>>> {
        self.sender.take()
    }

    /// Converts this job into its awaitable form.
    pub fn to_task(self) -> MultiJobReceiver<T> {
        self.receiver
    }

    /// Waits for the job's results.
    pub async fn wait(self) -> Result<ResultSet<T>, SteamError> {
        self.to_task().wait().await
    }
}

impl<T: CallbackMsg> std::fmt::Debug for AsyncJobMultiple<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncJobMultiple")
            .field("job_id", &self.job_id)
            .field("timeout", &self.timeout)
            .field("results", &self.results.len())
            .finish()
    }
}

/// The waiting side of a multi-result job.
pub struct MultiJobReceiver<T: CallbackMsg> {
    receiver: oneshot::Receiver<Result<ResultSet<T>, SteamError>>,
    _marker: PhantomData<fn() -> T>,
}

impl<T: CallbackMsg> MultiJobReceiver<T> {
    /// Waits for the job to complete.
    pub async fn wait(self) -> Result<ResultSet<T>, SteamError> {
        match self.receiver.await {
            Ok(result) => result,
            Err(_) => Err(SteamError::Cancelled),
        }
    }
}

impl<T: CallbackMsg> std::fmt::Debug for MultiJobReceiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MultiJobReceiver")
    }
}

/// Alias for the untyped [`AsyncJob`].
pub type JobState = AsyncJob;

/// Alias for the typed [`AsyncJob`].
pub type JobHandle<T> = AsyncJob<T>;

/// Alias for [`TypedJobReceiver`].
pub type TypedJob<T> = TypedJobReceiver<T>;

/// Alias for [`AsyncJobMultiple`].
pub type JobGroup<T> = AsyncJobMultiple<T>;

/// Alias for [`MultiJobReceiver`].
pub type MultiJob<T> = MultiJobReceiver<T>;

/// Downcasts a callback message to the concrete callback type.
fn downcast_callback<T: CallbackMsg>(callback: Box<dyn CallbackMsg>) -> Option<T> {
    // `CallbackMsg: Any`, so upcast to `dyn Any` and downcast from there.
    let any: Box<dyn Any> = callback;
    any.downcast::<T>().ok().map(|typed| *typed)
}
