//! Writes the captured traffic to `<steam directory>\nethook\<timestamp>\`,
//! one file per message. The file names are what the nethook analyzer
//! parses (see [`capture_file_name`]).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use prost::Message;
use steamkit_protos::gc::CMsgProtoBufHeader;
use steamkit_protos::steam::CMsgMulti;
use windows::core::PCSTR;
use windows::Win32::Foundation::{CloseHandle, GENERIC_WRITE, HANDLE, MAX_PATH};
use windows::Win32::Storage::FileSystem::{
    CreateDirectoryA, CreateFileA, DeleteFileA, MoveFileA, SetFilePointer, WriteFile,
    CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, FILE_END, FILE_SHARE_READ, OPEN_ALWAYS,
};
use windows::Win32::System::Console::{GetStdHandle, STD_OUTPUT_HANDLE};
use windows::Win32::System::LibraryLoader::GetModuleFileNameA;

use crate::binaryreader::BinaryReader;
use crate::crypto::message_name;
use crate::steam::emsg::EMsg;
use crate::zip;

/// The direction a captured message travelled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetDirection {
    /// The message was received from the Steam server.
    Incoming,
    /// The message was sent to the Steam server.
    Outgoing,
}

impl NetDirection {
    /// Returns the two letter tag the file name uses for this direction.
    pub fn tag(self) -> &'static str {
        match self {
            NetDirection::Incoming => "in",
            NetDirection::Outgoing => "out",
        }
    }
}

/// Builds the name of the file a captured message is written to.
///
/// The format is `%03u_%s_%d_%s`: the sequence number padded to at least
/// three digits, the direction, the message number and the message name. The
/// analyzer finds the first three parts with the regex
/// `(?<num>\d+)_(?<direction>in|out)_(?<emsg>\d+)`, so don't change it.
pub fn capture_file_name(sequence: u32, direction: NetDirection, emsg: u32, name: &str) -> String {
    format!(
        "{:03}_{}_{}_{}",
        sequence,
        direction.tag(),
        emsg,
        name
    )
}

/// The global logger.
static LOGGER: OnceLock<Logger> = OnceLock::new();

/// Creates the global logger.
pub fn init_logger() {
    let _ = LOGGER.set(Logger::new());
}

/// Returns the global logger.
///
/// The logger is created while the DLL is being attached, before any message can be
/// captured, so a missing logger is a programming error.
pub fn logger() -> &'static Logger {
    LOGGER
        .get()
        .expect("the logger is created before any message can be logged")
}

/// Writes the captured traffic to disk.
#[derive(Debug)]
pub struct Logger {
    root_dir: Vec<u8>,
    log_dir: Vec<u8>,
    msg_num: AtomicU32,
}

impl Logger {
    /// Creates a logger and the directory the dumps are written to.
    pub fn new() -> Logger {
        let module_path = current_module_path();
        let root = match module_path.iter().rposition(|byte| *byte == b'\\') {
            Some(index) => &module_path[..index],
            None => &module_path[..],
        };

        let mut root_dir = root.to_vec();
        root_dir.extend_from_slice(b"\\nethook\\");

        // SAFETY: `root_dir` is NUL terminated.
        unsafe { CreateDirectoryA(PCSTR(nul_terminated(&root_dir).as_ptr()), None).ok() };

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);

        let mut log_dir = root_dir.clone();
        log_dir.extend_from_slice(now.to_string().as_bytes());
        log_dir.push(b'\\');

        // SAFETY: `log_dir` is NUL terminated.
        unsafe { CreateDirectoryA(PCSTR(nul_terminated(&log_dir).as_ptr()), None).ok() };

        Logger {
            root_dir,
            log_dir,
            msg_num: AtomicU32::new(0),
        }
    }

    /// Writes a line to the console the DLL allocated for itself.
    pub fn log_console(&self, message: &str) {
        // SAFETY: writing to the standard output handle is always allowed.
        unsafe {
            let handle = GetStdHandle(STD_OUTPUT_HANDLE).unwrap_or(HANDLE(std::ptr::null_mut()));
            let _ = WriteFile(handle, Some(message.as_bytes()), None, None);
        }
    }

    /// Deletes a file from the root or from the session directory.
    pub fn delete_file(&self, file_name: &str, session: bool) {
        let mut path = if session {
            self.log_dir.clone()
        } else {
            self.root_dir.clone()
        };
        path.extend_from_slice(file_name.as_bytes());

        // SAFETY: `path` is NUL terminated.
        unsafe { DeleteFileA(PCSTR(nul_terminated(&path).as_ptr())).ok() };
    }

    /// Opens a file in the root or in the session directory for writing.
    pub fn open_file(&self, file_name: &str, session: bool) -> HANDLE {
        let mut path = if session {
            self.log_dir.clone()
        } else {
            self.root_dir.clone()
        };
        path.extend_from_slice(file_name.as_bytes());

        // SAFETY: `path` is NUL terminated.
        unsafe {
            CreateFileA(
                PCSTR(nul_terminated(&path).as_ptr()),
                GENERIC_WRITE.0,
                FILE_SHARE_READ,
                None,
                OPEN_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
            .unwrap_or(HANDLE(std::ptr::null_mut()))
        }
    }

    /// Closes a file that was opened with [`Logger::open_file`].
    pub fn close_file(&self, file: HANDLE) {
        // SAFETY: the handle was opened by this process.
        unsafe { CloseHandle(file).ok() };
    }

    /// Appends a line to an open file.
    pub fn log_open_file(&self, file: HANDLE, message: &str) {
        // SAFETY: the handle was opened by this process for writing.
        unsafe {
            SetFilePointer(file, 0, None, FILE_END);
            let _ = WriteFile(file, Some(message.as_bytes()), None, None);
        }
    }

    /// Records a captured message.
    pub fn log_net_message(&self, direction: NetDirection, data: &[u8]) {
        if data.len() < 2 {
            return;
        }

        let emsg = EMsg::from_raw(u16::from_le_bytes([data[0], data[1]]) as u32);

        if emsg == EMsg::MULTI {
            self.multiplex_multi(direction, data);
            return;
        }

        self.log_session_data(direction, data);
    }

    /// Writes a captured message to its own file.
    pub fn log_session_data(&self, direction: NetDirection, data: &[u8]) {
        let emsg = if data.len() >= 2 {
            EMsg::from_raw(u16::from_le_bytes([data[0], data[1]]) as u32)
        } else {
            EMsg::from_raw(0)
        };

        let name = self.file_name_base(direction, emsg, 0xFF);

        let mut full_file = self.log_dir.clone();
        full_file.extend_from_slice(name.as_bytes());

        let mut temporary = full_file.clone();
        temporary.extend_from_slice(b".tmp");

        let mut final_name = full_file.clone();
        final_name.extend_from_slice(b".bin");

        // SAFETY: both paths are NUL terminated.
        unsafe {
            if let Ok(handle) = CreateFileA(
                PCSTR(nul_terminated(&temporary).as_ptr()),
                GENERIC_WRITE.0,
                FILE_SHARE_READ,
                None,
                CREATE_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
                None,
            ) {
                let _ = WriteFile(handle, Some(data), None, None);
                let _ = CloseHandle(handle);
            }

            let _ = MoveFileA(
                PCSTR(nul_terminated(&temporary).as_ptr()),
                PCSTR(nul_terminated(&final_name).as_ptr()),
            );
        }

        self.log_console(&format!("Wrote {} bytes to {}\n", data.len(), name));
    }

    /// Builds the base name of the file a message is written to.
    fn file_name_base(&self, direction: NetDirection, emsg: EMsg, server_type: u8) -> String {
        let sequence = self.msg_num.fetch_add(1, Ordering::Relaxed) + 1;

        // Print "(null)" for a missing name, like printf's `%s` would.
        let name = message_name(emsg, server_type).unwrap_or("(null)");

        capture_file_name(sequence, direction, emsg.raw(), name)
    }

    /// Splits a multiplexed batch into the messages it carries.
    fn multiplex_multi(&self, direction: NetDirection, data: &[u8]) {
        if data.len() < 8 {
            return;
        }

        let message = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let header_length = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);

        self.log_console(&format!(
            "Multi: msg {} length {}\n",
            message & 0x7FFF_FFFF,
            header_length
        ));

        let header_length = header_length.max(0) as usize;
        let header_end = (8 + header_length).min(data.len());

        // The protobuf routing header that follows the eight byte batch header.
        let _ = CMsgProtoBufHeader::decode(&data[8..header_end]);
        self.log_console("MultiProto\n");

        let body = CMsgMulti::decode(&data[header_end..]).unwrap_or_default();
        self.log_console(&format!(
            "MultiMsg: {} {}\n",
            body.size_unzipped.unwrap_or(0),
            body.message_body.as_ref().map_or(0, |body| body.len())
        ));

        let size_unzipped = body.size_unzipped.unwrap_or(0);
        let message_body = body.message_body.unwrap_or_default();

        let payload = if size_unzipped != 0 {
            match zip::inflate(&message_body, size_unzipped) {
                Some(decompressed) => decompressed,
                None => {
                    self.log_console("Unable to decompress buffer\n");
                    return;
                }
            }
        } else {
            message_body
        };

        let mut reader = BinaryReader::new(&payload);

        while reader.size_left() > 0 {
            if reader.size_left() < 4 {
                break;
            }

            let length = reader.read_u32();
            let message = reader.read_bytes(length);

            if message.is_empty() {
                break;
            }

            self.log_net_message(direction, message);
        }
    }
}

impl Default for Logger {
    fn default() -> Logger {
        Logger::new()
    }
}

/// Returns a NUL terminated copy of `bytes`.
fn nul_terminated(bytes: &[u8]) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(bytes.len() + 1);
    buffer.extend_from_slice(bytes);
    buffer.push(0);
    buffer
}

/// Returns the path of the module the process was started from.
fn current_module_path() -> Vec<u8> {
    let mut buffer = vec![0u8; MAX_PATH as usize];

    // SAFETY: the buffer is large enough for the path and NUL terminated by the API.
    let length = unsafe { GetModuleFileNameA(None, &mut buffer) } as usize;
    buffer.truncate(length);

    while buffer.last() == Some(&0) {
        buffer.pop();
    }

    buffer
}

#[cfg(test)]
mod tests {
    use super::{capture_file_name, NetDirection};

    #[test]
    fn capture_file_name_matches_the_c_format_string() {
        assert_eq!(
            "001_in_2_ProtoBuf",
            capture_file_name(1, NetDirection::Incoming, 2, "ProtoBuf")
        );
        assert_eq!(
            "001_out_65535_ProtoBuf",
            capture_file_name(1, NetDirection::Outgoing, 65535, "ProtoBuf")
        );
        assert_eq!(
            "042_in_0_Null",
            capture_file_name(42, NetDirection::Incoming, 0, "Null")
        );
        assert_eq!(
            "1000_out_1_Name",
            capture_file_name(1000, NetDirection::Outgoing, 1, "Name")
        );
    }
}
