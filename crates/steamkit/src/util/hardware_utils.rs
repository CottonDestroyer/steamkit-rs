//! The machine identity Steam uses to recognize a machine, sent with the logon
//! as a `MessageObject`.
//!
//! Gathering the values is behind the [`IMachineInfoProvider`] trait, so you
//! can plug in your own.

use std::sync::{Arc, Mutex, OnceLock, Weak};

use sha1::{Digest, Sha1};

use crate::error::SteamError;
use crate::steam::steam_client::configuration::i_machine_info_provider::IMachineInfoProvider;
use crate::types::key_value::KeyValue;
use crate::types::message_object::MessageObject;
use crate::util::debug_log::DebugLog;
use crate::util::utils::Utils;

/// Picks the machine info provider for the current platform.
#[derive(Debug)]
pub struct MachineInfoProvider;

impl MachineInfoProvider {
    /// The provider for the current platform.
    pub fn get_default_provider() -> Arc<dyn IMachineInfoProvider> {
        #[cfg(target_os = "windows")]
        {
            Arc::new(WindowsMachineInfoProvider)
        }

        #[cfg(target_os = "macos")]
        {
            Arc::new(MacOSMachineInfoProvider)
        }

        #[cfg(target_os = "linux")]
        {
            Arc::new(LinuxMachineInfoProvider)
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            Arc::new(DefaultMachineInfoProvider)
        }
    }
}

/// The provider for unknown platforms, also the fallback for any value a
/// platform provider can't get.
#[derive(Debug)]
pub struct DefaultMachineInfoProvider;

impl DefaultMachineInfoProvider {
    /// The shared instance.
    pub fn instance() -> Arc<dyn IMachineInfoProvider> {
        static INSTANCE: OnceLock<Arc<dyn IMachineInfoProvider>> = OnceLock::new();

        INSTANCE
            .get_or_init(|| Arc::new(DefaultMachineInfoProvider))
            .clone()
    }

    /// Combines MAC addresses into the 16 bytes the Steam client hashes. Each
    /// address is padded with two zero bytes (so 6 bytes become 8), and two
    /// zero blocks are appended in case there are fewer than two adapters.
    pub fn get_combined_mac_address(addresses: Vec<Vec<u8>>) -> Vec<u8> {
        let mut sequences: Vec<Vec<u8>> = addresses
            .into_iter()
            .map(|mut address| {
                address.push(0);
                address.push(0);
                address
            })
            .collect();

        sequences.push(vec![0u8; 8]);
        sequences.push(vec![0u8; 8]);

        let mut result = Vec::with_capacity(16);

        for sequence in sequences.into_iter().take(2) {
            result.extend(sequence);
        }

        result
    }
}

impl IMachineInfoProvider for DefaultMachineInfoProvider {
    fn machine_guid(&self) -> Option<Vec<u8>> {
        Some(format!("{}-SteamKit", machine_name()).into_bytes())
    }

    fn mac_address(&self) -> Option<Vec<u8>> {
        // Use the first Ethernet or wireless adapter. Only Linux exposes the
        // addresses without OS calls (through sysfs).
        let addresses = read_mac_addresses(false);

        match addresses.into_iter().next() {
            Some(address) => Some(address),
            None => Some(b"SteamKit-MacAddress".to_vec()),
        }
    }

    fn disk_id(&self) -> Option<Vec<u8>> {
        Some(b"SteamKit-DiskId".to_vec())
    }
}

/// The provider for Windows.
#[derive(Debug)]
pub struct WindowsMachineInfoProvider;

impl IMachineInfoProvider for WindowsMachineInfoProvider {
    fn machine_guid(&self) -> Option<Vec<u8>> {
        query_machine_guid().map(|guid| guid.into_bytes())
    }

    fn mac_address(&self) -> Option<Vec<u8>> {
        // Physical adapters are the ones whose PnP instance id starts with "PCI".
        let addresses = read_physical_mac_addresses();

        Some(DefaultMachineInfoProvider::get_combined_mac_address(addresses))
    }

    fn disk_id(&self) -> Option<Vec<u8>> {
        crate::util::win32_helpers::Win32Helpers::get_boot_disk_serial_number()
            .map(|serial_number| serial_number.into_bytes())
    }
}

/// The provider for Linux.
#[derive(Debug)]
pub struct LinuxMachineInfoProvider;

impl IMachineInfoProvider for LinuxMachineInfoProvider {
    fn machine_guid(&self) -> Option<Vec<u8>> {
        let machine_files = [
            "/var/lib/dbus/machine-id",
            "/etc/machine-id",
            "/etc/hostname",
        ];

        for file_name in machine_files {
            if let Ok(data) = std::fs::read(file_name) {
                return Some(data);
            }
        }

        None
    }

    fn mac_address(&self) -> Option<Vec<u8>> {
        Some(DefaultMachineInfoProvider::get_combined_mac_address(
            read_mac_addresses(true),
        ))
    }

    fn disk_id(&self) -> Option<Vec<u8>> {
        let boot_params = get_boot_options();

        for param in ["root=UUID=", "root=PARTUUID="] {
            if let Some(value) = get_param_value(&boot_params, param) {
                if !value.is_empty() {
                    return Some(value.into_bytes());
                }
            }
        }

        let disk_uuids = get_disk_uuids();

        disk_uuids
            .into_iter()
            .next()
            .map(|uuid| uuid.into_bytes())
    }
}

/// The provider for macOS.
#[derive(Debug)]
pub struct MacOSMachineInfoProvider;

impl IMachineInfoProvider for MacOSMachineInfoProvider {
    fn machine_guid(&self) -> Option<Vec<u8>> {
        // Read through `ioreg`, since calling IOKit directly would need `unsafe`.
        match crate::util::mac_helpers::platform_serial_number() {
            Some(serial_number) => Some(serial_number.into_bytes()),
            None => DefaultMachineInfoProvider::instance().machine_guid(),
        }
    }

    fn mac_address(&self) -> Option<Vec<u8>> {
        // No disk id on macOS.
        None
    }

    fn disk_id(&self) -> Option<Vec<u8>> {
        match crate::util::mac_helpers::media_uuid("/") {
            Some(uuid) => Some(uuid.into_bytes()),
            None => DefaultMachineInfoProvider::instance().disk_id(),
        }
    }
}

/// Holds the machine id generated for a provider.
#[derive(Debug, Default)]
struct MachineIdSlot {
    /// The machine id, computed on first use.
    value: OnceLock<Option<Vec<u8>>>,
}

/// An entry in the generation table.
#[derive(Debug)]
struct GenerationEntry {
    /// The provider this entry belongs to.
    provider: Weak<dyn IMachineInfoProvider>,
    /// Its machine id slot.
    slot: Arc<MachineIdSlot>,
}

impl GenerationEntry {
    /// Whether this entry belongs to `provider`.
    fn matches(&self, provider: &Arc<dyn IMachineInfoProvider>) -> bool {
        match self.provider.upgrade() {
            Some(strong) => Arc::ptr_eq(&strong, provider),
            None => false,
        }
    }
}

/// The generation table, created on first use.
fn generation_table() -> &'static Mutex<Vec<GenerationEntry>> {
    static TABLE: OnceLock<Mutex<Vec<GenerationEntry>>> = OnceLock::new();

    TABLE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Registers a provider and returns its machine id slot.
fn register_provider(provider: &Arc<dyn IMachineInfoProvider>) -> Arc<MachineIdSlot> {
    let mut table = generation_table()
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    if let Some(entry) = table.iter().find(|entry| entry.matches(provider)) {
        return entry.slot.clone();
    }

    let slot = Arc::new(MachineIdSlot::default());

    table.push(GenerationEntry {
        provider: Arc::downgrade(provider),
        slot: slot.clone(),
    });

    slot
}

/// Finds a provider's machine id slot.
fn lookup_provider(provider: &Arc<dyn IMachineInfoProvider>) -> Option<Arc<MachineIdSlot>> {
    let mut table = generation_table()
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    table.retain(|entry| entry.provider.strong_count() > 0);

    table
        .iter()
        .find(|entry| entry.matches(provider))
        .map(|entry| entry.slot.clone())
}

/// Generates the machine id for a provider.
fn generate_machine_id(provider: &dyn IMachineInfoProvider) -> Result<Vec<u8>, SteamError> {
    // The August 2015 CM update made well-formed machine MessageObjects
    // required for logon. That was turned off shortly after, probably because
    // of Linux clients on distros without a machine id, so a valid one isn't
    // required right now. It might be again, so we send one.

    let fallback = DefaultMachineInfoProvider::instance();

    let mut message_object = MessageObject::new();

    {
        let key_values = message_object.key_values_mut();

        key_values.set("BB3", KeyValue::new());
        key_values.set("FF2", KeyValue::new());
        key_values.set("3B3", KeyValue::new());
    }

    // Custom providers can fail for any field; fall back to the default
    // provider then.
    let bb3 = provider
        .machine_guid()
        .or_else(|| fallback.machine_guid())
        .unwrap_or_default();
    let ff2 = provider
        .mac_address()
        .or_else(|| fallback.mac_address())
        .unwrap_or_default();
    let three_b3 = provider
        .disk_id()
        .or_else(|| fallback.disk_id())
        .unwrap_or_default();

    set_child_value(&mut message_object, "BB3", &get_hex_string(&bb3));
    set_child_value(&mut message_object, "FF2", &get_hex_string(&ff2));
    set_child_value(&mut message_object, "3B3", &get_hex_string(&three_b3));

    // 333 is some sort of user-supplied data and is currently unused.

    let mut stream: Vec<u8> = Vec::new();
    message_object.write_to_stream(&mut stream)?;

    Ok(stream)
}

/// Sets the value of the named child.
fn set_child_value(message_object: &mut MessageObject, key: &str, value: &str) {
    let children = message_object.key_values_mut().children_mut();

    if let Some(child) = children
        .iter_mut()
        .find(|child| child.name().is_some_and(|name| name.eq_ignore_ascii_case(key)))
    {
        child.set_value(Some(value.to_string()));
    }
}

/// SHA-1 of `data`, as a lowercase hex string.
fn get_hex_string(data: &[u8]) -> String {
    let digest = Sha1::digest(data);

    Utils::encode_hex_string(digest.as_slice())
}

/// The name of this machine.
fn machine_name() -> String {
    #[cfg(target_os = "windows")]
    {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown".to_string())
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string())
    }
}

/// Reads the machine GUID from the Windows registry.
#[cfg(target_os = "windows")]
fn query_machine_guid() -> Option<String> {
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Cryptography",
            "/v",
            "MachineGuid",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;

    for line in text.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("MachineGuid") {
            return trimmed.split_whitespace().last().map(str::to_string);
        }
    }

    None
}

/// PowerShell script that prints the MAC addresses of the physical network
/// adapters (registry `Connection\PnpInstanceID` starting with "PCI").
///
/// It goes through .NET's `NetworkInterface.GetAllNetworkInterfaces()` on
/// purpose: the addresses are hashed in enumeration order, and that's the
/// order the Steam client sees. WMI's `Win32_NetworkAdapter` enumerates in a
/// different order, which gives a different machine id.
#[cfg(target_os = "windows")]
const PHYSICAL_MAC_ADDRESSES_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$classKey = 'SYSTEM\CurrentControlSet\Control\Network\{4D36E972-E325-11CE-BFC1-08002BE10318}'
[System.Net.NetworkInformation.NetworkInterface]::GetAllNetworkInterfaces() | ForEach-Object {
    $adapter = $_
    $connection = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey("$classKey\$($adapter.Id)\Connection")
    if ($null -ne $connection) {
        $instanceId = $connection.GetValue('PnpInstanceID', '')
        $connection.Close()
        if ($instanceId.Length -gt 3 -and $instanceId.StartsWith('PCI', [System.StringComparison]::Ordinal)) {
            $address = $adapter.GetPhysicalAddress().GetAddressBytes()
            if ($address.Length -gt 0) { -join ($address | ForEach-Object { $_.ToString('X2') }) } else { '-' }
        }
    }
}
"#;

/// The MAC addresses of the physical network adapters, in the order Windows
/// reports them (which is the order they're hashed in).
#[cfg(target_os = "windows")]
fn read_physical_mac_addresses() -> Vec<Vec<u8>> {
    let text = match crate::util::win32_helpers::run_powershell(PHYSICAL_MAC_ADDRESSES_SCRIPT) {
        Ok(text) => text,
        Err(error) => {
            DebugLog::write_line(
                "HardwareUtils",
                &format!("Failed to read the MAC addresses of the network adapters: {error}"),
            );

            return Vec::new();
        }
    };

    text.lines()
        .filter_map(|line| {
            let hex: String = line
                .trim()
                .chars()
                .filter(|character| character.is_ascii_hexdigit())
                .collect();

            if hex.len() % 2 != 0 {
                return None;
            }

            let mut bytes = Vec::with_capacity(hex.len() / 2);

            for pair in hex.as_bytes().chunks(2) {
                let text = std::str::from_utf8(pair).ok()?;
                bytes.push(u8::from_str_radix(text, 16).ok()?);
            }

            Some(bytes)
        })
        .collect()
}

/// The MAC addresses of the network adapters, from sysfs.
#[cfg(target_os = "linux")]
fn read_mac_addresses(check_for_physical_device: bool) -> Vec<Vec<u8>> {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return Vec::new();
    };

    let mut addresses = Vec::new();

    for entry in entries.flatten() {
        let interface = entry.file_name().to_string_lossy().into_owned();

        if check_for_physical_device
            && !std::path::Path::new(&format!("/sys/class/net/{interface}/device")).exists()
        {
            continue;
        }

        let Ok(text) = std::fs::read_to_string(format!("/sys/class/net/{interface}/address")) else {
            continue;
        };

        let hex: String = text
            .trim()
            .chars()
            .filter(|character| character.is_ascii_hexdigit())
            .collect();

        if hex.len() < 2 || hex.len() % 2 != 0 {
            continue;
        }

        let mut bytes = Vec::with_capacity(hex.len() / 2);

        let mut valid = true;

        for pair in hex.as_bytes().chunks(2) {
            match std::str::from_utf8(pair).ok().and_then(|text| u8::from_str_radix(text, 16).ok()) {
                Some(byte) => bytes.push(byte),
                None => {
                    valid = false;
                    break;
                }
            }
        }

        if valid {
            addresses.push(bytes);
        }
    }

    if check_for_physical_device && addresses.is_empty() {
        return read_mac_addresses(false);
    }

    addresses
}

/// The MAC addresses of the network adapters. Only supported where they're
/// readable from the file system; elsewhere there are none.
#[cfg(not(target_os = "linux"))]
fn read_mac_addresses(_check_for_physical_device: bool) -> Vec<Vec<u8>> {
    Vec::new()
}

/// The kernel boot options.
#[cfg(target_os = "linux")]
fn get_boot_options() -> Vec<String> {
    let Ok(text) = std::fs::read_to_string("/proc/cmdline") else {
        return Vec::new();
    };

    text.split(' ')
        .map(|option| option.to_string())
        .collect()
}

/// The kernel boot options (none on this platform).
#[cfg(not(target_os = "linux"))]
fn get_boot_options() -> Vec<String> {
    Vec::new()
}

/// The value of the first boot option starting with `param`.
fn get_param_value(boot_options: &[String], param: &str) -> Option<String> {
    let option = boot_options
        .iter()
        .find(|option| option.len() >= param.len() && option[..param.len()].eq_ignore_ascii_case(param))?;

    Some(option[param.len()..].to_string())
}

/// The UUIDs of the attached disks, oldest first.
#[cfg(target_os = "linux")]
fn get_disk_uuids() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir("/dev/disk/by-uuid") else {
        return Vec::new();
    };

    let mut files: Vec<(std::time::SystemTime, String)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let modified = entry.metadata().ok()?.modified().ok()?;

            Some((modified, name))
        })
        .collect();

    files.sort_by_key(|(modified, _)| *modified);

    files.into_iter().map(|(_, name)| name).collect()
}

/// The UUIDs of the attached disks (none on this platform).
#[cfg(not(target_os = "linux"))]
fn get_disk_uuids() -> Vec<String> {
    Vec::new()
}

/// Generates the machine id that identifies this machine to Steam.
#[derive(Debug)]
pub struct HardwareUtils;

impl HardwareUtils {
    /// Starts generating the machine id for `machine_info_provider` in the
    /// background, so it's ready by the time we log on. Without a tokio runtime
    /// it's generated on first use instead.
    pub fn init(machine_info_provider: Arc<dyn IMachineInfoProvider>) {
        let slot = register_provider(&machine_info_provider);

        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };

        let provider = machine_info_provider.clone();

        handle.spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                slot.value.get_or_init(|| generate_machine_id(provider.as_ref()).ok());
            })
            .await;
        });
    }

    /// The machine id for the provider, or `None` if [`HardwareUtils::init`]
    /// wasn't called for it.
    pub fn get_machine_id(
        machine_info_provider: &Arc<dyn IMachineInfoProvider>,
    ) -> Option<Vec<u8>> {
        let slot = lookup_provider(machine_info_provider)?;

        let value = slot
            .value
            .get_or_init(|| generate_machine_id(machine_info_provider.as_ref()).ok());

        value.clone()
    }
}

/// Same as [`HardwareUtils::get_machine_id`].
pub fn get_machine_id(
    machine_info_provider: &Arc<dyn IMachineInfoProvider>,
) -> Option<Vec<u8>> {
    let Some(slot) = lookup_provider(machine_info_provider) else {
        DebugLog::write_line("HardwareUtils", "GetMachineID() called before Init()");
        return None;
    };

    let value = slot
        .value
        .get_or_init(|| generate_machine_id(machine_info_provider.as_ref()).ok());

    value.clone()
}
