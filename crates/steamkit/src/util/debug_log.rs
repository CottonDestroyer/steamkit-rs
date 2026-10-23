//! The global debug log. Listeners are either trait objects or plain functions,
//! and messages are also sent on a broadcast channel for anyone who'd rather
//! subscribe.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use tokio::sync::broadcast;

/// Capacity of the broadcast channel.
const BROADCAST_CAPACITY: usize = 1024;

/// A message that was written to the debug log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugLogMessage {
    /// The category of the message.
    pub category: String,
    /// The message itself.
    pub message: String,
}

/// A debug log listener.
pub trait IDebugListener: Send + Sync {
    /// Called for every line written to the debug log.
    fn write_line(&self, category: &str, msg: &str);
}

/// The global state of the debug log.
struct DebugLogState {
    /// Whether debug logging is enabled.
    enabled: AtomicBool,
    /// The registered listener objects.
    listeners: RwLock<Vec<Arc<dyn IDebugListener>>>,
    /// The registered listener functions, with their handles.
    functions: RwLock<Vec<(u64, Arc<dyn Fn(&str, &str) + Send + Sync>)>>,
    /// The handle the next listener function gets.
    next_function_id: AtomicU64,
    /// The channel log messages are broadcast on.
    sender: broadcast::Sender<DebugLogMessage>,
}

impl DebugLogState {
    fn new() -> DebugLogState {
        let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);

        DebugLogState {
            // On in debug builds, off in release builds.
            enabled: AtomicBool::new(cfg!(debug_assertions)),
            listeners: RwLock::new(Vec::new()),
            functions: RwLock::new(Vec::new()),
            next_function_id: AtomicU64::new(1),
            sender,
        }
    }
}

/// The global debug log state, created on first use.
fn state() -> &'static DebugLogState {
    static STATE: OnceLock<DebugLogState> = OnceLock::new();

    STATE.get_or_init(DebugLogState::new)
}

/// The global debug log.
#[derive(Debug)]
pub struct DebugLog;

impl DebugLog {
    /// Whether debug logging is on. Defaults to on in debug builds and off in
    /// release builds.
    pub fn enabled() -> bool {
        state().enabled.load(Ordering::Relaxed)
    }

    /// Turns debug logging on or off.
    pub fn set_enabled(enabled: bool) {
        state().enabled.store(enabled, Ordering::Relaxed);
    }

    /// Adds a listener.
    pub fn add_listener(listener: Arc<dyn IDebugListener>) {
        state().listeners.write().unwrap_or_else(|error| error.into_inner()).push(listener);
    }

    /// Adds a listener function. Closures can't be compared, so use the
    /// returned handle with [`DebugLog::remove_listener_function`] to remove it.
    pub fn add_listener_function(
        action: impl Fn(&str, &str) + Send + Sync + 'static,
    ) -> u64 {
        let id = state().next_function_id.fetch_add(1, Ordering::Relaxed);

        state()
            .functions
            .write()
            .unwrap_or_else(|error| error.into_inner())
            .push((id, Arc::new(action)));

        id
    }

    /// Removes a listener (compared by identity).
    pub fn remove_listener(listener: &Arc<dyn IDebugListener>) {
        let mut listeners = state().listeners.write().unwrap_or_else(|error| error.into_inner());

        if let Some(index) = listeners
            .iter()
            .position(|registered| Arc::ptr_eq(registered, listener))
        {
            listeners.remove(index);
        }
    }

    /// Removes a listener function added with
    /// [`DebugLog::add_listener_function`].
    pub fn remove_listener_function(id: u64) {
        let mut functions = state().functions.write().unwrap_or_else(|error| error.into_inner());

        if let Some(index) = functions.iter().position(|(registered, _)| *registered == id) {
            functions.remove(index);
        }
    }

    /// Removes all listeners.
    pub fn clear_listeners() {
        state()
            .listeners
            .write()
            .unwrap_or_else(|error| error.into_inner())
            .clear();

        state()
            .functions
            .write()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }

    /// Subscribes to debug log messages. A receiver that falls behind gets
    /// `RecvError::Lagged` and misses messages.
    pub fn subscribe() -> broadcast::Receiver<DebugLogMessage> {
        state().sender.subscribe()
    }

    /// Writes a line to the debug log.
    pub fn write_line(category: &str, msg: &str) {
        let state = state();

        if !state.enabled.load(Ordering::Relaxed) {
            return;
        }

        let message = DebugLogMessage {
            category: category.to_string(),
            message: msg.to_string(),
        };

        // Sending only fails when nobody's subscribed, which is fine.
        let _ = state.sender.send(message);

        let listeners = state.listeners.read().unwrap_or_else(|error| error.into_inner());

        for listener in listeners.iter() {
            listener.write_line(category, msg);
        }

        drop(listeners);

        let functions = state.functions.read().unwrap_or_else(|error| error.into_inner());

        for (_, action) in functions.iter() {
            action(category, msg);
        }
    }

    /// Writes a line to the debug log, filling `{0}`, `{1}`, ... in `msg` from
    /// `args` (`{{` and `}}` are literal braces). Format specifiers like
    /// `{0:X}` aren't supported and are left as they are.
    pub fn write_line_args(category: &str, msg: &str, args: &[&str]) {
        DebugLog::write_line(category, &format_with_args(msg, args));
    }

    /// If `condition` is false, writes `message` to the debug log.
    pub fn assert(condition: bool, category: &str, message: &str) {
        if !condition {
            DebugLog::write_line(category, &format!("Assertion Failed! {message}"));
        }
    }
}

/// Fills `{0}`, `{1}`, ... in `msg` from `args`. `{{` and `}}` are literal
/// braces; placeholders with a format specifier or alignment are left as they
/// are.
pub fn format_with_args(msg: &str, args: &[&str]) -> String {
    let mut result = String::with_capacity(msg.len());
    let mut characters = msg.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '{' => {
                if characters.peek() == Some(&'{') {
                    characters.next();
                    result.push('{');
                    continue;
                }

                let mut placeholder = String::new();
                let mut closed = false;

                for character in characters.by_ref() {
                    if character == '}' {
                        closed = true;
                        break;
                    }

                    placeholder.push(character);
                }

                if !closed {
                    // Unterminated placeholder: copy the rest as-is.
                    result.push('{');
                    result.push_str(&placeholder);
                    break;
                }

                match placeholder.parse::<usize>() {
                    Ok(index) => match args.get(index) {
                        Some(value) => result.push_str(value),
                        None => {
                            result.push('{');
                            result.push_str(&placeholder);
                            result.push('}');
                        }
                    },
                    Err(_) => {
                        result.push('{');
                        result.push_str(&placeholder);
                        result.push('}');
                    }
                }
            }
            '}' => {
                if characters.peek() == Some(&'}') {
                    characters.next();
                    result.push('}');
                    continue;
                }

                result.push('}');
            }
            _ => result.push(character),
        }
    }

    result
}
