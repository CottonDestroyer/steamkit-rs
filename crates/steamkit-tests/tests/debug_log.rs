//! `DebugLog` tests.
//!
//! `DebugLog` is process-wide state, so the tests in this file can't run at
//! the same time. Cargo runs the tests of one binary on parallel threads, so a
//! mutex serialises them. (Other test files are separate binaries and can't
//! interfere.)

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use steamkit::util::debug_log::{DebugLog, IDebugListener};

#[allow(dead_code)]
mod common;

use common::{connected_steam_client, connected_steam_client_with_identifier};

/// Serialises every test in this file.
static DEBUG_LOG_LOCK: Mutex<()> = Mutex::new(());

/// Holds [`DEBUG_LOG_LOCK`] for the duration of a test. Clears the listeners
/// first, and afterwards disables the log and clears them again.
struct DebugLogTestGuard {
    _lock: MutexGuard<'static, ()>,
}

impl DebugLogTestGuard {
    fn acquire() -> Self {
        // Ignore poisoning: the global state is reset on every acquire anyway.
        let lock = DEBUG_LOG_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        DebugLog::clear_listeners();
        DebugLogTestGuard { _lock: lock }
    }
}

impl Drop for DebugLogTestGuard {
    fn drop(&mut self) {
        DebugLog::set_enabled(false);
        DebugLog::clear_listeners();
    }
}

/// A listener that records its calls and the last `(category, msg)` pair it
/// saw.
struct TestListener {
    calls: AtomicUsize,
    last: Mutex<(String, String)>,
}

impl TestListener {
    fn new() -> Arc<Self> {
        Arc::new(TestListener {
            calls: AtomicUsize::new(0),
            last: Mutex::new((String::new(), String::new())),
        })
    }
}

impl IDebugListener for TestListener {
    fn write_line(&self, category: &str, msg: &str) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self
            .last
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = (category.to_string(), msg.to_string());
    }
}

#[test]
fn debug_log_action_listener_logs_message() {
    let _guard = DebugLogTestGuard::acquire();
    DebugLog::set_enabled(true);

    let calls = Arc::new(AtomicUsize::new(0));
    let last = Arc::new(Mutex::new((String::new(), String::new())));
    let calls_in_listener = calls.clone();
    let last_in_listener = last.clone();
    let _handle = DebugLog::add_listener_function(move |category, msg| {
        calls_in_listener.fetch_add(1, Ordering::SeqCst);
        *last_in_listener
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = (category.to_string(), msg.to_string());
    });

    DebugLog::write_line("category", "msg");

    assert_eq!(calls.load(Ordering::SeqCst), 1, "the action listener is called once");
    let (category, msg) = &*last.lock().unwrap_or_else(|error| error.into_inner());
    assert_eq!(category, "category");
    assert_eq!(msg, "msg");
}

#[test]
fn debug_log_debug_listener_logs_message() {
    let _guard = DebugLogTestGuard::acquire();
    DebugLog::set_enabled(true);

    let listener = TestListener::new();
    DebugLog::add_listener(listener.clone() as Arc<dyn IDebugListener>);

    DebugLog::write_line("category", "msg");

    assert_eq!(listener.calls.load(Ordering::SeqCst), 1);
    let (category, msg) = &*listener.last.lock().unwrap_or_else(|error| error.into_inner());
    assert_eq!(category, "category");
    assert_eq!(msg, "msg");
}

#[test]
fn debug_log_doesnt_log_when_disabled() {
    let _guard = DebugLogTestGuard::acquire();
    DebugLog::set_enabled(false);

    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_listener = calls.clone();
    let _handle = DebugLog::add_listener_function(move |_category, _msg| {
        calls_in_listener.fetch_add(1, Ordering::SeqCst);
    });

    DebugLog::write_line("category", "msg");

    // No listener may be called while the log is disabled.
    assert_eq!(calls.load(Ordering::SeqCst), 0, "no listener is called while disabled");
}

#[test]
fn debug_log_adds_and_removes_listener() {
    let _guard = DebugLogTestGuard::acquire();
    DebugLog::set_enabled(true);

    let listener = TestListener::new();

    // There's no listeners() accessor, so check that an added listener
    // receives the line.
    DebugLog::add_listener(listener.clone() as Arc<dyn IDebugListener>);
    DebugLog::write_line("category", "msg");
    assert_eq!(listener.calls.load(Ordering::SeqCst), 1, "the added listener receives the line");

    // A removed listener must not be called again.
    DebugLog::remove_listener(&(listener.clone() as Arc<dyn IDebugListener>));
    DebugLog::write_line("category", "msg");
    assert_eq!(listener.calls.load(Ordering::SeqCst), 1, "the removed listener is not called again");
}

#[test]
fn debug_log_clears_listeners() {
    let _guard = DebugLogTestGuard::acquire();
    DebugLog::set_enabled(true);

    let listener = TestListener::new();

    DebugLog::add_listener(listener.clone() as Arc<dyn IDebugListener>);
    DebugLog::write_line("category", "msg");
    assert_eq!(listener.calls.load(Ordering::SeqCst), 1);

    // A cleared listener must not be called again.
    DebugLog::clear_listeners();
    DebugLog::write_line("category", "msg");
    assert_eq!(listener.calls.load(Ordering::SeqCst), 1, "a cleared listener is not called");
}

#[test]
fn debug_log_can_write_safely_without_params() {
    let _guard = DebugLogTestGuard::acquire();
    DebugLog::set_enabled(true);

    let last = Arc::new(Mutex::new(String::new()));
    let last_in_listener = last.clone();
    let _handle = DebugLog::add_listener_function(move |_category, msg| {
        *last_in_listener.lock().unwrap_or_else(|error| error.into_inner()) = msg.to_string();
    });

    // Without arguments the message is written verbatim (no formatting), so
    // "{0}" survives.
    DebugLog::write_line("category", "msg{0}msg");

    assert_eq!(&*last.lock().unwrap_or_else(|error| error.into_inner()), "msg{0}msg");
}

#[test]
fn debug_log_formats_params() {
    let _guard = DebugLogTestGuard::acquire();
    DebugLog::set_enabled(true);

    let last = Arc::new(Mutex::new(String::new()));
    let last_in_listener = last.clone();
    let _handle = DebugLog::add_listener_function(move |_category, msg| {
        *last_in_listener.lock().unwrap_or_else(|error| error.into_inner()) = msg.to_string();
    });

    // With arguments the placeholders are filled in: "msg1msg2".
    DebugLog::write_line_args("category", "msg{0}{1}{2}", &["1", "msg", "2"]);

    assert_eq!(&*last.lock().unwrap_or_else(|error| error.into_inner()), "msg1msg2");
}

#[test]
fn generated_cm_client_id_prefixed() {
    // The category gets the client id as a prefix. `log_debug` takes no format
    // arguments, so the message is passed already formatted.
    let _guard = DebugLogTestGuard::acquire();
    DebugLog::set_enabled(true);

    let last = Arc::new(Mutex::new((String::new(), String::new())));
    let last_in_listener = last.clone();
    let _handle = DebugLog::add_listener_function(move |category, msg| {
        *last_in_listener
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = (category.to_string(), msg.to_string());
    });

    let client = connected_steam_client();
    client.log_debug("MyCategory", "My 1st message");

    let (category, msg) = &*last.lock().unwrap_or_else(|error| error.into_inner());
    assert_eq!(
        format!("{}/MyCategory", client.id()),
        *category,
        "the category must be prefixed with the id of the client"
    );
    assert_eq!("My 1st message", msg);
}

#[test]
fn custom_cm_client_id_prefixed() {
    // A custom identifier is used as the category prefix.
    let _guard = DebugLogTestGuard::acquire();
    DebugLog::set_enabled(true);

    let last = Arc::new(Mutex::new((String::new(), String::new())));
    let last_in_listener = last.clone();
    let _handle = DebugLog::add_listener_function(move |category, msg| {
        *last_in_listener
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = (category.to_string(), msg.to_string());
    });

    let client = connected_steam_client_with_identifier("My Custom Client");
    client.log_debug("MyCategory", "My 1st message");

    let (category, msg) = &*last.lock().unwrap_or_else(|error| error.into_inner());
    assert_eq!(
        "My Custom Client/MyCategory",
        *category,
        "the category must be prefixed with the id of the client"
    );
    assert_eq!("My 1st message", msg);
}
