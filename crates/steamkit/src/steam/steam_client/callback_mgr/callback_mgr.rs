//! Routes callbacks to the functions subscribed to them.
//!
//! Subscriptions match on the [`TypeId`](std::any::TypeId) of the concrete
//! callback type. The client hands every posted callback to
//! [`CallbackManager::dispatch`], so subscribing is all you need to do.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures::future::BoxFuture;
use parking_lot::RwLock;

use crate::types::job_id::JobID;

use super::callback_msg::CallbackMsg;
use super::callback_mgr_internals::{AsyncCallback, Callback, CallbackHandler, CallbackSubscription};

/// Identifies a subscription. Dropping it doesn't unsubscribe; call
/// [`CallbackManager::unsubscribe`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SubscriptionId(u64);

impl SubscriptionId {
    /// The numeric value of the id.
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Routes callbacks to the functions subscribed to them.
pub struct CallbackManager {
    subscriptions: RwLock<Vec<(SubscriptionId, Arc<CallbackSubscription>)>>,
    next_id: AtomicU64,
}

impl Default for CallbackManager {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for CallbackManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallbackManager")
            .field("subscriptions", &self.subscription_count())
            .finish()
    }
}

impl CallbackManager {
    /// Creates a manager with no subscriptions.
    pub fn new() -> Self {
        Self {
            subscriptions: RwLock::new(Vec::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Subscribes `callback_func` to callbacks of type `T`.
    pub fn subscribe<T>(&self, callback_func: Box<dyn Fn(&T) + Send + Sync>) -> SubscriptionId
    where
        T: CallbackMsg + 'static,
    {
        self.register(Callback::new(callback_func, None).into_subscription())
    }

    /// Subscribes `callback_func` to callbacks of type `T` for one job.
    pub fn subscribe_job<T>(&self, job_id: JobID, callback_func: Box<dyn Fn(&T) + Send + Sync>) -> SubscriptionId
    where
        T: CallbackMsg + 'static,
    {
        self.register(Callback::new(callback_func, Some(job_id)).into_subscription())
    }

    /// Subscribes an async `callback_func` to callbacks of type `T`.
    pub fn subscribe_async<T>(&self, callback_func: Box<dyn Fn(Arc<T>) -> BoxFuture<'static, ()> + Send + Sync>) -> SubscriptionId
    where
        T: CallbackMsg + 'static,
    {
        self.register(AsyncCallback::new(callback_func, None).into_subscription())
    }

    /// Subscribes an async `callback_func` to callbacks of type `T` for one job.
    pub fn subscribe_job_async<T>(
        &self,
        job_id: JobID,
        callback_func: Box<dyn Fn(Arc<T>) -> BoxFuture<'static, ()> + Send + Sync>,
    ) -> SubscriptionId
    where
        T: CallbackMsg + 'static,
    {
        self.register(AsyncCallback::new(callback_func, Some(job_id)).into_subscription())
    }

    /// Subscribes to the notifications of a unified service. Create the service
    /// through the unified messages handler first.
    pub fn subscribe_service_notification<T>(&self, callback_func: Box<dyn Fn(&T) + Send + Sync>) -> SubscriptionId
    where
        T: CallbackMsg + 'static,
    {
        self.register(Callback::new(callback_func, None).into_subscription())
    }

    /// Async version of [`CallbackManager::subscribe_service_notification`].
    pub fn subscribe_service_notification_async<T>(
        &self,
        callback_func: Box<dyn Fn(Arc<T>) -> BoxFuture<'static, ()> + Send + Sync>,
    ) -> SubscriptionId
    where
        T: CallbackMsg + 'static,
    {
        self.register(AsyncCallback::new(callback_func, None).into_subscription())
    }

    /// Subscribes to the responses of a unified service.
    pub fn subscribe_service_response<T>(&self, callback_func: Box<dyn Fn(&T) + Send + Sync>) -> SubscriptionId
    where
        T: CallbackMsg + 'static,
    {
        self.register(Callback::new(callback_func, None).into_subscription())
    }

    /// Async version of [`CallbackManager::subscribe_service_response`].
    pub fn subscribe_service_response_async<T>(
        &self,
        callback_func: Box<dyn Fn(Arc<T>) -> BoxFuture<'static, ()> + Send + Sync>,
    ) -> SubscriptionId
    where
        T: CallbackMsg + 'static,
    {
        self.register(AsyncCallback::new(callback_func, None).into_subscription())
    }

    /// Removes a subscription.
    pub fn unsubscribe(&self, id: SubscriptionId) {
        self.subscriptions.write().retain(|(subscription_id, _)| *subscription_id != id);
    }

    /// The number of subscriptions.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.read().len()
    }

    /// Delivers a callback to every subscription that wants it. Async
    /// subscriptions are spawned on the current Tokio runtime, and skipped if
    /// there isn't one.
    pub fn dispatch(&self, msg: Box<dyn CallbackMsg>) {
        self.dispatch_arc(Arc::from(msg));
    }

    /// Like [`CallbackManager::dispatch`], for an already shared callback.
    pub fn dispatch_arc(&self, msg: Arc<dyn CallbackMsg>) {
        let job_id = msg.job_id();
        let type_id = msg.as_any().type_id();

        for (_, subscription) in self.snapshot() {
            if !subscription.matches(type_id, job_id) {
                continue;
            }

            match subscription.handler() {
                CallbackHandler::Sync(handler) => handler(&*msg),
                CallbackHandler::Async(handler) => {
                    let owned: Arc<dyn std::any::Any + Send + Sync> = msg.clone();
                    let future = handler(owned);
                    if let Ok(handle) = tokio::runtime::Handle::try_current() {
                        handle.spawn(future);
                    }
                }
            }
        }
    }

    /// Delivers a callback to every subscription that wants it, awaiting the
    /// async ones.
    pub async fn dispatch_async(&self, msg: Box<dyn CallbackMsg>) {
        self.dispatch_arc_async(Arc::from(msg)).await;
    }

    /// Like [`CallbackManager::dispatch_async`], for an already shared callback.
    pub async fn dispatch_arc_async(&self, msg: Arc<dyn CallbackMsg>) {
        let job_id = msg.job_id();
        let type_id = msg.as_any().type_id();

        for (_, subscription) in self.snapshot() {
            if !subscription.matches(type_id, job_id) {
                continue;
            }

            match subscription.handler() {
                CallbackHandler::Sync(handler) => handler(&*msg),
                CallbackHandler::Async(handler) => {
                    let owned: Arc<dyn std::any::Any + Send + Sync> = msg.clone();
                    handler(owned).await;
                }
            }
        }
    }

    fn register(&self, subscription: CallbackSubscription) -> SubscriptionId {
        let id = SubscriptionId(self.next_id.fetch_add(1, Ordering::Relaxed));
        self.subscriptions.write().push((id, Arc::new(subscription)));
        id
    }

    fn snapshot(&self) -> Vec<(SubscriptionId, Arc<CallbackSubscription>)> {
        self.subscriptions.read().clone()
    }
}
