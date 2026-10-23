//! `AsyncJob` and `AsyncJobMultiple` tests.
//!
//! [`SteamClient::start_job`] fails the job right away when the client isn't
//! connected (the first test checks that), so the other tests create their
//! jobs through [`AsyncJobManager::start_job`] and
//! [`AsyncJobManager::start_job_multiple`], which only register them.
//!
//! The job manager has no per-job timeout and uses [`MANAGER_JOB_TIMEOUT`]
//! for every job, so the timeout tests have to wait that long.

use std::any::Any;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use steamkit::error::SteamError;
use steamkit::steam::steam_client::async_job_manager::JOB_HEARTBEAT_EXTENSION;
use steamkit::steam::steam_client::callback_mgr::callback_msg::{CallbackMsg, CallbackMsgBase};
use steamkit::steam::steam_client::steam_client::SteamClient;
use steamkit::types::job_id::{AsyncJob, AsyncJobMultiple, JobID};

/// The timeout the job manager gives every job. It can't be changed per job.
const MANAGER_JOB_TIMEOUT: Duration = Duration::from_secs(10);

/// The timeout of a job that received a heartbeat.
///
/// A heartbeat extends the timeout of a job by ten seconds, so a job that
/// received one is only cancelled after twice the default timeout.
const MANAGER_JOB_TIMEOUT_AFTER_HEARTBEAT: Duration = Duration::from_secs(20);

/// The margin that is added to the timeouts, so that the deadline has surely
/// passed when the job manager is asked to cancel the timed out jobs.
const TIMEOUT_MARGIN: Duration = Duration::from_millis(500);

/// A test callback that belongs to a job.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TestCallback {
    base: CallbackMsgBase,
    is_finished: bool,
}

impl TestCallback {
    /// Creates a callback for the given job.
    fn with_job_id(job_id: JobID) -> TestCallback {
        TestCallback {
            base: CallbackMsgBase::with_job_id(job_id),
            is_finished: false,
        }
    }

    /// Creates a callback for the given job with the given state.
    fn with_is_finished(job_id: JobID, is_finished: bool) -> TestCallback {
        TestCallback {
            base: CallbackMsgBase::with_job_id(job_id),
            is_finished,
        }
    }
}

impl CallbackMsg for TestCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A callback of a different type, used to check that the job manager does not
/// hand a callback of the wrong type to a job.
#[derive(Clone, Debug, PartialEq, Eq)]
struct OtherCallback {
    base: CallbackMsgBase,
}

impl OtherCallback {
    /// Creates a callback for the given job.
    fn new(job_id: JobID) -> OtherCallback {
        OtherCallback {
            base: CallbackMsgBase::with_job_id(job_id),
        }
    }
}

impl CallbackMsg for OtherCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Locks a mutex, ignoring poisoning.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// The number of results of a result set.
fn result_count<T>(result_set: &steamkit::types::job_id::ResultSet<T>) -> usize {
    result_set.results.as_ref().map(|results| results.len()).unwrap_or(0)
}

#[tokio::test]
async fn async_job_failure_when_client_disconnected() {
    let client = SteamClient::new();

    // Starting a job on a client that isn't connected fails it right away.
    let job = client.start_job::<TestCallback>(JobID::from_value(123));

    match job.to_task().wait().await {
        // The job is completed and faulted, but not cancelled.
        Err(SteamError::JobFailed) => {}
        other => panic!("the job must be faulted, not cancelled, got: {other:?}"),
    }
}

#[tokio::test]
async fn async_job_completes_on_callback() {
    let client = SteamClient::new();

    let job = client
        .job_manager()
        .start_job::<TestCallback>(JobID::from_value(123));
    let task = job.to_task();

    client.post_callback(Box::new(TestCallback::with_job_id(JobID::from_value(123))));

    // The job is completed, but neither cancelled nor faulted.
    let callback = match task.wait().await {
        Ok(callback) => callback,
        Err(error) => panic!("the job must complete with the callback, got: {error}"),
    };

    assert_eq!(JobID::from_value(123), callback.job_id());
}

#[tokio::test]
async fn async_job_gives_back_callback() {
    let client = SteamClient::new();

    let job = client
        .job_manager()
        .start_job::<TestCallback>(JobID::from_value(123));
    let task = job.to_task();

    let our_callback = TestCallback::with_job_id(JobID::from_value(123));

    client.post_callback(Box::new(our_callback.clone()));

    let callback = match task.wait().await {
        Ok(callback) => callback,
        Err(error) => panic!("the job must complete with the callback, got: {error}"),
    };

    // The manager hands back a copy of the callback (the queue keeps the
    // original), so compare values.
    assert_eq!(our_callback, callback);
}

#[tokio::test]
async fn async_job_ctor_registers_job() {
    let client = SteamClient::new();

    let job = client
        .job_manager()
        .start_job::<TestCallback>(JobID::from_value(123));

    // There's no lookup by job id, so check the id and the tracked job count.
    assert_eq!(JobID::from_value(123), job.job_id());
    assert_eq!(1usize, client.job_manager().tracked_job_count());
}

#[tokio::test]
async fn async_job_clears_on_completion() {
    let client = SteamClient::new();

    let _job = client
        .job_manager()
        .start_job::<TestCallback>(JobID::from_value(123));

    client.post_callback(Box::new(TestCallback::with_job_id(JobID::from_value(123))));

    assert_eq!(0usize, client.job_manager().tracked_job_count());
}

#[tokio::test]
async fn async_job_clears_on_timeout() {
    let client = SteamClient::new();

    let job = client
        .job_manager()
        .start_job::<TestCallback>(JobID::from_value(123));
    let task = job.to_task();

    tokio::time::sleep(MANAGER_JOB_TIMEOUT + TIMEOUT_MARGIN).await;
    client.job_manager().cancel_timed_out_jobs();

    assert_eq!(0usize, client.job_manager().tracked_job_count());

    match task.wait().await {
        Err(SteamError::Timeout(_)) => {}
        other => panic!("the job must be cancelled by the timeout, got: {other:?}"),
    }
}

#[tokio::test]
async fn async_job_cancels_on_set_failed_timeout() {
    // Jobs from the manager can't be failed directly, so use a standalone
    // `AsyncJob` from `types::job_id`.
    let mut job = AsyncJob::<TestCallback>::new(JobID::from_value(123));

    job.set_failed(false);

    let task = job.to_task();

    // The job is completed and cancelled, but not faulted.
    match task.wait().await {
        Err(SteamError::Timeout(_)) => {}
        other => panic!("the job must be cancelled, not faulted, got: {other:?}"),
    }
}

#[tokio::test]
async fn async_job_timesout() {
    let client = SteamClient::new();

    let job = client
        .job_manager()
        .start_job::<TestCallback>(JobID::from_value(123));
    let task = job.to_task();

    tokio::time::sleep(MANAGER_JOB_TIMEOUT + TIMEOUT_MARGIN).await;
    client.job_manager().cancel_timed_out_jobs();

    // The job is completed and cancelled, but not faulted.
    match task.wait().await {
        Err(SteamError::Timeout(_)) => {}
        other => panic!("the job must be cancelled by the timeout, got: {other:?}"),
    }
}

#[tokio::test]
async fn async_job_throws_exception_on_null_callback() {
    // There's no null in Rust. The closest check is that a callback of a
    // different type doesn't complete the job.
    let client = SteamClient::new();

    let job = client
        .job_manager()
        .start_job::<TestCallback>(JobID::from_value(123));
    let task = job.to_task();

    client.post_callback(Box::new(OtherCallback::new(JobID::from_value(123))));

    match task.wait().await {
        Err(SteamError::Malformed(_)) => {}
        other => panic!("a callback of the wrong type must not complete the job, got: {other:?}"),
    }
}

#[tokio::test]
async fn async_job_throws_failure_exception_on_failure() {
    let client = SteamClient::new();

    let job = client
        .job_manager()
        .start_job::<TestCallback>(JobID::from_value(123));
    let task = job.to_task();

    // Fail the job as if Steam reported a remote failure.
    client.job_manager().fail_job(JobID::from_value(123));

    // The job is completed and faulted, but not cancelled.
    match task.wait().await {
        Err(SteamError::JobFailed) => {}
        other => panic!("the job must be faulted, not cancelled, got: {other:?}"),
    }
}

#[tokio::test]
async fn async_job_multiple_finished_on_empty_predicate() {
    let client = SteamClient::new();

    let job = client.job_manager().start_job_multiple::<TestCallback>(
        JobID::from_value(123),
        |_callback| true,
    );
    let task = job.to_task();

    client.post_callback(Box::new(TestCallback::with_job_id(JobID::from_value(123))));

    // The job informs that it is completed, so it is no longer tracked.
    assert_eq!(0usize, client.job_manager().tracked_job_count());

    // The job is completed, but neither cancelled nor faulted.
    let result_set = match task.wait().await {
        Ok(result_set) => result_set,
        Err(error) => panic!("the job must complete with a result set, got: {error}"),
    };

    assert!(result_set.complete);
    assert!(!result_set.failed);
    assert_eq!(1usize, result_count(&result_set));
}

#[tokio::test]
async fn async_job_multiple_finished_on_predicate() {
    let client = SteamClient::new();

    let job = client.job_manager().start_job_multiple::<TestCallback>(
        JobID::from_value(123),
        |callback| callback.is_finished,
    );
    let task = job.to_task();

    client.post_callback(Box::new(TestCallback::with_is_finished(
        JobID::from_value(123),
        false,
    )));

    // The job does not inform that it is finished, so it is still tracked and
    // the task is neither completed, cancelled nor faulted.
    assert_eq!(1usize, client.job_manager().tracked_job_count());

    client.post_callback(Box::new(TestCallback::with_is_finished(
        JobID::from_value(123),
        true,
    )));

    assert_eq!(0usize, client.job_manager().tracked_job_count());

    let result_set = match task.wait().await {
        Ok(result_set) => result_set,
        Err(error) => panic!("the job must complete with a result set, got: {error}"),
    };

    assert!(result_set.complete);
    assert!(!result_set.failed);
    assert_eq!(2usize, result_count(&result_set));
}

#[tokio::test]
async fn async_job_multiple_clears_on_completion() {
    let client = SteamClient::new();

    let _job = client.job_manager().start_job_multiple::<TestCallback>(
        JobID::from_value(123),
        |callback| callback.is_finished,
    );

    client.post_callback(Box::new(TestCallback::with_is_finished(
        JobID::from_value(123),
        true,
    )));

    assert_eq!(0usize, client.job_manager().tracked_job_count());
}

#[tokio::test]
async fn async_job_multiple_clears_on_timeout() {
    let client = SteamClient::new();

    let _job = client.job_manager().start_job_multiple::<TestCallback>(
        JobID::from_value(123),
        |_callback| true,
    );

    tokio::time::sleep(MANAGER_JOB_TIMEOUT + TIMEOUT_MARGIN).await;
    client.job_manager().cancel_timed_out_jobs();

    assert_eq!(0usize, client.job_manager().tracked_job_count());
}

#[tokio::test]
async fn async_job_multiple_extends_timeout_on_message() {
    // A result that doesn't complete the job moves its deadline, so the job
    // must still be alive once the original deadline has passed. The manager's
    // timeout can't be shortened, so this waits the full timeout.
    let client = SteamClient::new();

    let job = client.job_manager().start_job_multiple::<TestCallback>(
        JobID::from_value(123),
        |callback| callback.is_finished,
    );
    let task = job.to_task();

    // give result 1 of 2
    client.post_callback(Box::new(TestCallback::with_is_finished(
        JobID::from_value(123),
        false,
    )));

    assert_eq!(
        1usize,
        client.job_manager().tracked_job_count(),
        "the job must still be pending after the first result"
    );

    // delay for what the original timeout would have been
    tokio::time::sleep(MANAGER_JOB_TIMEOUT + TIMEOUT_MARGIN).await;

    client.job_manager().cancel_timed_out_jobs();

    // we still shouldn't be completed or canceled (timed out)
    assert_eq!(
        1usize,
        client.job_manager().tracked_job_count(),
        "the result must extend the timeout of the job"
    );

    // The job object has its own timeout, so the short version can be tested
    // on it: a 50 ms timeout that the result extends.
    let mut job = AsyncJobMultiple::<TestCallback>::new(
        JobID::from_value(124),
        |callback| callback.is_finished,
    );
    job.set_timeout(Duration::from_millis(50));

    let finished = job.add_result(Box::new(TestCallback::with_is_finished(
        JobID::from_value(124),
        false,
    )));

    assert!(!finished, "the first result must not complete the job");
    assert_eq!(
        Duration::from_millis(50) + JOB_HEARTBEAT_EXTENSION,
        job.timeout(),
        "the result must extend the timeout of the job"
    );

    // delay for what the original timeout would have been
    tokio::time::sleep(Duration::from_millis(70)).await;

    assert!(
        !job.is_timedout(),
        "the job must not have timed out after its original timeout"
    );

    client.post_callback(Box::new(TestCallback::with_is_finished(
        JobID::from_value(123),
        true,
    )));

    assert_eq!(0usize, client.job_manager().tracked_job_count());

    // we should be completed but not canceled or faulted
    let result_set = match task.wait().await {
        Ok(result_set) => result_set,
        Err(error) => panic!("the job must complete with a result set, got: {error}"),
    };

    assert!(result_set.complete);
    assert!(!result_set.failed);
}

#[tokio::test]
async fn async_job_multiple_timesout() {
    let client = SteamClient::new();

    let job = client.job_manager().start_job_multiple::<TestCallback>(
        JobID::from_value(123),
        |_callback| false,
    );
    let task = job.to_task();

    tokio::time::sleep(MANAGER_JOB_TIMEOUT + TIMEOUT_MARGIN).await;
    client.job_manager().cancel_timed_out_jobs();

    // The job is completed and cancelled, but not faulted.
    match task.wait().await {
        Err(SteamError::Timeout(_)) => {}
        other => panic!("the job must be cancelled by the timeout, got: {other:?}"),
    }
}

#[tokio::test]
async fn async_job_multiple_completes_on_incomplete_result() {
    let client = SteamClient::new();

    let job = client.job_manager().start_job_multiple::<TestCallback>(
        JobID::from_value(123),
        |callback| callback.is_finished,
    );
    let task = job.to_task();

    client.post_callback(Box::new(TestCallback::with_is_finished(
        JobID::from_value(123),
        false,
    )));

    // adding a result extends the job's timeout
    tokio::time::sleep(MANAGER_JOB_TIMEOUT_AFTER_HEARTBEAT + TIMEOUT_MARGIN).await;
    client.job_manager().cancel_timed_out_jobs();

    // The job is completed on a partial (timed out) result set, but neither
    // cancelled nor faulted.
    let result_set = match task.wait().await {
        Ok(result_set) => result_set,
        Err(error) => panic!("the job must complete with a result set, got: {error}"),
    };

    assert!(!result_set.complete);
    assert!(!result_set.failed);
    assert_eq!(1usize, result_count(&result_set));
}

#[tokio::test]
async fn async_job_multiple_completes_on_incomplete_result_and_failure() {
    let client = SteamClient::new();

    let job = client.job_manager().start_job_multiple::<TestCallback>(
        JobID::from_value(123),
        |callback| callback.is_finished,
    );
    let task = job.to_task();

    client.post_callback(Box::new(TestCallback::with_is_finished(
        JobID::from_value(123),
        false,
    )));

    // Fail the job as if Steam reported a remote failure.
    client.job_manager().fail_job(JobID::from_value(123));

    // The job is completed on a partial (failed) result set, but neither
    // cancelled nor faulted.
    let result_set = match task.wait().await {
        Ok(result_set) => result_set,
        Err(error) => panic!("the job must complete with a result set, got: {error}"),
    };

    assert!(!result_set.complete);
    assert!(result_set.failed);
    assert_eq!(1usize, result_count(&result_set));
}

#[tokio::test]
async fn async_job_multiple_throws_exception_on_null_callback() {
    // There's no null in Rust. The closest check is that a callback of a
    // different type is ignored, so the job stays pending.
    let client = SteamClient::new();

    let _job = client.job_manager().start_job_multiple::<TestCallback>(
        JobID::from_value(123),
        |_callback| true,
    );

    client.post_callback(Box::new(OtherCallback::new(JobID::from_value(123))));

    assert_eq!(
        1usize,
        client.job_manager().tracked_job_count(),
        "a callback of the wrong type must not complete the job"
    );
}

#[tokio::test]
async fn async_job_multiple_throws_failure_exception_on_failure() {
    let client = SteamClient::new();

    let job = client.job_manager().start_job_multiple::<TestCallback>(
        JobID::from_value(123),
        |_callback| false,
    );
    let task = job.to_task();

    // Fail the job as if Steam reported a remote failure.
    client.job_manager().fail_job(JobID::from_value(123));

    // The job is completed and faulted, but not cancelled.
    match task.wait().await {
        Err(SteamError::JobFailed) => {}
        other => panic!("the job must be faulted, not cancelled, got: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_job_continues_asynchronously() {
    let client = SteamClient::new();

    let job = client
        .job_manager()
        .start_job::<TestCallback>(JobID::from_value(123));
    let task = job.to_task();

    let continuation_thread = Arc::new(Mutex::new(None));

    let continuation = {
        let continuation_thread = continuation_thread.clone();

        tokio::spawn(async move {
            let _ = task.wait().await;
            *lock(&continuation_thread) = Some(std::thread::current().id());
        })
    };

    let completion_thread = std::thread::current().id();

    client.post_callback(Box::new(TestCallback::with_job_id(JobID::from_value(123))));

    continuation
        .await
        .expect("the continuation of the job must run");

    let thread = *lock(&continuation_thread);

    assert!(thread.is_some(), "the continuation must have run");
    assert_ne!(
        thread,
        Some(completion_thread),
        "the continuation must not run on the thread that completed the job"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_job_multiple_continues_asynchronously() {
    let client = SteamClient::new();

    let job = client.job_manager().start_job_multiple::<TestCallback>(
        JobID::from_value(123),
        |_callback| true,
    );
    let task = job.to_task();

    let continuation_thread = Arc::new(Mutex::new(None));

    let continuation = {
        let continuation_thread = continuation_thread.clone();

        tokio::spawn(async move {
            let _ = task.wait().await;
            *lock(&continuation_thread) = Some(std::thread::current().id());
        })
    };

    let completion_thread = std::thread::current().id();

    client.post_callback(Box::new(TestCallback::with_job_id(JobID::from_value(123))));

    continuation
        .await
        .expect("the continuation of the job must run");

    let thread = *lock(&continuation_thread);

    assert!(thread.is_some(), "the continuation must have run");
    assert_ne!(
        thread,
        Some(completion_thread),
        "the continuation must not run on the thread that completed the job"
    );
}
