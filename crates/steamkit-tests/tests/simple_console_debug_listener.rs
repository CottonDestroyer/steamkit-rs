//! `SimpleConsoleDebugListener` test.
//!
//! There's no supported way to replace stdout in-process, so the listener
//! runs in a child process (this test binary re-run with a filter) and the
//! parent captures its output.

use std::process::Command;

use steamkit::util::debug_log::IDebugListener;
use steamkit::util::simple_console_debug_listener::SimpleConsoleDebugListener;

/// The environment variable that turns the helper test into a "print one line" run.
const CHILD_ENV: &str = "STEAMKIT_TEST_PRINT_LINE";

/// The helper test that [`writes_output_to_console`] runs in a child process.
///
/// It is ignored by default so that it only ever runs when it is started
/// explicitly by the parent test.
#[test]
#[ignore = "helper for writes_output_to_console; it is started in a child process"]
fn prints_the_line_to_stdout() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }

    let listener = SimpleConsoleDebugListener::new();
    listener.write_line("some cat", "a message");

    // Leave the harness before it writes its own summary, so that the captured
    // standard output holds nothing but the line of the listener.
    std::process::exit(0);
}

#[test]
fn writes_output_to_console() {
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => panic!("the path of the test binary must be available: {error}"),
    };

    let output = Command::new(executable)
        .args([
            "--exact",
            "prints_the_line_to_stdout",
            "--ignored",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .output()
        .unwrap_or_else(|error| panic!("the child process must run: {error}"));

    assert!(
        output.status.success(),
        "the child process must succeed, got: {:?}",
        output.status
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // The test harness writes its own progress lines to the standard output,
    // so the line of the listener is picked out of them.
    let line = stdout
        .lines()
        .find(|line| line.starts_with("[some cat]"))
        .unwrap_or_else(|| panic!("the listener must write a line, got: {stdout}"));

    assert_eq!("[some cat]: a message", line);
}
