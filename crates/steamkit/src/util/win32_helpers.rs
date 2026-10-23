//! Windows helpers for the machine id: the boot disk's serial number.
//!
//! This uses PowerShell's `Get-Partition` and `Get-Disk` rather than
//! `DeviceIoControl`, since this crate forbids `unsafe`.

use crate::util::debug_log::DebugLog;

/// PowerShell script that prints the boot disk's serial number.
const BOOT_DISK_SERIAL_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$letter = $env:SystemDrive.TrimEnd(':')
$partition = Get-Partition -DriveLetter $letter
$disk = Get-Disk -Number $partition.DiskNumber
if ($null -eq $disk.SerialNumber) { throw 'The boot disk has no serial number.' }
$disk.SerialNumber.Trim()
"#;

/// Windows helpers for the machine id.
#[derive(Debug)]
pub(crate) struct Win32Helpers;

impl Win32Helpers {
    /// The serial number of the physical drive holding the boot volume, or
    /// `None` if it can't be determined.
    pub(crate) fn get_boot_disk_serial_number() -> Option<String> {
        match run_powershell(BOOT_DISK_SERIAL_SCRIPT) {
            Ok(serial_number) => Some(serial_number),
            Err(error) => {
                DebugLog::write_line(
                    "Win32Helpers",
                    &format!("Failed to get boot disk serial number: {error}"),
                );

                None
            }
        }
    }
}

/// Runs a PowerShell script and returns its trimmed output.
pub(crate) fn run_powershell(script: &str) -> Result<String, String> {
    let mut last_error = String::from("no PowerShell interpreter was found");

    for program in ["powershell", "pwsh"] {
        let output = match std::process::Command::new(program)
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
        {
            Ok(output) => output,
            Err(error) => {
                last_error = format!("{program} could not be started: {error}");
                continue;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

            last_error = if stderr.is_empty() {
                format!("{program} exited with {}", output.status)
            } else {
                stderr
            };

            continue;
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

        if stdout.is_empty() {
            last_error = format!("{program} did not print anything");
            continue;
        }

        return Ok(stdout);
    }

    Err(last_error)
}
