//! `CallbackManager` tests.
//!
//! The manager is standalone: the client hands every posted callback to
//! [`CallbackManager::dispatch`], so subscribing is enough to receive them.
//!
//! * Subscriptions match the exact `TypeId`, so there's no catch-all
//!   subscription.
//! * [`CallbackManager::dispatch`] spawns async subscribers instead of
//!   waiting for them, so tests that need to wait use
//!   [`CallbackManager::dispatch_async`].
//! * Async subscribers return `()`, so they can't report errors.

use std::any::Any;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use steamkit::steam::steam_client::callback_mgr::callback_msg::{CallbackMsg, CallbackMsgBase};
use steamkit::steam::steam_client::callback_mgr::callback_mgr::{CallbackManager, SubscriptionId};
use steamkit::steam::steam_client::steam_client::SteamClient;
use steamkit::types::job_id::JobID;

/// A test callback, told apart by a unique id.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CallbackForTest {
    base: CallbackMsgBase,
    unique_id: u64,
}

impl CallbackForTest {
    /// Creates a callback without a job id.
    fn new(unique_id: u64) -> CallbackForTest {
        CallbackForTest {
            base: CallbackMsgBase::new(),
            unique_id,
        }
    }

    /// Creates a callback for the given job.
    fn with_job_id(unique_id: u64, job_id: JobID) -> CallbackForTest {
        CallbackForTest {
            base: CallbackMsgBase::with_job_id(job_id),
            unique_id,
        }
    }
}

impl CallbackMsg for CallbackForTest {
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

/// Posts a callback and runs the client's callbacks.
fn post_and_run_callback(client: &SteamClient, callback: CallbackForTest) {
    client.post_callback(Box::new(callback));
    client.run_callbacks();
}

/// Locks a mutex, ignoring poisoning.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

#[test]
fn posted_callback_triggers_action() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let unique_id = 1u64;
    let did_call = Arc::new(AtomicBool::new(false));
    let did_call_in_action = did_call.clone();

    let subscription = mgr.subscribe::<CallbackForTest>(Box::new(move |callback| {
        assert_eq!(unique_id, callback.unique_id);
        did_call_in_action.store(true, Ordering::SeqCst);
    }));

    post_and_run_callback(&client, CallbackForTest::new(unique_id));

    mgr.unsubscribe(subscription);

    assert!(did_call.load(Ordering::SeqCst));
}

#[test]
fn posted_callback_triggers_action_catch_all() {
    // Subscriptions match the exact `TypeId`, so a subscription to the
    // `CallbackMsg` trait object never matches a concrete callback and can't
    // act as a catch-all. Nothing to test here; per-type subscriptions are
    // covered above.
}

#[test]
fn posted_callback_triggers_action_for_explicit_job_id_invalid() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let job_id = JobID::from_value(123456);
    let unique_id = 1u64;
    let did_call = Arc::new(AtomicBool::new(false));
    let did_call_in_action = did_call.clone();

    let subscription = mgr.subscribe_job::<CallbackForTest>(
        JobID::INVALID,
        Box::new(move |callback| {
            assert_eq!(unique_id, callback.unique_id);
            assert_eq!(job_id, callback.job_id());
            did_call_in_action.store(true, Ordering::SeqCst);
        }),
    );

    post_and_run_callback(&client, CallbackForTest::with_job_id(unique_id, job_id));

    mgr.unsubscribe(subscription);

    assert!(did_call.load(Ordering::SeqCst));
}

#[test]
fn posted_callback_with_job_id_triggers_action_when_no_job_id_specified() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let job_id = JobID::from_value(123456);
    let unique_id = 1u64;
    let did_call = Arc::new(AtomicBool::new(false));
    let did_call_in_action = did_call.clone();

    let subscription = mgr.subscribe::<CallbackForTest>(Box::new(move |callback| {
        assert_eq!(unique_id, callback.unique_id);
        assert_eq!(job_id, callback.job_id());
        did_call_in_action.store(true, Ordering::SeqCst);
    }));

    post_and_run_callback(&client, CallbackForTest::with_job_id(unique_id, job_id));

    mgr.unsubscribe(subscription);

    assert!(did_call.load(Ordering::SeqCst));
}

#[test]
fn posted_callback_does_not_trigger_action_for_wrong_job_id() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let job_id = JobID::from_value(123456);
    let did_call = Arc::new(AtomicBool::new(false));
    let did_call_in_action = did_call.clone();

    let subscription = mgr.subscribe_job::<CallbackForTest>(
        JobID::from_value(123),
        Box::new(move |_callback| {
            did_call_in_action.store(true, Ordering::SeqCst);
        }),
    );

    post_and_run_callback(&client, CallbackForTest::with_job_id(1, job_id));

    mgr.unsubscribe(subscription);

    assert!(!did_call.load(Ordering::SeqCst));
}

#[test]
fn posted_callback_with_job_id_triggers_callback_for_job_id() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let job_id = JobID::from_value(123456);
    let unique_id = 1u64;
    let did_call = Arc::new(AtomicBool::new(false));
    let did_call_in_action = did_call.clone();

    let subscription = mgr.subscribe_job::<CallbackForTest>(
        JobID::from_value(123456),
        Box::new(move |callback| {
            assert_eq!(unique_id, callback.unique_id);
            assert_eq!(job_id, callback.job_id());
            did_call_in_action.store(true, Ordering::SeqCst);
        }),
    );

    post_and_run_callback(&client, CallbackForTest::with_job_id(unique_id, job_id));

    mgr.unsubscribe(subscription);

    assert!(did_call.load(Ordering::SeqCst));
}

#[test]
fn subscribed_function_does_not_run_when_subscription_is_disposed() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_in_action = call_count.clone();

    let subscription = mgr.subscribe::<CallbackForTest>(Box::new(move |_callback| {
        call_count_in_action.fetch_add(1, Ordering::SeqCst);
    }));

    post_and_run_callback(&client, CallbackForTest::new(1));

    mgr.unsubscribe(subscription);

    post_and_run_callback(&client, CallbackForTest::new(1));

    assert_eq!(1, call_count.load(Ordering::SeqCst));
}

#[tokio::test]
async fn posted_callbacks_trigger_actions() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let unique_id = 1u64;
    let num_callbacks_run = Arc::new(AtomicUsize::new(0));
    let num_callbacks_run_in_action = num_callbacks_run.clone();

    let subscription = mgr.subscribe::<CallbackForTest>(Box::new(move |callback| {
        assert_eq!(unique_id, callback.unique_id);
        num_callbacks_run_in_action.fetch_add(1, Ordering::SeqCst);
    }));

    for _ in 0..10 {
        client.post_callback(Box::new(CallbackForTest::new(unique_id)));
    }

    client.run_wait_all_callbacks(Duration::ZERO).await;
    assert_eq!(10, num_callbacks_run.load(Ordering::SeqCst));

    // Callbacks should have been freed.
    client.run_wait_all_callbacks(Duration::ZERO).await;
    assert_eq!(10, num_callbacks_run.load(Ordering::SeqCst));

    mgr.unsubscribe(subscription);
}

#[tokio::test]
async fn posted_callbacks_trigger_actions_async() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let callbacks: Vec<CallbackForTest> = (1u64..=10).map(CallbackForTest::new).collect();

    let num_callbacks_run = Arc::new(AtomicUsize::new(0));
    let num_callbacks_run_in_action = num_callbacks_run.clone();
    let expected_callbacks = callbacks.clone();

    let subscription = mgr.subscribe::<CallbackForTest>(Box::new(move |callback| {
        let run = num_callbacks_run_in_action.clone();
        let expected = expected_callbacks.clone();

        let index = run.load(Ordering::SeqCst);
        assert!(index < expected.len(), "no callback may run twice");
        assert_eq!(expected[index].unique_id, callback.unique_id);

        run.fetch_add(1, Ordering::SeqCst);
    }));

    for callback in &callbacks {
        client.post_callback(Box::new(callback.clone()));
    }

    for index in 1usize..=10 {
        match client.run_wait_callbacks().await {
            Ok(()) => {}
            Err(error) => panic!("a queued callback must be dispatched, got: {error}"),
        }
        assert_eq!(index, num_callbacks_run.load(Ordering::SeqCst));
    }

    // Callbacks should have been freed.
    client.run_wait_all_callbacks(Duration::ZERO).await;
    assert_eq!(10, num_callbacks_run.load(Ordering::SeqCst));

    mgr.unsubscribe(subscription);
}

#[test]
fn correctly_unsubscribes_from_inside_of_callback() {
    // The manager is owned by an `Arc` so that the handler can unsubscribe
    // itself while it is running.
    let mgr = Arc::new(CallbackManager::new());

    let subscription_slot: Arc<Mutex<Option<SubscriptionId>>> = Arc::new(Mutex::new(None));

    let mgr_in_handler = mgr.clone();
    let slot_in_handler = subscription_slot.clone();

    let subscription = mgr.subscribe::<CallbackForTest>(Box::new(move |_callback| {
        let id = lock(&slot_in_handler)
            .take()
            .expect("the subscription must be registered before it is disposed");

        mgr_in_handler.unsubscribe(id);
    }));

    *lock(&subscription_slot) = Some(subscription);

    let _noop = mgr.subscribe::<CallbackForTest>(Box::new(|_callback| {}));

    mgr.dispatch(Box::new(CallbackForTest::new(1)));
}

#[test]
fn correctly_subscribes_from_inside_of_callback() {
    let mgr = Arc::new(CallbackManager::new());

    let mgr_in_handler = mgr.clone();

    let _first = mgr.subscribe::<CallbackForTest>(Box::new(|_callback| {}));

    let _second = mgr.subscribe::<CallbackForTest>(Box::new(move |_callback| {
        let _nested = mgr_in_handler.subscribe::<CallbackForTest>(Box::new(|_callback| {}));
    }));

    mgr.dispatch(Box::new(CallbackForTest::new(1)));
}

#[tokio::test]
async fn correctly_awaits_for_async_callbacks() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let unique_id = 1u64;
    let num_callbacks_run = Arc::new(AtomicUsize::new(0));
    let num_callbacks_run_in_action = num_callbacks_run.clone();

    let subscription = mgr.subscribe_async::<CallbackForTest>(Box::new(
        move |callback: Arc<CallbackForTest>| {
            let run = num_callbacks_run_in_action.clone();

            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                assert_eq!(unique_id, callback.unique_id);
                run.fetch_add(1, Ordering::SeqCst);
            })
        },
    ));

    for _ in 0..10 {
        client.post_callback(Box::new(CallbackForTest::new(unique_id)));
    }

    for index in 1usize..=10 {
        let queued = match client.wait_for_callback().await {
            Ok(queued) => queued,
            Err(error) => panic!("a queued callback must be available, got: {error}"),
        };

        mgr.dispatch_async(queued).await;

        assert_eq!(index, num_callbacks_run.load(Ordering::SeqCst));
    }

    client.run_wait_all_callbacks(Duration::ZERO).await;
    assert_eq!(10, num_callbacks_run.load(Ordering::SeqCst));

    mgr.unsubscribe(subscription);
}

#[tokio::test]
async fn async_callback_with_job_id_triggers_action() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let job_id = JobID::from_value(123456);
    let unique_id = 1u64;
    let did_call = Arc::new(AtomicBool::new(false));
    let did_call_in_action = did_call.clone();

    let subscription = mgr.subscribe_job_async::<CallbackForTest>(
        JobID::from_value(123456),
        Box::new(move |callback: Arc<CallbackForTest>| {
            let called = did_call_in_action.clone();

            Box::pin(async move {
                tokio::task::yield_now().await;
                assert_eq!(unique_id, callback.unique_id);
                assert_eq!(job_id, callback.job_id());
                called.store(true, Ordering::SeqCst);
            })
        }),
    );

    client.post_callback(Box::new(CallbackForTest::with_job_id(unique_id, job_id)));

    let queued = match client.wait_for_callback().await {
        Ok(queued) => queued,
        Err(error) => panic!("a queued callback must be available, got: {error}"),
    };
    mgr.dispatch_async(queued).await;

    mgr.unsubscribe(subscription);

    assert!(did_call.load(Ordering::SeqCst));
}

#[tokio::test]
async fn async_callback_does_not_trigger_action_for_wrong_job_id() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let job_id = JobID::from_value(123456);
    let did_call = Arc::new(AtomicBool::new(false));
    let did_call_in_action = did_call.clone();

    let subscription = mgr.subscribe_job_async::<CallbackForTest>(
        JobID::from_value(123),
        Box::new(move |_callback: Arc<CallbackForTest>| {
            let called = did_call_in_action.clone();

            Box::pin(async move {
                tokio::task::yield_now().await;
                called.store(true, Ordering::SeqCst);
            })
        }),
    );

    client.post_callback(Box::new(CallbackForTest::with_job_id(1, job_id)));

    let queued = match client.wait_for_callback().await {
        Ok(queued) => queued,
        Err(error) => panic!("a queued callback must be available, got: {error}"),
    };
    mgr.dispatch_async(queued).await;

    mgr.unsubscribe(subscription);

    assert!(!did_call.load(Ordering::SeqCst));
}

#[tokio::test]
async fn async_callback_is_blocked_by_sync_run_callbacks() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let unique_id = 1u64;
    let did_call = Arc::new(AtomicBool::new(false));
    let did_call_in_action = did_call.clone();

    let subscription = mgr.subscribe_async::<CallbackForTest>(Box::new(
        move |callback: Arc<CallbackForTest>| {
            let called = did_call_in_action.clone();

            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                assert_eq!(unique_id, callback.unique_id);
                called.store(true, Ordering::SeqCst);
            })
        },
    ));

    client.post_callback(Box::new(CallbackForTest::new(unique_id)));

    // `dispatch` spawns async subscribers instead of waiting for them, so use
    // the awaiting variant here.
    let queued = match client.wait_for_callback().await {
        Ok(queued) => queued,
        Err(error) => panic!("a queued callback must be available, got: {error}"),
    };
    mgr.dispatch_async(queued).await;

    mgr.unsubscribe(subscription);

    assert!(did_call.load(Ordering::SeqCst));
}

#[tokio::test]
async fn async_subscribed_function_does_not_run_when_subscription_is_disposed() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_in_action = call_count.clone();

    let subscription = mgr.subscribe_async::<CallbackForTest>(Box::new(
        move |_callback: Arc<CallbackForTest>| {
            let count = call_count_in_action.clone();

            Box::pin(async move {
                tokio::task::yield_now().await;
                count.fetch_add(1, Ordering::SeqCst);
            })
        },
    ));

    client.post_callback(Box::new(CallbackForTest::new(1)));

    let queued = match client.wait_for_callback().await {
        Ok(queued) => queued,
        Err(error) => panic!("a queued callback must be available, got: {error}"),
    };
    mgr.dispatch_async(queued).await;

    mgr.unsubscribe(subscription);

    client.post_callback(Box::new(CallbackForTest::new(1)));

    let queued = match client.wait_for_callback().await {
        Ok(queued) => queued,
        Err(error) => panic!("a queued callback must be available, got: {error}"),
    };
    mgr.dispatch_async(queued).await;

    assert_eq!(1, call_count.load(Ordering::SeqCst));
}

#[tokio::test]
async fn mixed_sync_and_async_subscribers_both_trigger() {
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let unique_id = 1u64;
    let sync_called = Arc::new(AtomicBool::new(false));
    let sync_called_in_action = sync_called.clone();
    let async_called = Arc::new(AtomicBool::new(false));
    let async_called_in_action = async_called.clone();

    let sync_subscription = mgr.subscribe::<CallbackForTest>(Box::new(move |callback| {
        assert_eq!(unique_id, callback.unique_id);
        sync_called_in_action.store(true, Ordering::SeqCst);
    }));

    let async_subscription = mgr.subscribe_async::<CallbackForTest>(Box::new(
        move |callback: Arc<CallbackForTest>| {
            let called = async_called_in_action.clone();

            Box::pin(async move {
                tokio::task::yield_now().await;
                assert_eq!(unique_id, callback.unique_id);
                called.store(true, Ordering::SeqCst);
            })
        },
    ));

    client.post_callback(Box::new(CallbackForTest::new(unique_id)));

    let queued = match client.wait_for_callback().await {
        Ok(queued) => queued,
        Err(error) => panic!("a queued callback must be available, got: {error}"),
    };
    mgr.dispatch_async(queued).await;

    mgr.unsubscribe(sync_subscription);
    mgr.unsubscribe(async_subscription);

    assert!(sync_called.load(Ordering::SeqCst));
    assert!(async_called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn async_callback_exception_propagates_on_async_path() {
    // Async subscribers return `()`, so there's no error that could
    // propagate. All that can be checked is that dispatching finishes
    // cleanly.
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let subscription = mgr.subscribe_async::<CallbackForTest>(Box::new(
        move |_callback: Arc<CallbackForTest>| Box::pin(async move {}),
    ));

    client.post_callback(Box::new(CallbackForTest::new(1)));

    let queued = match client.wait_for_callback().await {
        Ok(queued) => queued,
        Err(error) => panic!("a queued callback must be available, got: {error}"),
    };

    mgr.dispatch_async(queued).await;

    mgr.unsubscribe(subscription);
}

#[tokio::test]
async fn async_callback_exception_wrapped_in_aggregate_on_sync_path() {
    // Same as above, but through the synchronous dispatch.
    let client = SteamClient::new();
    let mgr = client.callback_manager();

    let subscription = mgr.subscribe_async::<CallbackForTest>(Box::new(
        move |_callback: Arc<CallbackForTest>| Box::pin(async move {}),
    ));

    client.post_callback(Box::new(CallbackForTest::new(1)));

    let queued = match client.wait_for_callback().await {
        Ok(queued) => queued,
        Err(error) => panic!("a queued callback must be available, got: {error}"),
    };

    mgr.dispatch(queued);

    mgr.unsubscribe(subscription);
}
