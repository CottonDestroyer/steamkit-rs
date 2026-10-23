//! macOS helpers for the machine id: the platform serial number and the boot
//! disk's media UUID.
//!
//! These are read with the `ioreg` and `diskutil` tools that ship with macOS,
//! since this crate forbids `unsafe` and can't call IOKit / DiskArbitration
//! directly. If they fail, the caller falls back to a stable value so the
//! machine id doesn't change between runs.

/// The IORegistry key of the platform serial number.
pub(crate) const K_IO_PLATFORM_SERIAL_NUMBER_KEY: &str = "IOPlatformSerialNumber";

/// The disk description key of the media UUID.
pub(crate) const K_DA_DISK_DESCRIPTION_MEDIA_UUID_KEY: &str = "DAMediaUUID";

/// The `statfs` structure from `<sys/mount.h>`. Calling `statfs64` would need
/// `unsafe`, so this is kept for reference only.
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct StatFS {
    /// Fundamental file system block size.
    pub f_bsize: u32,
    /// Optimal transfer block size.
    pub f_iosize: i32,
    /// Total data blocks in file system.
    pub f_blocks: u64,
    /// Free blocks in fs.
    pub f_bfree: u64,
    /// Free blocks available to non-superuser.
    pub f_bavail: u64,
    /// Total file nodes in file system.
    pub f_files: u64,
    /// Free file nodes in fs.
    pub f_ffree: u64,
    /// File system id.
    pub f_fsid: [u32; 2],
    /// User that mounted the filesystem.
    pub f_owner: u32,
    /// Type of filesystem.
    pub f_type: u32,
    /// Copy of mount exported flags.
    pub f_flags: u32,
    /// Filesystem sub-type (flavor).
    pub f_fssubtype: u32,
    /// Filesystem type name.
    pub f_fstypename: String,
    /// Directory on which mounted.
    pub f_mntonname: String,
    /// Mounted filesystem.
    pub f_mntfromname: String,
    /// Reserved for future use.
    pub f_reserved: [u32; 8],
}

/// The platform serial number: the `IOPlatformSerialNumber` property of the
/// `IOPlatformExpertDevice` service.
pub(crate) fn platform_serial_number() -> Option<String> {
    let output = run_command("ioreg", &["-rd1", "-c", "IOPlatformExpertDevice"])?;

    parse_quoted_property(&output, K_IO_PLATFORM_SERIAL_NUMBER_KEY)
}

/// The media UUID of the disk holding `mount_point`.
pub(crate) fn media_uuid(mount_point: &str) -> Option<String> {
    let output = run_command("diskutil", &["info", mount_point])?;

    parse_property_line(&output, "Volume UUID:")
}

/// Runs a command and returns its stdout.
fn run_command(program: &str, arguments: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(arguments)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout).ok()
}

/// The value of a quoted property in `ioreg` output, from lines like
/// `"IOPlatformSerialNumber" = "C02XXXX"`.
fn parse_quoted_property(output: &str, key: &str) -> Option<String> {
    for line in output.lines() {
        let Some(rest) = line.trim().strip_prefix('"') else {
            continue;
        };

        let Some((name, rest)) = rest.split_once('"') else {
            continue;
        };

        if name != key {
            continue;
        }

        let Some(rest) = rest.trim().strip_prefix('=') else {
            continue;
        };

        let rest = rest.trim();
        let Some(rest) = rest.strip_prefix('"') else {
            continue;
        };

        let Some(value) = rest.strip_suffix('"') else {
            continue;
        };

        return Some(value.to_string());
    }

    None
}

/// The value of a property in `diskutil` output, from lines like
/// `   Volume UUID:  1B2C3D4E-...`.
fn parse_property_line(output: &str, key: &str) -> Option<String> {
    for line in output.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };

        if name.trim() != key.trim() {
            continue;
        }

        let value = value.trim();

        if value.is_empty() || value == "(null)" {
            return None;
        }

        return Some(value.to_string());
    }

    None
}
