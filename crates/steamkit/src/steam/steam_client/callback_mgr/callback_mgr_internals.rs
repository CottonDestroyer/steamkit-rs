//! Bookkeeping for the registered subscriptions.

use std::any::{Any, TypeId};
use std::fmt;
use std::sync::Arc;

use futures::future::BoxFuture;

use crate::types::job_id::JobID;

use super::callback_msg::CallbackMsg;

/// What gets called when a subscribed callback is dispatched.
pub enum CallbackHandler {
    /// A sync handler, getting the callback by reference.
    Sync(Box<dyn Fn(&dyn CallbackMsg) + Send + Sync>),
    /// An async handler, getting an owned callback.
    Async(Box<dyn Fn(Arc<dyn Any + Send + Sync>) -> BoxFuture<'static, ()> + Send + Sync>),
}

impl fmt::Debug for CallbackHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallbackHandler::Sync(_) => f.write_str("CallbackHandler::Sync"),
            CallbackHandler::Async(_) => f.write_str("CallbackHandler::Async"),
        }
    }
}

/// A registered subscription.
pub struct CallbackSubscription {
    type_id: TypeId,
    job_id: Option<JobID>,
    handler: CallbackHandler,
}

impl CallbackSubscription {
    /// Creates a subscription for callbacks of `type_id`, optionally limited to
    /// one job.
    pub fn new(type_id: TypeId, job_id: Option<JobID>, handler: CallbackHandler) -> Self {
        Self {
            type_id,
            job_id,
            handler,
        }
    }

    /// The callback type this subscription is for.
    pub fn callback_type(&self) -> TypeId {
        self.type_id
    }

    /// The job this subscription is limited to, if any.
    pub fn job_id(&self) -> Option<JobID> {
        self.job_id
    }

    /// Whether this subscription wants the given callback.
    pub fn matches(&self, type_id: TypeId, job_id: JobID) -> bool {
        if self.type_id != type_id {
            return false;
        }

        match self.job_id {
            None => true,
            Some(id) => id == JobID::INVALID || id == job_id,
        }
    }

    /// The handler of this subscription.
    pub fn handler(&self) -> &CallbackHandler {
        &self.handler
    }
}

impl fmt::Debug for CallbackSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallbackSubscription")
            .field("type_id", &self.type_id)
            .field("job_id", &self.job_id)
            .field("handler", &self.handler)
            .finish()
    }
}

/// A typed, sync subscription.
pub struct Callback<TCall>
where
    TCall: CallbackMsg + 'static,
{
    job_id: Option<JobID>,
    on_run: Box<dyn Fn(&TCall) + Send + Sync>,
}

impl<TCall> Callback<TCall>
where
    TCall: CallbackMsg + 'static,
{
    /// Creates a subscription for callbacks of type `TCall`.
    pub fn new(on_run: Box<dyn Fn(&TCall) + Send + Sync>, job_id: Option<JobID>) -> Self {
        Self { job_id, on_run }
    }

    /// The job this subscription is limited to, if any.
    pub fn job_id(&self) -> Option<JobID> {
        self.job_id
    }

    /// Converts into the type-erased form the manager stores.
    pub fn into_subscription(self) -> CallbackSubscription {
        let type_id = TypeId::of::<TCall>();
        let on_run = self.on_run;

        CallbackSubscription::new(
            type_id,
            self.job_id,
            CallbackHandler::Sync(Box::new(move |msg: &dyn CallbackMsg| {
                if let Some(callback) = msg.as_any().downcast_ref::<TCall>() {
                    on_run(callback);
                }
            })),
        )
    }
}

impl<TCall> fmt::Debug for Callback<TCall>
where
    TCall: CallbackMsg + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Callback")
            .field("type_id", &TypeId::of::<TCall>())
            .field("job_id", &self.job_id)
            .finish()
    }
}

/// A typed, async subscription.
pub struct AsyncCallback<TCall>
where
    TCall: CallbackMsg + 'static,
{
    job_id: Option<JobID>,
    on_run: Box<dyn Fn(Arc<TCall>) -> BoxFuture<'static, ()> + Send + Sync>,
}

impl<TCall> AsyncCallback<TCall>
where
    TCall: CallbackMsg + 'static,
{
    /// Creates a subscription for callbacks of type `TCall`.
    pub fn new(on_run: Box<dyn Fn(Arc<TCall>) -> BoxFuture<'static, ()> + Send + Sync>, job_id: Option<JobID>) -> Self {
        Self { job_id, on_run }
    }

    /// The job this subscription is limited to, if any.
    pub fn job_id(&self) -> Option<JobID> {
        self.job_id
    }

    /// Converts into the type-erased form the manager stores.
    pub fn into_subscription(self) -> CallbackSubscription {
        let type_id = TypeId::of::<TCall>();
        let on_run = self.on_run;

        CallbackSubscription::new(
            type_id,
            self.job_id,
            CallbackHandler::Async(Box::new(move |msg: Arc<dyn Any + Send + Sync>| {
                match msg.downcast::<TCall>() {
                    Ok(callback) => on_run(callback),
                    Err(_) => Box::pin(async {}),
                }
            })),
        )
    }
}

impl<TCall> fmt::Debug for AsyncCallback<TCall>
where
    TCall: CallbackMsg + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncCallback")
            .field("type_id", &TypeId::of::<TCall>())
            .field("job_id", &self.job_id)
            .finish()
    }
}
