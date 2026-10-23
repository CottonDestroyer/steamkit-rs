//! Writing to the debug log on behalf of a client.

use std::sync::{Arc, OnceLock};

use crate::util::debug_log::{format_with_args, DebugLog};

/// Writes to the debug log in the context of a particular client.
pub trait ILogContext: Send + Sync {
    /// Writes a line to the debug log.
    fn log_debug(&self, category: &str, message: &str);

    /// Writes a line to the debug log, filling `{0}`, `{1}`, ... in `message`
    /// from `args`.
    fn log_debug_args(&self, category: &str, message: &str, args: &[&str]) {
        self.log_debug(category, &format_with_args(message, args));
    }
}

/// The default log context: the global [`DebugLog`].
#[derive(Debug)]
pub(crate) struct DebugLogContext;

impl DebugLogContext {
    /// The shared instance.
    pub(crate) fn instance() -> Arc<dyn ILogContext> {
        static INSTANCE: OnceLock<Arc<dyn ILogContext>> = OnceLock::new();

        INSTANCE
            .get_or_init(|| Arc::new(DebugLogContext))
            .clone()
    }
}

impl ILogContext for DebugLogContext {
    fn log_debug(&self, category: &str, message: &str) {
        DebugLog::write_line(category, message);
    }
}
