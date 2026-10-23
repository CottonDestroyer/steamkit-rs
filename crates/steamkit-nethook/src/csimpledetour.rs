//! A minimal inline function hook (in place of Microsoft Detours).
//!
//! * the original prologue is decoded instruction by instruction and copied
//!   into a freshly allocated executable trampoline,
//! * relative branches and RIP-relative operands are rewritten so they still
//!   point at the same absolute addresses from inside the trampoline,
//! * the target function is patched with an absolute jump to the replacement,
//! * the caller's function pointer is pointed at the trampoline, so calls
//!   through it still reach the original code.
//!
//! Every other thread of the process is suspended while the patch is written.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, THREADENTRY32, TH32CS_SNAPTHREAD,
};
use windows::Win32::System::Memory::{
    VirtualAlloc, VirtualFree, VirtualProtect, VirtualQuery, MEMORY_BASIC_INFORMATION,
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, OpenThread, ResumeThread,
    SuspendThread, THREAD_SUSPEND_RESUME,
};

/// A slot holding the address of a function that may be detoured.
///
/// Atomic so concurrent reads from the hooked function stay well defined.
pub type FnSlot = AtomicPtr<c_void>;

/// The reason an inline hook could not be installed or removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetourError {
    /// The address of the function to hook was null.
    NullTarget,
    /// The prologue of the original function could not be decoded.
    DecodeFailed,
    /// The prologue of the original function contains an instruction that cannot be relocated.
    UnsupportedInstruction,
    /// The memory for the trampoline could not be allocated.
    AllocationFailed,
    /// A relocated branch did not fit into its displacement field.
    DisplacementOutOfRange,
    /// The memory protection of the target could not be changed.
    ProtectionFailed,
    /// The memory around the target could not be queried.
    QueryFailed,
}

/// The maximum number of bytes read from the start of the hooked function.
const MAX_PROLOGUE: usize = 64;

/// Returns the number of bytes the jump over the original prologue occupies.
const fn patch_size() -> usize {
    if cfg!(target_pointer_width = "64") {
        14
    } else {
        5
    }
}

/// Returns the number of bytes the jump back into the original function occupies.
const fn jump_back_size() -> usize {
    patch_size()
}

/// An inline hook that redirects a function to a replacement.
#[derive(Debug)]
pub struct SimpleDetour {
    slot: &'static FnSlot,
    replacement: *mut c_void,
    original: *mut c_void,
    trampoline: *mut c_void,
    saved: Vec<u8>,
    attached: bool,
}

impl SimpleDetour {
    /// Creates a detour that will replace the function currently held by `slot`.
    pub fn new(slot: &'static FnSlot, replacement: *mut c_void) -> SimpleDetour {
        SimpleDetour {
            slot,
            replacement,
            original: ptr::null_mut(),
            trampoline: ptr::null_mut(),
            saved: Vec::new(),
            attached: false,
        }
    }

    /// Installs the hook.
    ///
    /// On success the slot holds the address of the trampoline that runs the original
    /// prologue, so calling through the slot still reaches the original function.
    pub fn attach(&mut self) -> Result<(), DetourError> {
        if self.attached {
            return Ok(());
        }

        let original = self.slot.load(Ordering::Acquire);
        if original.is_null() {
            return Err(DetourError::NullTarget);
        }

        let _suspended = suspend_other_threads();

        let hook = unsafe { install_hook(original, self.replacement)? };

        self.original = original;
        self.trampoline = hook.trampoline;
        self.saved = hook.saved;
        self.slot.store(hook.trampoline, Ordering::Release);
        self.attached = true;

        Ok(())
    }

    /// Removes the hook and restores the original bytes of the function.
    pub fn detach(&mut self) -> Result<(), DetourError> {
        if !self.attached {
            return Ok(());
        }

        let _suspended = suspend_other_threads();

        unsafe {
            write_code(self.original, &self.saved)?;
            VirtualFree(self.trampoline, 0, MEM_RELEASE).ok();
        }

        self.slot.store(self.original, Ordering::Release);
        self.trampoline = ptr::null_mut();
        self.saved.clear();
        self.attached = false;

        Ok(())
    }

    /// Returns `true` while the hook is installed.
    pub fn is_attached(&self) -> bool {
        self.attached
    }
}

impl Drop for SimpleDetour {
    fn drop(&mut self) {
        let _ = self.detach();
    }
}

// SAFETY: the detour is installed and removed while the DLL is being attached or
// detached, before the hooked function can be reached by another thread. The pointer
// fields are plain addresses that are only dereferenced by the trampoline builder,
// which runs inside the same critical section.
unsafe impl Send for SimpleDetour {}
unsafe impl Sync for SimpleDetour {}

/// The result of installing an inline hook.
struct InstalledHook {
    /// The address of the trampoline.
    trampoline: *mut c_void,
    /// The bytes that were overwritten at the target.
    saved: Vec<u8>,
}

/// Builds the trampoline, patches the target and returns the installed hook.
///
/// # Safety
///
/// `target` must point at executable code that stays mapped for the lifetime of the
/// process, and `replacement` must be a valid function pointer.
unsafe fn install_hook(
    target: *mut c_void,
    replacement: *mut c_void,
) -> Result<InstalledHook, DetourError> {
    let Trampoline { address, saved } = build_trampoline(target)?;
    let patch = build_patch(target as usize, replacement as usize)?;

    write_code(target, &patch)?;

    Ok(InstalledHook {
        trampoline: address,
        saved,
    })
}

/// A relocated trampoline.
struct Trampoline {
    /// The address of the trampoline.
    address: *mut c_void,
    /// The original bytes that were overwritten at the target.
    saved: Vec<u8>,
}

/// Builds the trampoline that runs the relocated prologue of `target`.
///
/// # Safety
///
/// `target` must point at valid executable code.
unsafe fn build_trampoline(target: *mut c_void) -> Result<Trampoline, DetourError> {
    let target_addr = target as usize;
    let available = readable_bytes_at(target_addr)?;
    let code = read_bytes(target_addr, available)?;

    // Decode just enough instructions to cover the jump that is written over the
    // original prologue.
    let mut instructions = Vec::new();
    let mut original_len = 0usize;
    while original_len < patch_size() {
        let insn = decoder::decode(&code, original_len).ok_or(DetourError::DecodeFailed)?;
        instructions.push(insn);
        original_len += insn.len;
    }

    // Work out where every instruction ends up inside the trampoline. Short branches
    // have to be widened, so the emitted image is not the same size as the original.
    let mut relocated: Vec<RelocatedInsn> = Vec::with_capacity(instructions.len());
    let mut emitted_offset = 0usize;
    let mut original_offset = 0usize;
    for insn in &instructions {
        let len = emitted_len(insn);
        relocated.push(RelocatedInsn {
            original_offset,
            emitted_offset,
            insn: *insn,
            emitted_len: len,
        });
        original_offset += insn.len;
        emitted_offset += len;
    }

    // The mapping from the original offset of an instruction to its offset inside the
    // trampoline, used to fix up branches that target a relocated instruction.
    let offsets: Vec<(usize, usize)> = relocated
        .iter()
        .map(|entry| (entry.original_offset, entry.emitted_offset))
        .collect();

    let total_emitted = emitted_offset;
    let image_size = total_emitted + jump_back_size();

    let trampoline = VirtualAlloc(
        None,
        image_size,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    );
    if trampoline.is_null() {
        return Err(DetourError::AllocationFailed);
    }
    let trampoline_addr = trampoline as usize;

    let mut image = Vec::with_capacity(image_size);
    for entry in &relocated {
        image.extend_from_slice(&emit_instruction(
            &code,
            entry,
            &offsets,
            trampoline_addr,
            target_addr,
            original_len,
        )?);
    }

    // Jump back into the original function, right behind the relocated prologue.
    let resume = target_addr + original_len;
    let back = trampoline_addr + image.len();
    match size_of::<usize>() {
        8 => {
            image.extend_from_slice(&[0xFF, 0x25, 0, 0, 0, 0]);
            image.extend_from_slice(&(resume as u64).to_le_bytes());
        }
        _ => {
            let displacement = resume as i64 - (back as i64 + 5);
            let displacement = i32::try_from(displacement).map_err(|_| DetourError::DisplacementOutOfRange)?;
            image.push(0xE9);
            image.extend_from_slice(&displacement.to_le_bytes());
        }
    }

    // The prologue is copied into the trampoline verbatim, so it can be restored later.
    let saved = code[..original_len].to_vec();

    // SAFETY: `trampoline` points at `image_size` writable bytes that were just
    // allocated with PAGE_EXECUTE_READWRITE.
    std::ptr::copy_nonoverlapping(image.as_ptr(), trampoline as *mut u8, image.len());

    Ok(Trampoline {
        address: trampoline,
        saved,
    })
}

/// Builds the jump that is written over the original prologue.
///
/// # Safety
///
/// Both addresses must be valid code addresses.
unsafe fn build_patch(target: usize, replacement: usize) -> Result<Vec<u8>, DetourError> {
    let mut patch = Vec::with_capacity(patch_size());

    match size_of::<usize>() {
        8 => {
            // jmp qword ptr [rip + 0] followed by the absolute address of the replacement.
            patch.extend_from_slice(&[0xFF, 0x25, 0, 0, 0, 0]);
            patch.extend_from_slice(&(replacement as u64).to_le_bytes());
        }
        _ => {
            // jmp rel32
            let displacement = replacement as i64 - (target as i64 + 5);
            let displacement =
                i32::try_from(displacement).map_err(|_| DetourError::DisplacementOutOfRange)?;
            patch.push(0xE9);
            patch.extend_from_slice(&displacement.to_le_bytes());
        }
    }

    Ok(patch)
}

/// Overwrites `target` with `bytes`, restoring the original memory protection afterwards.
///
/// # Safety
///
/// `target` must point at writable memory that is at least `bytes.len()` bytes long.
unsafe fn write_code(target: *mut c_void, bytes: &[u8]) -> Result<(), DetourError> {
    if target.is_null() {
        return Err(DetourError::NullTarget);
    }

    let mut old_protect = PAGE_PROTECTION_FLAGS(0);
    VirtualProtect(
        target as *const c_void,
        bytes.len(),
        PAGE_EXECUTE_READWRITE,
        &mut old_protect,
    )
    .map_err(|_| DetourError::ProtectionFailed)?;

    // SAFETY: the protection was just changed to writable and the target is known to
    // hold at least `bytes.len()` bytes of mapped memory.
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), target as *mut u8, bytes.len());

    let mut ignored = PAGE_PROTECTION_FLAGS(0);
    VirtualProtect(
        target as *const c_void,
        bytes.len(),
        old_protect,
        &mut ignored,
    )
    .map_err(|_| DetourError::ProtectionFailed)?;

    FlushInstructionCache(GetCurrentProcess(), Some(target as *const c_void), bytes.len())
        .map_err(|_| DetourError::ProtectionFailed)?;

    Ok(())
}

/// Returns how many bytes can safely be read at `address`.
///
/// # Safety
///
/// `address` must be a mapped address.
unsafe fn readable_bytes_at(address: usize) -> Result<usize, DetourError> {
    let mut info = std::mem::MaybeUninit::<MEMORY_BASIC_INFORMATION>::uninit();
    let written = VirtualQuery(
        Some(address as *const c_void),
        info.as_mut_ptr(),
        size_of::<MEMORY_BASIC_INFORMATION>(),
    );

    if written == 0 {
        return Err(DetourError::QueryFailed);
    }

    let info = info.assume_init();
    let region_start = info.BaseAddress as usize;
    let region_end = region_start + info.RegionSize;

    if address >= region_end {
        return Err(DetourError::QueryFailed);
    }

    Ok((region_end - address).min(MAX_PROLOGUE))
}

/// Reads `len` bytes at `address`.
///
/// # Safety
///
/// `address` must point at `len` readable bytes.
unsafe fn read_bytes(address: usize, len: usize) -> Result<Vec<u8>, DetourError> {
    let mut buffer = vec![0u8; len];
    std::ptr::copy_nonoverlapping(address as *const u8, buffer.as_mut_ptr(), len);
    Ok(buffer)
}

/// One instruction of the relocated prologue.
struct RelocatedInsn {
    /// The offset of the instruction inside the original function.
    original_offset: usize,
    /// The offset of the instruction inside the trampoline.
    emitted_offset: usize,
    /// The decoded instruction.
    insn: decoder::Insn,
    /// The number of bytes the instruction occupies inside the trampoline.
    emitted_len: usize,
}

/// Returns the number of bytes an instruction occupies inside the trampoline.
fn emitted_len(insn: &decoder::Insn) -> usize {
    match insn.rel.map(|(_, kind)| kind) {
        Some(decoder::RelKind::ShortJump) => 6,
        Some(decoder::RelKind::ShortJumpUnconditional) => 5,
        Some(decoder::RelKind::ShortLoop) => 7,
        _ => insn.len,
    }
}

/// Emits the relocated form of a single instruction.
///
/// # Safety
///
/// `code` must hold the original bytes of the function and `trampoline_addr` must be
/// the address the trampoline will be placed at.
unsafe fn emit_instruction(
    code: &[u8],
    entry: &RelocatedInsn,
    offsets: &[(usize, usize)],
    trampoline_addr: usize,
    target_addr: usize,
    original_len: usize,
) -> Result<Vec<u8>, DetourError> {
    let insn = &entry.insn;
    let bytes = &code[entry.original_offset..entry.original_offset + insn.len];
    let mut out = bytes.to_vec();

    // The address of the instruction that follows the current one in the original
    // function; relative displacements are relative to it.
    let next = target_addr + entry.original_offset + insn.len;

    // Fix up a relative branch, if the instruction has one.
    if let Some((rel_offset, kind)) = insn.rel {
        let field = rel_offset - entry.original_offset;

        let displacement = match kind {
            decoder::RelKind::Long => {
                let raw = i32::from_le_bytes([
                    bytes[field],
                    bytes[field + 1],
                    bytes[field + 2],
                    bytes[field + 3],
                ]);
                raw as i64
            }
            _ => i8::from_le_bytes([bytes[field]]) as i64,
        };

        let mut branch_target = next as i64 + displacement;

        // A branch that stays inside the relocated prologue has to point at the copy
        // inside the trampoline instead.
        if branch_target >= target_addr as i64 && branch_target < (target_addr + original_len) as i64
        {
            let inner = (branch_target - target_addr as i64) as usize;
            if let Some((_, emitted)) = offsets.iter().find(|(original, _)| *original == inner) {
                branch_target = (trampoline_addr + emitted) as i64;
            }
        }

        let instruction_end = trampoline_addr + entry.emitted_offset + entry.emitted_len;

        match kind {
            decoder::RelKind::Long => {
                let new_displacement = branch_target - instruction_end as i64;
                let new_displacement = i32::try_from(new_displacement)
                    .map_err(|_| DetourError::DisplacementOutOfRange)?;
                out[field..field + 4].copy_from_slice(&new_displacement.to_le_bytes());
            }
            decoder::RelKind::ShortJump => {
                // jcc rel8 becomes jcc rel32.
                let condition = insn.opcode & 0x0F;
                let new_displacement = branch_target - instruction_end as i64;
                let new_displacement = i32::try_from(new_displacement)
                    .map_err(|_| DetourError::DisplacementOutOfRange)?;
                out = vec![0x0F, 0x80 | condition];
                out.extend_from_slice(&new_displacement.to_le_bytes());
            }
            decoder::RelKind::ShortJumpUnconditional => {
                // jmp rel8 becomes jmp rel32.
                let new_displacement = branch_target - instruction_end as i64;
                let new_displacement = i32::try_from(new_displacement)
                    .map_err(|_| DetourError::DisplacementOutOfRange)?;
                out = vec![0xE9];
                out.extend_from_slice(&new_displacement.to_le_bytes());
            }
            decoder::RelKind::ShortLoop => {
                // loop/jcxz rel8 becomes "jmp +5" followed by "jmp rel32". The short jump
                // skips over the long one, so the loop semantics are preserved.
                let new_displacement = branch_target - instruction_end as i64;
                let new_displacement = i32::try_from(new_displacement)
                    .map_err(|_| DetourError::DisplacementOutOfRange)?;
                out = vec![0xEB, 0x05, 0xE9];
                out.extend_from_slice(&new_displacement.to_le_bytes());
            }
        }
    }

    // Fix up a RIP-relative memory operand, if the instruction has one.
    if let Some(disp_offset) = insn.rip_disp {
        let field = disp_offset - entry.original_offset;
        let raw = i32::from_le_bytes([
            bytes[field],
            bytes[field + 1],
            bytes[field + 2],
            bytes[field + 3],
        ]);
        let absolute = next as i64 + raw as i64;
        let instruction_end = trampoline_addr + entry.emitted_offset + entry.emitted_len;
        let new_displacement = absolute - instruction_end as i64;
        let new_displacement =
            i32::try_from(new_displacement).map_err(|_| DetourError::DisplacementOutOfRange)?;
        out[field..field + 4].copy_from_slice(&new_displacement.to_le_bytes());
    }

    Ok(out)
}

/// Every thread of the current process except the calling one, suspended.
struct SuspendedThreads(Vec<(HANDLE, u32)>);

impl SuspendedThreads {
    /// Suspends every other thread of the current process.
    fn suspend() -> SuspendedThreads {
        let mut handles: Vec<(HANDLE, u32)> = Vec::new();

        // SAFETY: the snapshot handle is closed on every path out of this function.
        unsafe {
            let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else {
                return SuspendedThreads(handles);
            };

            let process_id = GetCurrentProcessId();
            let current_thread_id = GetCurrentThreadId();

            let mut entry = THREADENTRY32 {
                dwSize: size_of::<THREADENTRY32>() as u32,
                cntUsage: 0,
                th32ThreadID: 0,
                th32OwnerProcessID: 0,
                tpBasePri: 0,
                tpDeltaPri: 0,
                dwFlags: 0,
            };

            if Thread32First(snapshot, &mut entry).is_ok() {
                loop {
                    if entry.th32OwnerProcessID == process_id
                        && entry.th32ThreadID != current_thread_id
                    {
                        if let Ok(handle) = OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID)
                        {
                            // `SuspendThread` returns the previous suspend count; a
                            // thread that was already suspended must not be resumed.
                            let previous = SuspendThread(handle);
                            handles.push((handle, previous));
                        }
                    }

                    entry.dwSize = size_of::<THREADENTRY32>() as u32;
                    if Thread32Next(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }

            let _ = CloseHandle(snapshot);
        }

        SuspendedThreads(handles)
    }
}

impl Drop for SuspendedThreads {
    fn drop(&mut self) {
        // SAFETY: the handles were opened by `suspend` and are closed exactly once.
        unsafe {
            for (handle, previous) in &self.0 {
                if *previous == 0 {
                    ResumeThread(*handle);
                }
                let _ = CloseHandle(*handle);
            }
        }
    }
}

/// Suspends every other thread of the current process.
fn suspend_other_threads() -> SuspendedThreads {
    SuspendedThreads::suspend()
}

/// The x86/x64 instruction decoder used to find instruction boundaries and to detect
/// the operands that have to be fixed up when an instruction is relocated.
mod decoder {
    /// The encoding of a relative branch.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum RelKind {
        /// `jcc rel8`.
        ShortJump,
        /// `jmp rel8`.
        ShortJumpUnconditional,
        /// `loop`/`jcxz` `rel8`.
        ShortLoop,
        /// `call rel32`, `jmp rel32` or `jcc rel32`.
        Long,
    }

    /// Everything the trampoline builder needs to know about one instruction.
    #[derive(Clone, Copy, Debug)]
    pub struct Insn {
        /// The length of the instruction in bytes.
        pub len: usize,
        /// The primary opcode byte.
        pub opcode: u8,
        /// The offset of the relative displacement, and how it is encoded.
        pub rel: Option<(usize, RelKind)>,
        /// The offset of a RIP-relative displacement.
        pub rip_disp: Option<usize>,
    }

    /// The kind of immediate operand an instruction carries.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Imm {
        /// No immediate.
        None,
        /// An 8 bit immediate.
        Ib,
        /// A 16 bit immediate.
        Iw,
        /// A 16 or 32 bit immediate, depending on the operand size.
        Iz,
        /// A 16, 32 or 64 bit immediate, depending on the operand size.
        Iv,
        /// The absolute address of `mov al, [moffs]`.
        Moffs,
        /// A 16 bit followed by an 8 bit immediate (`enter`).
        IwIb,
    }

    /// Decodes the instruction at `pos` inside `code`.
    ///
    /// Returns `None` when the instruction cannot be decoded, which makes the caller
    /// refuse to hook the function rather than corrupt it.
    pub fn decode(code: &[u8], pos: usize) -> Option<Insn> {
        let mut i = pos;
        let mut rex_w = false;
        let mut operand_size_16 = false;

        // Legacy prefixes and the REX prefix.
        loop {
            let byte = *code.get(i)?;
            match byte {
                0xF0 | 0xF2 | 0xF3 | 0x2E | 0x36 | 0x3E | 0x26 | 0x64 | 0x65 => i += 1,
                0x66 => {
                    operand_size_16 = true;
                    i += 1;
                }
                0x67 => i += 1,
                0x40..=0x4F if cfg!(target_arch = "x86_64") => {
                    rex_w = byte & 0x08 != 0;
                    i += 1;
                }
                _ => break,
            }
        }

        let opcode = *code.get(i)?;
        i += 1;

        let mut has_modrm = false;
        let mut group3 = false;
        let mut imm = Imm::None;
        let mut rel: Option<(usize, RelKind)> = None;

        if opcode == 0x0F {
            let second = *code.get(i)?;
            i += 1;

            match second {
                // Instructions without a ModRM byte.
                0x05 | 0x06 | 0x07 | 0x08 | 0x09 | 0x0B | 0x0E | 0x30..=0x37 | 0x77 | 0xA0
                | 0xA1 | 0xA2 | 0xA8 | 0xA9 | 0xAA | 0xC8..=0xCF => {}
                // 3DNow! and the SSE groups that carry an extra immediate byte.
                0x0F | 0x70..=0x73 | 0xA4 | 0xAC | 0xBA | 0xC2 | 0xC4 | 0xC5 | 0xC6 => {
                    has_modrm = true;
                    imm = Imm::Ib;
                }
                // The three byte opcode maps.
                0x38 => has_modrm = true,
                0x3A => {
                    has_modrm = true;
                    imm = Imm::Ib;
                }
                // The near conditional jumps.
                0x80..=0x8F => rel = Some((i, RelKind::Long)),
                _ => has_modrm = true,
            }
        } else {
            match opcode {
                0x00..=0x3F => match opcode & 0x07 {
                    0..=3 => has_modrm = true,
                    4 => imm = Imm::Ib,
                    5 => imm = Imm::Iz,
                    _ => {}
                },
                0x40..=0x4F => {}
                0x50..=0x5F => {}
                0x60 | 0x61 => {}
                0x62 | 0x63 => has_modrm = true,
                0x68 => imm = Imm::Iz,
                0x69 => {
                    has_modrm = true;
                    imm = Imm::Iz;
                }
                0x6A => imm = Imm::Ib,
                0x6B => {
                    has_modrm = true;
                    imm = Imm::Ib;
                }
                0x6C..=0x6F => {}
                0x70..=0x7F => rel = Some((i, RelKind::ShortJump)),
                0x80 | 0x82 => {
                    has_modrm = true;
                    imm = Imm::Ib;
                }
                0x81 => {
                    has_modrm = true;
                    imm = Imm::Iz;
                }
                0x83 => {
                    has_modrm = true;
                    imm = Imm::Ib;
                }
                0x84..=0x8F => has_modrm = true,
                0x90..=0x9F => {}
                0xA0..=0xA3 => imm = Imm::Moffs,
                0xA4..=0xA7 => {}
                0xA8 => imm = Imm::Ib,
                0xA9 => imm = Imm::Iz,
                0xAA..=0xAF => {}
                0xB0..=0xB7 => imm = Imm::Ib,
                0xB8..=0xBF => imm = Imm::Iv,
                0xC0 | 0xC1 => {
                    has_modrm = true;
                    imm = Imm::Ib;
                }
                0xC2 => imm = Imm::Iw,
                0xC3 => {}
                0xC4 | 0xC5 => has_modrm = true,
                0xC6 => {
                    has_modrm = true;
                    imm = Imm::Ib;
                }
                0xC7 => {
                    has_modrm = true;
                    imm = Imm::Iz;
                }
                0xC8 => imm = Imm::IwIb,
                0xC9 => {}
                0xCA => imm = Imm::Iw,
                0xCB | 0xCC => {}
                0xCD => imm = Imm::Ib,
                0xCE | 0xCF => {}
                0xD0..=0xD3 => has_modrm = true,
                0xD4 | 0xD5 => imm = Imm::Ib,
                0xD6 | 0xD7 => {}
                0xD8..=0xDF => has_modrm = true,
                0xE0..=0xE3 => rel = Some((i, RelKind::ShortLoop)),
                0xE4..=0xE7 => imm = Imm::Ib,
                0xE8 | 0xE9 => rel = Some((i, RelKind::Long)),
                0xEB => rel = Some((i, RelKind::ShortJumpUnconditional)),
                0xEC..=0xEF => {}
                0xF0 | 0xF1 | 0xF4 | 0xF5 => {}
                0xF6 | 0xF7 => {
                    has_modrm = true;
                    group3 = true;
                }
                0xF8..=0xFD => {}
                0xFE | 0xFF => has_modrm = true,
                _ => return None,
            }
        }

        // The ModRM byte and the displacement that may follow it.
        let mut modrm_reg = 0u8;
        let mut rip_disp = None;

        if has_modrm {
            let modrm = *code.get(i)?;
            i += 1;

            let md = modrm >> 6;
            modrm_reg = (modrm >> 3) & 0x07;
            let rm = modrm & 0x07;

            if md != 3 {
                if cfg!(target_arch = "x86_64") && md == 0 && rm == 5 {
                    // RIP-relative addressing.
                    rip_disp = Some(i);
                    code.get(i..i + 4)?;
                    i += 4;
                } else if rm == 4 {
                    // A SIB byte follows.
                    let sib = *code.get(i)?;
                    i += 1;
                    let base = sib & 0x07;

                    if md == 0 && base == 5 {
                        code.get(i..i + 4)?;
                        i += 4;
                    } else if md == 1 {
                        code.get(i..i + 1)?;
                        i += 1;
                    } else if md == 2 {
                        code.get(i..i + 4)?;
                        i += 4;
                    }
                } else if md == 0 && rm == 5 {
                    // A 32 bit absolute address.
                    code.get(i..i + 4)?;
                    i += 4;
                } else if md == 1 {
                    code.get(i..i + 1)?;
                    i += 1;
                } else if md == 2 {
                    code.get(i..i + 4)?;
                    i += 4;
                }
            }
        }

        // The TEST/NOT/NEG/MUL family only carries an immediate for TEST.
        if group3 {
            imm = if opcode == 0xF6 {
                if modrm_reg <= 1 {
                    Imm::Ib
                } else {
                    Imm::None
                }
            } else if modrm_reg <= 1 {
                Imm::Iz
            } else {
                Imm::None
            };
        }

        // The immediate operand.
        let imm_len = match imm {
            Imm::None => 0,
            Imm::Ib => 1,
            Imm::Iw => 2,
            Imm::Iz => {
                if operand_size_16 {
                    2
                } else {
                    4
                }
            }
            Imm::Iv => {
                if rex_w {
                    8
                } else if operand_size_16 {
                    2
                } else {
                    4
                }
            }
            Imm::Moffs => {
                if cfg!(target_arch = "x86_64") {
                    8
                } else if operand_size_16 {
                    2
                } else {
                    4
                }
            }
            Imm::IwIb => 3,
        };

        if imm_len > 0 {
            code.get(i..i + imm_len)?;
            i += imm_len;
        }

        Some(Insn {
            len: i - pos,
            opcode,
            rel,
            rip_disp,
        })
    }
}
