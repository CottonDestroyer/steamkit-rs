//! The callbacks the client posts for its own lifecycle: connected and
//! disconnected.

use std::any::Any;

use crate::types::job_id::JobID;

use super::callback_mgr::callback_msg::{CallbackMsg, CallbackMsgBase};

/// Posted once the client has connected to Steam.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConnectedCallback {
    base: CallbackMsgBase,
}

impl ConnectedCallback {
    /// Creates the callback.
    pub fn new() -> Self {
        Self {
            base: CallbackMsgBase::new(),
        }
    }
}

impl CallbackMsg for ConnectedCallback {
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

/// Posted when the client's connection to Steam is gone.
#[derive(Clone, Copy, Debug, Default)]
pub struct DisconnectedCallback {
    base: CallbackMsgBase,
    user_initiated: bool,
}

impl DisconnectedCallback {
    /// Creates the callback.
    pub fn new(user_initiated: bool) -> Self {
        Self {
            base: CallbackMsgBase::new(),
            user_initiated,
        }
    }

    /// `true` if the disconnect came from calling `disconnect`; `false` if it
    /// was something else, like a network failure or the server closing the
    /// connection.
    pub fn user_initiated(&self) -> bool {
        self.user_initiated
    }
}

impl CallbackMsg for DisconnectedCallback {
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
