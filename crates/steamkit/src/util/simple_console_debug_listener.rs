//! A debug listener that prints to the console.

use crate::util::debug_log::IDebugListener;

/// A debug listener that prints to the console.
#[derive(Debug)]
pub struct SimpleConsoleDebugListener;

impl SimpleConsoleDebugListener {
    /// Creates the listener.
    pub fn new() -> SimpleConsoleDebugListener {
        SimpleConsoleDebugListener
    }
}

impl Default for SimpleConsoleDebugListener {
    fn default() -> SimpleConsoleDebugListener {
        SimpleConsoleDebugListener::new()
    }
}

impl IDebugListener for SimpleConsoleDebugListener {
    /// Prints a debug log line.
    fn write_line(&self, category: &str, msg: &str) {
        println!("[{category}]: {msg}");
    }
}
