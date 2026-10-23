//! The [`CallbackMsg`] trait, implemented by every callback a client posts.

use std::any::Any;

use crate::types::job_id::JobID;

/// A callback. Implementors store the job id the callback belongs to; embed a
/// [`CallbackMsgBase`] for that.
pub trait CallbackMsg: Any + Send + Sync {
    /// The job this callback belongs to, or [`JobID::INVALID`] if it isn't a
    /// job callback.
    fn job_id(&self) -> JobID;

    /// Sets the job this callback belongs to.
    fn set_job_id(&mut self, id: JobID);

    /// This callback as [`Any`], for downcasting to the concrete type.
    fn as_any(&self) -> &dyn Any;

    /// This callback as a mutable [`Any`], for downcasting to the concrete type.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Storage for the job id of a [`CallbackMsg`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CallbackMsgBase {
    job_id: JobID,
}

impl CallbackMsgBase {
    /// Creates a base whose job id is [`JobID::INVALID`].
    pub fn new() -> Self {
        Self {
            job_id: JobID::INVALID,
        }
    }

    /// Creates a base with the given job id.
    pub fn with_job_id(job_id: JobID) -> Self {
        Self { job_id }
    }

    /// The stored job id.
    pub fn job_id(&self) -> JobID {
        self.job_id
    }

    /// Stores a job id.
    pub fn set_job_id(&mut self, id: JobID) {
        self.job_id = id;
    }
}

impl Default for CallbackMsgBase {
    fn default() -> Self {
        Self::new()
    }
}
